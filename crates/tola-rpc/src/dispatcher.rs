//! High-performance JSON-RPC 2.0 dispatcher for Ethereum compatibility and TOLA protocol endpoints.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};

use tola_codec::{CanonicalDeserialize, CanonicalSerialize};
use tola_core::StateDB;
use tola_crypto::{blake3_hash, keccak256_hash};
use tola_mempool::Mempool;
use tola_types::*;

use crate::json::JsonValue;
use crate::subscription::SubscriptionManager;
use crate::types::{JsonRpcError, JsonRpcRequest, JsonRpcResponse};

pub fn compute_transaction_id(tx: &Transaction) -> TransactionId {
    let wire_bytes = tx.to_canonical_bytes();
    TransactionId(blake3_hash(&wire_bytes))
}

pub fn parse_access_list_json(val: Option<&JsonValue>) -> Vec<AccessListItem> {
    let arr = match val.and_then(|v| v.as_array()) {
        Some(a) => a,
        None => return Vec::new(),
    };
    let mut list = Vec::new();
    for item in arr {
        let addr_hex = item.get("address").and_then(|v| v.as_str());
        let address = match addr_hex.and_then(|s| Address::from_hex(s).ok()) {
            Some(a) => a,
            None => continue,
        };
        let mut storage_keys = Vec::new();
        if let Some(keys) = item.get("storageKeys").and_then(|v| v.as_array()) {
            for k in keys {
                if let Some(s) = k.as_str() {
                    if let Ok(h) = Hash::from_hex(s) {
                        storage_keys.push(h);
                    }
                }
            }
        }
        list.push(AccessListItem {
            address,
            storage_keys,
        });
    }
    list
}

pub type TxBroadcaster = Arc<dyn Fn(&Transaction) + Send + Sync>;

pub const MAX_LOG_BLOCK_RANGE: u64 = 2000;
pub const MAX_LOG_RESULTS: usize = 10000;
pub const FILTER_TTL_SECS: u64 = 300;
pub const MAX_ACTIVE_FILTERS: usize = 1024;

#[derive(Clone, Debug, PartialEq)]
pub struct FilterCriteria {
    pub from_block: Option<u64>,
    pub to_block: Option<u64>,
    pub block_hash: Option<Hash>,
    pub addresses: Vec<Address>,
    pub topics: Vec<Option<Vec<Hash>>>,
}

impl FilterCriteria {
    pub fn default_range(from: u64, to: u64) -> Self {
        Self {
            from_block: Some(from),
            to_block: Some(to),
            block_hash: None,
            addresses: Vec::new(),
            topics: Vec::new(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct LogFilter {
    pub id: u64,
    pub criteria: FilterCriteria,
    pub last_polled_height: u64,
    pub last_active: std::time::Instant,
}

pub struct RpcDispatcher<S: StateDB> {
    pub chain_id: ChainId,
    pub client_version: String,
    pub state_db: Arc<RwLock<S>>,
    pub mempool: Arc<RwLock<Mempool>>,
    pub current_height: Arc<AtomicU64>,
    pub base_fee: Arc<RwLock<Amount>>,
    pub peer_count: Arc<AtomicUsize>,
    pub active_committee: Arc<RwLock<Option<ValidatorSet>>>,
    pub blocks_by_hash: Arc<RwLock<HashMap<Hash, Block>>>,
    pub blocks_by_height: Arc<RwLock<HashMap<u64, Hash>>>,
    pub receipts: Arc<RwLock<HashMap<TransactionId, Receipt>>>,
    pub tx_lookup: Arc<RwLock<HashMap<TransactionId, (Hash, u64)>>>, // (block_hash, tx_index)
    pub tx_broadcaster: Arc<RwLock<Option<TxBroadcaster>>>,
    pub eth_tx_lookup: Arc<RwLock<HashMap<Hash, TransactionId>>>,
    pub external_asset_registry: Arc<RwLock<HashMap<u64, ExternalAsset>>>,
    pub external_asset_by_canonical_id: Arc<RwLock<HashMap<[u8; 32], u64>>>,
    pub next_filter_id: Arc<AtomicU64>,
    pub filters: Arc<RwLock<HashMap<u64, LogFilter>>>,
    pub subscription_manager: Arc<SubscriptionManager>,
    pub governance: Arc<RwLock<tola_consensus::governance::GovernanceEngine>>,
    pub emergency_council: Arc<RwLock<tola_consensus::governance::EmergencyCouncilConfig>>,
}

impl<S: StateDB> RpcDispatcher<S> {
    pub fn new(
        chain_id: ChainId,
        client_version: String,
        state_db: Arc<RwLock<S>>,
        mempool: Arc<RwLock<Mempool>>,
    ) -> Self {
        let init_height = {
            let guard = state_db.read().unwrap();
            guard.current_height().0
        };
        Self {
            chain_id,
            client_version,
            state_db,
            mempool,
            current_height: Arc::new(AtomicU64::new(init_height)),
            base_fee: Arc::new(RwLock::new(Amount::from_u128(1_000_000_000))), // 1 Gwei
            peer_count: Arc::new(AtomicUsize::new(0)),
            active_committee: Arc::new(RwLock::new(None)),
            blocks_by_hash: Arc::new(RwLock::new(HashMap::new())),
            blocks_by_height: Arc::new(RwLock::new(HashMap::new())),
            receipts: Arc::new(RwLock::new(HashMap::new())),
            tx_lookup: Arc::new(RwLock::new(HashMap::new())),
            tx_broadcaster: Arc::new(RwLock::new(None)),
            eth_tx_lookup: Arc::new(RwLock::new(HashMap::new())),
            external_asset_registry: Arc::new(RwLock::new(HashMap::new())),
            external_asset_by_canonical_id: Arc::new(RwLock::new(HashMap::new())),
            next_filter_id: Arc::new(AtomicU64::new(1)),
            filters: Arc::new(RwLock::new(HashMap::new())),
            subscription_manager: Arc::new(SubscriptionManager::new()),
            governance: Arc::new(RwLock::new(
                tola_consensus::governance::GovernanceEngine::new(
                    tola_consensus::governance::GovernanceConfig::default(),
                    tola_consensus::governance::ActiveProtocolParams::default(),
                ),
            )),
            emergency_council: Arc::new(RwLock::new(
                tola_consensus::governance::EmergencyCouncilConfig::default(),
            )),
        }
    }

    pub fn register_external_asset(&self, asset: ExternalAsset) -> Result<(), &'static str> {
        let mut by_canon = self.external_asset_by_canonical_id.write().unwrap();
        if by_canon.contains_key(&asset.canonical_id) {
            return Err("Asset already minted with this canonical ID");
        }
        let token_id = asset.token_id;
        by_canon.insert(asset.canonical_id, token_id);
        let mut reg = self.external_asset_registry.write().unwrap();
        reg.insert(token_id, asset);
        Ok(())
    }

    /// Stores a finalized or committed block in RPC index caches.
    pub fn record_block(&self, block: Block, receipts: Vec<Receipt>) {
        let block_hash = tola_consensus::PipelinedBftEngine::compute_block_hash(&block);
        let h = block.header.height.0;

        self.current_height.store(h, Ordering::SeqCst);
        *self.base_fee.write().unwrap() = block.header.base_fee_per_gas;

        let mut b_by_hash = self.blocks_by_hash.write().unwrap();
        let mut b_by_height = self.blocks_by_height.write().unwrap();
        let mut rec_map = self.receipts.write().unwrap();
        let mut tx_map = self.tx_lookup.write().unwrap();
        let mut eth_tx_map = self.eth_tx_lookup.write().unwrap();

        for (idx, (tx, r)) in block.transactions.iter().zip(receipts.iter()).enumerate() {
            let tx_id = compute_transaction_id(tx);
            rec_map.insert(tx_id, r.clone());
            tx_map.insert(tx_id, (block_hash, idx as u64));

            if tx.is_evm() {
                if let Some(eth_hash) = tola_codec::rlp::compute_eth_tx_hash(tx) {
                    eth_tx_map.insert(eth_hash, tx_id);
                }
            }

            // Index confirmed external asset events from receipt logs
            if r.success {
                for log in &r.logs {
                    if log.address == EXTERNAL_ASSET_CONTRACT_ADDRESS && !log.topics.is_empty() {
                        if log.topics[0] == tola_execution::TOPIC_EXTERNAL_ASSET_MINTED {
                            if let Some(asset) =
                                tola_execution::ExternalAssetStorage::deserialize_asset(&log.data)
                            {
                                let mut reg = self.external_asset_registry.write().unwrap();
                                let mut by_canon =
                                    self.external_asset_by_canonical_id.write().unwrap();
                                by_canon.insert(asset.canonical_id, asset.token_id);
                                reg.insert(asset.token_id, asset);
                            }
                        } else if log.topics[0] == tola_execution::TOPIC_TRANSFER
                            && log.topics.len() >= 4
                        {
                            let mut tid_bytes = [0u8; 8];
                            tid_bytes.copy_from_slice(&log.topics[3].0[24..32]);
                            let token_id = u64::from_be_bytes(tid_bytes);

                            let mut to_addr_bytes = [0u8; 20];
                            to_addr_bytes.copy_from_slice(&log.topics[2].0[12..32]);
                            let to_addr = Address(to_addr_bytes);

                            let mut reg = self.external_asset_registry.write().unwrap();
                            if let Some(asset) = reg.get_mut(&token_id) {
                                asset.owner = to_addr;
                            }
                        }
                    }
                }
            }
        }

        b_by_height.insert(h, block_hash);
        b_by_hash.insert(block_hash, block.clone());
        drop(b_by_hash);
        drop(b_by_height);
        drop(rec_map);
        drop(tx_map);
        drop(eth_tx_map);

        let bloom_bytes = tola_crypto::compute_block_logs_bloom(&receipts);
        let bloom_hex = format!("0x{}", hex_encode(&bloom_bytes));
        let header_json = JsonValue::Object(vec![
            (
                "number".to_string(),
                JsonValue::String(format!("0x{:x}", block.header.height.0)),
            ),
            ("hash".to_string(), JsonValue::String(block_hash.to_hex())),
            (
                "parentHash".to_string(),
                JsonValue::String(block.header.parent_hash.to_hex()),
            ),
            (
                "stateRoot".to_string(),
                JsonValue::String(block.header.state_root.to_hex()),
            ),
            (
                "transactionsRoot".to_string(),
                JsonValue::String(block.header.txs_root.to_hex()),
            ),
            (
                "receiptsRoot".to_string(),
                JsonValue::String(block.header.receipts_root.to_hex()),
            ),
            (
                "miner".to_string(),
                JsonValue::String(block.header.proposer.to_hex()),
            ),
            (
                "gasUsed".to_string(),
                JsonValue::String(format!("0x{:x}", block.header.gas_used.as_u64())),
            ),
            (
                "gasLimit".to_string(),
                JsonValue::String(format!("0x{:x}", block.header.gas_limit.as_u64())),
            ),
            (
                "baseFeePerGas".to_string(),
                JsonValue::String(format!("0x{:x}", block.header.base_fee_per_gas.0)),
            ),
            ("logsBloom".to_string(), JsonValue::String(bloom_hex)),
            (
                "timestamp".to_string(),
                JsonValue::String(format!("0x{:x}", block.header.timestamp.0)),
            ),
            (
                "extraData".to_string(),
                JsonValue::String(format!("0x{}", hex_encode(&block.header.extra_data))),
            ),
        ]);

        self.subscription_manager.notify_new_head(&header_json);
        self.subscription_manager
            .notify_logs(&block, block_hash, &receipts);
    }

    pub fn get_current_height(&self) -> u64 {
        let mem_h = self.current_height.load(Ordering::SeqCst);
        let db_h = self.state_db.read().unwrap().current_height().0;
        mem_h.max(db_h)
    }

    pub fn latest_committed_block_context(&self) -> (BlockHeight, Timestamp, Amount, Address, Gas) {
        let cur_h = self.get_current_height();
        if let Some((_, block)) = self.get_block_and_hash_by_height(cur_h) {
            (
                block.header.height,
                block.header.timestamp,
                block.header.base_fee_per_gas,
                block.header.proposer,
                block.header.gas_limit,
            )
        } else if let Ok(Some(header)) = self
            .state_db
            .read()
            .unwrap()
            .get_block_header_by_height(BlockHeight(cur_h))
        {
            (
                header.height,
                header.timestamp,
                header.base_fee_per_gas,
                header.proposer,
                header.gas_limit,
            )
        } else {
            let base_fee = *self.base_fee.read().unwrap();
            (
                BlockHeight(cur_h),
                Timestamp(0),
                base_fee,
                Address([0u8; 20]),
                Gas(30_000_000),
            )
        }
    }

    /// Resolves any block tag, number, or EIP-1898 object to canonical BlockHeight.
    pub fn resolve_block_tag(
        &self,
        param: Option<&JsonValue>,
    ) -> Result<BlockHeight, JsonRpcError> {
        let cur_h = self.get_current_height();
        let param_val = match param {
            None => return Ok(BlockHeight(cur_h)),
            Some(v) => v,
        };

        match param_val {
            JsonValue::Object(map) => {
                // EIP-1898: { "blockNumber": "0x..." } or { "blockHash": "0x...", "requireCanonical": bool }
                let require_canonical = map
                    .iter()
                    .find(|(k, _)| k == "requireCanonical")
                    .and_then(|(_, v)| v.as_bool())
                    .unwrap_or(false);

                if let Some((_, bnum_val)) = map.iter().find(|(k, _)| k == "blockNumber") {
                    let h = Self::parse_block_tag_or_number(bnum_val, cur_h)?;
                    if h > cur_h {
                        return Err(JsonRpcError::server_error(
                            -32000,
                            &format!(
                                "header not found: block height {} exceeds latest height {}",
                                h, cur_h
                            ),
                        ));
                    }
                    return Ok(BlockHeight(h));
                }

                if let Some((_, bhash_val)) = map.iter().find(|(k, _)| k == "blockHash") {
                    let hash_str = bhash_val.as_str().ok_or_else(|| {
                        JsonRpcError::invalid_params("blockHash must be a string")
                    })?;
                    let hash = Self::parse_hash_32(hash_str)
                        .map_err(|e| JsonRpcError::invalid_params(&e))?;

                    let block_opt = {
                        let b_hashes = self.blocks_by_hash.read().unwrap();
                        b_hashes.get(&hash).cloned()
                    }
                    .or_else(|| {
                        self.state_db
                            .read()
                            .unwrap()
                            .get_block_by_hash(&hash)
                            .ok()
                            .flatten()
                    });

                    let block = match block_opt {
                        Some(b) => b,
                        None => return Err(JsonRpcError::server_error(-32000, "header not found")),
                    };

                    let h = block.header.height;
                    if h.0 > cur_h {
                        return Err(JsonRpcError::server_error(-32000, "header not found"));
                    }

                    if require_canonical {
                        let canon_hash_opt = {
                            let b_heights = self.blocks_by_height.read().unwrap();
                            b_heights.get(&h.0).cloned()
                        }
                        .or_else(|| {
                            self.state_db
                                .read()
                                .unwrap()
                                .get_block_hash_by_height(h)
                                .ok()
                                .flatten()
                        });

                        match canon_hash_opt {
                            Some(canon_h) if canon_h == hash => {}
                            _ => {
                                return Err(JsonRpcError::server_error(
                                    -32000,
                                    "block is not in the canonical chain",
                                ))
                            }
                        }
                    }

                    return Ok(h);
                }

                Err(JsonRpcError::invalid_params(
                    "EIP-1898 object must contain blockNumber or blockHash",
                ))
            }
            JsonValue::String(s) => match s.as_str() {
                "latest" | "pending" | "safe" | "finalized" => Ok(BlockHeight(cur_h)),
                "earliest" => Ok(BlockHeight(0)),
                hex if hex.starts_with("0x") || hex.starts_with("0X") => {
                    let h = u64::from_str_radix(&hex[2..], 16).map_err(|e| {
                        JsonRpcError::invalid_params(&format!("Invalid hex block number: {}", e))
                    })?;
                    if h > cur_h {
                        return Err(JsonRpcError::server_error(
                            -32000,
                            &format!(
                                "header not found: block height {} exceeds latest height {}",
                                h, cur_h
                            ),
                        ));
                    }
                    Ok(BlockHeight(h))
                }
                dec => {
                    let h = dec.parse::<u64>().map_err(|e| {
                        JsonRpcError::invalid_params(&format!(
                            "Invalid decimal block number: {}",
                            e
                        ))
                    })?;
                    if h > cur_h {
                        return Err(JsonRpcError::server_error(
                            -32000,
                            &format!(
                                "header not found: block height {} exceeds latest height {}",
                                h, cur_h
                            ),
                        ));
                    }
                    Ok(BlockHeight(h))
                }
            },
            JsonValue::Integer(i) => {
                if *i >= 0 {
                    let h = *i as u64;
                    if h > cur_h {
                        return Err(JsonRpcError::server_error(
                            -32000,
                            &format!(
                                "header not found: block height {} exceeds latest height {}",
                                h, cur_h
                            ),
                        ));
                    }
                    Ok(BlockHeight(h))
                } else {
                    Err(JsonRpcError::invalid_params("Negative block number"))
                }
            }
            JsonValue::Number(n) => {
                if *n >= 0.0 {
                    let h = *n as u64;
                    if h > cur_h {
                        return Err(JsonRpcError::server_error(
                            -32000,
                            &format!(
                                "header not found: block height {} exceeds latest height {}",
                                h, cur_h
                            ),
                        ));
                    }
                    Ok(BlockHeight(h))
                } else {
                    Err(JsonRpcError::invalid_params("Negative block number"))
                }
            }
            _ => Err(JsonRpcError::invalid_params(
                "Block tag must be a string, integer, or EIP-1898 object",
            )),
        }
    }

    /// Resolves canonical block context (height, timestamp, base fee, proposer, gas limit) for simulation.
    pub fn resolve_block_context(
        &self,
        param: Option<&JsonValue>,
    ) -> Result<(BlockHeight, Timestamp, Amount, Address, Gas), JsonRpcError> {
        let height = self.resolve_block_tag(param)?;
        let cur_h = self.current_height.load(Ordering::SeqCst);

        if height.0 == cur_h {
            return Ok(self.latest_committed_block_context());
        }

        // Canonical historical block header lookup
        if let Some((_, block)) = self.get_block_and_hash_by_height(height.0) {
            Ok((
                block.header.height,
                block.header.timestamp,
                block.header.base_fee_per_gas,
                block.header.proposer,
                block.header.gas_limit,
            ))
        } else if let Ok(Some(header)) = self
            .state_db
            .read()
            .unwrap()
            .get_block_header_by_height(height)
        {
            Ok((
                header.height,
                header.timestamp,
                header.base_fee_per_gas,
                header.proposer,
                header.gas_limit,
            ))
        } else if height.0 == 0 {
            // Genesis fallback context
            let base_fee = *self.base_fee.read().unwrap();
            Ok((
                BlockHeight(0),
                Timestamp(0),
                base_fee,
                Address([0u8; 20]),
                Gas(30_000_000),
            ))
        } else {
            Err(JsonRpcError::server_error(
                -32000,
                &format!("header not found: block header {} not found", height.0),
            ))
        }
    }

    /// Executes a JSON-RPC 2.0 string payload (single request or batch array) and returns the response string.
    pub fn dispatch_json(&self, raw_json: &str) -> String {
        let parsed = match JsonValue::parse(raw_json) {
            Ok(v) => v,
            Err(e) => {
                return JsonRpcResponse::error(None, JsonRpcError::parse_error(&e)).serialize()
            }
        };

        match parsed {
            JsonValue::Array(batch) => {
                if batch.is_empty() {
                    return JsonRpcResponse::error(
                        None,
                        JsonRpcError::invalid_request("Empty batch"),
                    )
                    .serialize();
                }
                if batch.len() > crate::types::MAX_BATCH_REQUESTS {
                    return JsonRpcResponse::error(
                        None,
                        JsonRpcError::invalid_request(&format!(
                            "Batch too large (limit: {})",
                            crate::types::MAX_BATCH_REQUESTS
                        )),
                    )
                    .serialize();
                }

                let mut responses = Vec::with_capacity(batch.len());
                for item in batch {
                    let is_notification =
                        matches!(item, JsonValue::Object(_)) && item.get("id").is_none();
                    let resp = match JsonRpcRequest::from_json(&item) {
                        Ok(req) => {
                            let resp = self.dispatch_request(&req);
                            if req.id.is_none() {
                                None
                            } else {
                                Some(resp)
                            }
                        }
                        Err(err) => {
                            if is_notification {
                                None
                            } else {
                                let id = item.get("id").cloned();
                                Some(JsonRpcResponse::error(id, err))
                            }
                        }
                    };
                    if let Some(r) = resp {
                        responses.push(r.to_json());
                    }
                }
                if responses.is_empty() {
                    String::new()
                } else {
                    JsonValue::Array(responses).serialize()
                }
            }
            ref obj @ JsonValue::Object(_) => {
                let req = match JsonRpcRequest::from_json(obj) {
                    Ok(r) => r,
                    Err(e) => {
                        let id = obj.get("id").cloned();
                        return JsonRpcResponse::error(id, e).serialize();
                    }
                };
                let resp = self.dispatch_request(&req);
                resp.serialize()
            }
            _ => JsonRpcResponse::error(
                None,
                JsonRpcError::invalid_request("Expected JSON object or array"),
            )
            .serialize(),
        }
    }

    /// Dispatches a parsed `JsonRpcRequest` to the matching handler.
    pub fn dispatch_request(&self, req: &JsonRpcRequest) -> JsonRpcResponse {
        let id = req.id.clone();

        match req.method.as_str() {
            // Web3 & Net methods
            "web3_clientVersion" => {
                JsonRpcResponse::success(id, JsonValue::String(self.client_version.clone()))
            }
            "web3_sha3" => self.handle_web3_sha3(id, &req.params),
            "net_version" => {
                JsonRpcResponse::success(id, JsonValue::String(self.chain_id.0.to_string()))
            }
            "net_listening" => JsonRpcResponse::success(id, JsonValue::Bool(true)),
            "net_peerCount" => {
                let count = self.peer_count.load(Ordering::SeqCst);
                JsonRpcResponse::success(id, JsonValue::String(format!("0x{:x}", count)))
            }

            // Standard Ethereum methods
            "eth_chainId" => {
                JsonRpcResponse::success(id, JsonValue::String(format!("0x{:x}", self.chain_id.0)))
            }
            "eth_blockNumber" => {
                let h = self.get_current_height();
                JsonRpcResponse::success(id, JsonValue::String(format!("0x{:x}", h)))
            }
            "eth_gasPrice" => {
                let fee = self.base_fee.read().unwrap().0;
                JsonRpcResponse::success(id, JsonValue::String(format!("0x{:x}", fee)))
            }
            "eth_maxPriorityFeePerGas" => {
                JsonRpcResponse::success(id, JsonValue::String("0x3b9aca00".to_string()))
            }
            "eth_feeHistory" => self.handle_eth_fee_history(id, &req.params),
            "eth_getBalance" => self.handle_eth_get_balance(id, &req.params),
            "eth_getTransactionCount" => self.handle_eth_get_transaction_count(id, &req.params),
            "eth_getCode" => self.handle_eth_get_code(id, &req.params),
            "eth_getStorageAt" => self.handle_eth_get_storage_at(id, &req.params),
            "eth_call" => self.handle_eth_call(id, &req.params),
            "eth_estimateGas" => self.handle_eth_estimate_gas(id, &req.params),
            "eth_sendRawTransaction" => self.handle_eth_send_raw_transaction(id, &req.params),
            "eth_getBlockByNumber" => self.handle_eth_get_block_by_number(id, &req.params),
            "eth_getBlockByHash" => self.handle_eth_get_block_by_hash(id, &req.params),
            "eth_getTransactionByHash" => self.handle_eth_get_transaction_by_hash(id, &req.params),
            "eth_getTransactionReceipt" => self.handle_eth_get_transaction_receipt(id, &req.params),
            "eth_getLogs" => self.handle_eth_get_logs(id, &req.params),
            "eth_newFilter" => self.handle_eth_new_filter(id, &req.params),
            "eth_getFilterChanges" => self.handle_eth_get_filter_changes(id, &req.params),
            "eth_getFilterLogs" => self.handle_eth_get_filter_logs(id, &req.params),
            "eth_uninstallFilter" => self.handle_eth_uninstall_filter(id, &req.params),
            "eth_accounts" => JsonRpcResponse::success(id, JsonValue::Array(vec![])),
            "eth_syncing" => JsonRpcResponse::success(id, JsonValue::Bool(false)),
            "eth_protocolVersion" => {
                JsonRpcResponse::success(id, JsonValue::String("0x41".to_string()))
            }
            "eth_coinbase" => {
                let (_, _, _, proposer, _) = self.latest_committed_block_context();
                JsonRpcResponse::success(id, JsonValue::String(proposer.to_hex()))
            }
            "eth_mining" => JsonRpcResponse::success(id, JsonValue::Bool(false)),
            "eth_hashrate" => JsonRpcResponse::success(id, JsonValue::String("0x0".to_string())),
            "eth_getBlockTransactionCountByNumber" => {
                self.handle_eth_get_block_transaction_count_by_number(id, &req.params)
            }
            "eth_getBlockTransactionCountByHash" => {
                self.handle_eth_get_block_transaction_count_by_hash(id, &req.params)
            }
            "eth_getTransactionByBlockNumberAndIndex" => {
                self.handle_eth_get_transaction_by_block_number_and_index(id, &req.params)
            }
            "eth_getTransactionByBlockHashAndIndex" => {
                self.handle_eth_get_transaction_by_block_hash_and_index(id, &req.params)
            }
            "eth_getUncleCountByBlockHash" => {
                self.handle_eth_get_uncle_count_by_block_hash(id, &req.params)
            }
            "eth_getUncleCountByBlockNumber" => {
                self.handle_eth_get_uncle_count_by_block_number(id, &req.params)
            }
            "eth_getUncleByBlockHashAndIndex" => {
                self.handle_eth_get_uncle_by_block_hash_and_index(id, &req.params)
            }
            "eth_getUncleByBlockNumberAndIndex" => {
                self.handle_eth_get_uncle_by_block_number_and_index(id, &req.params)
            }
            "eth_subscribe" => JsonRpcResponse::error(
                id,
                JsonRpcError::invalid_request(
                    "Notifications not supported over HTTP transport; use WebSocket",
                ),
            ),
            "eth_unsubscribe" => JsonRpcResponse::error(
                id,
                JsonRpcError::invalid_request(
                    "Notifications not supported over HTTP transport; use WebSocket",
                ),
            ),

            // TOLA Protocol Extensions
            "tola_getHealth" => {
                let h = self.current_height.load(Ordering::SeqCst);
                let peers = self.peer_count.load(Ordering::SeqCst);
                let chain_id = self.chain_id.0;
                let obj = JsonValue::Object(vec![
                    ("status".to_string(), JsonValue::String("ok".to_string())),
                    ("chain_id".to_string(), JsonValue::Number(chain_id as f64)),
                    ("height".to_string(), JsonValue::Number(h as f64)),
                    ("peers".to_string(), JsonValue::Number(peers as f64)),
                ]);
                JsonRpcResponse::success(id, obj)
            }
            "tola_getEpoch" => {
                let h = self.current_height.load(Ordering::SeqCst);
                let epoch = h / 21600; // ~12 hours at 2.0s blocks
                JsonRpcResponse::success(id, JsonValue::String(format!("0x{:x}", epoch)))
            }
            "tola_getValidators" => self.handle_tola_get_validators(id),
            "tola_getQuorumCertificate" => self.handle_tola_get_quorum_certificate(id, &req.params),
            "tola_getExternalAsset" => self.handle_tola_get_external_asset(id, &req.params),
            "tola_getExternalAssetOwner" => {
                self.handle_tola_get_external_asset_owner(id, &req.params)
            }
            "tola_getExternalAssetMetadata" => {
                self.handle_tola_get_external_asset_metadata(id, &req.params)
            }
            "tola_verifyExternalAssetAttestation" => {
                self.handle_tola_verify_external_asset_attestation(id, &req.params)
            }
            "tola_getCanonicalId" => self.handle_tola_get_canonical_id(id, &req.params),
            "tola_getGovernanceParams" => self.handle_tola_get_governance_params(id),
            "tola_getProposal" => self.handle_tola_get_proposal(id, &req.params),
            "tola_listProposals" => self.handle_tola_list_proposals(id),
            "tola_submitProposal" => self.handle_tola_submit_proposal(id, &req.params),
            "tola_castVote" => self.handle_tola_cast_vote(id, &req.params),
            "tola_emergencyParamChange" => self.handle_tola_emergency_param_change(id, &req.params),

            _ => JsonRpcResponse::error(id, JsonRpcError::method_not_found(&req.method)),
        }
    }

    fn parse_address(param: &JsonValue) -> Result<Address, JsonRpcError> {
        let s = param
            .as_str()
            .ok_or_else(|| JsonRpcError::invalid_params("Address must be a string"))?;

        if s.starts_with("0x") || s.starts_with("0X") {
            Address::from_hex(s).map_err(|e| JsonRpcError::invalid_params(&format!("{:?}", e)))
        } else if s.starts_with("tola1") || s.starts_with("ttola1") || s.starts_with("dtola1") {
            Address::from_bech32m(s)
                .map(|(_net, addr)| addr)
                .map_err(|e| JsonRpcError::invalid_params(&format!("{:?}", e)))
        } else {
            Address::from_hex(s).map_err(|e| JsonRpcError::invalid_params(&format!("{:?}", e)))
        }
    }

    fn handle_web3_sha3(&self, id: Option<JsonValue>, params: &[JsonValue]) -> JsonRpcResponse {
        let hex_str = match params.get(0).and_then(|p| p.as_str()) {
            Some(s) => s,
            None => {
                return JsonRpcResponse::error(
                    id,
                    JsonRpcError::invalid_params("Expected data hex string"),
                )
            }
        };

        let clean = hex_str
            .strip_prefix("0x")
            .or_else(|| hex_str.strip_prefix("0X"))
            .unwrap_or(hex_str);
        let bytes = match hex_to_bytes(clean) {
            Ok(b) => b,
            Err(e) => return JsonRpcResponse::error(id, JsonRpcError::invalid_params(&e)),
        };

        let hash = keccak256_hash(&bytes);
        JsonRpcResponse::success(id, JsonValue::String(format!("0x{}", hex_encode(&hash.0))))
    }

    fn handle_eth_get_balance(
        &self,
        id: Option<JsonValue>,
        params: &[JsonValue],
    ) -> JsonRpcResponse {
        let addr = match params.get(0).map(Self::parse_address) {
            Some(Ok(a)) => a,
            Some(Err(e)) => return JsonRpcResponse::error(id, e),
            None => {
                return JsonRpcResponse::error(
                    id,
                    JsonRpcError::invalid_params("Missing address parameter"),
                )
            }
        };

        let height = match self.resolve_block_tag(params.get(1)) {
            Ok(h) => h,
            Err(e) => return JsonRpcResponse::error(id, e),
        };

        let state = self.state_db.read().unwrap();
        match state.get_historical_account(height, &addr) {
            Ok(Some(acct)) => {
                JsonRpcResponse::success(id, JsonValue::String(format!("0x{:x}", acct.balance.0)))
            }
            Ok(None) => JsonRpcResponse::success(id, JsonValue::String("0x0".into())),
            Err(e) => JsonRpcResponse::error(id, JsonRpcError::internal_error(&format!("{:?}", e))),
        }
    }

    fn handle_eth_get_transaction_count(
        &self,
        id: Option<JsonValue>,
        params: &[JsonValue],
    ) -> JsonRpcResponse {
        let addr = match params.get(0).map(Self::parse_address) {
            Some(Ok(a)) => a,
            Some(Err(e)) => return JsonRpcResponse::error(id, e),
            None => {
                return JsonRpcResponse::error(
                    id,
                    JsonRpcError::invalid_params("Missing address parameter"),
                )
            }
        };

        let is_pending = params
            .get(1)
            .and_then(|p| p.as_str())
            .map(|s| s == "pending")
            .unwrap_or(false);

        let height = match self.resolve_block_tag(params.get(1)) {
            Ok(h) => h,
            Err(e) => return JsonRpcResponse::error(id, e),
        };

        let state = self.state_db.read().unwrap();
        let committed_nonce = match state.get_historical_account(height, &addr) {
            Ok(Some(acct)) => acct.nonce,
            Ok(None) => Nonce::ZERO,
            Err(e) => {
                return JsonRpcResponse::error(
                    id,
                    JsonRpcError::internal_error(&format!("{:?}", e)),
                )
            }
        };

        let nonce = if is_pending {
            let mempool = self.mempool.read().unwrap();
            mempool.get_pending_nonce(&addr, committed_nonce)
        } else {
            committed_nonce
        };

        JsonRpcResponse::success(id, JsonValue::String(format!("0x{:x}", nonce.0)))
    }

    fn handle_eth_get_code(&self, id: Option<JsonValue>, params: &[JsonValue]) -> JsonRpcResponse {
        let addr = match params.get(0).map(Self::parse_address) {
            Some(Ok(a)) => a,
            Some(Err(e)) => return JsonRpcResponse::error(id, e),
            None => {
                return JsonRpcResponse::error(
                    id,
                    JsonRpcError::invalid_params("Missing address parameter"),
                )
            }
        };

        let height = match self.resolve_block_tag(params.get(1)) {
            Ok(h) => h,
            Err(e) => return JsonRpcResponse::error(id, e),
        };

        let state = self.state_db.read().unwrap();
        match state.get_historical_account(height, &addr) {
            Ok(Some(acct)) => {
                if acct.code_hash != Hash::ZERO {
                    if let Ok(Some(code)) = state.get_code(&acct.code_hash) {
                        return JsonRpcResponse::success(
                            id,
                            JsonValue::String(format!("0x{}", hex_encode(&code))),
                        );
                    }
                }
                JsonRpcResponse::success(id, JsonValue::String("0x".into()))
            }
            Ok(None) => JsonRpcResponse::success(id, JsonValue::String("0x".into())),
            Err(e) => JsonRpcResponse::error(id, JsonRpcError::internal_error(&format!("{:?}", e))),
        }
    }

    fn handle_eth_get_storage_at(
        &self,
        id: Option<JsonValue>,
        params: &[JsonValue],
    ) -> JsonRpcResponse {
        let addr = match params.get(0).map(Self::parse_address) {
            Some(Ok(a)) => a,
            Some(Err(e)) => return JsonRpcResponse::error(id, e),
            None => {
                return JsonRpcResponse::error(
                    id,
                    JsonRpcError::invalid_params("Missing address parameter"),
                )
            }
        };

        let slot_str = match params.get(1).and_then(|p| p.as_str()) {
            Some(s) => s,
            None => {
                return JsonRpcResponse::error(
                    id,
                    JsonRpcError::invalid_params("Missing slot parameter"),
                )
            }
        };

        let slot = match Hash::from_hex(slot_str) {
            Ok(h) => h,
            Err(_) => {
                let clean = slot_str
                    .strip_prefix("0x")
                    .or_else(|| slot_str.strip_prefix("0X"))
                    .unwrap_or(slot_str);
                let padded = format!("{:0>64}", clean);
                match Hash::from_hex(&padded) {
                    Ok(h) => h,
                    Err(e) => return JsonRpcResponse::error(id, JsonRpcError::invalid_params(&e)),
                }
            }
        };

        let height = match self.resolve_block_tag(params.get(2)) {
            Ok(h) => h,
            Err(e) => return JsonRpcResponse::error(id, e),
        };

        let state = self.state_db.read().unwrap();
        match state.get_historical_storage(height, &addr, &slot) {
            Ok(val) => JsonRpcResponse::success(id, JsonValue::String(val.to_hex())),
            Err(e) => JsonRpcResponse::error(id, JsonRpcError::internal_error(&format!("{:?}", e))),
        }
    }

    fn handle_eth_call(&self, id: Option<JsonValue>, params: &[JsonValue]) -> JsonRpcResponse {
        let call_obj = match params.get(0) {
            Some(obj) => obj,
            None => {
                return JsonRpcResponse::error(
                    id,
                    JsonRpcError::invalid_params("Missing call object"),
                )
            }
        };

        let to = match call_obj.get("to").and_then(|t| t.as_str()) {
            Some(s) if !s.is_empty() => match Address::from_hex(s) {
                Ok(a) => Some(a),
                Err(e) => {
                    return JsonRpcResponse::error(id, JsonRpcError::invalid_params(&e.to_string()))
                }
            },
            _ => None,
        };

        let from = match call_obj.get("from").and_then(|f| f.as_str()) {
            Some(s) if !s.is_empty() => match Address::from_hex(s) {
                Ok(a) => a,
                Err(e) => {
                    return JsonRpcResponse::error(id, JsonRpcError::invalid_params(&e.to_string()))
                }
            },
            _ => Address([0u8; 20]),
        };

        let data = match call_obj
            .get("data")
            .or_else(|| call_obj.get("input"))
            .and_then(|d| d.as_str())
        {
            Some(s) => {
                let clean = s
                    .strip_prefix("0x")
                    .or_else(|| s.strip_prefix("0X"))
                    .unwrap_or(s);
                match hex_to_bytes(clean) {
                    Ok(b) => b,
                    Err(e) => return JsonRpcResponse::error(id, JsonRpcError::invalid_params(&e)),
                }
            }
            None => vec![],
        };

        let value = match call_obj.get("value").and_then(|v| v.as_str()) {
            Some(s) => {
                let clean = s.strip_prefix("0x").unwrap_or(s);
                Amount::from_u128(u128::from_str_radix(clean, 16).unwrap_or(0))
            }
            None => Amount::ZERO,
        };

        let (height, timestamp, base_fee, proposer, block_gas_limit) =
            match self.resolve_block_context(params.get(1)) {
                Ok(ctx) => ctx,
                Err(e) => return JsonRpcResponse::error(id, e),
            };

        let gas_limit = match call_obj.get("gas").and_then(|g| g.as_str()) {
            Some(s) => {
                let clean = s
                    .strip_prefix("0x")
                    .or_else(|| s.strip_prefix("0X"))
                    .unwrap_or(s);
                let parsed = match u64::from_str_radix(clean, 16) {
                    Ok(g) => g,
                    Err(_) => {
                        return JsonRpcResponse::error(
                            id,
                            JsonRpcError::invalid_params("invalid hex for gas"),
                        )
                    }
                };
                if parsed > block_gas_limit.as_u64() {
                    return JsonRpcResponse::error(
                        id,
                        JsonRpcError::invalid_params("gas limit exceeds block gas limit"),
                    );
                }
                Gas(parsed)
            }
            None => block_gas_limit,
        };

        let gas_price = match call_obj.get("gasPrice").and_then(|g| g.as_str()) {
            Some(s) => {
                let clean = s.strip_prefix("0x").unwrap_or(s);
                Amount::from_u128(u128::from_str_radix(clean, 16).unwrap_or(0))
            }
            None => Amount::ZERO,
        };

        let access_list = parse_access_list_json(call_obj.get("accessList"));

        let tx = Transaction {
            chain_id: self.chain_id,
            nonce: Nonce::ZERO,
            from,
            to,
            value,
            gas_limit,
            max_fee_per_gas: gas_price,
            max_priority_fee_per_gas: Amount::ZERO,
            valid_until_block: BlockHeight(u64::MAX),
            data,
            memo: None,
            signature: Signature::Secp256k1Eth {
                r: [0u8; 32],
                s: [0u8; 32],
                y_parity: 0,
                chain_id: self.chain_id.0,
            },
            tx_type: 2,
            access_list,
        };

        let state = self.state_db.read().unwrap();

        match tola_execution::evm::EvmExecutor::simulate_call(
            &tx,
            height,
            timestamp,
            base_fee,
            proposer,
            block_gas_limit,
            &*state,
        ) {
            Ok(outcome) => {
                if outcome.success {
                    JsonRpcResponse::success(
                        id,
                        JsonValue::String(format!("0x{}", hex_encode(&outcome.return_data))),
                    )
                } else {
                    let ret_hex = format!("0x{}", hex_encode(&outcome.return_data));
                    match outcome.status {
                        tola_execution::evm::EvmExecutionStatus::Halt(ref reason) => {
                            JsonRpcResponse::error(
                                id,
                                JsonRpcError::server_error(
                                    -32000,
                                    &format!("execution halted: {reason}"),
                                ),
                            )
                        }
                        _ => JsonRpcResponse::error(id, JsonRpcError::revert(&ret_hex)),
                    }
                }
            }
            Err(e) => JsonRpcResponse::error(id, JsonRpcError::internal_error(&e)),
        }
    }

    fn handle_eth_estimate_gas(
        &self,
        id: Option<JsonValue>,
        params: &[JsonValue],
    ) -> JsonRpcResponse {
        let call_obj = match params.get(0) {
            Some(obj) => obj,
            None => return JsonRpcResponse::success(id, JsonValue::String("0x5208".into())),
        };

        let to = match call_obj.get("to").and_then(|t| t.as_str()) {
            Some(s) if !s.is_empty() => match Address::from_hex(s) {
                Ok(a) => Some(a),
                Err(e) => {
                    return JsonRpcResponse::error(id, JsonRpcError::invalid_params(&e.to_string()))
                }
            },
            _ => None,
        };

        let from = match call_obj.get("from").and_then(|f| f.as_str()) {
            Some(s) if !s.is_empty() => match Address::from_hex(s) {
                Ok(a) => a,
                Err(e) => {
                    return JsonRpcResponse::error(id, JsonRpcError::invalid_params(&e.to_string()))
                }
            },
            _ => Address([0u8; 20]),
        };

        let data = match call_obj
            .get("data")
            .or_else(|| call_obj.get("input"))
            .and_then(|d| d.as_str())
        {
            Some(s) => {
                let clean = s
                    .strip_prefix("0x")
                    .or_else(|| s.strip_prefix("0X"))
                    .unwrap_or(s);
                match hex_to_bytes(clean) {
                    Ok(b) => b,
                    Err(e) => return JsonRpcResponse::error(id, JsonRpcError::invalid_params(&e)),
                }
            }
            None => vec![],
        };

        if to.is_some() || !data.is_empty() {
            let (height, timestamp, base_fee, proposer, block_gas_limit) =
                match self.resolve_block_context(params.get(1)) {
                    Ok(ctx) => ctx,
                    Err(e) => return JsonRpcResponse::error(id, e),
                };

            let gas_limit = match call_obj.get("gas").and_then(|g| g.as_str()) {
                Some(s) => {
                    let clean = s
                        .strip_prefix("0x")
                        .or_else(|| s.strip_prefix("0X"))
                        .unwrap_or(s);
                    let parsed = match u64::from_str_radix(clean, 16) {
                        Ok(g) => g,
                        Err(_) => {
                            return JsonRpcResponse::error(
                                id,
                                JsonRpcError::invalid_params("invalid hex for gas"),
                            )
                        }
                    };
                    if parsed > block_gas_limit.as_u64() {
                        return JsonRpcResponse::error(
                            id,
                            JsonRpcError::invalid_params("gas limit exceeds block gas limit"),
                        );
                    }
                    Gas(parsed)
                }
                None => block_gas_limit,
            };

            let value = match call_obj.get("value").and_then(|v| v.as_str()) {
                Some(s) => {
                    let clean = s.strip_prefix("0x").unwrap_or(s);
                    Amount::from_u128(u128::from_str_radix(clean, 16).unwrap_or(0))
                }
                None => Amount::ZERO,
            };

            let access_list = parse_access_list_json(call_obj.get("accessList"));

            let tx = Transaction {
                chain_id: self.chain_id,
                nonce: Nonce::ZERO,
                from,
                to,
                value,
                gas_limit,
                max_fee_per_gas: Amount::ZERO,
                max_priority_fee_per_gas: Amount::ZERO,
                valid_until_block: BlockHeight(u64::MAX),
                data,
                memo: None,
                signature: Signature::Secp256k1Eth {
                    r: [0u8; 32],
                    s: [0u8; 32],
                    y_parity: 0,
                    chain_id: self.chain_id.0,
                },
                tx_type: 2,
                access_list,
            };

            let state = self.state_db.read().unwrap();

            match tola_execution::evm::EvmExecutor::simulate_call(
                &tx,
                height,
                timestamp,
                base_fee,
                proposer,
                block_gas_limit,
                &*state,
            ) {
                Ok(outcome) => match outcome.status {
                    tola_execution::evm::EvmExecutionStatus::Success => {
                        let estimated =
                            (outcome.gas_used.as_u64().saturating_mul(120) / 100).max(21_000);
                        return JsonRpcResponse::success(
                            id,
                            JsonValue::String(format!("0x{:x}", estimated)),
                        );
                    }
                    tola_execution::evm::EvmExecutionStatus::Revert => {
                        let ret_hex = format!("0x{}", hex_encode(&outcome.return_data));
                        return JsonRpcResponse::error(id, JsonRpcError::revert(&ret_hex));
                    }
                    tola_execution::evm::EvmExecutionStatus::Halt(ref reason) => {
                        return JsonRpcResponse::error(
                            id,
                            JsonRpcError::server_error(
                                -32000,
                                &format!("execution halted: {}", reason),
                            ),
                        );
                    }
                },
                Err(e) => return JsonRpcResponse::error(id, JsonRpcError::internal_error(&e)),
            }
        }

        let mut gas = 21_000u64;
        let byte_len = call_obj
            .get("data")
            .and_then(|d| d.as_str())
            .map(|s| s.strip_prefix("0x").unwrap_or(s).len() / 2)
            .unwrap_or(0);
        gas = gas.saturating_add((byte_len as u64) * 16);

        JsonRpcResponse::success(id, JsonValue::String(format!("0x{:x}", gas)))
    }

    fn handle_eth_send_raw_transaction(
        &self,
        id: Option<JsonValue>,
        params: &[JsonValue],
    ) -> JsonRpcResponse {
        let raw_hex = match params.get(0).and_then(|p| p.as_str()) {
            Some(s) => s,
            None => {
                return JsonRpcResponse::error(
                    id,
                    JsonRpcError::invalid_params("Missing raw transaction parameter"),
                )
            }
        };

        let clean = raw_hex
            .strip_prefix("0x")
            .or_else(|| raw_hex.strip_prefix("0X"))
            .unwrap_or(raw_hex);
        let raw_bytes = match hex_to_bytes(clean) {
            Ok(b) => b,
            Err(e) => return JsonRpcResponse::error(id, JsonRpcError::invalid_params(&e)),
        };

        let (tx, eth_tx_hash) = match tola_codec::rlp::decode_raw_eth_transaction(&raw_bytes) {
            Ok((t, h)) => (t, h),
            Err(_) => match Transaction::deserialize_canonical(&raw_bytes) {
                Ok(t) => {
                    let tx_id = compute_transaction_id(&t);
                    (t, tx_id.0)
                }
                Err(e) => {
                    return JsonRpcResponse::error(
                        id,
                        JsonRpcError::invalid_params(&format!(
                            "Transaction decoding failed: {:?}",
                            e
                        )),
                    )
                }
            },
        };

        if let Err(e) = tx.stateless_validate(self.chain_id, Gas(30_000_000)) {
            return JsonRpcResponse::error(
                id,
                JsonRpcError::invalid_params(&format!("Stateless validation failed: {:?}", e)),
            );
        }

        let mut mp = self.mempool.write().unwrap();
        let state = self.state_db.read().unwrap();
        let cur_h = self.current_height.load(Ordering::SeqCst);

        if let Err(e) = mp.insert(tx.clone(), &*state, BlockHeight(cur_h)) {
            return JsonRpcResponse::error(
                id,
                JsonRpcError::server_error(-32000, &format!("Mempool admission failed: {:?}", e)),
            );
        }

        if let Some(ref broadcaster) = *self.tx_broadcaster.read().unwrap() {
            broadcaster(&tx);
        }

        self.subscription_manager
            .notify_pending_transaction(&eth_tx_hash.to_hex());

        JsonRpcResponse::success(id, JsonValue::String(eth_tx_hash.to_hex()))
    }

    /// Emits pending transaction notification for transactions admitted via P2P gossip or local insertion.
    pub fn notify_pending_transaction(&self, tx: &Transaction) {
        let hash_hex = if tx.is_evm() {
            tola_codec::rlp::compute_eth_tx_hash(tx)
                .map(|h| h.to_hex())
                .unwrap_or_else(|| compute_transaction_id(tx).0.to_hex())
        } else {
            compute_transaction_id(tx).0.to_hex()
        };
        self.subscription_manager
            .notify_pending_transaction(&hash_hex);
    }

    fn handle_eth_fee_history(
        &self,
        id: Option<JsonValue>,
        params: &[JsonValue],
    ) -> JsonRpcResponse {
        let block_count = match params.get(0) {
            Some(JsonValue::Number(n)) => (*n as u64).clamp(1, 1024),
            Some(JsonValue::String(s)) => {
                if s.starts_with("0x") || s.starts_with("0X") {
                    u64::from_str_radix(&s[2..], 16).unwrap_or(1).clamp(1, 1024)
                } else {
                    s.parse::<u64>().unwrap_or(1).clamp(1, 1024)
                }
            }
            _ => 1,
        };

        let latest_h = self.current_height.load(Ordering::SeqCst);
        let newest_height = match params.get(1) {
            Some(v) => match Self::parse_block_tag_or_number(v, latest_h) {
                Ok(h) => h.min(latest_h),
                Err(e) => return JsonRpcResponse::error(id, e),
            },
            None => latest_h,
        };

        let reward_percentiles: Vec<f64> = match params.get(2) {
            Some(JsonValue::Array(arr)) => arr
                .iter()
                .filter_map(|v| match v {
                    JsonValue::Number(n) => Some(*n),
                    JsonValue::Integer(i) => Some(*i as f64),
                    JsonValue::String(s) => s.parse::<f64>().ok(),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        };

        let count = block_count.min(newest_height + 1);
        let oldest_height = newest_height + 1 - count;

        let mut base_fee_per_gas = Vec::new();
        let mut gas_used_ratio = Vec::new();
        let mut rewards = Vec::new();

        let default_base_fee = self.base_fee.read().unwrap().0;

        for h in oldest_height..=newest_height {
            let block_opt = {
                let b_heights = self.blocks_by_height.read().unwrap();
                let b_hashes = self.blocks_by_hash.read().unwrap();
                b_heights.get(&h).and_then(|bh| b_hashes.get(bh).cloned())
            }
            .or_else(|| {
                self.state_db
                    .read()
                    .unwrap()
                    .get_block_by_height(BlockHeight(h))
                    .ok()
                    .flatten()
            });

            if let Some(b) = block_opt {
                let bf = b.header.base_fee_per_gas.0;
                base_fee_per_gas.push(JsonValue::String(format!("0x{:x}", bf)));
                let gu = b.header.gas_used.as_u64();
                let gl = b.header.gas_limit.as_u64().max(1);
                gas_used_ratio.push(JsonValue::Number(gu as f64 / gl as f64));

                if !reward_percentiles.is_empty() {
                    let mut tips: Vec<u128> = Vec::new();
                    for tx in &b.transactions {
                        let eff = tx.effective_gas_price(b.header.base_fee_per_gas).0;
                        let tip = eff.saturating_sub(bf);
                        tips.push(tip);
                    }
                    tips.sort_unstable();

                    let mut block_reward = Vec::new();
                    for &p in &reward_percentiles {
                        if tips.is_empty() {
                            block_reward.push(JsonValue::String("0x0".to_string()));
                        } else {
                            let idx = ((p / 100.0) * (tips.len() as f64 - 1.0)).round() as usize;
                            let clamped_idx = idx.min(tips.len() - 1);
                            block_reward
                                .push(JsonValue::String(format!("0x{:x}", tips[clamped_idx])));
                        }
                    }
                    rewards.push(JsonValue::Array(block_reward));
                }
            } else {
                base_fee_per_gas.push(JsonValue::String(format!("0x{:x}", default_base_fee)));
                gas_used_ratio.push(JsonValue::Number(0.0));
                if !reward_percentiles.is_empty() {
                    let block_reward = reward_percentiles
                        .iter()
                        .map(|_| JsonValue::String("0x0".to_string()))
                        .collect();
                    rewards.push(JsonValue::Array(block_reward));
                }
            }
        }

        let next_base_fee = self.base_fee.read().unwrap().0;
        base_fee_per_gas.push(JsonValue::String(format!("0x{:x}", next_base_fee)));

        let mut res_obj = vec![
            (
                "oldestBlock".to_string(),
                JsonValue::String(format!("0x{:x}", oldest_height)),
            ),
            (
                "baseFeePerGas".to_string(),
                JsonValue::Array(base_fee_per_gas),
            ),
            ("gasUsedRatio".to_string(), JsonValue::Array(gas_used_ratio)),
        ];

        if !reward_percentiles.is_empty() {
            res_obj.push(("reward".to_string(), JsonValue::Array(rewards)));
        } else {
            res_obj.push(("reward".to_string(), JsonValue::Array(vec![])));
        }

        JsonRpcResponse::success(id, JsonValue::Object(res_obj))
    }

    fn handle_eth_get_block_by_number(
        &self,
        id: Option<JsonValue>,
        params: &[JsonValue],
    ) -> JsonRpcResponse {
        let cur_h = self.current_height.load(Ordering::SeqCst);
        let height = match params.get(0) {
            Some(v) => match Self::parse_block_tag_or_number(v, cur_h) {
                Ok(h) => h,
                Err(e) => return JsonRpcResponse::error(id, e),
            },
            None => cur_h,
        };

        let block_opt = {
            let b_heights = self.blocks_by_height.read().unwrap();
            let b_hashes = self.blocks_by_hash.read().unwrap();
            b_heights
                .get(&height)
                .and_then(|h| b_hashes.get(h).cloned().map(|b| (*h, b)))
        }
        .or_else(|| {
            self.state_db
                .read()
                .unwrap()
                .get_block_by_height(BlockHeight(height))
                .ok()
                .flatten()
                .map(|b| {
                    let h = tola_consensus::PipelinedBftEngine::compute_block_hash(&b);
                    (h, b)
                })
        });

        let (block_hash, block) = match block_opt {
            Some((h, b)) => (h, b),
            None => return JsonRpcResponse::success(id, JsonValue::Null),
        };

        let full_txs = params.get(1).and_then(|p| p.as_bool()).unwrap_or(false);
        let rec_guard = self.receipts.read().unwrap();
        let mut block_receipts = Vec::new();
        for tx in &block.transactions {
            let tx_id = compute_transaction_id(tx);
            if let Some(r) = rec_guard.get(&tx_id).cloned().or_else(|| {
                self.state_db
                    .read()
                    .unwrap()
                    .get_receipt(&tx_id)
                    .ok()
                    .flatten()
            }) {
                block_receipts.push(r);
            }
        }
        let bloom = tola_crypto::compute_block_logs_bloom(&block_receipts);
        let bloom_hex = format!("0x{}", hex_encode(&bloom));
        JsonRpcResponse::success(
            id,
            format_block_json(&block, block_hash, full_txs, &bloom_hex),
        )
    }

    fn handle_eth_get_block_by_hash(
        &self,
        id: Option<JsonValue>,
        params: &[JsonValue],
    ) -> JsonRpcResponse {
        let hash_str = match params.get(0).and_then(|p| p.as_str()) {
            Some(s) => s,
            None => {
                return JsonRpcResponse::error(
                    id,
                    JsonRpcError::invalid_params("Missing block hash"),
                )
            }
        };

        let hash = match Hash::from_hex(hash_str) {
            Ok(h) => h,
            Err(e) => return JsonRpcResponse::error(id, JsonRpcError::invalid_params(&e)),
        };

        let block_opt = self
            .blocks_by_hash
            .read()
            .unwrap()
            .get(&hash)
            .cloned()
            .or_else(|| {
                self.state_db
                    .read()
                    .unwrap()
                    .get_block_by_hash(&hash)
                    .ok()
                    .flatten()
            });

        let block = match block_opt {
            Some(b) => b,
            None => return JsonRpcResponse::success(id, JsonValue::Null),
        };

        let full_txs = params.get(1).and_then(|p| p.as_bool()).unwrap_or(false);
        let rec_guard = self.receipts.read().unwrap();
        let mut block_receipts = Vec::new();
        for tx in &block.transactions {
            let tx_id = compute_transaction_id(tx);
            if let Some(r) = rec_guard.get(&tx_id).cloned().or_else(|| {
                self.state_db
                    .read()
                    .unwrap()
                    .get_receipt(&tx_id)
                    .ok()
                    .flatten()
            }) {
                block_receipts.push(r);
            }
        }
        let bloom = tola_crypto::compute_block_logs_bloom(&block_receipts);
        let bloom_hex = format!("0x{}", hex_encode(&bloom));
        JsonRpcResponse::success(id, format_block_json(&block, hash, full_txs, &bloom_hex))
    }

    fn handle_eth_get_transaction_by_hash(
        &self,
        id: Option<JsonValue>,
        params: &[JsonValue],
    ) -> JsonRpcResponse {
        let hash_str = match params.get(0).and_then(|p| p.as_str()) {
            Some(s) => s,
            None => {
                return JsonRpcResponse::error(
                    id,
                    JsonRpcError::invalid_params("Missing transaction hash"),
                )
            }
        };

        let raw_hash = match Hash::from_hex(hash_str) {
            Ok(h) => h,
            Err(e) => return JsonRpcResponse::error(id, JsonRpcError::invalid_params(&e)),
        };

        let tx_id = self
            .eth_tx_lookup
            .read()
            .unwrap()
            .get(&raw_hash)
            .copied()
            .or_else(|| {
                self.state_db
                    .read()
                    .unwrap()
                    .get_eth_tx(&raw_hash)
                    .ok()
                    .flatten()
            })
            .unwrap_or(TransactionId(raw_hash));

        let location_opt = self
            .tx_lookup
            .read()
            .unwrap()
            .get(&tx_id)
            .copied()
            .or_else(|| {
                self.state_db
                    .read()
                    .unwrap()
                    .get_tx_location(&tx_id)
                    .ok()
                    .flatten()
            });

        let (block_hash, tx_idx) = match location_opt {
            Some(loc) => loc,
            None => return JsonRpcResponse::success(id, JsonValue::Null),
        };

        let block_opt = self
            .blocks_by_hash
            .read()
            .unwrap()
            .get(&block_hash)
            .cloned()
            .or_else(|| {
                self.state_db
                    .read()
                    .unwrap()
                    .get_block_by_hash(&block_hash)
                    .ok()
                    .flatten()
            });

        let block = match block_opt {
            Some(b) => b,
            None => return JsonRpcResponse::success(id, JsonValue::Null),
        };

        let tx = match block.transactions.get(tx_idx as usize) {
            Some(t) => t,
            None => return JsonRpcResponse::success(id, JsonValue::Null),
        };

        JsonRpcResponse::success(
            id,
            format_transaction_json(tx, tx_id, block_hash, block.header.height.0, tx_idx),
        )
    }

    fn handle_eth_get_transaction_receipt(
        &self,
        id: Option<JsonValue>,
        params: &[JsonValue],
    ) -> JsonRpcResponse {
        let hash_str = match params.get(0).and_then(|p| p.as_str()) {
            Some(s) => s,
            None => {
                return JsonRpcResponse::error(
                    id,
                    JsonRpcError::invalid_params("Missing transaction hash"),
                )
            }
        };

        let raw_hash = match Hash::from_hex(hash_str) {
            Ok(h) => h,
            Err(e) => return JsonRpcResponse::error(id, JsonRpcError::invalid_params(&e)),
        };

        let tx_id = self
            .eth_tx_lookup
            .read()
            .unwrap()
            .get(&raw_hash)
            .copied()
            .or_else(|| {
                self.state_db
                    .read()
                    .unwrap()
                    .get_eth_tx(&raw_hash)
                    .ok()
                    .flatten()
            })
            .unwrap_or(TransactionId(raw_hash));

        let rec_guard = self.receipts.read().unwrap();
        let receipt_opt = rec_guard.get(&tx_id).cloned().or_else(|| {
            self.state_db
                .read()
                .unwrap()
                .get_receipt(&tx_id)
                .ok()
                .flatten()
        });

        let receipt = match receipt_opt {
            Some(r) => r,
            None => return JsonRpcResponse::success(id, JsonValue::Null),
        };

        let lookup = self.tx_lookup.read().unwrap();
        let (block_hash, tx_idx) = lookup
            .get(&tx_id)
            .copied()
            .or_else(|| {
                self.state_db
                    .read()
                    .unwrap()
                    .get_tx_location(&tx_id)
                    .ok()
                    .flatten()
            })
            .unwrap_or((Hash::ZERO, 0));

        let blocks = self.blocks_by_hash.read().unwrap();
        let block_opt = blocks.get(&block_hash).cloned().or_else(|| {
            self.state_db
                .read()
                .unwrap()
                .get_block_by_hash(&block_hash)
                .ok()
                .flatten()
        });

        let height = block_opt
            .as_ref()
            .map(|b| b.header.height.0)
            .unwrap_or_else(|| self.current_height.load(Ordering::SeqCst));

        let (from_hex, to_json, contract_addr_json, effective_gas_price, tx_type_str) =
            if let Some(b) = &block_opt {
                if let Some(t) = b.transactions.get(tx_idx as usize) {
                    let to_val = match t.to {
                        Some(to) => JsonValue::String(to.to_hex()),
                        None => JsonValue::Null,
                    };
                    let contract_val = if t.to.is_none() && receipt.success {
                        let deployed_addr =
                            tola_codec::rlp::compute_contract_address(&t.from, t.nonce.0);
                        JsonValue::String(deployed_addr.to_hex())
                    } else {
                        JsonValue::Null
                    };
                    let (eff_price, type_str) = match &t.signature {
                        Signature::Secp256k1Eth { .. } => {
                            if t.tx_type == 0 {
                                (t.max_fee_per_gas, "0x0")
                            } else if t.tx_type == 1 {
                                (t.max_fee_per_gas, "0x1")
                            } else {
                                (t.effective_gas_price(b.header.base_fee_per_gas), "0x2")
                            }
                        }
                        _ => (t.max_fee_per_gas, "0x0"),
                    };
                    (t.from.to_hex(), to_val, contract_val, eff_price, type_str)
                } else {
                    (
                        "0x0000000000000000000000000000000000000000".into(),
                        JsonValue::Null,
                        JsonValue::Null,
                        Amount::ZERO,
                        "0x2",
                    )
                }
            } else {
                (
                    "0x0000000000000000000000000000000000000000".into(),
                    JsonValue::Null,
                    JsonValue::Null,
                    Amount::ZERO,
                    "0x2",
                )
            };

        let bloom = tola_crypto::compute_logs_bloom(&receipt.logs);
        let bloom_hex = format!("0x{}", hex_encode(&bloom));

        let prior_logs: usize = if let Some(b) = &block_opt {
            (0..tx_idx as usize)
                .filter_map(|i| b.transactions.get(i))
                .filter_map(|prev_tx| {
                    let prev_id = compute_transaction_id(prev_tx);
                    rec_guard.get(&prev_id).cloned().or_else(|| {
                        self.state_db
                            .read()
                            .unwrap()
                            .get_receipt(&prev_id)
                            .ok()
                            .flatten()
                    })
                })
                .map(|r| r.logs.len())
                .sum()
        } else {
            0
        };

        let mut logs_json = Vec::new();
        for (log_idx, log) in receipt.logs.iter().enumerate() {
            let topics_json: Vec<JsonValue> = log
                .topics
                .iter()
                .map(|t| JsonValue::String(t.to_hex()))
                .collect();
            let data_hex = format!("0x{}", hex_encode(&log.data));
            let log_obj = JsonValue::Object(vec![
                (
                    "address".to_string(),
                    JsonValue::String(log.address.to_hex()),
                ),
                ("topics".to_string(), JsonValue::Array(topics_json)),
                ("data".to_string(), JsonValue::String(data_hex)),
                (
                    "blockNumber".to_string(),
                    JsonValue::String(format!("0x{:x}", height)),
                ),
                (
                    "transactionHash".to_string(),
                    JsonValue::String(raw_hash.to_hex()),
                ),
                (
                    "transactionIndex".to_string(),
                    JsonValue::String(format!("0x{:x}", tx_idx)),
                ),
                (
                    "blockHash".to_string(),
                    JsonValue::String(block_hash.to_hex()),
                ),
                (
                    "logIndex".to_string(),
                    JsonValue::String(format!("0x{:x}", prior_logs + log_idx)),
                ),
                ("removed".to_string(), JsonValue::Bool(false)),
            ]);
            logs_json.push(log_obj);
        }

        let obj = JsonValue::Object(vec![
            (
                "transactionHash".to_string(),
                JsonValue::String(raw_hash.to_hex()),
            ),
            (
                "transactionIndex".to_string(),
                JsonValue::String(format!("0x{:x}", tx_idx)),
            ),
            (
                "blockHash".to_string(),
                JsonValue::String(block_hash.to_hex()),
            ),
            (
                "blockNumber".to_string(),
                JsonValue::String(format!("0x{:x}", height)),
            ),
            ("from".to_string(), JsonValue::String(from_hex)),
            ("to".to_string(), to_json),
            ("contractAddress".to_string(), contract_addr_json),
            (
                "cumulativeGasUsed".to_string(),
                JsonValue::String(format!("0x{:x}", receipt.cumulative_gas_used.as_u64())),
            ),
            (
                "gasUsed".to_string(),
                JsonValue::String(format!("0x{:x}", receipt.gas_used.as_u64())),
            ),
            ("logsBloom".to_string(), JsonValue::String(bloom_hex)),
            (
                "status".to_string(),
                JsonValue::String(if receipt.success {
                    "0x1".into()
                } else {
                    "0x0".into()
                }),
            ),
            ("logs".to_string(), JsonValue::Array(logs_json)),
            (
                "effectiveGasPrice".to_string(),
                JsonValue::String(format!("0x{:x}", effective_gas_price.0)),
            ),
            ("type".to_string(), JsonValue::String(tx_type_str.into())),
        ]);

        JsonRpcResponse::success(id, obj)
    }

    pub fn parse_block_tag_or_number(
        val: &JsonValue,
        default_latest: u64,
    ) -> Result<u64, JsonRpcError> {
        match val {
            JsonValue::String(s) => match s.as_str() {
                "latest" | "pending" | "safe" | "finalized" => Ok(default_latest),
                "earliest" => Ok(0),
                hex if hex.starts_with("0x") || hex.starts_with("0X") => {
                    u64::from_str_radix(&hex[2..], 16).map_err(|e| {
                        JsonRpcError::invalid_params(&format!("Invalid hex block number: {}", e))
                    })
                }
                dec => dec.parse::<u64>().map_err(|e| {
                    JsonRpcError::invalid_params(&format!("Invalid decimal block number: {}", e))
                }),
            },
            JsonValue::Integer(i) => {
                if *i >= 0 {
                    Ok(*i as u64)
                } else {
                    Err(JsonRpcError::invalid_params("Negative block number"))
                }
            }
            JsonValue::Number(n) => {
                if *n >= 0.0 {
                    Ok(*n as u64)
                } else {
                    Err(JsonRpcError::invalid_params("Negative block number"))
                }
            }
            _ => Err(JsonRpcError::invalid_params(
                "Block tag must be a string or integer",
            )),
        }
    }

    pub fn parse_hash_32(s: &str) -> Result<Hash, String> {
        match Hash::from_hex(s) {
            Ok(h) => Ok(h),
            Err(_) => {
                let clean = s
                    .strip_prefix("0x")
                    .or_else(|| s.strip_prefix("0X"))
                    .unwrap_or(s);
                if clean.len() > 64 {
                    return Err("Hash hex string exceeds 32 bytes".into());
                }
                let padded = format!("{:0>64}", clean);
                Hash::from_hex(&padded)
            }
        }
    }

    pub fn parse_filter_criteria(
        param: &JsonValue,
        default_latest: u64,
    ) -> Result<FilterCriteria, JsonRpcError> {
        let block_hash = match param.get("blockHash") {
            Some(v) => {
                let s = v
                    .as_str()
                    .ok_or_else(|| JsonRpcError::invalid_params("blockHash must be a string"))?;
                let h = Self::parse_hash_32(s).map_err(|e| JsonRpcError::invalid_params(&e))?;
                Some(h)
            }
            None => None,
        };

        let has_from = param.get("fromBlock").is_some();
        let has_to = param.get("toBlock").is_some();

        if block_hash.is_some() && (has_from || has_to) {
            return Err(JsonRpcError::invalid_params(
                "cannot specify both blockHash and fromBlock/toBlock",
            ));
        }

        let from_block = match param.get("fromBlock") {
            Some(v) => Some(Self::parse_block_tag_or_number(v, default_latest)?),
            None => None,
        };

        let to_block = match param.get("toBlock") {
            Some(v) => Some(Self::parse_block_tag_or_number(v, default_latest)?),
            None => None,
        };

        let mut addresses = Vec::new();
        match param.get("address") {
            Some(JsonValue::String(s)) => {
                let addr = Self::parse_address(&JsonValue::String(s.clone()))?;
                addresses.push(addr);
            }
            Some(JsonValue::Array(arr)) => {
                for item in arr {
                    let addr = Self::parse_address(item)?;
                    addresses.push(addr);
                }
            }
            Some(JsonValue::Null) | None => {}
            _ => {
                return Err(JsonRpcError::invalid_params(
                    "address must be a string or array of strings",
                ))
            }
        }

        let mut topics = Vec::new();
        match param.get("topics") {
            Some(JsonValue::Array(arr)) => {
                if arr.len() > 4 {
                    return Err(JsonRpcError::invalid_params(
                        "topics array cannot exceed 4 elements",
                    ));
                }
                for item in arr {
                    match item {
                        JsonValue::Null => {
                            topics.push(None);
                        }
                        JsonValue::String(s) => {
                            let h = Self::parse_hash_32(s)
                                .map_err(|e| JsonRpcError::invalid_params(&e))?;
                            topics.push(Some(vec![h]));
                        }
                        JsonValue::Array(nested_arr) => {
                            let mut options = Vec::new();
                            for sub in nested_arr {
                                let s = sub.as_str().ok_or_else(|| {
                                    JsonRpcError::invalid_params("topic in array must be a string")
                                })?;
                                let h = Self::parse_hash_32(s)
                                    .map_err(|e| JsonRpcError::invalid_params(&e))?;
                                options.push(h);
                            }
                            topics.push(Some(options));
                        }
                        _ => {
                            return Err(JsonRpcError::invalid_params(
                                "topics elements must be null, string, or array of strings",
                            ))
                        }
                    }
                }
            }
            Some(JsonValue::Null) | None => {}
            _ => {
                return Err(JsonRpcError::invalid_params(
                    "topics must be an array of topic specifications",
                ))
            }
        }

        Ok(FilterCriteria {
            from_block,
            to_block,
            block_hash,
            addresses,
            topics,
        })
    }

    pub fn parse_filter_id(param: Option<&JsonValue>) -> Result<u64, JsonRpcError> {
        let val =
            param.ok_or_else(|| JsonRpcError::invalid_params("Missing filter id parameter"))?;
        match val {
            JsonValue::String(s) => {
                if s.starts_with("0x") || s.starts_with("0X") {
                    u64::from_str_radix(&s[2..], 16).map_err(|e| {
                        JsonRpcError::invalid_params(&format!("Invalid hex filter id: {}", e))
                    })
                } else {
                    s.parse::<u64>().map_err(|e| {
                        JsonRpcError::invalid_params(&format!("Invalid decimal filter id: {}", e))
                    })
                }
            }
            JsonValue::Integer(i) => {
                if *i >= 0 {
                    Ok(*i as u64)
                } else {
                    Err(JsonRpcError::invalid_params("Negative filter id"))
                }
            }
            JsonValue::Number(n) => {
                if *n >= 0.0 {
                    Ok(*n as u64)
                } else {
                    Err(JsonRpcError::invalid_params("Negative filter id"))
                }
            }
            _ => Err(JsonRpcError::invalid_params(
                "Filter id must be a string or number",
            )),
        }
    }

    fn prune_expired_filters(&self) {
        let mut filters = self.filters.write().unwrap();
        let now = std::time::Instant::now();
        let ttl = std::time::Duration::from_secs(FILTER_TTL_SECS);
        filters.retain(|_, f| now.duration_since(f.last_active) < ttl);
    }

    pub fn get_transaction_hash_hex(tx: &Transaction, tx_id: &TransactionId) -> String {
        if tx.is_evm() {
            tola_codec::rlp::compute_eth_tx_hash(tx)
                .map(|h| h.to_hex())
                .unwrap_or_else(|| tx_id.0.to_hex())
        } else {
            tx_id.0.to_hex()
        }
    }

    pub fn log_matches_criteria(log: &Log, criteria: &FilterCriteria) -> bool {
        if !criteria.addresses.is_empty() && !criteria.addresses.contains(&log.address) {
            return false;
        }

        for (i, filter_pos) in criteria.topics.iter().enumerate() {
            match filter_pos {
                None => {
                    if i >= log.topics.len() {
                        return false;
                    }
                }
                Some(allowed_hashes) => {
                    if i >= log.topics.len() {
                        return false;
                    }
                    if allowed_hashes.is_empty() {
                        return false;
                    }
                    if !allowed_hashes.contains(&log.topics[i]) {
                        return false;
                    }
                }
            }
        }

        true
    }

    pub fn format_log_json(
        log: &Log,
        block_height: u64,
        block_hash: Hash,
        tx_hash_hex: &str,
        tx_idx: u64,
        log_index: u64,
    ) -> JsonValue {
        let topics_json: Vec<JsonValue> = log
            .topics
            .iter()
            .map(|t| JsonValue::String(t.to_hex()))
            .collect();
        let data_hex = format!("0x{}", hex_encode(&log.data));

        JsonValue::Object(vec![
            (
                "address".to_string(),
                JsonValue::String(log.address.to_hex()),
            ),
            ("topics".to_string(), JsonValue::Array(topics_json)),
            ("data".to_string(), JsonValue::String(data_hex)),
            (
                "blockNumber".to_string(),
                JsonValue::String(format!("0x{:x}", block_height)),
            ),
            (
                "transactionHash".to_string(),
                JsonValue::String(tx_hash_hex.to_string()),
            ),
            (
                "transactionIndex".to_string(),
                JsonValue::String(format!("0x{:x}", tx_idx)),
            ),
            (
                "blockHash".to_string(),
                JsonValue::String(block_hash.to_hex()),
            ),
            (
                "logIndex".to_string(),
                JsonValue::String(format!("0x{:x}", log_index)),
            ),
            ("removed".to_string(), JsonValue::Bool(false)),
        ])
    }

    fn get_block_and_hash_by_height(&self, height: u64) -> Option<(Hash, Block)> {
        let in_mem = {
            let b_heights = self.blocks_by_height.read().unwrap();
            let b_hashes = self.blocks_by_hash.read().unwrap();
            b_heights
                .get(&height)
                .and_then(|h| b_hashes.get(h).cloned().map(|b| (*h, b)))
        };

        in_mem.or_else(|| {
            self.state_db
                .read()
                .unwrap()
                .get_block_by_height(BlockHeight(height))
                .ok()
                .flatten()
                .map(|b| {
                    let h = tola_consensus::PipelinedBftEngine::compute_block_hash(&b);
                    (h, b)
                })
        })
    }

    fn get_block_by_hash_internal(&self, hash: &Hash) -> Option<Block> {
        let in_mem = self.blocks_by_hash.read().unwrap().get(hash).cloned();
        in_mem.or_else(|| {
            self.state_db
                .read()
                .unwrap()
                .get_block_by_hash(hash)
                .ok()
                .flatten()
        })
    }

    fn collect_logs_for_block(
        &self,
        block: &Block,
        block_hash: Hash,
        criteria: &FilterCriteria,
        logs: &mut Vec<JsonValue>,
    ) -> Result<(), JsonRpcError> {
        let rec_guard = self.receipts.read().unwrap();
        let height = block.header.height.0;
        let mut block_log_index = 0u64;

        for (tx_idx, tx) in block.transactions.iter().enumerate() {
            let tx_id = compute_transaction_id(tx);
            let receipt_opt = rec_guard.get(&tx_id).cloned().or_else(|| {
                self.state_db
                    .read()
                    .unwrap()
                    .get_receipt(&tx_id)
                    .ok()
                    .flatten()
            });

            if let Some(receipt) = receipt_opt {
                let tx_hash_hex = Self::get_transaction_hash_hex(tx, &tx_id);
                for log in &receipt.logs {
                    let current_log_index = block_log_index;
                    block_log_index += 1;

                    if Self::log_matches_criteria(log, criteria) {
                        if logs.len() >= MAX_LOG_RESULTS {
                            return Err(JsonRpcError::server_error(
                                -32005,
                                "query returned more than 10000 logs",
                            ));
                        }
                        logs.push(Self::format_log_json(
                            log,
                            height,
                            block_hash,
                            &tx_hash_hex,
                            tx_idx as u64,
                            current_log_index,
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    fn handle_eth_get_logs(&self, id: Option<JsonValue>, params: &[JsonValue]) -> JsonRpcResponse {
        let current_h = self.current_height.load(Ordering::SeqCst);
        let criteria = match params.get(0) {
            Some(p) => match Self::parse_filter_criteria(p, current_h) {
                Ok(c) => c,
                Err(e) => return JsonRpcResponse::error(id, e),
            },
            None => FilterCriteria::default_range(current_h, current_h),
        };

        if let Some(bh) = criteria.block_hash {
            let block = match self.get_block_by_hash_internal(&bh) {
                Some(b) => b,
                None => {
                    return JsonRpcResponse::error(
                        id,
                        JsonRpcError::server_error(-32000, "unknown block"),
                    );
                }
            };
            let mut logs = Vec::new();
            if let Err(e) = self.collect_logs_for_block(&block, bh, &criteria, &mut logs) {
                return JsonRpcResponse::error(id, e);
            }
            return JsonRpcResponse::success(id, JsonValue::Array(logs));
        }

        let from_h = criteria.from_block.unwrap_or(current_h);
        let to_h = criteria.to_block.unwrap_or(current_h);

        if from_h > to_h {
            return JsonRpcResponse::success(id, JsonValue::Array(vec![]));
        }

        let range = to_h.saturating_sub(from_h).saturating_add(1);
        if range > MAX_LOG_BLOCK_RANGE {
            return JsonRpcResponse::error(
                id,
                JsonRpcError::server_error(-32005, "query exceeds maximum block range of 2000"),
            );
        }

        let mut logs = Vec::new();
        for h in from_h..=to_h {
            if let Some((block_hash, block)) = self.get_block_and_hash_by_height(h) {
                if let Err(e) =
                    self.collect_logs_for_block(&block, block_hash, &criteria, &mut logs)
                {
                    return JsonRpcResponse::error(id, e);
                }
            }
        }

        JsonRpcResponse::success(id, JsonValue::Array(logs))
    }

    fn handle_eth_new_filter(
        &self,
        id: Option<JsonValue>,
        params: &[JsonValue],
    ) -> JsonRpcResponse {
        let current_h = self.current_height.load(Ordering::SeqCst);
        let filter_obj = match params.get(0) {
            Some(obj) => obj,
            None => {
                return JsonRpcResponse::error(
                    id,
                    JsonRpcError::invalid_params("Missing filter parameter object"),
                )
            }
        };

        let criteria = match Self::parse_filter_criteria(filter_obj, current_h) {
            Ok(c) => c,
            Err(e) => return JsonRpcResponse::error(id, e),
        };

        if criteria.block_hash.is_some() {
            return JsonRpcResponse::error(
                id,
                JsonRpcError::invalid_params("blockHash is not supported for eth_newFilter"),
            );
        }

        self.prune_expired_filters();
        let mut filters = self.filters.write().unwrap();
        if filters.len() >= MAX_ACTIVE_FILTERS {
            return JsonRpcResponse::error(
                id,
                JsonRpcError::server_error(-32000, "filter limit reached"),
            );
        }

        let last_polled_height = match criteria.from_block {
            Some(fb) => fb.saturating_sub(1),
            None => current_h,
        };

        let filter_id = self.next_filter_id.fetch_add(1, Ordering::SeqCst);
        let log_filter = LogFilter {
            id: filter_id,
            criteria,
            last_polled_height,
            last_active: std::time::Instant::now(),
        };

        filters.insert(filter_id, log_filter);
        JsonRpcResponse::success(id, JsonValue::String(format!("0x{:x}", filter_id)))
    }

    fn handle_eth_get_filter_changes(
        &self,
        id: Option<JsonValue>,
        params: &[JsonValue],
    ) -> JsonRpcResponse {
        let filter_id = match Self::parse_filter_id(params.get(0)) {
            Ok(fid) => fid,
            Err(e) => return JsonRpcResponse::error(id, e),
        };

        self.prune_expired_filters();

        let mut filters = self.filters.write().unwrap();
        let filter = match filters.get_mut(&filter_id) {
            Some(f) => f,
            None => {
                return JsonRpcResponse::error(
                    id,
                    JsonRpcError::server_error(-32000, "filter not found"),
                );
            }
        };

        filter.last_active = std::time::Instant::now();
        let current_h = self.current_height.load(Ordering::SeqCst);
        let start_h = filter.last_polled_height.saturating_add(1);
        let end_h = match filter.criteria.to_block {
            Some(tb) => tb.min(current_h),
            None => current_h,
        };

        if start_h > end_h {
            return JsonRpcResponse::success(id, JsonValue::Array(vec![]));
        }

        let range = end_h.saturating_sub(start_h).saturating_add(1);
        if range > MAX_LOG_BLOCK_RANGE {
            return JsonRpcResponse::error(
                id,
                JsonRpcError::server_error(-32005, "query exceeds maximum block range of 2000"),
            );
        }

        let mut logs = Vec::new();
        for h in start_h..=end_h {
            if let Some((block_hash, block)) = self.get_block_and_hash_by_height(h) {
                if let Err(e) =
                    self.collect_logs_for_block(&block, block_hash, &filter.criteria, &mut logs)
                {
                    return JsonRpcResponse::error(id, e);
                }
            }
        }

        filter.last_polled_height = end_h;
        JsonRpcResponse::success(id, JsonValue::Array(logs))
    }

    fn handle_eth_get_filter_logs(
        &self,
        id: Option<JsonValue>,
        params: &[JsonValue],
    ) -> JsonRpcResponse {
        let filter_id = match Self::parse_filter_id(params.get(0)) {
            Ok(fid) => fid,
            Err(e) => return JsonRpcResponse::error(id, e),
        };

        self.prune_expired_filters();

        let criteria = {
            let filters = self.filters.read().unwrap();
            match filters.get(&filter_id) {
                Some(f) => f.criteria.clone(),
                None => {
                    return JsonRpcResponse::error(
                        id,
                        JsonRpcError::server_error(-32000, "filter not found"),
                    );
                }
            }
        };

        let current_h = self.current_height.load(Ordering::SeqCst);
        let from_h = criteria.from_block.unwrap_or(0);
        let to_h = criteria.to_block.unwrap_or(current_h).min(current_h);

        if from_h > to_h {
            return JsonRpcResponse::success(id, JsonValue::Array(vec![]));
        }

        let range = to_h.saturating_sub(from_h).saturating_add(1);
        if range > MAX_LOG_BLOCK_RANGE {
            return JsonRpcResponse::error(
                id,
                JsonRpcError::server_error(-32005, "query exceeds maximum block range of 2000"),
            );
        }

        let mut logs = Vec::new();
        for h in from_h..=to_h {
            if let Some((block_hash, block)) = self.get_block_and_hash_by_height(h) {
                if let Err(e) =
                    self.collect_logs_for_block(&block, block_hash, &criteria, &mut logs)
                {
                    return JsonRpcResponse::error(id, e);
                }
            }
        }

        JsonRpcResponse::success(id, JsonValue::Array(logs))
    }

    fn handle_eth_uninstall_filter(
        &self,
        id: Option<JsonValue>,
        params: &[JsonValue],
    ) -> JsonRpcResponse {
        let filter_id = match Self::parse_filter_id(params.get(0)) {
            Ok(fid) => fid,
            Err(e) => return JsonRpcResponse::error(id, e),
        };

        let mut filters = self.filters.write().unwrap();
        let removed = filters.remove(&filter_id).is_some();
        JsonRpcResponse::success(id, JsonValue::Bool(removed))
    }

    fn get_block_by_param_tag(
        &self,
        param: Option<&JsonValue>,
    ) -> Result<Option<(Hash, Block)>, JsonRpcError> {
        let cur_h = self.current_height.load(Ordering::SeqCst);
        let height = match param {
            Some(v) => Self::parse_block_tag_or_number(v, cur_h)?,
            None => cur_h,
        };
        Ok(self.get_block_and_hash_by_height(height))
    }

    fn get_block_by_param_hash(
        &self,
        param: Option<&JsonValue>,
    ) -> Result<Option<(Hash, Block)>, JsonRpcError> {
        let hash_str = match param.and_then(|p| p.as_str()) {
            Some(s) => s,
            None => return Err(JsonRpcError::invalid_params("Missing block hash")),
        };
        let hash = match Hash::from_hex(hash_str) {
            Ok(h) => h,
            Err(e) => return Err(JsonRpcError::invalid_params(&e)),
        };
        Ok(self.get_block_by_hash_internal(&hash).map(|b| (hash, b)))
    }

    fn parse_tx_index(param: Option<&JsonValue>) -> Result<usize, JsonRpcError> {
        let val = param
            .ok_or_else(|| JsonRpcError::invalid_params("Missing transaction index parameter"))?;
        match val {
            JsonValue::String(s) => {
                if s.starts_with("0x") || s.starts_with("0X") {
                    usize::from_str_radix(&s[2..], 16).map_err(|e| {
                        JsonRpcError::invalid_params(&format!(
                            "Invalid hex transaction index: {}",
                            e
                        ))
                    })
                } else {
                    s.parse::<usize>().map_err(|e| {
                        JsonRpcError::invalid_params(&format!(
                            "Invalid decimal transaction index: {}",
                            e
                        ))
                    })
                }
            }
            JsonValue::Integer(i) => {
                if *i >= 0 {
                    Ok(*i as usize)
                } else {
                    Err(JsonRpcError::invalid_params("Negative transaction index"))
                }
            }
            JsonValue::Number(n) => {
                if *n >= 0.0 {
                    Ok(*n as usize)
                } else {
                    Err(JsonRpcError::invalid_params("Negative transaction index"))
                }
            }
            _ => Err(JsonRpcError::invalid_params(
                "Transaction index must be a string or integer",
            )),
        }
    }

    fn handle_eth_get_block_transaction_count_by_number(
        &self,
        id: Option<JsonValue>,
        params: &[JsonValue],
    ) -> JsonRpcResponse {
        match self.get_block_by_param_tag(params.get(0)) {
            Ok(Some((_hash, block))) => JsonRpcResponse::success(
                id,
                JsonValue::String(format!("0x{:x}", block.transactions.len())),
            ),
            Ok(None) => JsonRpcResponse::success(id, JsonValue::Null),
            Err(e) => JsonRpcResponse::error(id, e),
        }
    }

    fn handle_eth_get_block_transaction_count_by_hash(
        &self,
        id: Option<JsonValue>,
        params: &[JsonValue],
    ) -> JsonRpcResponse {
        match self.get_block_by_param_hash(params.get(0)) {
            Ok(Some((_hash, block))) => JsonRpcResponse::success(
                id,
                JsonValue::String(format!("0x{:x}", block.transactions.len())),
            ),
            Ok(None) => JsonRpcResponse::success(id, JsonValue::Null),
            Err(e) => JsonRpcResponse::error(id, e),
        }
    }

    fn handle_eth_get_transaction_by_block_number_and_index(
        &self,
        id: Option<JsonValue>,
        params: &[JsonValue],
    ) -> JsonRpcResponse {
        let (block_hash, block) = match self.get_block_by_param_tag(params.get(0)) {
            Ok(Some(res)) => res,
            Ok(None) => return JsonRpcResponse::success(id, JsonValue::Null),
            Err(e) => return JsonRpcResponse::error(id, e),
        };

        let index = match Self::parse_tx_index(params.get(1)) {
            Ok(idx) => idx,
            Err(e) => return JsonRpcResponse::error(id, e),
        };

        let tx = match block.transactions.get(index) {
            Some(t) => t,
            None => return JsonRpcResponse::success(id, JsonValue::Null),
        };

        let tx_id = compute_transaction_id(tx);
        JsonRpcResponse::success(
            id,
            format_transaction_json(tx, tx_id, block_hash, block.header.height.0, index as u64),
        )
    }

    fn handle_eth_get_transaction_by_block_hash_and_index(
        &self,
        id: Option<JsonValue>,
        params: &[JsonValue],
    ) -> JsonRpcResponse {
        let (block_hash, block) = match self.get_block_by_param_hash(params.get(0)) {
            Ok(Some(res)) => res,
            Ok(None) => return JsonRpcResponse::success(id, JsonValue::Null),
            Err(e) => return JsonRpcResponse::error(id, e),
        };

        let index = match Self::parse_tx_index(params.get(1)) {
            Ok(idx) => idx,
            Err(e) => return JsonRpcResponse::error(id, e),
        };

        let tx = match block.transactions.get(index) {
            Some(t) => t,
            None => return JsonRpcResponse::success(id, JsonValue::Null),
        };

        let tx_id = compute_transaction_id(tx);
        JsonRpcResponse::success(
            id,
            format_transaction_json(tx, tx_id, block_hash, block.header.height.0, index as u64),
        )
    }

    fn handle_eth_get_uncle_count_by_block_hash(
        &self,
        id: Option<JsonValue>,
        params: &[JsonValue],
    ) -> JsonRpcResponse {
        match self.get_block_by_param_hash(params.get(0)) {
            Ok(Some(_)) => JsonRpcResponse::success(id, JsonValue::String("0x0".into())),
            Ok(None) => JsonRpcResponse::success(id, JsonValue::Null),
            Err(e) => JsonRpcResponse::error(id, e),
        }
    }

    fn handle_eth_get_uncle_count_by_block_number(
        &self,
        id: Option<JsonValue>,
        params: &[JsonValue],
    ) -> JsonRpcResponse {
        match self.get_block_by_param_tag(params.get(0)) {
            Ok(Some(_)) => JsonRpcResponse::success(id, JsonValue::String("0x0".into())),
            Ok(None) => JsonRpcResponse::success(id, JsonValue::Null),
            Err(e) => JsonRpcResponse::error(id, e),
        }
    }

    fn handle_eth_get_uncle_by_block_hash_and_index(
        &self,
        id: Option<JsonValue>,
        _params: &[JsonValue],
    ) -> JsonRpcResponse {
        JsonRpcResponse::success(id, JsonValue::Null)
    }

    fn handle_eth_get_uncle_by_block_number_and_index(
        &self,
        id: Option<JsonValue>,
        _params: &[JsonValue],
    ) -> JsonRpcResponse {
        JsonRpcResponse::success(id, JsonValue::Null)
    }

    fn handle_tola_get_validators(&self, id: Option<JsonValue>) -> JsonRpcResponse {
        let comm_guard = self.active_committee.read().unwrap();
        let set = match *comm_guard {
            Some(ref s) => s.clone(),
            None => {
                return JsonRpcResponse::success(id, JsonValue::Array(vec![]));
            }
        };

        let mut val_arr = Vec::new();
        for val in set.validators {
            let pk_hex = match val.consensus_pubkey {
                PublicKey::Ed25519(bytes) => format!("0x{}", hex_encode(&bytes)),
                _ => "0x".into(),
            };

            let obj = JsonValue::Object(vec![
                (
                    "validator_id".to_string(),
                    JsonValue::String(val.validator_id.0.to_hex()),
                ),
                ("consensus_pubkey".to_string(), JsonValue::String(pk_hex)),
                (
                    "voting_power".to_string(),
                    JsonValue::Integer(val.voting_power as i64),
                ),
                (
                    "commission_bp".to_string(),
                    JsonValue::Integer(val.commission_bp as i64),
                ),
            ]);
            val_arr.push(obj);
        }

        JsonRpcResponse::success(id, JsonValue::Array(val_arr))
    }

    fn handle_tola_get_quorum_certificate(
        &self,
        id: Option<JsonValue>,
        params: &[JsonValue],
    ) -> JsonRpcResponse {
        let height = match params.get(0) {
            Some(JsonValue::String(s)) => {
                if s.starts_with("0x") || s.starts_with("0X") {
                    u64::from_str_radix(&s[2..], 16).unwrap_or(0)
                } else {
                    s.parse::<u64>().unwrap_or(0)
                }
            }
            Some(p) if p.as_u64().is_some() => p.as_u64().unwrap(),
            _ => {
                return JsonRpcResponse::error(
                    id,
                    JsonRpcError::invalid_params("Missing height parameter"),
                )
            }
        };

        let b_heights = self.blocks_by_height.read().unwrap();
        let block_hash = match b_heights.get(&height) {
            Some(h) => *h,
            None => return JsonRpcResponse::success(id, JsonValue::Null),
        };

        let b_hashes = self.blocks_by_hash.read().unwrap();
        let block = match b_hashes.get(&block_hash) {
            Some(b) => b,
            None => return JsonRpcResponse::success(id, JsonValue::Null),
        };

        match &block.last_commit_qc {
            Some(qc) => {
                let obj = JsonValue::Object(vec![
                    (
                        "height".to_string(),
                        JsonValue::String(format!("0x{:x}", qc.height.0)),
                    ),
                    (
                        "round".to_string(),
                        JsonValue::String(format!("0x{:x}", qc.round.0)),
                    ),
                    (
                        "block_hash".to_string(),
                        JsonValue::String(qc.block_hash.to_hex()),
                    ),
                    (
                        "validator_bitfield".to_string(),
                        JsonValue::String(format!("0x{}", hex_encode(&qc.validator_bitfield))),
                    ),
                    (
                        "aggregated_signature".to_string(),
                        JsonValue::String(format!("0x{}", hex_encode(&qc.aggregated_signature))),
                    ),
                ]);
                JsonRpcResponse::success(id, obj)
            }
            None => JsonRpcResponse::success(id, JsonValue::Null),
        }
    }

    fn handle_tola_get_external_asset(
        &self,
        id: Option<JsonValue>,
        params: &[JsonValue],
    ) -> JsonRpcResponse {
        let param = match params.get(0) {
            Some(p) => p,
            None => {
                return JsonRpcResponse::error(
                    id,
                    JsonRpcError::invalid_params("Expected tokenId or canonicalId parameter"),
                )
            }
        };

        let reg = self.external_asset_registry.read().unwrap();
        let by_canon = self.external_asset_by_canonical_id.read().unwrap();

        let asset = match param {
            JsonValue::Integer(i) => reg.get(&(*i as u64)).cloned(),
            JsonValue::Number(n) => reg.get(&(*n as u64)).cloned(),
            JsonValue::String(s) => {
                let clean = s
                    .strip_prefix("0x")
                    .or_else(|| s.strip_prefix("0X"))
                    .unwrap_or(s);
                if clean.len() == 64 {
                    if let Ok(bytes) = hex_to_bytes(clean) {
                        let mut canon = [0u8; 32];
                        canon.copy_from_slice(&bytes[0..32]);
                        by_canon.get(&canon).and_then(|t| reg.get(t)).cloned()
                    } else {
                        None
                    }
                } else if let Ok(t) = s.parse::<u64>() {
                    reg.get(&t).cloned()
                } else {
                    None
                }
            }
            _ => None,
        };

        // If not found in cache, read directly from authoritative StateDB
        let asset = if asset.is_some() {
            asset
        } else {
            let state = self.state_db.read().unwrap();
            match param {
                JsonValue::Integer(i) => {
                    tola_execution::ExternalAssetStorage::read_asset(&*state, *i as u64)
                }
                JsonValue::Number(n) => {
                    tola_execution::ExternalAssetStorage::read_asset(&*state, *n as u64)
                }
                JsonValue::String(s) => {
                    let clean = s
                        .strip_prefix("0x")
                        .or_else(|| s.strip_prefix("0X"))
                        .unwrap_or(s);
                    if clean.len() == 64 {
                        if let Ok(bytes) = hex_to_bytes(clean) {
                            let mut canon = [0u8; 32];
                            canon.copy_from_slice(&bytes[0..32]);
                            tola_execution::ExternalAssetStorage::read_token_by_canonical_id(
                                &*state, &canon,
                            )
                            .and_then(|t| {
                                tola_execution::ExternalAssetStorage::read_asset(&*state, t)
                            })
                        } else {
                            None
                        }
                    } else if let Ok(t) = s.parse::<u64>() {
                        tola_execution::ExternalAssetStorage::read_asset(&*state, t)
                    } else {
                        None
                    }
                }
                _ => None,
            }
        };

        match asset {
            Some(entry) => {
                let obj = JsonValue::Object(vec![
                    (
                        "tokenId".to_string(),
                        JsonValue::Integer(entry.token_id as i64),
                    ),
                    (
                        "canonicalId".to_string(),
                        JsonValue::String(format!("0x{}", hex_encode(&entry.canonical_id))),
                    ),
                    (
                        "sourcePlatform".to_string(),
                        JsonValue::String(entry.source_platform),
                    ),
                    (
                        "assetType".to_string(),
                        JsonValue::String(entry.asset_type.as_str().to_string()),
                    ),
                    (
                        "externalAssetId".to_string(),
                        JsonValue::String(entry.external_asset_id),
                    ),
                    (
                        "externalReference".to_string(),
                        JsonValue::String(entry.external_reference),
                    ),
                    (
                        "metadataUri".to_string(),
                        JsonValue::String(entry.metadata_uri),
                    ),
                    ("owner".to_string(), JsonValue::String(entry.owner.to_hex())),
                    (
                        "authorizer".to_string(),
                        JsonValue::String(entry.authorizer.to_hex()),
                    ),
                    (
                        "verifierAddress".to_string(),
                        JsonValue::String(entry.verifier_address.to_hex()),
                    ),
                    (
                        "mintedAt".to_string(),
                        JsonValue::Integer(entry.minted_at as i64),
                    ),
                ]);
                JsonRpcResponse::success(id, obj)
            }
            None => JsonRpcResponse::success(id, JsonValue::Null),
        }
    }

    fn handle_tola_get_external_asset_owner(
        &self,
        id: Option<JsonValue>,
        params: &[JsonValue],
    ) -> JsonRpcResponse {
        let res = self.handle_tola_get_external_asset(id.clone(), params);
        match res.result {
            Some(JsonValue::Object(ref fields)) => {
                let owner_val = fields
                    .iter()
                    .find(|(k, _)| k == "owner")
                    .map(|(_, v)| v.clone())
                    .unwrap_or(JsonValue::Null);
                JsonRpcResponse::success(id, owner_val)
            }
            _ => JsonRpcResponse::success(id, JsonValue::Null),
        }
    }

    fn handle_tola_get_external_asset_metadata(
        &self,
        id: Option<JsonValue>,
        params: &[JsonValue],
    ) -> JsonRpcResponse {
        let res = self.handle_tola_get_external_asset(id.clone(), params);
        match res.result {
            Some(JsonValue::Object(ref fields)) => {
                let mut meta_fields = Vec::new();
                for (k, v) in fields {
                    if matches!(
                        k.as_str(),
                        "metadataUri"
                            | "externalReference"
                            | "sourcePlatform"
                            | "assetType"
                            | "externalAssetId"
                    ) {
                        meta_fields.push((k.clone(), v.clone()));
                    }
                }
                JsonRpcResponse::success(id, JsonValue::Object(meta_fields))
            }
            _ => JsonRpcResponse::success(id, JsonValue::Null),
        }
    }

    fn handle_tola_verify_external_asset_attestation(
        &self,
        id: Option<JsonValue>,
        params: &[JsonValue],
    ) -> JsonRpcResponse {
        let obj = match params.get(0) {
            Some(o) => o,
            None => {
                return JsonRpcResponse::error(
                    id,
                    JsonRpcError::invalid_params("Missing attestation object parameter"),
                )
            }
        };

        let chain_id = match obj.get("chainId").and_then(|v| v.as_u64()) {
            Some(c) => c,
            None => {
                return JsonRpcResponse::error(id, JsonRpcError::invalid_params("Missing chainId"))
            }
        };

        let nonce = match obj.get("nonce").and_then(|v| v.as_u64()) {
            Some(n) => n,
            None => {
                return JsonRpcResponse::error(id, JsonRpcError::invalid_params("Missing nonce"))
            }
        };

        let dest_addr = match obj.get("destinationWallet").map(Self::parse_address) {
            Some(Ok(a)) => a,
            Some(Err(e)) => return JsonRpcResponse::error(id, e),
            None => {
                return JsonRpcResponse::error(
                    id,
                    JsonRpcError::invalid_params("Missing destinationWallet"),
                )
            }
        };

        let expires_at = match obj.get("expiresAt").and_then(|v| v.as_u64()) {
            Some(e) => e,
            None => {
                return JsonRpcResponse::error(
                    id,
                    JsonRpcError::invalid_params("Missing expiresAt"),
                )
            }
        };

        let source_platform = match obj.get("sourcePlatform").and_then(|v| v.as_str()) {
            Some(s) if !s.trim().is_empty() => s,
            _ => {
                return JsonRpcResponse::error(
                    id,
                    JsonRpcError::invalid_params("Missing or empty sourcePlatform"),
                )
            }
        };

        let asset_type = match obj.get("assetType") {
            Some(JsonValue::String(s)) => match s.parse::<ExternalAssetType>() {
                Ok(t) => t,
                Err(_) => {
                    return JsonRpcResponse::error(
                        id,
                        JsonRpcError::invalid_params("Invalid assetType string"),
                    )
                }
            },
            Some(JsonValue::Integer(i)) => match ExternalAssetType::from_u8(*i as u8) {
                Some(t) => t,
                None => {
                    return JsonRpcResponse::error(
                        id,
                        JsonRpcError::invalid_params("Invalid assetType number"),
                    )
                }
            },
            _ => {
                return JsonRpcResponse::error(
                    id,
                    JsonRpcError::invalid_params("Missing assetType"),
                )
            }
        };

        let external_asset_id = match obj.get("externalAssetId").and_then(|v| v.as_str()) {
            Some(s) if !s.trim().is_empty() => s,
            _ => {
                return JsonRpcResponse::error(
                    id,
                    JsonRpcError::invalid_params("Missing or empty externalAssetId"),
                )
            }
        };

        let verifier_addr = match obj
            .get("authorizer")
            .or_else(|| obj.get("authorizerAddress"))
            .or_else(|| obj.get("verifierAddress"))
            .map(Self::parse_address)
        {
            Some(Ok(a)) => a,
            Some(Err(e)) => return JsonRpcResponse::error(id, e),
            None => {
                return JsonRpcResponse::error(
                    id,
                    JsonRpcError::invalid_params("Missing authorizer or verifierAddress"),
                )
            }
        };

        let sig_obj = match obj.get("signature") {
            Some(s) => s,
            None => {
                return JsonRpcResponse::error(
                    id,
                    JsonRpcError::invalid_params("Missing signature object"),
                )
            }
        };

        let r_hex = match sig_obj.get("r").and_then(|v| v.as_str()) {
            Some(r) => r,
            None => {
                return JsonRpcResponse::error(
                    id,
                    JsonRpcError::invalid_params("Missing r in signature"),
                )
            }
        };
        let s_hex = match sig_obj.get("s").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => {
                return JsonRpcResponse::error(
                    id,
                    JsonRpcError::invalid_params("Missing s in signature"),
                )
            }
        };
        let v = match sig_obj.get("v").and_then(|v| v.as_u64()) {
            Some(v) => v as u8,
            None => {
                return JsonRpcResponse::error(
                    id,
                    JsonRpcError::invalid_params("Missing v in signature"),
                )
            }
        };

        let r_clean = r_hex
            .strip_prefix("0x")
            .or_else(|| r_hex.strip_prefix("0X"))
            .unwrap_or(r_hex);
        let s_clean = s_hex
            .strip_prefix("0x")
            .or_else(|| s_hex.strip_prefix("0X"))
            .unwrap_or(s_hex);

        let r_bytes = match hex_to_bytes(r_clean) {
            Ok(b) if b.len() == 32 => b,
            _ => {
                return JsonRpcResponse::error(id, JsonRpcError::invalid_params("Invalid r bytes"))
            }
        };
        let s_bytes = match hex_to_bytes(s_clean) {
            Ok(b) if b.len() == 32 => b,
            _ => {
                return JsonRpcResponse::error(id, JsonRpcError::invalid_params("Invalid s bytes"))
            }
        };

        let recid = match v {
            0 | 1 => v,
            27 | 28 => v - 27,
            _ => {
                return JsonRpcResponse::error(
                    id,
                    JsonRpcError::invalid_params("Invalid recovery id v"),
                )
            }
        };

        let mut sig_65 = [0u8; 65];
        sig_65[0..32].copy_from_slice(&r_bytes);
        sig_65[32..64].copy_from_slice(&s_bytes);
        sig_65[64] = recid;

        let canonical_id = tola_crypto::compute_external_asset_canonical_id(
            source_platform,
            asset_type,
            external_asset_id,
        );

        let already_minted = self
            .external_asset_by_canonical_id
            .read()
            .unwrap()
            .get(&canonical_id)
            .copied()
            .or_else(|| {
                let state = self.state_db.read().unwrap();
                tola_execution::ExternalAssetStorage::read_token_by_canonical_id(
                    &*state,
                    &canonical_id,
                )
            });

        let digest = tola_crypto::compute_external_asset_attestation_digest(
            chain_id,
            nonce,
            &dest_addr,
            expires_at,
            source_platform,
            asset_type,
            external_asset_id,
        );

        let is_used = {
            let state = self.state_db.read().unwrap();
            tola_execution::ExternalAssetStorage::is_attestation_used(&*state, &digest)
        };

        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        if is_used {
            let resp_obj = JsonValue::Object(vec![
                ("valid".to_string(), JsonValue::Bool(false)),
                (
                    "reason".to_string(),
                    JsonValue::String("Attestation already used".to_string()),
                ),
                (
                    "canonicalId".to_string(),
                    JsonValue::String(format!("0x{}", hex_encode(&canonical_id))),
                ),
                (
                    "sourcePlatform".to_string(),
                    JsonValue::String(source_platform.to_string()),
                ),
                (
                    "assetType".to_string(),
                    JsonValue::String(asset_type.as_str().to_string()),
                ),
                (
                    "externalAssetId".to_string(),
                    JsonValue::String(external_asset_id.to_string()),
                ),
            ]);
            return JsonRpcResponse::success(id, resp_obj);
        }

        match tola_crypto::verify_external_asset_attestation(
            chain_id,
            nonce,
            &dest_addr,
            expires_at,
            source_platform,
            asset_type,
            external_asset_id,
            &verifier_addr,
            &sig_65,
            current_time,
        ) {
            Ok(valid) => {
                let resp_obj = JsonValue::Object(vec![
                    ("valid".to_string(), JsonValue::Bool(valid)),
                    (
                        "canonicalId".to_string(),
                        JsonValue::String(format!("0x{}", hex_encode(&canonical_id))),
                    ),
                    (
                        "sourcePlatform".to_string(),
                        JsonValue::String(source_platform.to_string()),
                    ),
                    (
                        "assetType".to_string(),
                        JsonValue::String(asset_type.as_str().to_string()),
                    ),
                    (
                        "externalAssetId".to_string(),
                        JsonValue::String(external_asset_id.to_string()),
                    ),
                    (
                        "destinationWallet".to_string(),
                        JsonValue::String(dest_addr.to_hex()),
                    ),
                    (
                        "authorizer".to_string(),
                        JsonValue::String(verifier_addr.to_hex()),
                    ),
                    (
                        "verifierAddress".to_string(),
                        JsonValue::String(verifier_addr.to_hex()),
                    ),
                    (
                        "alreadyMintedTokenId".to_string(),
                        match already_minted {
                            Some(t) => JsonValue::Integer(t as i64),
                            None => JsonValue::Null,
                        },
                    ),
                ]);
                JsonRpcResponse::success(id, resp_obj)
            }
            Err(e) => {
                let resp_obj = JsonValue::Object(vec![
                    ("valid".to_string(), JsonValue::Bool(false)),
                    ("reason".to_string(), JsonValue::String(format!("{:?}", e))),
                    (
                        "canonicalId".to_string(),
                        JsonValue::String(format!("0x{}", hex_encode(&canonical_id))),
                    ),
                    (
                        "sourcePlatform".to_string(),
                        JsonValue::String(source_platform.to_string()),
                    ),
                    (
                        "assetType".to_string(),
                        JsonValue::String(asset_type.as_str().to_string()),
                    ),
                    (
                        "externalAssetId".to_string(),
                        JsonValue::String(external_asset_id.to_string()),
                    ),
                ]);
                JsonRpcResponse::success(id, resp_obj)
            }
        }
    }

    fn handle_tola_get_canonical_id(
        &self,
        id: Option<JsonValue>,
        params: &[JsonValue],
    ) -> JsonRpcResponse {
        let (source_platform, asset_type, external_asset_id) =
            if let Some(JsonValue::Object(ref fields)) = params.get(0) {
                let platform = fields
                    .iter()
                    .find(|(k, _)| k == "sourcePlatform")
                    .and_then(|(_, v)| v.as_str())
                    .unwrap_or("");
                let a_type_str = fields
                    .iter()
                    .find(|(k, _)| k == "assetType")
                    .and_then(|(_, v)| v.as_str())
                    .unwrap_or("");
                let a_id = fields
                    .iter()
                    .find(|(k, _)| k == "externalAssetId")
                    .and_then(|(_, v)| v.as_str())
                    .unwrap_or("");
                let a_type = match a_type_str.parse::<ExternalAssetType>() {
                    Ok(t) => t,
                    Err(_) => {
                        return JsonRpcResponse::error(
                            id,
                            JsonRpcError::invalid_params("Invalid assetType"),
                        )
                    }
                };
                (platform.to_string(), a_type, a_id.to_string())
            } else if params.len() >= 3 {
                let platform = match params[0].as_str() {
                    Some(p) => p.to_string(),
                    None => {
                        return JsonRpcResponse::error(
                            id,
                            JsonRpcError::invalid_params("Invalid platform string"),
                        )
                    }
                };
                let a_type = match params[1]
                    .as_str()
                    .and_then(|s| s.parse::<ExternalAssetType>().ok())
                {
                    Some(t) => t,
                    None => {
                        return JsonRpcResponse::error(
                            id,
                            JsonRpcError::invalid_params("Invalid assetType"),
                        )
                    }
                };
                let a_id = match params[2].as_str() {
                    Some(i) => i.to_string(),
                    None => {
                        return JsonRpcResponse::error(
                            id,
                            JsonRpcError::invalid_params("Invalid externalAssetId"),
                        )
                    }
                };
                (platform, a_type, a_id)
            } else {
                return JsonRpcResponse::error(
                    id,
                    JsonRpcError::invalid_params(
                        "Expected sourcePlatform, assetType, externalAssetId",
                    ),
                );
            };

        let canon = tola_crypto::compute_external_asset_canonical_id(
            &source_platform,
            asset_type,
            &external_asset_id,
        );
        JsonRpcResponse::success(id, JsonValue::String(format!("0x{}", hex_encode(&canon))))
    }

    fn handle_tola_get_governance_params(&self, id: Option<JsonValue>) -> JsonRpcResponse {
        let gov = self.governance.read().unwrap();
        let p = gov.active_params();
        let c = gov.config();

        let obj = JsonValue::Object(vec![
            (
                "max_block_gas_limit".to_string(),
                JsonValue::String(format!("0x{:x}", p.max_block_gas_limit.as_u64())),
            ),
            (
                "target_base_fee".to_string(),
                JsonValue::String(format!("0x{:x}", p.target_base_fee.0)),
            ),
            (
                "min_validator_self_stake".to_string(),
                JsonValue::String(format!("0x{:x}", p.min_validator_self_stake.0)),
            ),
            (
                "unbonding_period_blocks".to_string(),
                JsonValue::Integer(p.unbonding_period_blocks as i64),
            ),
            (
                "slashing_penalty_bp".to_string(),
                JsonValue::Integer(p.slashing_penalty_bp as i64),
            ),
            (
                "quorum_bp".to_string(),
                JsonValue::Integer(c.quorum_bp as i64),
            ),
            (
                "approval_threshold_bp".to_string(),
                JsonValue::Integer(c.approval_threshold_bp as i64),
            ),
            (
                "timelock_delay_blocks".to_string(),
                JsonValue::Integer(c.timelock_delay_blocks as i64),
            ),
            (
                "voting_period_blocks".to_string(),
                JsonValue::Integer(c.voting_period_blocks as i64),
            ),
            (
                "min_proposal_deposit".to_string(),
                JsonValue::String(format!("0x{:x}", c.min_proposal_deposit.0)),
            ),
        ]);

        JsonRpcResponse::success(id, obj)
    }

    fn format_proposal_json(p: &tola_consensus::governance::Proposal) -> JsonValue {
        let status_str = match &p.status {
            tola_consensus::governance::ProposalStatus::DepositPending => {
                "DepositPending".to_string()
            }
            tola_consensus::governance::ProposalStatus::VotingActive => "VotingActive".to_string(),
            tola_consensus::governance::ProposalStatus::Passed => "Passed".to_string(),
            tola_consensus::governance::ProposalStatus::Rejected { reason } => {
                format!("Rejected: {}", reason)
            }
            tola_consensus::governance::ProposalStatus::Vetoed => "Vetoed".to_string(),
            tola_consensus::governance::ProposalStatus::Timelocked { execution_height } => {
                format!("Timelocked@{}", execution_height.0)
            }
            tola_consensus::governance::ProposalStatus::Executed => "Executed".to_string(),
        };

        let mut changes_arr = Vec::new();
        for ch in &p.changes {
            let (param_name, val_str) = match ch {
                tola_consensus::governance::ParamChange::MaxBlockGasLimit(g) => {
                    ("max_block_gas_limit", format!("0x{:x}", g.as_u64()))
                }
                tola_consensus::governance::ParamChange::TargetBaseFee(f) => {
                    ("target_base_fee", format!("0x{:x}", f.0))
                }
                tola_consensus::governance::ParamChange::MinValidatorSelfStake(s) => {
                    ("min_validator_self_stake", format!("0x{:x}", s.0))
                }
                tola_consensus::governance::ParamChange::UnbondingPeriodBlocks(b) => {
                    ("unbonding_period_blocks", b.to_string())
                }
                tola_consensus::governance::ParamChange::SlashingPenaltyBp(bp) => {
                    ("slashing_penalty_bp", bp.to_string())
                }
            };
            changes_arr.push(JsonValue::Object(vec![
                (
                    "param".to_string(),
                    JsonValue::String(param_name.to_string()),
                ),
                ("value".to_string(), JsonValue::String(val_str)),
            ]));
        }

        JsonValue::Object(vec![
            ("id".to_string(), JsonValue::Integer(p.id as i64)),
            (
                "proposer".to_string(),
                JsonValue::String(p.proposer.to_hex()),
            ),
            ("title".to_string(), JsonValue::String(p.title.clone())),
            (
                "description".to_string(),
                JsonValue::String(p.description.clone()),
            ),
            (
                "deposit".to_string(),
                JsonValue::String(format!("0x{:x}", p.deposit.0)),
            ),
            (
                "min_deposit".to_string(),
                JsonValue::String(format!("0x{:x}", p.min_deposit.0)),
            ),
            (
                "start_block".to_string(),
                JsonValue::Integer(p.start_block.0 as i64),
            ),
            (
                "end_block".to_string(),
                JsonValue::Integer(p.end_block.0 as i64),
            ),
            ("status".to_string(), JsonValue::String(status_str)),
            ("changes".to_string(), JsonValue::Array(changes_arr)),
            (
                "vote_count".to_string(),
                JsonValue::Integer(p.votes.len() as i64),
            ),
        ])
    }

    fn handle_tola_get_proposal(
        &self,
        id: Option<JsonValue>,
        params: &[JsonValue],
    ) -> JsonRpcResponse {
        let proposal_id = match params.get(0) {
            Some(JsonValue::Integer(i)) => *i as u64,
            Some(JsonValue::String(s)) => {
                if s.starts_with("0x") || s.starts_with("0X") {
                    u64::from_str_radix(&s[2..], 16).unwrap_or(0)
                } else {
                    s.parse::<u64>().unwrap_or(0)
                }
            }
            _ => {
                return JsonRpcResponse::error(
                    id,
                    JsonRpcError::invalid_params("Missing or invalid proposal id"),
                )
            }
        };

        let gov = self.governance.read().unwrap();
        match gov.get_proposal(proposal_id) {
            Some(prop) => JsonRpcResponse::success(id, Self::format_proposal_json(prop)),
            None => JsonRpcResponse::success(id, JsonValue::Null),
        }
    }

    fn handle_tola_list_proposals(&self, id: Option<JsonValue>) -> JsonRpcResponse {
        let gov = self.governance.read().unwrap();
        let list = gov.list_proposals();
        let arr: Vec<JsonValue> = list.iter().map(Self::format_proposal_json).collect();
        JsonRpcResponse::success(id, JsonValue::Array(arr))
    }

    fn parse_param_change(obj: &JsonValue) -> Option<tola_consensus::governance::ParamChange> {
        let param_name = obj.get("param")?.as_str()?;
        let val_str = obj.get("value")?.as_str()?;
        let val_num = if val_str.starts_with("0x") || val_str.starts_with("0X") {
            u128::from_str_radix(&val_str[2..], 16).ok()?
        } else {
            val_str.parse::<u128>().ok()?
        };

        match param_name {
            "max_block_gas_limit" => Some(
                tola_consensus::governance::ParamChange::MaxBlockGasLimit(Gas(val_num as u64)),
            ),
            "target_base_fee" => Some(tola_consensus::governance::ParamChange::TargetBaseFee(
                Amount(val_num),
            )),
            "min_validator_self_stake" => Some(
                tola_consensus::governance::ParamChange::MinValidatorSelfStake(Amount(val_num)),
            ),
            "unbonding_period_blocks" => {
                Some(tola_consensus::governance::ParamChange::UnbondingPeriodBlocks(val_num as u64))
            }
            "slashing_penalty_bp" => Some(
                tola_consensus::governance::ParamChange::SlashingPenaltyBp(val_num as u16),
            ),
            _ => None,
        }
    }

    fn handle_tola_submit_proposal(
        &self,
        id: Option<JsonValue>,
        params: &[JsonValue],
    ) -> JsonRpcResponse {
        let (proposer_val, title_val, desc_val, changes_val, deposit_val) = match params.get(0) {
            Some(JsonValue::Object(_)) => {
                let obj = &params[0];
                (
                    obj.get("proposer"),
                    obj.get("title"),
                    obj.get("description"),
                    obj.get("changes"),
                    obj.get("initial_deposit").or_else(|| obj.get("deposit")),
                )
            }
            _ => (
                params.get(0),
                params.get(1),
                params.get(2),
                params.get(3),
                params.get(4),
            ),
        };

        let proposer = match proposer_val.and_then(|v| Self::parse_address(v).ok()) {
            Some(a) => a,
            None => {
                return JsonRpcResponse::error(
                    id,
                    JsonRpcError::invalid_params("Invalid proposer address"),
                )
            }
        };

        let title = match title_val.and_then(|v| v.as_str()) {
            Some(t) => t.to_string(),
            None => {
                return JsonRpcResponse::error(
                    id,
                    JsonRpcError::invalid_params("Invalid proposal title"),
                )
            }
        };

        let description = match desc_val.and_then(|v| v.as_str()) {
            Some(d) => d.to_string(),
            None => String::new(),
        };

        let mut changes = Vec::new();
        if let Some(ch_arr) = changes_val.and_then(|v| v.as_array()) {
            for item in ch_arr {
                if let Some(c) = Self::parse_param_change(item) {
                    changes.push(c);
                }
            }
        }

        let deposit = match deposit_val {
            Some(JsonValue::Integer(i)) => Amount(*i as u128),
            Some(JsonValue::String(s)) => {
                let val = if s.starts_with("0x") || s.starts_with("0X") {
                    u128::from_str_radix(&s[2..], 16).unwrap_or(0)
                } else {
                    s.parse::<u128>().unwrap_or(0)
                };
                Amount(val)
            }
            _ => Amount::ZERO,
        };

        let cur_h = BlockHeight(self.get_current_height());
        let pid = {
            let mut gov = self.governance.write().unwrap();
            gov.submit_proposal(proposer, title, description, changes, deposit, cur_h)
        };

        JsonRpcResponse::success(id, JsonValue::String(format!("0x{:x}", pid)))
    }

    fn handle_tola_cast_vote(
        &self,
        id: Option<JsonValue>,
        params: &[JsonValue],
    ) -> JsonRpcResponse {
        let (pid_val, voter_val, option_val, power_val) = match params.get(0) {
            Some(JsonValue::Object(_)) => {
                let obj = &params[0];
                (
                    obj.get("proposal_id"),
                    obj.get("voter"),
                    obj.get("option"),
                    obj.get("voting_power"),
                )
            }
            _ => (params.get(0), params.get(1), params.get(2), params.get(3)),
        };

        let proposal_id = match pid_val {
            Some(JsonValue::Integer(i)) => *i as u64,
            Some(JsonValue::String(s)) => {
                if s.starts_with("0x") || s.starts_with("0X") {
                    u64::from_str_radix(&s[2..], 16).unwrap_or(0)
                } else {
                    s.parse::<u64>().unwrap_or(0)
                }
            }
            _ => {
                return JsonRpcResponse::error(
                    id,
                    JsonRpcError::invalid_params("Invalid proposal_id"),
                )
            }
        };

        let voter = match voter_val.and_then(|v| Self::parse_address(v).ok()) {
            Some(a) => a,
            None => {
                return JsonRpcResponse::error(
                    id,
                    JsonRpcError::invalid_params("Invalid voter address"),
                )
            }
        };

        let option = match option_val.and_then(|v| v.as_str()) {
            Some(s) => match s.to_lowercase().as_str() {
                "yes" | "1" => tola_consensus::governance::VoteOption::Yes,
                "no" | "2" => tola_consensus::governance::VoteOption::No,
                "nowithveto" | "veto" | "3" => tola_consensus::governance::VoteOption::NoWithVeto,
                "abstain" | "0" | "4" => tola_consensus::governance::VoteOption::Abstain,
                _ => {
                    return JsonRpcResponse::error(
                        id,
                        JsonRpcError::invalid_params("Unknown vote option"),
                    )
                }
            },
            _ => {
                return JsonRpcResponse::error(
                    id,
                    JsonRpcError::invalid_params("Missing vote option"),
                )
            }
        };

        let voting_power = match power_val {
            Some(JsonValue::Integer(i)) => Amount(*i as u128),
            Some(JsonValue::String(s)) => {
                let val = if s.starts_with("0x") || s.starts_with("0X") {
                    u128::from_str_radix(&s[2..], 16).unwrap_or(0)
                } else {
                    s.parse::<u128>().unwrap_or(0)
                };
                Amount(val)
            }
            _ => {
                let state = self.state_db.read().unwrap();
                state
                    .get_account(&voter)
                    .ok()
                    .flatten()
                    .map(|a| a.balance)
                    .unwrap_or(Amount::ZERO)
            }
        };

        let cur_h = BlockHeight(self.get_current_height());
        let res = {
            let mut gov = self.governance.write().unwrap();
            gov.cast_vote(proposal_id, voter, option, voting_power, cur_h)
        };

        match res {
            Ok(()) => JsonRpcResponse::success(id, JsonValue::Bool(true)),
            Err(e) => {
                JsonRpcResponse::error(id, JsonRpcError::server_error(-32000, &format!("{:?}", e)))
            }
        }
    }

    fn handle_tola_emergency_param_change(
        &self,
        id: Option<JsonValue>,
        params: &[JsonValue],
    ) -> JsonRpcResponse {
        let (approvers_val, changes_val) = match params.get(0) {
            Some(JsonValue::Object(_)) => {
                let obj = &params[0];
                (obj.get("approvers"), obj.get("changes"))
            }
            _ => (params.get(0), params.get(1)),
        };

        let mut approvers = Vec::new();
        if let Some(app_arr) = approvers_val.and_then(|v| v.as_array()) {
            for a_val in app_arr {
                if let Ok(addr) = Self::parse_address(a_val) {
                    approvers.push(addr);
                }
            }
        }

        let mut changes = Vec::new();
        if let Some(ch_arr) = changes_val.and_then(|v| v.as_array()) {
            for item in ch_arr {
                if let Some(c) = Self::parse_param_change(item) {
                    changes.push(c);
                }
            }
        }

        let council = self.emergency_council.read().unwrap().clone();
        let res = {
            let mut gov = self.governance.write().unwrap();
            gov.execute_emergency_param_change(&approvers, changes, &council)
        };

        match res {
            Ok(()) => JsonRpcResponse::success(id, JsonValue::Bool(true)),
            Err(e) => {
                JsonRpcResponse::error(id, JsonRpcError::server_error(-32000, &format!("{:?}", e)))
            }
        }
    }
}

fn format_block_json(block: &Block, hash: Hash, full_txs: bool, bloom_hex: &str) -> JsonValue {
    let mut tx_items = Vec::new();

    for (idx, tx) in block.transactions.iter().enumerate() {
        let tx_id = compute_transaction_id(tx);
        if full_txs {
            tx_items.push(format_transaction_json(
                tx,
                tx_id,
                hash,
                block.header.height.0,
                idx as u64,
            ));
        } else {
            let hash_str = if tx.is_evm() {
                tola_codec::rlp::compute_eth_tx_hash(tx)
                    .map(|h| h.to_hex())
                    .unwrap_or_else(|| tx_id.0.to_hex())
            } else {
                tx_id.0.to_hex()
            };
            tx_items.push(JsonValue::String(hash_str));
        }
    }

    JsonValue::Object(vec![
        (
            "number".to_string(),
            JsonValue::String(format!("0x{:x}", block.header.height.0)),
        ),
        ("hash".to_string(), JsonValue::String(hash.to_hex())),
        (
            "parentHash".to_string(),
            JsonValue::String(block.header.parent_hash.to_hex()),
        ),
        (
            "stateRoot".to_string(),
            JsonValue::String(block.header.state_root.to_hex()),
        ),
        (
            "transactionsRoot".to_string(),
            JsonValue::String(block.header.txs_root.to_hex()),
        ),
        (
            "receiptsRoot".to_string(),
            JsonValue::String(block.header.receipts_root.to_hex()),
        ),
        (
            "miner".to_string(),
            JsonValue::String(block.header.proposer.to_hex()),
        ),
        (
            "gasUsed".to_string(),
            JsonValue::String(format!("0x{:x}", block.header.gas_used.as_u64())),
        ),
        (
            "gasLimit".to_string(),
            JsonValue::String(format!("0x{:x}", block.header.gas_limit.as_u64())),
        ),
        (
            "baseFeePerGas".to_string(),
            JsonValue::String(format!("0x{:x}", block.header.base_fee_per_gas.0)),
        ),
        (
            "logsBloom".to_string(),
            JsonValue::String(bloom_hex.to_string()),
        ),
        (
            "timestamp".to_string(),
            JsonValue::String(format!("0x{:x}", block.header.timestamp.0)),
        ),
        ("transactions".to_string(), JsonValue::Array(tx_items)),
    ])
}

fn format_transaction_json(
    tx: &Transaction,
    tx_id: TransactionId,
    block_hash: Hash,
    block_number: u64,
    tx_idx: u64,
) -> JsonValue {
    let tx_hash_str = if tx.is_evm() {
        tola_codec::rlp::compute_eth_tx_hash(tx)
            .map(|h| h.to_hex())
            .unwrap_or_else(|| tx_id.0.to_hex())
    } else {
        tx_id.0.to_hex()
    };

    let mut fields = vec![
        ("hash".to_string(), JsonValue::String(tx_hash_str)),
        (
            "nonce".to_string(),
            JsonValue::String(format!("0x{:x}", tx.nonce.0)),
        ),
        (
            "blockHash".to_string(),
            JsonValue::String(block_hash.to_hex()),
        ),
        (
            "blockNumber".to_string(),
            JsonValue::String(format!("0x{:x}", block_number)),
        ),
        (
            "transactionIndex".to_string(),
            JsonValue::String(format!("0x{:x}", tx_idx)),
        ),
        ("from".to_string(), JsonValue::String(tx.from.to_hex())),
        (
            "to".to_string(),
            match tx.to {
                Some(to) => JsonValue::String(to.to_hex()),
                None => JsonValue::Null,
            },
        ),
        (
            "value".to_string(),
            JsonValue::String(format!("0x{:x}", tx.value.0)),
        ),
        (
            "gas".to_string(),
            JsonValue::String(format!("0x{:x}", tx.gas_limit.as_u64())),
        ),
        (
            "gasPrice".to_string(),
            JsonValue::String(format!("0x{:x}", tx.max_fee_per_gas.0)),
        ),
        (
            "maxFeePerGas".to_string(),
            JsonValue::String(format!("0x{:x}", tx.max_fee_per_gas.0)),
        ),
        (
            "maxPriorityFeePerGas".to_string(),
            JsonValue::String(format!("0x{:x}", tx.max_priority_fee_per_gas.0)),
        ),
        (
            "input".to_string(),
            JsonValue::String(format!("0x{}", hex_encode(&tx.data))),
        ),
    ];

    match &tx.signature {
        Signature::Secp256k1Eth {
            r,
            s,
            y_parity,
            chain_id,
        } => {
            fields.push((
                "r".to_string(),
                JsonValue::String(format!("0x{}", hex_encode(r))),
            ));
            fields.push((
                "s".to_string(),
                JsonValue::String(format!("0x{}", hex_encode(s))),
            ));
            if tx.tx_type == 0 {
                let v = if *chain_id > 0 {
                    *chain_id * 2 + 35 + (*y_parity as u64)
                } else {
                    27 + (*y_parity as u64)
                };
                fields.push(("v".to_string(), JsonValue::String(format!("0x{:x}", v))));
                fields.push(("type".to_string(), JsonValue::String("0x0".into())));
            } else if tx.tx_type == 1 {
                fields.push((
                    "v".to_string(),
                    JsonValue::String(format!("0x{:x}", y_parity)),
                ));
                fields.push(("type".to_string(), JsonValue::String("0x1".into())));
                let al_json: Vec<JsonValue> = tx
                    .access_list
                    .iter()
                    .map(|item| {
                        let keys: Vec<JsonValue> = item
                            .storage_keys
                            .iter()
                            .map(|k| JsonValue::String(k.to_hex()))
                            .collect();
                        JsonValue::Object(vec![
                            (
                                "address".to_string(),
                                JsonValue::String(item.address.to_hex()),
                            ),
                            ("storageKeys".to_string(), JsonValue::Array(keys)),
                        ])
                    })
                    .collect();
                fields.push(("accessList".to_string(), JsonValue::Array(al_json)));
            } else {
                fields.push((
                    "v".to_string(),
                    JsonValue::String(format!("0x{:x}", y_parity)),
                ));
                fields.push(("type".to_string(), JsonValue::String("0x2".into())));
                let al_json: Vec<JsonValue> = tx
                    .access_list
                    .iter()
                    .map(|item| {
                        let keys: Vec<JsonValue> = item
                            .storage_keys
                            .iter()
                            .map(|k| JsonValue::String(k.to_hex()))
                            .collect();
                        JsonValue::Object(vec![
                            (
                                "address".to_string(),
                                JsonValue::String(item.address.to_hex()),
                            ),
                            ("storageKeys".to_string(), JsonValue::Array(keys)),
                        ])
                    })
                    .collect();
                fields.push(("accessList".to_string(), JsonValue::Array(al_json)));
            }
            fields.push((
                "chainId".to_string(),
                JsonValue::String(format!("0x{:x}", chain_id)),
            ));
        }
        Signature::Secp256k1 { r, s, v } => {
            fields.push((
                "r".to_string(),
                JsonValue::String(format!("0x{}", hex_encode(r))),
            ));
            fields.push((
                "s".to_string(),
                JsonValue::String(format!("0x{}", hex_encode(s))),
            ));
            fields.push(("v".to_string(), JsonValue::String(format!("0x{:x}", v))));
            fields.push(("type".to_string(), JsonValue::String("0x0".into())));
        }
        _ => {}
    }

    if let Some(ref m) = tx.memo {
        fields.push(("memo".to_string(), JsonValue::String(m.clone())));
    } else {
        fields.push(("memo".to_string(), JsonValue::Null));
    }

    JsonValue::Object(fields)
}

fn hex_to_bytes(s: &str) -> Result<Vec<u8>, String> {
    if s.len() % 2 != 0 {
        return Err("Hex string has odd length".into());
    }
    let mut bytes = Vec::with_capacity(s.len() / 2);
    for i in (0..s.len()).step_by(2) {
        let b =
            u8::from_str_radix(&s[i..i + 2], 16).map_err(|e| format!("Invalid hex byte: {}", e))?;
        bytes.push(b);
    }
    Ok(bytes)
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use tola_mempool::MempoolConfig;
    use tola_state::MemoryStateJournal;

    fn setup_test_dispatcher() -> RpcDispatcher<MemoryStateJournal> {
        let state = Arc::new(RwLock::new(MemoryStateJournal::new()));
        let mempool = Arc::new(RwLock::new(Mempool::new(MempoolConfig::new(
            ChainId::MAINNET,
        ))));
        RpcDispatcher::new(ChainId::MAINNET, "TOLA/v0.1.0-alpha".into(), state, mempool)
    }

    #[test]
    fn test_web3_and_net_endpoints() {
        let disp = setup_test_dispatcher();
        disp.peer_count.store(16, Ordering::SeqCst);

        let req1 = r#"{"jsonrpc":"2.0","id":1,"method":"web3_clientVersion","params":[]}"#;
        let res1 = disp.dispatch_json(req1);
        let val1 = JsonValue::parse(&res1).unwrap();
        assert_eq!(
            val1.get("result").unwrap().as_str(),
            Some("TOLA/v0.1.0-alpha")
        );

        let req2 = r#"{"jsonrpc":"2.0","id":2,"method":"net_version","params":[]}"#;
        let res2 = disp.dispatch_json(req2);
        let val2 = JsonValue::parse(&res2).unwrap();
        assert_eq!(val2.get("result").unwrap().as_str(), Some("9999"));

        let req3 = r#"{"jsonrpc":"2.0","id":3,"method":"net_peerCount","params":[]}"#;
        let res3 = disp.dispatch_json(req3);
        let val3 = JsonValue::parse(&res3).unwrap();
        assert_eq!(val3.get("result").unwrap().as_str(), Some("0x10"));
    }

    #[test]
    fn test_eth_state_queries_and_block_number() {
        let disp = setup_test_dispatcher();
        disp.current_height.store(42, Ordering::SeqCst);

        let addr = Address([0x11; 20]);
        let mut acct = AccountState::new_with_balance(Amount::from_u128(500_000_000_000));
        acct.nonce = Nonce(5);
        disp.state_db.write().unwrap().set_account(addr, acct);

        // eth_blockNumber
        let req_bn = r#"{"jsonrpc":"2.0","id":10,"method":"eth_blockNumber","params":[]}"#;
        let res_bn = disp.dispatch_json(req_bn);
        let val_bn = JsonValue::parse(&res_bn).unwrap();
        assert_eq!(val_bn.get("result").unwrap().as_str(), Some("0x2a"));

        // eth_getBalance
        let req_bal = format!(
            r#"{{"jsonrpc":"2.0","id":11,"method":"eth_getBalance","params":["{}", "latest"]}}"#,
            addr.to_hex()
        );
        let res_bal = disp.dispatch_json(&req_bal);
        let val_bal = JsonValue::parse(&res_bal).unwrap();
        assert_eq!(
            val_bal.get("result").unwrap().as_str(),
            Some("0x746a528800")
        ); // 500,000,000,000

        // eth_getTransactionCount
        let req_nonce = format!(
            r#"{{"jsonrpc":"2.0","id":12,"method":"eth_getTransactionCount","params":["{}", "latest"]}}"#,
            addr.to_hex()
        );
        let res_nonce = disp.dispatch_json(&req_nonce);
        let val_nonce = JsonValue::parse(&res_nonce).unwrap();
        assert_eq!(val_nonce.get("result").unwrap().as_str(), Some("0x5"));
    }

    #[test]
    fn test_tola_extensions_and_block_queries() {
        let disp = setup_test_dispatcher();
        disp.current_height.store(21600 * 2, Ordering::SeqCst); // Epoch 2

        // tola_getEpoch
        let req_ep = r#"{"jsonrpc":"2.0","id":20,"method":"tola_getEpoch","params":[]}"#;
        let res_ep = disp.dispatch_json(req_ep);
        let val_ep = JsonValue::parse(&res_ep).unwrap();
        assert_eq!(val_ep.get("result").unwrap().as_str(), Some("0x2"));

        // tola_getValidators
        let v1 = ValidatorInfo {
            validator_id: ValidatorId(Address([0x01; 20])),
            consensus_pubkey: PublicKey::Ed25519([0xaa; 32]),
            voting_power: 500,
            commission_bp: 300,
        };
        let set = ValidatorSet {
            epoch: Epoch(2),
            validators: vec![v1],
            total_voting_power: 500,
        };
        *disp.active_committee.write().unwrap() = Some(set);

        let req_val = r#"{"jsonrpc":"2.0","id":21,"method":"tola_getValidators","params":[]}"#;
        let res_val = disp.dispatch_json(req_val);
        let val_val = JsonValue::parse(&res_val).unwrap();
        let val_arr = val_val.get("result").unwrap().as_array().unwrap();
        assert_eq!(val_arr.len(), 1);
        assert_eq!(val_arr[0].get("voting_power").unwrap().as_i64(), Some(500));
        assert_eq!(val_arr[0].get("commission_bp").unwrap().as_i64(), Some(300));
    }

    #[test]
    fn test_tola_external_asset_and_rejection_of_native_identity_rpc_endpoints() {
        let disp = setup_test_dispatcher();

        // 1. Verify that legacy native identity / gift RPC methods are REJECTED (Method Not Found)
        let req_native_user =
            r#"{"jsonrpc":"2.0","id":30,"method":"tola_resolveUsername","params":["@alice"]}"#;
        let res_native_user = disp.dispatch_json(req_native_user);
        let val_native_user = JsonValue::parse(&res_native_user).unwrap();
        assert!(
            val_native_user.get("error").is_some(),
            "tola_resolveUsername must be rejected as method not found"
        );

        let req_native_rec =
            r#"{"jsonrpc":"2.0","id":31,"method":"tola_getUsernameRecord","params":["alice"]}"#;
        let res_native_rec = disp.dispatch_json(req_native_rec);
        let val_native_rec = JsonValue::parse(&res_native_rec).unwrap();
        assert!(
            val_native_rec.get("error").is_some(),
            "tola_getUsernameRecord must be rejected as method not found"
        );

        let req_native_gift =
            r#"{"jsonrpc":"2.0","id":32,"method":"tola_getGiftDetails","params":[101]}"#;
        let res_native_gift = disp.dispatch_json(req_native_gift);
        let val_native_gift = JsonValue::parse(&res_native_gift).unwrap();
        assert!(
            val_native_gift.get("error").is_some(),
            "tola_getGiftDetails must be rejected as method not found"
        );

        let req_native_badge =
            r#"{"jsonrpc":"2.0","id":33,"method":"tola_verifyBadgeAttestation","params":[{}]}"#;
        let res_native_badge = disp.dispatch_json(req_native_badge);
        let val_native_badge = JsonValue::parse(&res_native_badge).unwrap();
        assert!(
            val_native_badge.get("error").is_some(),
            "tola_verifyBadgeAttestation must be rejected as method not found"
        );

        let req_get_verifier = r#"{"jsonrpc":"2.0","id":34,"method":"tola_getVerifier","params":["0x1111111111111111111111111111111111111111"]}"#;
        let res_get_verifier = disp.dispatch_json(req_get_verifier);
        let val_get_verifier = JsonValue::parse(&res_get_verifier).unwrap();
        assert!(
            val_get_verifier.get("error").is_some(),
            "tola_getVerifier must be rejected as method not found"
        );

        // 9. External Asset Tokenization RPC Endpoints
        let ext_token_id = 1001u64;
        let verifier_kp = tola_crypto::Secp256k1Keypair::generate();
        let verifier_addr = verifier_kp.address();
        let ext_owner = Address([0x55; 20]);
        let source_plat = "PaxtaGram";
        let asset_type = ExternalAssetType::MessengerUsername;
        let ext_asset_id = "@anvar";
        let canon_id =
            tola_crypto::compute_external_asset_canonical_id(source_plat, asset_type, ext_asset_id);

        let ext_asset = ExternalAsset {
            token_id: ext_token_id,
            canonical_id: canon_id,
            source_platform: source_plat.to_string(),
            asset_type,
            external_asset_id: ext_asset_id.to_string(),
            external_reference: "tg://user?id=12345".to_string(),
            metadata_uri: "ipfs://bafybeiexternalasset1001".to_string(),
            owner: ext_owner,
            authorizer: verifier_addr,
            verifier_address: verifier_addr,
            minted_at: 1700000000,
        };

        disp.register_external_asset(ext_asset)
            .expect("register external asset");

        // Duplicate registration must fail
        let duplicate_asset = ExternalAsset {
            token_id: 1002,
            canonical_id: canon_id,
            source_platform: source_plat.to_string(),
            asset_type,
            external_asset_id: ext_asset_id.to_string(),
            external_reference: "".to_string(),
            metadata_uri: "".to_string(),
            owner: Address([0x66; 20]),
            authorizer: verifier_addr,
            verifier_address: verifier_addr,
            minted_at: 1700000001,
        };
        assert!(disp.register_external_asset(duplicate_asset).is_err());

        // Query by tokenId
        let req_ext = format!(
            r#"{{"jsonrpc":"2.0","id":60,"method":"tola_getExternalAsset","params":[{}]}}"#,
            ext_token_id
        );
        let res_ext = disp.dispatch_json(&req_ext);
        let val_ext = JsonValue::parse(&res_ext).unwrap();
        let r_obj = val_ext.get("result").unwrap();
        assert_eq!(
            r_obj.get("sourcePlatform").and_then(|v| v.as_str()),
            Some(source_plat)
        );
        assert_eq!(
            r_obj.get("externalAssetId").and_then(|v| v.as_str()),
            Some(ext_asset_id)
        );
        assert_eq!(
            r_obj.get("owner").and_then(|v| v.as_str()),
            Some(ext_owner.to_hex().as_str())
        );
        assert_eq!(
            r_obj.get("authorizer").and_then(|v| v.as_str()),
            Some(verifier_addr.to_hex().as_str())
        );
        assert_eq!(
            r_obj.get("verifierAddress").and_then(|v| v.as_str()),
            Some(verifier_addr.to_hex().as_str())
        );

        // Query by canonicalId hex
        let req_canon = format!(
            r#"{{"jsonrpc":"2.0","id":61,"method":"tola_getExternalAsset","params":["0x{}"]}}"#,
            hex_encode(&canon_id)
        );
        let res_canon = disp.dispatch_json(&req_canon);
        let val_canon = JsonValue::parse(&res_canon).unwrap();
        assert_eq!(
            val_canon
                .get("result")
                .unwrap()
                .get("tokenId")
                .and_then(|v| v.as_i64()),
            Some(ext_token_id as i64)
        );

        // Query owner
        let req_owner = format!(
            r#"{{"jsonrpc":"2.0","id":62,"method":"tola_getExternalAssetOwner","params":[{}]}}"#,
            ext_token_id
        );
        let res_owner = disp.dispatch_json(&req_owner);
        let val_owner = JsonValue::parse(&res_owner).unwrap();
        assert_eq!(
            val_owner.get("result").and_then(|v| v.as_str()),
            Some(ext_owner.to_hex().as_str())
        );

        // Query metadata
        let req_meta = format!(
            r#"{{"jsonrpc":"2.0","id":63,"method":"tola_getExternalAssetMetadata","params":[{}]}}"#,
            ext_token_id
        );
        let res_meta = disp.dispatch_json(&req_meta);
        let val_meta = JsonValue::parse(&res_meta).unwrap();
        assert_eq!(
            val_meta
                .get("result")
                .unwrap()
                .get("metadataUri")
                .and_then(|v| v.as_str()),
            Some("ipfs://bafybeiexternalasset1001")
        );

        // Verify External Asset Attestation via RPC
        let nonce = 42u64;
        let future_exp = 2500000000u64;
        let dest_wallet = Address([0x77; 20]);
        let ext_digest = tola_crypto::compute_external_asset_attestation_digest(
            disp.chain_id.0,
            nonce,
            &dest_wallet,
            future_exp,
            source_plat,
            asset_type,
            ext_asset_id,
        );
        let ext_sig = verifier_kp.sign_recoverable(&ext_digest).unwrap();

        let req_verify_att = format!(
            r#"{{"jsonrpc":"2.0","id":64,"method":"tola_verifyExternalAssetAttestation","params":[{{
                "chainId": {},
                "nonce": {},
                "destinationWallet": "{}",
                "expiresAt": {},
                "sourcePlatform": "{}",
                "assetType": "{}",
                "externalAssetId": "{}",
                "verifierAddress": "{}",
                "signature": {{ "r": "0x{}", "s": "0x{}", "v": {} }}
            }}]}}"#,
            disp.chain_id.0,
            nonce,
            dest_wallet.to_hex(),
            future_exp,
            source_plat,
            asset_type.as_str(),
            ext_asset_id,
            verifier_addr.to_hex(),
            hex_encode(&ext_sig[0..32]),
            hex_encode(&ext_sig[32..64]),
            ext_sig[64]
        );
        let res_verify_att = disp.dispatch_json(&req_verify_att);
        let val_verify_att = JsonValue::parse(&res_verify_att).unwrap();
        let att_result = val_verify_att.get("result").unwrap();
        assert_eq!(
            att_result.get("valid").and_then(|v| v.as_bool()),
            Some(true)
        );
        assert_eq!(
            att_result
                .get("alreadyMintedTokenId")
                .and_then(|v| v.as_i64()),
            Some(ext_token_id as i64)
        );
    }

    #[test]
    fn test_eth_jsonrpc_lifecycle_and_evm_queries() {
        use tola_crypto::Secp256k1Keypair;

        let disp = setup_test_dispatcher();
        let kp = Secp256k1Keypair::generate();
        let sender = kp.address();

        // 1. Fund sender in state
        let mut acct = AccountState::new_with_balance(Amount::from_u128(1_000_000_000_000_000));
        acct.nonce = Nonce(0);
        disp.state_db.write().unwrap().set_account(sender, acct);

        // 2. eth_chainId
        let req_cid = r#"{"jsonrpc":"2.0","id":1,"method":"eth_chainId","params":[]}"#;
        let res_cid = disp.dispatch_json(req_cid);
        let val_cid = JsonValue::parse(&res_cid).unwrap();
        assert_eq!(val_cid.get("result").unwrap().as_str(), Some("0x270f")); // 9999

        // 3. Build an EIP-1559 Type-2 Transaction
        let target = Address([0x42; 20]);
        let mut tx = Transaction {
            chain_id: ChainId(9999),
            nonce: Nonce(0),
            from: sender,
            to: Some(target),
            value: Amount::from_u128(500),
            gas_limit: Gas(100_000),
            max_fee_per_gas: Amount::from_u128(1_000_000_000),
            max_priority_fee_per_gas: Amount::from_u128(100_000_000),
            valid_until_block: BlockHeight(100),
            data: vec![0x12, 0x34],
            memo: None,
            signature: Signature::Ed25519([0u8; 64]),
            tx_type: 2,
            access_list: vec![],
        };
        let hash_to_sign = tola_codec::rlp::compute_type2_signing_hash(&tx);
        let sig = kp.sign_recoverable(hash_to_sign.as_bytes()).unwrap();
        let mut r = [0u8; 32];
        let mut s = [0u8; 32];
        r.copy_from_slice(&sig[0..32]);
        s.copy_from_slice(&sig[32..64]);
        let y_parity = sig[64];
        tx.signature = Signature::Secp256k1Eth {
            r,
            s,
            y_parity,
            chain_id: 9999,
        };

        let raw_tx_bytes = tola_codec::rlp::encode_raw_eth_transaction(&tx).unwrap();
        let raw_tx_hex = format!("0x{}", hex_encode(&raw_tx_bytes));
        let eth_hash = tola_codec::rlp::compute_eth_tx_hash(&tx).unwrap();

        // 4. Send raw transaction via eth_sendRawTransaction
        let req_send = format!(
            r#"{{"jsonrpc":"2.0","id":2,"method":"eth_sendRawTransaction","params":["{}"]}}"#,
            raw_tx_hex
        );
        let res_send = disp.dispatch_json(&req_send);
        let val_send = JsonValue::parse(&res_send).unwrap();
        assert_eq!(
            val_send.get("result").unwrap().as_str(),
            Some(eth_hash.to_hex().as_str())
        );

        // Before block commit, eth_getTransactionByHash should return null
        let req_tx_pre = format!(
            r#"{{"jsonrpc":"2.0","id":3,"method":"eth_getTransactionByHash","params":["{}"]}}"#,
            eth_hash.to_hex()
        );
        let res_tx_pre = disp.dispatch_json(&req_tx_pre);
        let val_tx_pre = JsonValue::parse(&res_tx_pre).unwrap();
        assert_eq!(val_tx_pre.get("result").unwrap(), &JsonValue::Null);

        // 5. Commit block with this tx
        let tx_id = compute_transaction_id(&tx);
        let block_header = BlockHeader {
            parent_hash: Hash::ZERO,
            height: BlockHeight(1),
            epoch: Epoch(0),
            round: Round(1),
            timestamp: Timestamp(1000),
            proposer: Address([0x01; 20]),
            state_root: Hash([0xaa; 32]),
            txs_root: Hash([0xbb; 32]),
            receipts_root: Hash([0xcc; 32]),
            gas_used: Gas(21_000),
            gas_limit: Gas(30_000_000),
            base_fee_per_gas: Amount::from_u128(1_000_000),
            extra_data: vec![],
        };
        let block = Block {
            header: block_header,
            transactions: vec![tx.clone()],
            last_commit_qc: None,
        };
        let log = Log {
            address: target,
            topics: vec![Hash([0xee; 32])],
            data: vec![0x99; 32],
        };
        let receipt = Receipt {
            tx_id,
            success: true,
            gas_used: Gas(21_000),
            cumulative_gas_used: Gas(21_000),
            logs: vec![log],
            return_data: vec![],
        };
        disp.record_block(block.clone(), vec![receipt]);

        // 6. After block commit, eth_getTransactionByHash returns transaction
        let res_tx_post = disp.dispatch_json(&req_tx_pre);
        let val_tx_post = JsonValue::parse(&res_tx_post).unwrap();
        let tx_obj = val_tx_post.get("result").unwrap();
        assert_eq!(
            tx_obj.get("hash").and_then(|v| v.as_str()),
            Some(eth_hash.to_hex().as_str())
        );
        assert_eq!(
            tx_obj.get("blockNumber").and_then(|v| v.as_str()),
            Some("0x1")
        );
        assert_eq!(
            tx_obj.get("from").and_then(|v| v.as_str()),
            Some(sender.to_hex().as_str())
        );
        assert_eq!(
            tx_obj.get("to").and_then(|v| v.as_str()),
            Some(target.to_hex().as_str())
        );
        assert_eq!(tx_obj.get("type").and_then(|v| v.as_str()), Some("0x2"));

        // 7. eth_getTransactionReceipt
        let req_rcpt = format!(
            r#"{{"jsonrpc":"2.0","id":4,"method":"eth_getTransactionReceipt","params":["{}"]}}"#,
            eth_hash.to_hex()
        );
        let res_rcpt = disp.dispatch_json(&req_rcpt);
        let val_rcpt = JsonValue::parse(&res_rcpt).unwrap();
        let rcpt_obj = val_rcpt.get("result").unwrap();
        assert_eq!(rcpt_obj.get("status").and_then(|v| v.as_str()), Some("0x1"));
        assert_eq!(
            rcpt_obj.get("gasUsed").and_then(|v| v.as_str()),
            Some("0x5208") // 21,000
        );
        let bloom_str = rcpt_obj.get("logsBloom").and_then(|v| v.as_str()).unwrap();
        assert!(bloom_str.starts_with("0x"));
        assert_eq!(bloom_str.len(), 2 + 512); // 256 bytes = 512 hex chars

        let logs_arr = rcpt_obj.get("logs").and_then(|v| v.as_array()).unwrap();
        assert_eq!(logs_arr.len(), 1);
        assert_eq!(
            logs_arr[0].get("address").and_then(|v| v.as_str()),
            Some(target.to_hex().as_str())
        );

        // 8. eth_getBlockByNumber
        let req_blk =
            r#"{"jsonrpc":"2.0","id":5,"method":"eth_getBlockByNumber","params":["latest", true]}"#;
        let res_blk = disp.dispatch_json(req_blk);
        let val_blk = JsonValue::parse(&res_blk).unwrap();
        let blk_obj = val_blk.get("result").unwrap();
        let blk_bloom = blk_obj.get("logsBloom").and_then(|v| v.as_str()).unwrap();
        assert_eq!(blk_bloom, bloom_str);

        // 9. Contract deploy, eth_getCode, eth_call, eth_estimateGas
        // Bytecode: 60 2a 60 00 52 60 20 60 00 f3 (PUSH1 42, PUSH1 0, MSTORE, PUSH1 32, PUSH1 0, RETURN)
        let bytecode = vec![0x60, 0x2a, 0x60, 0x00, 0x52, 0x60, 0x20, 0x60, 0x00, 0xf3];
        let code_hash = tola_crypto::keccak256_hash(&bytecode);
        let contract_addr = Address([0x99; 20]);
        let mut contract_acct = AccountState::new_with_balance(Amount::ZERO);
        contract_acct.code_hash = code_hash;
        disp.state_db
            .write()
            .unwrap()
            .set_account(contract_addr, contract_acct);
        disp.state_db
            .write()
            .unwrap()
            .set_code(code_hash, bytecode.clone());

        // eth_getCode
        let req_code = format!(
            r#"{{"jsonrpc":"2.0","id":6,"method":"eth_getCode","params":["{}", "latest"]}}"#,
            contract_addr.to_hex()
        );
        let res_code = disp.dispatch_json(&req_code);
        let val_code = JsonValue::parse(&res_code).unwrap();
        assert_eq!(
            val_code.get("result").unwrap().as_str(),
            Some(format!("0x{}", hex_encode(&bytecode)).as_str())
        );

        // eth_call
        let req_call = format!(
            r#"{{"jsonrpc":"2.0","id":7,"method":"eth_call","params":[{{ "to": "{}" }}, "latest"]}}"#,
            contract_addr.to_hex()
        );
        let res_call = disp.dispatch_json(&req_call);
        let val_call = JsonValue::parse(&res_call).unwrap();
        let call_res = val_call.get("result").unwrap().as_str().unwrap();
        assert!(call_res.ends_with("2a"));

        // eth_estimateGas
        let req_est = format!(
            r#"{{"jsonrpc":"2.0","id":8,"method":"eth_estimateGas","params":[{{ "to": "{}" }}]}}"#,
            contract_addr.to_hex()
        );
        let res_est = disp.dispatch_json(&req_est);
        let val_est = JsonValue::parse(&res_est).unwrap();
        let est_gas = val_est.get("result").unwrap().as_str().unwrap();
        assert!(est_gas.starts_with("0x"));
    }

    #[test]
    fn test_p0_eth_estimate_gas_semantics() {
        let disp = setup_test_dispatcher();

        // 1. Success contract: returns 42
        let ok_bytecode = vec![0x60, 0x2a, 0x60, 0x00, 0x52, 0x60, 0x20, 0x60, 0x00, 0xf3];
        let ok_hash = tola_crypto::keccak256_hash(&ok_bytecode);
        let ok_addr = Address([0x11; 20]);
        let mut ok_acct = AccountState::new_with_balance(Amount::ZERO);
        ok_acct.code_hash = ok_hash;
        disp.state_db.write().unwrap().set_account(ok_addr, ok_acct);
        disp.state_db
            .write()
            .unwrap()
            .set_code(ok_hash, ok_bytecode);

        let req_ok = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"eth_estimateGas","params":[{{ "to": "{}" }}]}}"#,
            ok_addr.to_hex()
        );
        let res_ok = disp.dispatch_json(&req_ok);
        let val_ok = JsonValue::parse(&res_ok).unwrap();
        assert!(val_ok.get("result").is_some());
        let gas_str = val_ok.get("result").unwrap().as_str().unwrap();
        assert!(gas_str.starts_with("0x"));

        // 2. Reverting contract with custom error payload:
        // PUSH4 0x08c379a0, PUSH1 0, MSTORE, PUSH1 4, PUSH1 28, REVERT
        let rev_bytecode = vec![
            0x63, 0x08, 0xc3, 0x79, 0xa0, 0x60, 0x00, 0x52, 0x60, 0x04, 0x60, 0x1c, 0xfd,
        ];
        let rev_hash = tola_crypto::keccak256_hash(&rev_bytecode);
        let rev_addr = Address([0x22; 20]);
        let mut rev_acct = AccountState::new_with_balance(Amount::ZERO);
        rev_acct.code_hash = rev_hash;
        disp.state_db
            .write()
            .unwrap()
            .set_account(rev_addr, rev_acct);
        disp.state_db
            .write()
            .unwrap()
            .set_code(rev_hash, rev_bytecode);

        let req_rev = format!(
            r#"{{"jsonrpc":"2.0","id":2,"method":"eth_estimateGas","params":[{{ "to": "{}" }}]}}"#,
            rev_addr.to_hex()
        );
        let res_rev = disp.dispatch_json(&req_rev);
        let val_rev = JsonValue::parse(&res_rev).unwrap();
        let err_obj = val_rev
            .get("error")
            .expect("eth_estimateGas must return error on revert");
        assert_eq!(err_obj.get("code").unwrap().as_i64(), Some(3));
        assert_eq!(
            err_obj.get("message").unwrap().as_str(),
            Some("execution reverted")
        );
        let data_str = err_obj.get("data").unwrap().as_str().unwrap();
        assert_eq!(data_str, "0x08c379a0");

        // 3. Halting contract with INVALID opcode (0xfe):
        let halt_bytecode = vec![0xfe];
        let halt_hash = tola_crypto::keccak256_hash(&halt_bytecode);
        let halt_addr = Address([0x33; 20]);
        let mut halt_acct = AccountState::new_with_balance(Amount::ZERO);
        halt_acct.code_hash = halt_hash;
        disp.state_db
            .write()
            .unwrap()
            .set_account(halt_addr, halt_acct);
        disp.state_db
            .write()
            .unwrap()
            .set_code(halt_hash, halt_bytecode);

        let req_halt = format!(
            r#"{{"jsonrpc":"2.0","id":3,"method":"eth_estimateGas","params":[{{ "to": "{}" }}]}}"#,
            halt_addr.to_hex()
        );
        let res_halt = disp.dispatch_json(&req_halt);
        let val_halt = JsonValue::parse(&res_halt).unwrap();
        let err_halt = val_halt
            .get("error")
            .expect("eth_estimateGas must return error on halt");
        assert_eq!(err_halt.get("code").unwrap().as_i64(), Some(-32000));
        assert!(err_halt
            .get("message")
            .unwrap()
            .as_str()
            .unwrap()
            .contains("execution halted"));
    }

    #[test]
    fn test_p0_eth_call_revert_data_payloads() {
        let disp = setup_test_dispatcher();

        // 1. Success call: returns 42
        let ok_bytecode = vec![0x60, 0x2a, 0x60, 0x00, 0x52, 0x60, 0x20, 0x60, 0x00, 0xf3];
        let ok_hash = tola_crypto::keccak256_hash(&ok_bytecode);
        let ok_addr = Address([0x44; 20]);
        let mut ok_acct = AccountState::new_with_balance(Amount::ZERO);
        ok_acct.code_hash = ok_hash;
        disp.state_db.write().unwrap().set_account(ok_addr, ok_acct);
        disp.state_db
            .write()
            .unwrap()
            .set_code(ok_hash, ok_bytecode);

        let req_ok = format!(
            r#"{{"jsonrpc":"2.0","id":10,"method":"eth_call","params":[{{ "to": "{}" }}, "latest"]}}"#,
            ok_addr.to_hex()
        );
        let res_ok = disp.dispatch_json(&req_ok);
        let val_ok = JsonValue::parse(&res_ok).unwrap();
        assert!(val_ok
            .get("result")
            .unwrap()
            .as_str()
            .unwrap()
            .ends_with("2a"));

        // 2. Revert with custom payload
        let rev_bytecode = vec![
            0x63, 0x08, 0xc3, 0x79, 0xa0, 0x60, 0x00, 0x52, 0x60, 0x04, 0x60, 0x1c, 0xfd,
        ];
        let rev_hash = tola_crypto::keccak256_hash(&rev_bytecode);
        let rev_addr = Address([0x55; 20]);
        let mut rev_acct = AccountState::new_with_balance(Amount::ZERO);
        rev_acct.code_hash = rev_hash;
        disp.state_db
            .write()
            .unwrap()
            .set_account(rev_addr, rev_acct);
        disp.state_db
            .write()
            .unwrap()
            .set_code(rev_hash, rev_bytecode);

        let req_rev = format!(
            r#"{{"jsonrpc":"2.0","id":11,"method":"eth_call","params":[{{ "to": "{}" }}, "latest"]}}"#,
            rev_addr.to_hex()
        );
        let res_rev = disp.dispatch_json(&req_rev);
        let val_rev = JsonValue::parse(&res_rev).unwrap();
        let err_obj = val_rev
            .get("error")
            .expect("eth_call must return error on revert");
        assert_eq!(err_obj.get("code").unwrap().as_i64(), Some(3));
        assert_eq!(
            err_obj.get("message").unwrap().as_str(),
            Some("execution reverted")
        );
        assert_eq!(err_obj.get("data").unwrap().as_str(), Some("0x08c379a0"));

        // 3. Revert with empty bytes
        let empty_rev_bytecode = vec![0x60, 0x00, 0x60, 0x00, 0xfd];
        let empty_rev_hash = tola_crypto::keccak256_hash(&empty_rev_bytecode);
        let empty_rev_addr = Address([0x66; 20]);
        let mut empty_rev_acct = AccountState::new_with_balance(Amount::ZERO);
        empty_rev_acct.code_hash = empty_rev_hash;
        disp.state_db
            .write()
            .unwrap()
            .set_account(empty_rev_addr, empty_rev_acct);
        disp.state_db
            .write()
            .unwrap()
            .set_code(empty_rev_hash, empty_rev_bytecode);

        let req_empty_rev = format!(
            r#"{{"jsonrpc":"2.0","id":12,"method":"eth_call","params":[{{ "to": "{}" }}, "latest"]}}"#,
            empty_rev_addr.to_hex()
        );
        let res_empty_rev = disp.dispatch_json(&req_empty_rev);
        let val_empty_rev = JsonValue::parse(&res_empty_rev).unwrap();
        let err_empty = val_empty_rev
            .get("error")
            .expect("eth_call must return error on empty revert");
        assert_eq!(err_empty.get("code").unwrap().as_i64(), Some(3));
        assert_eq!(
            err_empty.get("message").unwrap().as_str(),
            Some("execution reverted")
        );
        assert_eq!(err_empty.get("data").unwrap().as_str(), Some("0x"));
    }

    #[test]
    fn test_p0_receipt_effective_gas_price_type2_and_legacy() {
        let disp = setup_test_dispatcher();

        let base_fee = Amount::from_u128(1_000_000_000); // 1 Gwei
        let block_hash = Hash([0xbb; 32]);
        let sender = Address([0xaa; 20]);

        // Tx 0: Type-2 EIP-1559: max_fee = 10 Gwei, max_priority = 2 Gwei
        // effectiveGasPrice = base_fee (1 Gwei) + min(2 Gwei, 10 - 1 Gwei) = 3 Gwei (0xb2d05e00)
        let tx_type2 = Transaction {
            chain_id: ChainId::MAINNET,
            nonce: Nonce(0),
            from: sender,
            to: Some(Address([0x99; 20])),
            value: Amount::ZERO,
            gas_limit: Gas(21_000),
            max_fee_per_gas: Amount::from_u128(10_000_000_000),
            max_priority_fee_per_gas: Amount::from_u128(2_000_000_000),
            valid_until_block: BlockHeight(100),
            data: vec![],
            memo: None,
            signature: Signature::Secp256k1Eth {
                r: [0u8; 32],
                s: [0u8; 32],
                y_parity: 0,
                chain_id: ChainId::MAINNET.0,
            },
            tx_type: 2,
            access_list: vec![],
        };

        // Tx 1: Legacy: max_fee = 5 Gwei, max_priority = 5 Gwei
        // effectiveGasPrice = gasPrice = 5 Gwei (0x12a05f200)
        let tx_legacy = Transaction {
            chain_id: ChainId::MAINNET,
            nonce: Nonce(1),
            from: sender,
            to: Some(Address([0x99; 20])),
            value: Amount::ZERO,
            gas_limit: Gas(21_000),
            max_fee_per_gas: Amount::from_u128(5_000_000_000),
            max_priority_fee_per_gas: Amount::from_u128(5_000_000_000),
            valid_until_block: BlockHeight(100),
            data: vec![],
            memo: None,
            signature: Signature::Secp256k1Eth {
                r: [0u8; 32],
                s: [0u8; 32],
                y_parity: 0,
                chain_id: ChainId::MAINNET.0,
            },
            tx_type: 0,
            access_list: vec![],
        };

        let tx_id_type2 = compute_transaction_id(&tx_type2);
        let tx_id_legacy = compute_transaction_id(&tx_legacy);

        let block = Block {
            header: BlockHeader {
                parent_hash: Hash::ZERO,
                height: BlockHeight(1),
                epoch: Epoch(0),
                round: Round(0),
                timestamp: Timestamp(100),
                proposer: Address([0x01; 20]),
                state_root: Hash::ZERO,
                txs_root: Hash::ZERO,
                receipts_root: Hash::ZERO,
                gas_used: Gas(42_000),
                gas_limit: Gas(30_000_000),
                base_fee_per_gas: base_fee,
                extra_data: vec![],
            },
            transactions: vec![tx_type2, tx_legacy],
            last_commit_qc: None,
        };

        disp.blocks_by_hash
            .write()
            .unwrap()
            .insert(block_hash, block);
        disp.tx_lookup
            .write()
            .unwrap()
            .insert(tx_id_type2, (block_hash, 0));
        disp.tx_lookup
            .write()
            .unwrap()
            .insert(tx_id_legacy, (block_hash, 1));

        let rcpt_type2 = Receipt {
            tx_id: tx_id_type2,
            success: true,
            gas_used: Gas(21_000),
            cumulative_gas_used: Gas(21_000),
            logs: vec![],
            return_data: vec![],
        };
        let rcpt_legacy = Receipt {
            tx_id: tx_id_legacy,
            success: true,
            gas_used: Gas(21_000),
            cumulative_gas_used: Gas(42_000),
            logs: vec![],
            return_data: vec![],
        };

        disp.receipts
            .write()
            .unwrap()
            .insert(tx_id_type2, rcpt_type2);
        disp.receipts
            .write()
            .unwrap()
            .insert(tx_id_legacy, rcpt_legacy);

        // Check Type-2 receipt
        let req_t2 = format!(
            r#"{{"jsonrpc":"2.0","id":20,"method":"eth_getTransactionReceipt","params":["{}"]}}"#,
            tx_id_type2.0.to_hex()
        );
        let res_t2 = disp.dispatch_json(&req_t2);
        let val_t2 = JsonValue::parse(&res_t2).unwrap();
        let obj_t2 = val_t2.get("result").unwrap();
        assert_eq!(
            obj_t2.get("effectiveGasPrice").unwrap().as_str(),
            Some("0xb2d05e00") // 3 Gwei
        );
        assert_eq!(obj_t2.get("type").unwrap().as_str(), Some("0x2"));

        // Check Legacy receipt
        let req_leg = format!(
            r#"{{"jsonrpc":"2.0","id":21,"method":"eth_getTransactionReceipt","params":["{}"]}}"#,
            tx_id_legacy.0.to_hex()
        );
        let res_leg = disp.dispatch_json(&req_leg);
        let val_leg = JsonValue::parse(&res_leg).unwrap();
        let obj_leg = val_leg.get("result").unwrap();
        assert_eq!(
            obj_leg.get("effectiveGasPrice").unwrap().as_str(),
            Some("0x12a05f200") // 5 Gwei
        );
        assert_eq!(obj_leg.get("type").unwrap().as_str(), Some("0x0"));
    }

    #[test]
    fn test_p0_eth_get_transaction_count_pending_rpc() {
        use tola_crypto::Secp256k1Keypair;

        let disp = setup_test_dispatcher();
        let kp = Secp256k1Keypair::generate();
        let addr = kp.address();

        // 1. Account with committed nonce = 5
        let mut acct = AccountState::new_with_balance(Amount::from_u128(1_000_000_000_000_000_000));
        acct.nonce = Nonce(5);
        disp.state_db.write().unwrap().set_account(addr, acct);

        // eth_getTransactionCount "latest" -> 0x5
        let req_latest = format!(
            r#"{{"jsonrpc":"2.0","id":30,"method":"eth_getTransactionCount","params":["{}", "latest"]}}"#,
            addr.to_hex()
        );
        let res_latest = disp.dispatch_json(&req_latest);
        let val_latest = JsonValue::parse(&res_latest).unwrap();
        assert_eq!(val_latest.get("result").unwrap().as_str(), Some("0x5"));

        // eth_getTransactionCount "pending" (no txs in mempool) -> 0x5
        let req_pending_init = format!(
            r#"{{"jsonrpc":"2.0","id":31,"method":"eth_getTransactionCount","params":["{}", "pending"]}}"#,
            addr.to_hex()
        );
        let res_pending_init = disp.dispatch_json(&req_pending_init);
        let val_pending_init = JsonValue::parse(&res_pending_init).unwrap();
        assert_eq!(
            val_pending_init.get("result").unwrap().as_str(),
            Some("0x5")
        );

        let make_raw_tx = |nonce: u64| -> String {
            let mut tx = Transaction {
                chain_id: ChainId(9999),
                nonce: Nonce(nonce),
                from: addr,
                to: Some(Address([0x99; 20])),
                value: Amount::from_u128(100),
                gas_limit: Gas(21_000),
                max_fee_per_gas: Amount::from_u128(10_000_000_000),
                max_priority_fee_per_gas: Amount::from_u128(2_000_000_000),
                valid_until_block: BlockHeight(100),
                data: vec![],
                memo: None,
                signature: Signature::Ed25519([0u8; 64]),
                tx_type: 2,
                access_list: vec![],
            };
            let hash_to_sign = tola_codec::rlp::compute_type2_signing_hash(&tx);
            let sig = kp.sign_recoverable(hash_to_sign.as_bytes()).unwrap();
            let mut r = [0u8; 32];
            let mut s = [0u8; 32];
            r.copy_from_slice(&sig[0..32]);
            s.copy_from_slice(&sig[32..64]);
            let y_parity = sig[64];
            tx.signature = Signature::Secp256k1Eth {
                r,
                s,
                y_parity,
                chain_id: 9999,
            };
            let raw_bytes = tola_codec::rlp::encode_raw_eth_transaction(&tx).unwrap();
            format!("0x{}", hex_encode(&raw_bytes))
        };

        // Insert tx with nonce 5 into mempool via eth_sendRawTransaction
        let req_send5 = format!(
            r#"{{"jsonrpc":"2.0","id":40,"method":"eth_sendRawTransaction","params":["{}"]}}"#,
            make_raw_tx(5)
        );
        let res_send5 = disp.dispatch_json(&req_send5);
        let parsed_send5 = JsonValue::parse(&res_send5).unwrap();
        assert!(
            parsed_send5.get("result").is_some(),
            "res_send5 failed: {:?}",
            res_send5
        );

        // Now eth_getTransactionCount "pending" -> 0x6
        let res_pending_1 = disp.dispatch_json(&req_pending_init);
        let val_pending_1 = JsonValue::parse(&res_pending_1).unwrap();
        assert_eq!(val_pending_1.get("result").unwrap().as_str(), Some("0x6"));

        // "latest" still returns 0x5!
        let res_latest_still = disp.dispatch_json(&req_latest);
        let val_latest_still = JsonValue::parse(&res_latest_still).unwrap();
        assert_eq!(
            val_latest_still.get("result").unwrap().as_str(),
            Some("0x5")
        );

        // Insert tx with nonce 6 into mempool via eth_sendRawTransaction
        let req_send6 = format!(
            r#"{{"jsonrpc":"2.0","id":41,"method":"eth_sendRawTransaction","params":["{}"]}}"#,
            make_raw_tx(6)
        );
        let res_send6 = disp.dispatch_json(&req_send6);
        assert!(JsonValue::parse(&res_send6)
            .unwrap()
            .get("result")
            .is_some());

        // Now eth_getTransactionCount "pending" -> 0x7
        let res_pending_2 = disp.dispatch_json(&req_pending_init);
        let val_pending_2 = JsonValue::parse(&res_pending_2).unwrap();
        assert_eq!(val_pending_2.get("result").unwrap().as_str(), Some("0x7"));
    }

    #[test]
    fn test_p1_eth_max_priority_fee_per_gas() {
        let disp = setup_test_dispatcher();
        let req = r#"{"jsonrpc":"2.0","id":1,"method":"eth_maxPriorityFeePerGas","params":[]}"#;
        let res = disp.dispatch_json(req);
        let val = JsonValue::parse(&res).unwrap();
        assert_eq!(
            val.get("result").unwrap().as_str(),
            Some("0x3b9aca00") // 1 Gwei canonical policy
        );
    }

    #[test]
    fn test_p1_eth_fee_history_semantics() {
        let disp = setup_test_dispatcher();

        let base_fee = Amount::from_u128(1_000_000_000); // 1 Gwei
        let sender = Address([0xaa; 20]);

        // Construct block 1 with 2 transactions having different priority tips
        let tx1 = Transaction {
            chain_id: ChainId::MAINNET,
            nonce: Nonce(0),
            from: sender,
            to: Some(Address([0x11; 20])),
            value: Amount::ZERO,
            gas_limit: Gas(21_000),
            max_fee_per_gas: Amount::from_u128(10_000_000_000),
            max_priority_fee_per_gas: Amount::from_u128(2_000_000_000), // tip = 2 Gwei
            valid_until_block: BlockHeight(100),
            data: vec![],
            memo: None,
            signature: Signature::Secp256k1Eth {
                r: [1u8; 32],
                s: [2u8; 32],
                y_parity: 0,
                chain_id: 9999,
            },
            tx_type: 2,
            access_list: vec![],
        };

        let tx2 = Transaction {
            chain_id: ChainId::MAINNET,
            nonce: Nonce(1),
            from: sender,
            to: Some(Address([0x22; 20])),
            value: Amount::ZERO,
            gas_limit: Gas(21_000),
            max_fee_per_gas: Amount::from_u128(10_000_000_000),
            max_priority_fee_per_gas: Amount::from_u128(5_000_000_000), // tip = 5 Gwei
            valid_until_block: BlockHeight(100),
            data: vec![],
            memo: None,
            signature: Signature::Secp256k1Eth {
                r: [3u8; 32],
                s: [4u8; 32],
                y_parity: 0,
                chain_id: 9999,
            },
            tx_type: 2,
            access_list: vec![],
        };

        let b1 = Block {
            header: BlockHeader {
                parent_hash: Hash::ZERO,
                height: BlockHeight(1),
                epoch: Epoch(0),
                round: Round(1),
                timestamp: Timestamp(1000),
                proposer: Address([0x01; 20]),
                state_root: Hash::ZERO,
                txs_root: Hash::ZERO,
                receipts_root: Hash::ZERO,
                gas_used: Gas(42_000),
                gas_limit: Gas(30_000_000),
                base_fee_per_gas: base_fee,
                extra_data: vec![],
            },
            transactions: vec![tx1.clone(), tx2.clone()],
            last_commit_qc: None,
        };

        let r1 = Receipt {
            tx_id: compute_transaction_id(&tx1),
            success: true,
            gas_used: Gas(21_000),
            cumulative_gas_used: Gas(21_000),
            logs: vec![],
            return_data: vec![],
        };
        let r2 = Receipt {
            tx_id: compute_transaction_id(&tx2),
            success: true,
            gas_used: Gas(21_000),
            cumulative_gas_used: Gas(42_000),
            logs: vec![],
            return_data: vec![],
        };

        disp.record_block(b1, vec![r1, r2]);

        // Query fee history: 1 block, newest="latest", percentiles=[25, 75]
        let req = r#"{"jsonrpc":"2.0","id":2,"method":"eth_feeHistory","params":["0x1", "latest", [25, 75]]}"#;
        let res = disp.dispatch_json(req);
        let val = JsonValue::parse(&res).unwrap();
        let result = val.get("result").expect("Must return result object");

        assert_eq!(result.get("oldestBlock").unwrap().as_str(), Some("0x1"));

        // baseFeePerGas length must be N + 1 (1 block -> 2 values)
        let base_fees = result.get("baseFeePerGas").unwrap().as_array().unwrap();
        assert_eq!(base_fees.len(), 2);
        assert_eq!(base_fees[0].as_str(), Some("0x3b9aca00")); // 1 Gwei
        assert_eq!(base_fees[1].as_str(), Some("0x3b9aca00")); // next block base fee

        // gasUsedRatio length must be N (1 block -> 1 value: 42_000 / 30_000_000 = 0.0014)
        let ratios = result.get("gasUsedRatio").unwrap().as_array().unwrap();
        assert_eq!(ratios.len(), 1);
        let ratio_val = ratios[0].as_f64().unwrap();
        assert!((ratio_val - 0.0014).abs() < 1e-6);

        // reward length must be N (1 block -> 1 array of 2 percentiles)
        let rewards = result.get("reward").unwrap().as_array().unwrap();
        assert_eq!(rewards.len(), 1);
        let b1_rewards = rewards[0].as_array().unwrap();
        assert_eq!(b1_rewards.len(), 2);
        // Tips are sorted: [2 Gwei, 5 Gwei] = [0x77359400, 0x12a05f200]
        assert_eq!(b1_rewards[0].as_str(), Some("0x77359400")); // 2 Gwei
        assert_eq!(b1_rewards[1].as_str(), Some("0x12a05f200")); // 5 Gwei
    }

    #[test]
    fn test_p1_unknown_and_pending_tx_receipt_lookup() {
        let disp = setup_test_dispatcher();

        // 1. Random unknown transaction hash
        let rand_hash = Hash([0x99; 32]);
        let req_unknown_rec = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"eth_getTransactionReceipt","params":["{}"]}}"#,
            rand_hash.to_hex()
        );
        let res_unknown_rec = disp.dispatch_json(&req_unknown_rec);
        let val_unknown_rec = JsonValue::parse(&res_unknown_rec).unwrap();
        assert_eq!(val_unknown_rec.get("result").unwrap(), &JsonValue::Null);

        let req_unknown_tx = format!(
            r#"{{"jsonrpc":"2.0","id":2,"method":"eth_getTransactionByHash","params":["{}"]}}"#,
            rand_hash.to_hex()
        );
        let res_unknown_tx = disp.dispatch_json(&req_unknown_tx);
        let val_unknown_tx = JsonValue::parse(&res_unknown_tx).unwrap();
        assert_eq!(val_unknown_tx.get("result").unwrap(), &JsonValue::Null);

        // 2. Pending uncommitted transaction in mempool
        let kp = tola_crypto::Secp256k1Keypair::generate();
        let sender = kp.address();
        let mut acct = AccountState::new_with_balance(Amount::from_u128(1_000_000_000_000_000));
        acct.nonce = Nonce(0);
        disp.state_db.write().unwrap().set_account(sender, acct);
        let mut uncommitted_tx = Transaction {
            chain_id: ChainId::MAINNET,
            nonce: Nonce(0),
            from: sender,
            to: Some(Address([0x88; 20])),
            value: Amount::from_u128(100),
            gas_limit: Gas(21_000),
            max_fee_per_gas: Amount::from_u128(10_000_000_000),
            max_priority_fee_per_gas: Amount::from_u128(2_000_000_000),
            valid_until_block: BlockHeight(100),
            data: vec![],
            memo: None,
            signature: Signature::Ed25519([0u8; 64]),
            tx_type: 2,
            access_list: vec![],
        };
        let hash_to_sign = tola_codec::rlp::compute_type2_signing_hash(&uncommitted_tx);
        let sig = kp.sign_recoverable(hash_to_sign.as_bytes()).unwrap();
        let mut r = [0u8; 32];
        let mut s = [0u8; 32];
        r.copy_from_slice(&sig[0..32]);
        s.copy_from_slice(&sig[32..64]);
        let y_parity = sig[64];
        uncommitted_tx.signature = Signature::Secp256k1Eth {
            r,
            s,
            y_parity,
            chain_id: 9999,
        };
        let eth_hash = tola_codec::rlp::compute_eth_tx_hash(&uncommitted_tx).unwrap();
        let raw_bytes = tola_codec::rlp::encode_raw_eth_transaction(&uncommitted_tx).unwrap();

        // Send to mempool
        let req_send = format!(
            r#"{{"jsonrpc":"2.0","id":3,"method":"eth_sendRawTransaction","params":["0x{}"]}}"#,
            hex_encode(&raw_bytes)
        );
        let res_send = disp.dispatch_json(&req_send);
        assert!(JsonValue::parse(&res_send).unwrap().get("result").is_some());

        // Prior to block inclusion, receipt must return null!
        let req_pending_rec = format!(
            r#"{{"jsonrpc":"2.0","id":4,"method":"eth_getTransactionReceipt","params":["{}"]}}"#,
            eth_hash.to_hex()
        );
        let res_pending_rec = disp.dispatch_json(&req_pending_rec);
        let val_pending_rec = JsonValue::parse(&res_pending_rec).unwrap();
        assert_eq!(val_pending_rec.get("result").unwrap(), &JsonValue::Null);
    }

    #[test]
    fn test_p2_eth_get_logs_ranges_addresses_and_topics() {
        use tola_crypto::Secp256k1Keypair;

        let disp = setup_test_dispatcher();
        let kp = Secp256k1Keypair::generate();
        let sender = kp.address();

        let contract_a = Address([0x11; 20]);
        let contract_b = Address([0x22; 20]);
        let contract_c = Address([0x33; 20]);

        let event_a = Hash([0xaa; 32]);
        let event_b = Hash([0xbb; 32]);
        let event_c = Hash([0xcc; 32]);
        let topic_x = Hash([0x77; 32]);
        let topic_y = Hash([0x88; 32]);

        let make_signed_tx = |nonce: u64, to: Address| {
            let mut tx = Transaction {
                chain_id: ChainId::MAINNET,
                nonce: Nonce(nonce),
                from: sender,
                to: Some(to),
                value: Amount::ZERO,
                gas_limit: Gas(100_000),
                max_fee_per_gas: Amount::from_u128(1_000_000_000),
                max_priority_fee_per_gas: Amount::from_u128(100_000_000),
                valid_until_block: BlockHeight(100),
                data: vec![0x01],
                memo: None,
                signature: Signature::Ed25519([0u8; 64]),
                tx_type: 2,
                access_list: vec![],
            };
            let hash_to_sign = tola_codec::rlp::compute_type2_signing_hash(&tx);
            let sig = kp.sign_recoverable(hash_to_sign.as_bytes()).unwrap();
            let mut r = [0u8; 32];
            let mut s = [0u8; 32];
            r.copy_from_slice(&sig[0..32]);
            s.copy_from_slice(&sig[32..64]);
            tx.signature = Signature::Secp256k1Eth {
                r,
                s,
                y_parity: sig[64],
                chain_id: 9999,
            };
            tx
        };

        // Block 1: Tx 1 with Contract A, Event A
        let tx1 = make_signed_tx(0, contract_a);
        let tx1_id = compute_transaction_id(&tx1);
        let log1 = Log {
            address: contract_a,
            topics: vec![event_a],
            data: vec![0x11],
        };
        let r1 = Receipt {
            tx_id: tx1_id,
            success: true,
            gas_used: Gas(21_000),
            cumulative_gas_used: Gas(21_000),
            logs: vec![log1],
            return_data: vec![],
        };
        let b1 = Block {
            header: BlockHeader {
                parent_hash: Hash::ZERO,
                height: BlockHeight(1),
                epoch: Epoch(0),
                round: Round(1),
                timestamp: Timestamp(1000),
                proposer: Address([0x01; 20]),
                state_root: Hash([0x01; 32]),
                txs_root: Hash([0x01; 32]),
                receipts_root: Hash([0x01; 32]),
                gas_used: Gas(21_000),
                gas_limit: Gas(30_000_000),
                base_fee_per_gas: Amount::from_u128(1_000_000_000),
                extra_data: vec![],
            },
            transactions: vec![tx1],
            last_commit_qc: None,
        };
        disp.record_block(b1, vec![r1]);

        // Block 2: Tx 2 (Contract B, Event B + Topic X) & Tx 3 (Contract A, Event A)
        let tx2 = make_signed_tx(1, contract_b);
        let tx2_id = compute_transaction_id(&tx2);
        let log2 = Log {
            address: contract_b,
            topics: vec![event_b, topic_x],
            data: vec![0x22],
        };
        let r2 = Receipt {
            tx_id: tx2_id,
            success: true,
            gas_used: Gas(21_000),
            cumulative_gas_used: Gas(21_000),
            logs: vec![log2],
            return_data: vec![],
        };

        let tx3 = make_signed_tx(2, contract_a);
        let tx3_id = compute_transaction_id(&tx3);
        let log3 = Log {
            address: contract_a,
            topics: vec![event_a],
            data: vec![0x33],
        };
        let r3 = Receipt {
            tx_id: tx3_id,
            success: true,
            gas_used: Gas(21_000),
            cumulative_gas_used: Gas(42_000),
            logs: vec![log3],
            return_data: vec![],
        };

        let b2 = Block {
            header: BlockHeader {
                parent_hash: Hash([0x01; 32]),
                height: BlockHeight(2),
                epoch: Epoch(0),
                round: Round(2),
                timestamp: Timestamp(2000),
                proposer: Address([0x01; 20]),
                state_root: Hash([0x02; 32]),
                txs_root: Hash([0x02; 32]),
                receipts_root: Hash([0x02; 32]),
                gas_used: Gas(42_000),
                gas_limit: Gas(30_000_000),
                base_fee_per_gas: Amount::from_u128(1_000_000_000),
                extra_data: vec![],
            },
            transactions: vec![tx2, tx3],
            last_commit_qc: None,
        };
        disp.record_block(b2, vec![r2, r3]);

        // Block 3: Tx 4 (Contract C, Event C + Topic Y)
        let tx4 = make_signed_tx(3, contract_c);
        let tx4_id = compute_transaction_id(&tx4);
        let log4 = Log {
            address: contract_c,
            topics: vec![event_c, topic_y],
            data: vec![0x44],
        };
        let r4 = Receipt {
            tx_id: tx4_id,
            success: true,
            gas_used: Gas(21_000),
            cumulative_gas_used: Gas(21_000),
            logs: vec![log4],
            return_data: vec![],
        };
        let b3 = Block {
            header: BlockHeader {
                parent_hash: Hash([0x02; 32]),
                height: BlockHeight(3),
                epoch: Epoch(0),
                round: Round(3),
                timestamp: Timestamp(3000),
                proposer: Address([0x01; 20]),
                state_root: Hash([0x03; 32]),
                txs_root: Hash([0x03; 32]),
                receipts_root: Hash([0x03; 32]),
                gas_used: Gas(21_000),
                gas_limit: Gas(30_000_000),
                base_fee_per_gas: Amount::from_u128(1_000_000_000),
                extra_data: vec![],
            },
            transactions: vec![tx4],
            last_commit_qc: None,
        };
        disp.record_block(b3, vec![r4]);

        // 1. Range query fromBlock: 1, toBlock: 2 -> 3 logs
        let req1 = r#"{"jsonrpc":"2.0","id":10,"method":"eth_getLogs","params":[{"fromBlock":"0x1","toBlock":"0x2"}]}"#;
        let res1 = disp.dispatch_json(req1);
        let val1 = JsonValue::parse(&res1).unwrap();
        let logs1 = val1.get("result").unwrap().as_array().unwrap();
        assert_eq!(logs1.len(), 3);

        // 2. Address filter: Contract A -> 2 logs (Log 1 from Block 1, Log 3 from Block 2)
        let req_addr = format!(
            r#"{{"jsonrpc":"2.0","id":11,"method":"eth_getLogs","params":[{{
                "fromBlock": "0x1",
                "toBlock": "0x3",
                "address": "{}"
            }}]}}"#,
            contract_a.to_hex()
        );
        let res_addr = disp.dispatch_json(&req_addr);
        let val_addr = JsonValue::parse(&res_addr).unwrap();
        let logs_addr = val_addr.get("result").unwrap().as_array().unwrap();
        assert_eq!(logs_addr.len(), 2);
        assert_eq!(
            logs_addr[0].get("address").and_then(|v| v.as_str()),
            Some(contract_a.to_hex().as_str())
        );
        assert_eq!(
            logs_addr[1].get("address").and_then(|v| v.as_str()),
            Some(contract_a.to_hex().as_str())
        );

        // 3. Multi-address filter: [Contract A, Contract B] -> 3 logs
        let req_multi_addr = format!(
            r#"{{"jsonrpc":"2.0","id":12,"method":"eth_getLogs","params":[{{
                "fromBlock": "0x1",
                "toBlock": "0x3",
                "address": ["{}", "{}"]
            }}]}}"#,
            contract_a.to_hex(),
            contract_b.to_hex()
        );
        let res_multi = disp.dispatch_json(&req_multi_addr);
        let val_multi = JsonValue::parse(&res_multi).unwrap();
        let logs_multi = val_multi.get("result").unwrap().as_array().unwrap();
        assert_eq!(logs_multi.len(), 3);

        // 4. Exact topic0 filter: Event A -> 2 logs
        let req_topic = format!(
            r#"{{"jsonrpc":"2.0","id":13,"method":"eth_getLogs","params":[{{
                "fromBlock": "0x1",
                "toBlock": "0x3",
                "topics": ["{}"]
            }}]}}"#,
            event_a.to_hex()
        );
        let res_topic = disp.dispatch_json(&req_topic);
        let val_topic = JsonValue::parse(&res_topic).unwrap();
        let logs_topic = val_topic.get("result").unwrap().as_array().unwrap();
        assert_eq!(logs_topic.len(), 2);

        // 5. Wildcard topic0, exact topic1: [null, Topic Y] -> 1 log
        let req_wild = format!(
            r#"{{"jsonrpc":"2.0","id":14,"method":"eth_getLogs","params":[{{
                "fromBlock": "0x1",
                "toBlock": "0x3",
                "topics": [null, "{}"]
            }}]}}"#,
            topic_y.to_hex()
        );
        let res_wild = disp.dispatch_json(&req_wild);
        let val_wild = JsonValue::parse(&res_wild).unwrap();
        let logs_wild = val_wild.get("result").unwrap().as_array().unwrap();
        assert_eq!(logs_wild.len(), 1);
        assert_eq!(
            logs_wild[0].get("address").and_then(|v| v.as_str()),
            Some(contract_c.to_hex().as_str())
        );

        // 6. Nested array OR topic filter: [[Event A, Event C]] -> 3 logs (Log 1, Log 3, Log 4)
        let req_or = format!(
            r#"{{"jsonrpc":"2.0","id":15,"method":"eth_getLogs","params":[{{
                "fromBlock": "0x1",
                "toBlock": "0x3",
                "topics": [["{}", "{}"]]
            }}]}}"#,
            event_a.to_hex(),
            event_c.to_hex()
        );
        let res_or = disp.dispatch_json(&req_or);
        let val_or = JsonValue::parse(&res_or).unwrap();
        let logs_or = val_or.get("result").unwrap().as_array().unwrap();
        assert_eq!(logs_or.len(), 3);

        // 7. Non-matching topic filter -> empty array
        let req_mismatch = r#"{"jsonrpc":"2.0","id":16,"method":"eth_getLogs","params":[{"fromBlock":"0x1","toBlock":"0x3","topics":["0x1111111111111111111111111111111111111111111111111111111111111111"]}]}"#;
        let res_mismatch = disp.dispatch_json(req_mismatch);
        let val_mismatch = JsonValue::parse(&res_mismatch).unwrap();
        let logs_mismatch = val_mismatch.get("result").unwrap().as_array().unwrap();
        assert_eq!(logs_mismatch.len(), 0);

        // 8. fromBlock > toBlock -> empty array
        let req_inv = r#"{"jsonrpc":"2.0","id":17,"method":"eth_getLogs","params":[{"fromBlock":"0x3","toBlock":"0x1"}]}"#;
        let res_inv = disp.dispatch_json(req_inv);
        let val_inv = JsonValue::parse(&res_inv).unwrap();
        let logs_inv = val_inv.get("result").unwrap().as_array().unwrap();
        assert_eq!(logs_inv.len(), 0);
    }

    #[test]
    fn test_p2_eth_get_logs_block_hash_and_errors() {
        let disp = setup_test_dispatcher();

        let contract_a = Address([0x44; 20]);
        let event_a = Hash([0x55; 32]);

        let tx = Transaction {
            chain_id: ChainId::MAINNET,
            nonce: Nonce(0),
            from: Address([0x01; 20]),
            to: Some(contract_a),
            value: Amount::ZERO,
            gas_limit: Gas(21_000),
            max_fee_per_gas: Amount::from_u128(1_000_000_000),
            max_priority_fee_per_gas: Amount::ZERO,
            valid_until_block: BlockHeight(100),
            data: vec![],
            memo: None,
            tx_type: 2,
            access_list: vec![],
            signature: Signature::Ed25519([0u8; 64]),
        };
        let tx_id = compute_transaction_id(&tx);
        let log = Log {
            address: contract_a,
            topics: vec![event_a],
            data: vec![0x99],
        };
        let receipt = Receipt {
            tx_id,
            success: true,
            gas_used: Gas(21_000),
            cumulative_gas_used: Gas(21_000),
            logs: vec![log],
            return_data: vec![],
        };
        let block = Block {
            header: BlockHeader {
                parent_hash: Hash::ZERO,
                height: BlockHeight(1),
                epoch: Epoch(0),
                round: Round(1),
                timestamp: Timestamp(1000),
                proposer: Address([0x01; 20]),
                state_root: Hash([0xaa; 32]),
                txs_root: Hash([0xbb; 32]),
                receipts_root: Hash([0xcc; 32]),
                gas_used: Gas(21_000),
                gas_limit: Gas(30_000_000),
                base_fee_per_gas: Amount::from_u128(1_000_000_000),
                extra_data: vec![],
            },
            transactions: vec![tx],
            last_commit_qc: None,
        };
        let block_hash = tola_consensus::PipelinedBftEngine::compute_block_hash(&block);
        disp.record_block(block, vec![receipt]);

        // 1. Query by valid blockHash
        let req_bh = format!(
            r#"{{"jsonrpc":"2.0","id":20,"method":"eth_getLogs","params":[{{ "blockHash": "{}" }}]}}"#,
            block_hash.to_hex()
        );
        let res_bh = disp.dispatch_json(&req_bh);
        let val_bh = JsonValue::parse(&res_bh).unwrap();
        let logs_bh = val_bh.get("result").unwrap().as_array().unwrap();
        assert_eq!(logs_bh.len(), 1);
        assert_eq!(
            logs_bh[0].get("blockHash").and_then(|v| v.as_str()),
            Some(block_hash.to_hex().as_str())
        );

        // 2. Query by unknown blockHash -> error -32000 "unknown block"
        let unknown_h = Hash([0xfe; 32]);
        let req_unk = format!(
            r#"{{"jsonrpc":"2.0","id":21,"method":"eth_getLogs","params":[{{ "blockHash": "{}" }}]}}"#,
            unknown_h.to_hex()
        );
        let res_unk = disp.dispatch_json(&req_unk);
        let val_unk = JsonValue::parse(&res_unk).unwrap();
        let err_unk = val_unk.get("error").unwrap();
        assert_eq!(err_unk.get("code").and_then(|v| v.as_i64()), Some(-32000));
        assert_eq!(
            err_unk.get("message").and_then(|v| v.as_str()),
            Some("unknown block")
        );

        // 3. Mutually exclusive blockHash with fromBlock -> error invalid params
        let req_conflict = format!(
            r#"{{"jsonrpc":"2.0","id":22,"method":"eth_getLogs","params":[{{ "blockHash": "{}", "fromBlock": "0x1" }}]}}"#,
            block_hash.to_hex()
        );
        let res_conflict = disp.dispatch_json(&req_conflict);
        let val_conflict = JsonValue::parse(&res_conflict).unwrap();
        assert!(val_conflict.get("error").is_some());
        assert_eq!(
            val_conflict
                .get("error")
                .unwrap()
                .get("code")
                .and_then(|v| v.as_i64()),
            Some(-32602)
        );

        // 4. Exceeds MAX_LOG_BLOCK_RANGE (2000 blocks) -> error -32005
        let req_range = r#"{"jsonrpc":"2.0","id":23,"method":"eth_getLogs","params":[{"fromBlock":"0x0","toBlock":"0x7d1"}]}"#; // 0..2001 = 2002 blocks
        let res_range = disp.dispatch_json(req_range);
        let val_range = JsonValue::parse(&res_range).unwrap();
        let err_range = val_range.get("error").unwrap();
        assert_eq!(err_range.get("code").and_then(|v| v.as_i64()), Some(-32005));
        assert!(err_range
            .get("message")
            .unwrap()
            .as_str()
            .unwrap()
            .contains("query exceeds maximum block range of 2000"));
    }

    #[test]
    fn test_p2_eth_filters_polling_lifecycle_and_cumulative_log_index() {
        use tola_crypto::Secp256k1Keypair;

        let disp = setup_test_dispatcher();
        let kp = Secp256k1Keypair::generate();
        let sender = kp.address();

        let contract_x = Address([0x77; 20]);
        let event_x = Hash([0x88; 32]);

        let make_signed_tx = |nonce: u64| {
            let mut tx = Transaction {
                chain_id: ChainId::MAINNET,
                nonce: Nonce(nonce),
                from: sender,
                to: Some(contract_x),
                value: Amount::ZERO,
                gas_limit: Gas(100_000),
                max_fee_per_gas: Amount::from_u128(1_000_000_000),
                max_priority_fee_per_gas: Amount::from_u128(100_000_000),
                valid_until_block: BlockHeight(100),
                data: vec![0x55],
                memo: None,
                tx_type: 2,
                access_list: vec![],
                signature: Signature::Ed25519([0u8; 64]),
            };
            let hash_to_sign = tola_codec::rlp::compute_type2_signing_hash(&tx);
            let sig = kp.sign_recoverable(hash_to_sign.as_bytes()).unwrap();
            let mut r = [0u8; 32];
            let mut s = [0u8; 32];
            r.copy_from_slice(&sig[0..32]);
            s.copy_from_slice(&sig[32..64]);
            tx.signature = Signature::Secp256k1Eth {
                r,
                s,
                y_parity: sig[64],
                chain_id: 9999,
            };
            tx
        };

        // Create Block 1 with 2 transactions, both emitting logs:
        // Tx A: 2 logs (index 0 and index 1 in block)
        // Tx B: 1 log (index 2 in block)
        let tx_a = make_signed_tx(0);
        let tx_a_id = compute_transaction_id(&tx_a);
        let eth_hash_a = tola_codec::rlp::compute_eth_tx_hash(&tx_a).unwrap();
        let r_a = Receipt {
            tx_id: tx_a_id,
            success: true,
            gas_used: Gas(21_000),
            cumulative_gas_used: Gas(21_000),
            logs: vec![
                Log {
                    address: contract_x,
                    topics: vec![event_x],
                    data: vec![0x01],
                },
                Log {
                    address: contract_x,
                    topics: vec![event_x],
                    data: vec![0x02],
                },
            ],
            return_data: vec![],
        };

        let tx_b = make_signed_tx(1);
        let tx_b_id = compute_transaction_id(&tx_b);
        let eth_hash_b = tola_codec::rlp::compute_eth_tx_hash(&tx_b).unwrap();
        let r_b = Receipt {
            tx_id: tx_b_id,
            success: true,
            gas_used: Gas(21_000),
            cumulative_gas_used: Gas(42_000),
            logs: vec![Log {
                address: contract_x,
                topics: vec![event_x],
                data: vec![0x03],
            }],
            return_data: vec![],
        };

        let b1 = Block {
            header: BlockHeader {
                parent_hash: Hash::ZERO,
                height: BlockHeight(1),
                epoch: Epoch(0),
                round: Round(1),
                timestamp: Timestamp(1000),
                proposer: Address([0x01; 20]),
                state_root: Hash([0x11; 32]),
                txs_root: Hash([0x11; 32]),
                receipts_root: Hash([0x11; 32]),
                gas_used: Gas(42_000),
                gas_limit: Gas(30_000_000),
                base_fee_per_gas: Amount::from_u128(1_000_000_000),
                extra_data: vec![],
            },
            transactions: vec![tx_a, tx_b],
            last_commit_qc: None,
        };
        disp.record_block(b1, vec![r_a, r_b]);

        // Verify cumulative logIndex in eth_getLogs
        let req_logs = r#"{"jsonrpc":"2.0","id":30,"method":"eth_getLogs","params":[{"fromBlock":"0x1","toBlock":"0x1"}]}"#;
        let res_logs = disp.dispatch_json(req_logs);
        let val_logs = JsonValue::parse(&res_logs).unwrap();
        let logs = val_logs.get("result").unwrap().as_array().unwrap();
        assert_eq!(logs.len(), 3);
        assert_eq!(logs[0].get("logIndex").unwrap().as_str(), Some("0x0"));
        assert_eq!(
            logs[0].get("transactionIndex").unwrap().as_str(),
            Some("0x0")
        );
        assert_eq!(
            logs[0].get("transactionHash").unwrap().as_str(),
            Some(eth_hash_a.to_hex().as_str())
        );

        assert_eq!(logs[1].get("logIndex").unwrap().as_str(), Some("0x1"));
        assert_eq!(
            logs[1].get("transactionIndex").unwrap().as_str(),
            Some("0x0")
        );

        assert_eq!(logs[2].get("logIndex").unwrap().as_str(), Some("0x2"));
        assert_eq!(
            logs[2].get("transactionIndex").unwrap().as_str(),
            Some("0x1")
        );
        assert_eq!(
            logs[2].get("transactionHash").unwrap().as_str(),
            Some(eth_hash_b.to_hex().as_str())
        );

        // Verify receipt for Tx B also has logIndex = "0x2"
        let req_rec_b = format!(
            r#"{{"jsonrpc":"2.0","id":31,"method":"eth_getTransactionReceipt","params":["{}"]}}"#,
            eth_hash_b.to_hex()
        );
        let res_rec_b = disp.dispatch_json(&req_rec_b);
        let val_rec_b = JsonValue::parse(&res_rec_b).unwrap();
        let rec_b_logs = val_rec_b
            .get("result")
            .unwrap()
            .get("logs")
            .unwrap()
            .as_array()
            .unwrap();
        assert_eq!(rec_b_logs.len(), 1);
        assert_eq!(rec_b_logs[0].get("logIndex").unwrap().as_str(), Some("0x2"));

        // 1. Create a new filter starting from block 1
        let req_new_filter =
            r#"{"jsonrpc":"2.0","id":32,"method":"eth_newFilter","params":[{"fromBlock":"0x1"}]}"#;
        let res_new_filter = disp.dispatch_json(req_new_filter);
        let val_new_filter = JsonValue::parse(&res_new_filter).unwrap();
        let filter_id = val_new_filter.get("result").unwrap().as_str().unwrap();
        assert_eq!(filter_id, "0x1");

        // 2. Poll filter changes: first poll returns the 3 logs from block 1
        let req_changes = format!(
            r#"{{"jsonrpc":"2.0","id":33,"method":"eth_getFilterChanges","params":["{}"]}}"#,
            filter_id
        );
        let res_changes = disp.dispatch_json(&req_changes);
        let val_changes = JsonValue::parse(&res_changes).unwrap();
        let ch_logs = val_changes.get("result").unwrap().as_array().unwrap();
        assert_eq!(ch_logs.len(), 3);

        // 3. Poll immediately again: empty
        let res_empty = disp.dispatch_json(&req_changes);
        let val_empty = JsonValue::parse(&res_empty).unwrap();
        assert_eq!(
            val_empty.get("result").unwrap().as_array().unwrap().len(),
            0
        );

        // 4. eth_getFilterLogs: returns all matching logs without consuming cursor
        let req_filter_logs = format!(
            r#"{{"jsonrpc":"2.0","id":34,"method":"eth_getFilterLogs","params":["{}"]}}"#,
            filter_id
        );
        let res_fl = disp.dispatch_json(&req_filter_logs);
        let val_fl = JsonValue::parse(&res_fl).unwrap();
        assert_eq!(val_fl.get("result").unwrap().as_array().unwrap().len(), 3);

        // 5. Add Block 2 with 1 new log
        let tx_c = make_signed_tx(2);
        let tx_c_id = compute_transaction_id(&tx_c);
        let r_c = Receipt {
            tx_id: tx_c_id,
            success: true,
            gas_used: Gas(21_000),
            cumulative_gas_used: Gas(21_000),
            logs: vec![Log {
                address: contract_x,
                topics: vec![event_x],
                data: vec![0x99],
            }],
            return_data: vec![],
        };
        let b2 = Block {
            header: BlockHeader {
                parent_hash: Hash([0x11; 32]),
                height: BlockHeight(2),
                epoch: Epoch(0),
                round: Round(2),
                timestamp: Timestamp(2000),
                proposer: Address([0x01; 20]),
                state_root: Hash([0x22; 32]),
                txs_root: Hash([0x22; 32]),
                receipts_root: Hash([0x22; 32]),
                gas_used: Gas(21_000),
                gas_limit: Gas(30_000_000),
                base_fee_per_gas: Amount::from_u128(1_000_000_000),
                extra_data: vec![],
            },
            transactions: vec![tx_c],
            last_commit_qc: None,
        };
        disp.record_block(b2, vec![r_c]);

        // 6. Poll filter changes: returns ONLY the 1 new log from block 2!
        let res_new_ch = disp.dispatch_json(&req_changes);
        let val_new_ch = JsonValue::parse(&res_new_ch).unwrap();
        let new_logs = val_new_ch.get("result").unwrap().as_array().unwrap();
        assert_eq!(new_logs.len(), 1);
        assert_eq!(
            new_logs[0].get("blockNumber").unwrap().as_str(),
            Some("0x2")
        );
        assert_eq!(new_logs[0].get("data").unwrap().as_str(), Some("0x99"));

        // 7. Uninstall filter
        let req_un = format!(
            r#"{{"jsonrpc":"2.0","id":35,"method":"eth_uninstallFilter","params":["{}"]}}"#,
            filter_id
        );
        let res_un = disp.dispatch_json(&req_un);
        let val_un = JsonValue::parse(&res_un).unwrap();
        assert_eq!(val_un.get("result").unwrap().as_bool(), Some(true));

        // 8. Poll uninstalled filter -> error -32000 "filter not found"
        let res_post_un = disp.dispatch_json(&req_changes);
        let val_post_un = JsonValue::parse(&res_post_un).unwrap();
        assert_eq!(
            val_post_un
                .get("error")
                .unwrap()
                .get("code")
                .and_then(|v| v.as_i64()),
            Some(-32000)
        );

        // 9. Uninstall already removed filter -> returns false
        let res_un2 = disp.dispatch_json(&req_un);
        let val_un2 = JsonValue::parse(&res_un2).unwrap();
        assert_eq!(val_un2.get("result").unwrap().as_bool(), Some(false));
    }

    #[test]
    fn test_p3_rpc_compatibility_suite() {
        let state = Arc::new(RwLock::new(tola_state::MemoryStateJournal::new()));
        let mempool = Arc::new(RwLock::new(Mempool::new(MempoolConfig::new(ChainId(9999)))));
        let disp = RpcDispatcher::new(
            ChainId(9999),
            "TOLA/v0.1.0-alpha".into(),
            state.clone(),
            mempool.clone(),
        );

        // 1. eth_accounts returns empty array
        let res_accounts =
            disp.dispatch_json(r#"{"jsonrpc":"2.0","id":1,"method":"eth_accounts","params":[]}"#);
        let val_accounts = JsonValue::parse(&res_accounts).unwrap();
        assert_eq!(
            val_accounts
                .get("result")
                .unwrap()
                .as_array()
                .unwrap()
                .len(),
            0
        );

        // 2. eth_syncing returns false
        let res_syncing =
            disp.dispatch_json(r#"{"jsonrpc":"2.0","id":2,"method":"eth_syncing","params":[]}"#);
        let val_syncing = JsonValue::parse(&res_syncing).unwrap();
        assert_eq!(val_syncing.get("result").unwrap().as_bool(), Some(false));

        // 3. eth_protocolVersion returns "0x41"
        let res_proto = disp.dispatch_json(
            r#"{"jsonrpc":"2.0","id":3,"method":"eth_protocolVersion","params":[]}"#,
        );
        let val_proto = JsonValue::parse(&res_proto).unwrap();
        assert_eq!(val_proto.get("result").unwrap().as_str(), Some("0x41"));

        // Setup a block with 2 transactions
        let tx0 = Transaction {
            chain_id: ChainId(9999),
            nonce: Nonce(0),
            from: Address([0x11; 20]),
            to: Some(Address([0x22; 20])),
            value: Amount::from_u128(100),
            gas_limit: Gas(21_000),
            max_fee_per_gas: Amount::from_u128(1_000_000_000),
            max_priority_fee_per_gas: Amount::from_u128(100_000_000),
            valid_until_block: BlockHeight(100),
            data: vec![],
            memo: None,
            tx_type: 2,
            access_list: vec![],
            signature: Signature::Secp256k1Eth {
                r: [0x01; 32],
                s: [0x02; 32],
                y_parity: 0,
                chain_id: 9999,
            },
        };
        let tx1 = Transaction {
            chain_id: ChainId(9999),
            nonce: Nonce(1),
            from: Address([0x11; 20]),
            to: Some(Address([0x33; 20])),
            value: Amount::from_u128(200),
            gas_limit: Gas(21_000),
            max_fee_per_gas: Amount::from_u128(1_000_000_000),
            max_priority_fee_per_gas: Amount::from_u128(100_000_000),
            valid_until_block: BlockHeight(100),
            data: vec![],
            memo: None,
            tx_type: 2,
            access_list: vec![],
            signature: Signature::Secp256k1Eth {
                r: [0x03; 32],
                s: [0x04; 32],
                y_parity: 1,
                chain_id: 9999,
            },
        };

        let block1 = Block {
            header: BlockHeader {
                parent_hash: Hash::ZERO,
                height: BlockHeight(1),
                epoch: Epoch(0),
                round: Round(1),
                timestamp: Timestamp(1000),
                proposer: Address([0xaa; 20]),
                state_root: Hash([0xbb; 32]),
                txs_root: Hash([0xcc; 32]),
                receipts_root: Hash([0xdd; 32]),
                gas_used: Gas(42_000),
                gas_limit: Gas(30_000_000),
                base_fee_per_gas: Amount::from_u128(1_000_000_000),
                extra_data: vec![],
            },
            transactions: vec![tx0.clone(), tx1.clone()],
            last_commit_qc: None,
        };

        let block1_hash = tola_consensus::PipelinedBftEngine::compute_block_hash(&block1);
        let r0 = Receipt {
            tx_id: compute_transaction_id(&tx0),
            success: true,
            gas_used: Gas(21_000),
            cumulative_gas_used: Gas(21_000),
            logs: vec![],
            return_data: vec![],
        };
        let r1 = Receipt {
            tx_id: compute_transaction_id(&tx1),
            success: true,
            gas_used: Gas(21_000),
            cumulative_gas_used: Gas(42_000),
            logs: vec![],
            return_data: vec![],
        };

        disp.record_block(block1, vec![r0, r1]);

        // 4. eth_getBlockTransactionCountByNumber
        let res_cnt_num = disp.dispatch_json(r#"{"jsonrpc":"2.0","id":4,"method":"eth_getBlockTransactionCountByNumber","params":["0x1"]}"#);
        let val_cnt_num = JsonValue::parse(&res_cnt_num).unwrap();
        assert_eq!(val_cnt_num.get("result").unwrap().as_str(), Some("0x2"));

        let res_cnt_latest = disp.dispatch_json(r#"{"jsonrpc":"2.0","id":5,"method":"eth_getBlockTransactionCountByNumber","params":["latest"]}"#);
        let val_cnt_latest = JsonValue::parse(&res_cnt_latest).unwrap();
        assert_eq!(val_cnt_latest.get("result").unwrap().as_str(), Some("0x2"));

        let res_cnt_none = disp.dispatch_json(r#"{"jsonrpc":"2.0","id":6,"method":"eth_getBlockTransactionCountByNumber","params":["0x999"]}"#);
        let val_cnt_none = JsonValue::parse(&res_cnt_none).unwrap();
        assert!(matches!(val_cnt_none.get("result"), Some(JsonValue::Null)));

        // 5. eth_getBlockTransactionCountByHash
        let req_cnt_hash = format!(
            r#"{{"jsonrpc":"2.0","id":7,"method":"eth_getBlockTransactionCountByHash","params":["{}"]}}"#,
            block1_hash.to_hex()
        );
        let res_cnt_hash = disp.dispatch_json(&req_cnt_hash);
        let val_cnt_hash = JsonValue::parse(&res_cnt_hash).unwrap();
        assert_eq!(val_cnt_hash.get("result").unwrap().as_str(), Some("0x2"));

        let res_cnt_bad_hash = disp.dispatch_json(r#"{"jsonrpc":"2.0","id":8,"method":"eth_getBlockTransactionCountByHash","params":["0x1111111111111111111111111111111111111111111111111111111111111111"]}"#);
        let val_cnt_bad_hash = JsonValue::parse(&res_cnt_bad_hash).unwrap();
        assert!(matches!(
            val_cnt_bad_hash.get("result"),
            Some(JsonValue::Null)
        ));

        // 6. eth_getTransactionByBlockNumberAndIndex
        let res_tx_num0 = disp.dispatch_json(r#"{"jsonrpc":"2.0","id":9,"method":"eth_getTransactionByBlockNumberAndIndex","params":["0x1","0x0"]}"#);
        let val_tx_num0 = JsonValue::parse(&res_tx_num0).unwrap();
        assert_eq!(
            val_tx_num0
                .get("result")
                .unwrap()
                .get("transactionIndex")
                .unwrap()
                .as_str(),
            Some("0x0")
        );
        assert_eq!(
            val_tx_num0
                .get("result")
                .unwrap()
                .get("value")
                .unwrap()
                .as_str(),
            Some("0x64")
        );

        let res_tx_num1 = disp.dispatch_json(r#"{"jsonrpc":"2.0","id":10,"method":"eth_getTransactionByBlockNumberAndIndex","params":["0x1","0x1"]}"#);
        let val_tx_num1 = JsonValue::parse(&res_tx_num1).unwrap();
        assert_eq!(
            val_tx_num1
                .get("result")
                .unwrap()
                .get("transactionIndex")
                .unwrap()
                .as_str(),
            Some("0x1")
        );
        assert_eq!(
            val_tx_num1
                .get("result")
                .unwrap()
                .get("value")
                .unwrap()
                .as_str(),
            Some("0xc8")
        );

        let res_tx_oob = disp.dispatch_json(r#"{"jsonrpc":"2.0","id":11,"method":"eth_getTransactionByBlockNumberAndIndex","params":["0x1","0x2"]}"#);
        let val_tx_oob = JsonValue::parse(&res_tx_oob).unwrap();
        assert!(matches!(val_tx_oob.get("result"), Some(JsonValue::Null)));

        // 7. eth_getTransactionByBlockHashAndIndex
        let req_tx_hash0 = format!(
            r#"{{"jsonrpc":"2.0","id":12,"method":"eth_getTransactionByBlockHashAndIndex","params":["{}","0x0"]}}"#,
            block1_hash.to_hex()
        );
        let res_tx_hash0 = disp.dispatch_json(&req_tx_hash0);
        let val_tx_hash0 = JsonValue::parse(&res_tx_hash0).unwrap();
        assert_eq!(
            val_tx_hash0
                .get("result")
                .unwrap()
                .get("transactionIndex")
                .unwrap()
                .as_str(),
            Some("0x0")
        );

        let req_tx_hash_oob = format!(
            r#"{{"jsonrpc":"2.0","id":13,"method":"eth_getTransactionByBlockHashAndIndex","params":["{}","0x5"]}}"#,
            block1_hash.to_hex()
        );
        let res_tx_hash_oob = disp.dispatch_json(&req_tx_hash_oob);
        let val_tx_hash_oob = JsonValue::parse(&res_tx_hash_oob).unwrap();
        assert!(matches!(
            val_tx_hash_oob.get("result"),
            Some(JsonValue::Null)
        ));

        // 8. Uncle methods
        let req_uncle_hash = format!(
            r#"{{"jsonrpc":"2.0","id":14,"method":"eth_getUncleCountByBlockHash","params":["{}"]}}"#,
            block1_hash.to_hex()
        );
        let res_uncle_hash = disp.dispatch_json(&req_uncle_hash);
        let val_uncle_hash = JsonValue::parse(&res_uncle_hash).unwrap();
        assert_eq!(val_uncle_hash.get("result").unwrap().as_str(), Some("0x0"));

        let res_uncle_num = disp.dispatch_json(r#"{"jsonrpc":"2.0","id":15,"method":"eth_getUncleCountByBlockNumber","params":["0x1"]}"#);
        let val_uncle_num = JsonValue::parse(&res_uncle_num).unwrap();
        assert_eq!(val_uncle_num.get("result").unwrap().as_str(), Some("0x0"));

        let req_uncle_body_hash = format!(
            r#"{{"jsonrpc":"2.0","id":16,"method":"eth_getUncleByBlockHashAndIndex","params":["{}","0x0"]}}"#,
            block1_hash.to_hex()
        );
        let res_uncle_body_hash = disp.dispatch_json(&req_uncle_body_hash);
        let val_uncle_body_hash = JsonValue::parse(&res_uncle_body_hash).unwrap();
        assert!(matches!(
            val_uncle_body_hash.get("result"),
            Some(JsonValue::Null)
        ));

        let res_uncle_body_num = disp.dispatch_json(r#"{"jsonrpc":"2.0","id":17,"method":"eth_getUncleByBlockNumberAndIndex","params":["0x1","0x0"]}"#);
        let val_uncle_body_num = JsonValue::parse(&res_uncle_body_num).unwrap();
        assert!(matches!(
            val_uncle_body_num.get("result"),
            Some(JsonValue::Null)
        ));
    }

    #[test]
    fn test_p10_access_list_and_type1_rpc_support() {
        let disp = setup_test_dispatcher();

        // 1. Verify eth_estimateGas parses non-zero value and accessList
        let req_estimate = r#"{
            "jsonrpc": "2.0",
            "id": 1,
            "method": "eth_estimateGas",
            "params": [{
                "from": "0x1111111111111111111111111111111111111111",
                "to": "0x2222222222222222222222222222222222222222",
                "value": "0xde0b6b3a7640000",
                "accessList": [
                    {
                        "address": "0x3333333333333333333333333333333333333333",
                        "storageKeys": [
                            "0x0000000000000000000000000000000000000000000000000000000000000001"
                        ]
                    }
                ]
            }]
        }"#;
        let res_estimate = disp.dispatch_json(req_estimate);
        let val_estimate = JsonValue::parse(&res_estimate).unwrap();
        assert!(
            val_estimate.get("result").is_some(),
            "eth_estimateGas with value and accessList must succeed: {res_estimate}"
        );
        let gas_hex = val_estimate.get("result").unwrap().as_str().unwrap();
        assert!(gas_hex.starts_with("0x"));

        // 2. Setup a block with a Type-1 (EIP-2930) transaction containing an access list
        let sender = Address([0xaa; 20]);
        let al_item = AccessListItem {
            address: Address([0x44; 20]),
            storage_keys: vec![Hash([0x55; 32])],
        };
        let tx_type1 = Transaction {
            chain_id: ChainId::MAINNET,
            nonce: Nonce(2),
            from: sender,
            to: Some(Address([0x88; 20])),
            value: Amount::from_u128(100),
            gas_limit: Gas(50_000),
            max_fee_per_gas: Amount::from_u128(20_000_000_000),
            max_priority_fee_per_gas: Amount::from_u128(20_000_000_000),
            valid_until_block: BlockHeight(100),
            data: vec![0x11, 0x22],
            memo: None,
            signature: Signature::Secp256k1Eth {
                r: [0x01; 32],
                s: [0x02; 32],
                y_parity: 1,
                chain_id: ChainId::MAINNET.0,
            },
            tx_type: 1,
            access_list: vec![al_item],
        };

        let tx_id = compute_transaction_id(&tx_type1);
        let block = Block {
            header: BlockHeader {
                parent_hash: Hash::ZERO,
                height: BlockHeight(2),
                epoch: Epoch(0),
                round: Round(0),
                timestamp: Timestamp(200),
                proposer: Address([0x01; 20]),
                state_root: Hash::ZERO,
                txs_root: Hash::ZERO,
                receipts_root: Hash::ZERO,
                gas_used: Gas(21_000),
                gas_limit: Gas(30_000_000),
                base_fee_per_gas: Amount::from_u128(1_000_000_000),
                extra_data: vec![],
            },
            transactions: vec![tx_type1.clone()],
            last_commit_qc: None,
        };
        let receipt = Receipt {
            tx_id,
            success: true,
            gas_used: Gas(21_000),
            cumulative_gas_used: Gas(21_000),
            logs: vec![],
            return_data: vec![],
        };

        disp.record_block(block, vec![receipt]);

        // 3. Query eth_getTransactionByHash
        let eth_hash = tola_codec::rlp::compute_eth_tx_hash(&tx_type1).unwrap();
        let req_tx = format!(
            r#"{{"jsonrpc":"2.0","id":2,"method":"eth_getTransactionByHash","params":["{}"]}}"#,
            eth_hash.to_hex()
        );
        let res_tx = disp.dispatch_json(&req_tx);
        let val_tx = JsonValue::parse(&res_tx).unwrap();
        let tx_res = val_tx.get("result").unwrap();
        assert_eq!(tx_res.get("type").unwrap().as_str(), Some("0x1"));
        let al_res = tx_res.get("accessList").unwrap().as_array().unwrap();
        assert_eq!(al_res.len(), 1);
        assert_eq!(
            al_res[0].get("address").unwrap().as_str(),
            Some(Address([0x44; 20]).to_hex().as_str())
        );

        // 4. Query eth_getTransactionReceipt
        let req_rcpt = format!(
            r#"{{"jsonrpc":"2.0","id":3,"method":"eth_getTransactionReceipt","params":["{}"]}}"#,
            eth_hash.to_hex()
        );
        let res_rcpt = disp.dispatch_json(&req_rcpt);
        let val_rcpt = JsonValue::parse(&res_rcpt).unwrap();
        let rcpt_res = val_rcpt.get("result").unwrap();
        assert_eq!(rcpt_res.get("type").unwrap().as_str(), Some("0x1"));
    }

    #[test]
    fn test_governance_rpc_lifecycle_and_security_council() {
        let chain_id = ChainId(9999);
        let state_db = Arc::new(RwLock::new(tola_state::MemoryStateJournal::new()));
        let mempool = Arc::new(RwLock::new(Mempool::new(tola_mempool::MempoolConfig::new(
            chain_id,
        ))));
        let disp = RpcDispatcher::new(chain_id, "TOLA/v0.1.0-test".into(), state_db, mempool);

        // 1. tola_getGovernanceParams
        let res_params = disp.dispatch_json(
            r#"{"jsonrpc":"2.0","id":1,"method":"tola_getGovernanceParams","params":[]}"#,
        );
        let val_params = JsonValue::parse(&res_params).unwrap();
        let r = val_params.get("result").unwrap();
        assert_eq!(
            r.get("quorum_bp").unwrap().as_i64(),
            Some(500),
            "5% quorum per D03"
        );
        assert_eq!(
            r.get("approval_threshold_bp").unwrap().as_i64(),
            Some(6000),
            "60% approval per D03"
        );
        assert_eq!(
            r.get("timelock_delay_blocks").unwrap().as_i64(),
            Some(604800),
            "14-day timelock per D03"
        );
        assert_eq!(
            r.get("voting_period_blocks").unwrap().as_i64(),
            Some(302400),
            "7-day voting per D03"
        );

        // 2. tola_submitProposal
        let submit_req = r#"{
            "jsonrpc":"2.0","id":2,"method":"tola_submitProposal","params":[{
                "proposer": "0x1111111111111111111111111111111111111111",
                "title": "Increase Block Gas Limit",
                "description": "Scale gas limit to 40M",
                "changes": [{"param": "max_block_gas_limit", "value": "0x2625a00"}],
                "deposit": "0x21e19e0c9bab2400000"
            }]
        }"#;
        let res_sub = disp.dispatch_json(submit_req);
        let val_sub = JsonValue::parse(&res_sub).unwrap();
        assert_eq!(val_sub.get("result").unwrap().as_str(), Some("0x1"));

        // 3. tola_getProposal
        let res_get = disp
            .dispatch_json(r#"{"jsonrpc":"2.0","id":3,"method":"tola_getProposal","params":[1]}"#);
        let val_get = JsonValue::parse(&res_get).unwrap();
        let prop_obj = val_get.get("result").unwrap();
        assert_eq!(
            prop_obj.get("title").unwrap().as_str(),
            Some("Increase Block Gas Limit")
        );
        assert_eq!(
            prop_obj.get("status").unwrap().as_str(),
            Some("VotingActive")
        );

        // 4. tola_listProposals
        let res_list = disp
            .dispatch_json(r#"{"jsonrpc":"2.0","id":4,"method":"tola_listProposals","params":[]}"#);
        let val_list = JsonValue::parse(&res_list).unwrap();
        let list_arr = val_list.get("result").unwrap().as_array().unwrap();
        assert_eq!(list_arr.len(), 1);

        // 5. tola_castVote
        let vote_req = r#"{
            "jsonrpc":"2.0","id":5,"method":"tola_castVote","params":[{
                "proposal_id": 1,
                "voter": "0x2222222222222222222222222222222222222222",
                "option": "yes",
                "voting_power": "0x1000"
            }]
        }"#;
        let res_vote = disp.dispatch_json(vote_req);
        let val_vote = JsonValue::parse(&res_vote).unwrap();
        assert_eq!(val_vote.get("result").unwrap().as_bool(), Some(true));

        // 6. tola_emergencyParamChange - 2 of 5 fails (threshold is 3)
        let emer_fail_req = r#"{
            "jsonrpc":"2.0","id":6,"method":"tola_emergencyParamChange","params":[{
                "approvers": [
                    "0x0101010101010101010101010101010101010101",
                    "0x0202020202020202020202020202020202020202"
                ],
                "changes": [{"param": "slashing_penalty_bp", "value": "600"}]
            }]
        }"#;
        let res_emer_fail = disp.dispatch_json(emer_fail_req);
        let val_emer_fail = JsonValue::parse(&res_emer_fail).unwrap();
        assert!(
            val_emer_fail.get("error").is_some(),
            "2-of-5 must fail threshold"
        );

        // 7. tola_emergencyParamChange - 3 authorized members succeeds
        let emer_pass_req = r#"{
            "jsonrpc":"2.0","id":7,"method":"tola_emergencyParamChange","params":[{
                "approvers": [
                    "0x0101010101010101010101010101010101010101",
                    "0x0202020202020202020202020202020202020202",
                    "0x0303030303030303030303030303030303030303"
                ],
                "changes": [{"param": "slashing_penalty_bp", "value": "600"}]
            }]
        }"#;
        let res_emer_pass = disp.dispatch_json(emer_pass_req);
        let val_emer_pass = JsonValue::parse(&res_emer_pass).unwrap();
        assert_eq!(val_emer_pass.get("result").unwrap().as_bool(), Some(true));

        // Check updated parameter via tola_getGovernanceParams
        let res_params_updated = disp.dispatch_json(
            r#"{"jsonrpc":"2.0","id":8,"method":"tola_getGovernanceParams","params":[]}"#,
        );
        let val_params_updated = JsonValue::parse(&res_params_updated).unwrap();
        let r_up = val_params_updated.get("result").unwrap();
        assert_eq!(r_up.get("slashing_penalty_bp").unwrap().as_i64(), Some(600));
    }
}
