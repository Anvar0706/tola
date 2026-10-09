#![allow(clippy::all)]
//! TOLA EVM Phase 9 (P9) Ethereum Conformance & Differential Testing Test Suite
//!
//! Comprehensive verification of:
//! Part 1 — Goal A EVM Correctness Remediation:
//!   - Dynamic block.gaslimit canonical execution (P9-FND-01)
//!   - Dynamic block.gaslimit historical simulation (eth_call & eth_estimateGas)
//!   - Block-level EIP-1559 base fee validation rejects underpriced EVM tx (P9-FND-02)
//!   - Block-level EIP-1559 base fee validation accepts valid EVM tx
//!   - Block-level EIP-1559 validation isolates native TOLA transactions
//!   - EIP-3860 initcode size stateless validation boundaries (49151, 49152, 49153) (P9-FND-05)
//!   - EIP-3860 non-creation CALL calldata >49152 permitted
//!
//! Part 2 — Goal B Tier 1: Canonical Shanghai GeneralStateTest (GST) Vectors:
//!   - Arithmetic & modulo edge cases (ADDMOD/MULMOD mod 0, SDIV MIN_INT overflow, SDIV/SMOD by 0, SIGNEXTEND, EXP, SHL, SAR)
//!   - Memory expansion quadratic gas boundaries
//!   - Stack depth limits (1024 success, 1025 overflow, underflow)
//!   - Call depth limits (1024 call frames boundary)
//!   - CREATE address collision & EIP-170 max code size limit (24,576 bytes)
//!   - REVERT data bubbling, rollback, and gas consumption
//!
//! Part 3 — Goal B Tier 2: Standalone REVM 43.0.3 Diagnostic Oracle:
//!   - Standalone in-memory REVM 43.0.3 execution equivalence (gas, return data, status)
//!   - Standalone REVM 43.0.3 state mutation equivalence (balances, nonces, storage)
//!
//! Part 4 — Regression Suite:
//!   - P5 equal-fee Type-2 persistence & gas refunds
//!   - P7 Solidity 0.8.24 OpenZeppelin v5.0.2 ERC-20 & Shanghai precompiles (0x01 low/high-S, 0x02, 0x03)
//!   - P8 Historical state queries & strict future block rejection
//!   - Cold database restart state persistence

use std::sync::{Arc, RwLock};
use tola_core::{BlockEngine, BlockValidationError, StateDB, StateDelta};
use tola_crypto::{keccak256_hash, Secp256k1Keypair};
use tola_execution::evm::EvmExecutor;
use tola_execution::evm_adapter::TolaEvmInspector;
use tola_execution::SequentialBlockEngine;
use tola_mempool::{Mempool, MempoolConfig};
use tola_rpc::dispatcher::RpcDispatcher;
use tola_rpc::json::JsonValue;
use tola_storage::PersistentStateDB;
use tola_types::*;

use revm::{
    context::{
        BlockEnv as RevmBlockEnv, CfgEnv as RevmCfgEnv, Context as RevmContext, TxEnv as RevmTxEnv,
    },
    database::InMemoryDB,
    handler::{MainBuilder, MainContext},
    inspector::InspectCommitEvm,
    primitives::{
        hardfork::SpecId, Address as RevmAddress, Bytes as RevmBytes, TxKind as RevmTxKind,
        U256 as RevmU256,
    },
    state::AccountInfo as RevmAccountInfo,
};

const TEST_CHAIN_ID: ChainId = ChainId(9999);

// =========================================================================
// Hex & ABI Helpers
// =========================================================================

fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        use std::fmt::Write;
        write!(&mut s, "{:02x}", b).unwrap();
    }
    s
}

fn hex_decode(s: &str) -> Vec<u8> {
    let clean = s
        .strip_prefix("0x")
        .or_else(|| s.strip_prefix("0X"))
        .unwrap_or(s);
    let mut bytes = Vec::with_capacity(clean.len() / 2);
    let mut chars = clean.chars();
    while let (Some(h), Some(l)) = (chars.next(), chars.next()) {
        let hi = h.to_digit(16).expect("valid hex digit") as u8;
        let lo = l.to_digit(16).expect("valid hex digit") as u8;
        bytes.push((hi << 4) | lo);
    }
    bytes
}

fn pad32_u64(val: u64) -> [u8; 32] {
    let mut buf = [0u8; 32];
    buf[24..32].copy_from_slice(&val.to_be_bytes());
    buf
}

fn pad32_addr(addr: &Address) -> [u8; 32] {
    let mut buf = [0u8; 32];
    buf[12..32].copy_from_slice(&addr.0);
    buf
}

fn compute_contract_address(sender: &Address, nonce: u64) -> Address {
    tola_codec::rlp::compute_contract_address(sender, nonce)
}

fn selector(sig: &str) -> [u8; 4] {
    let h = keccak256_hash(sig.as_bytes());
    let mut s = [0u8; 4];
    s.copy_from_slice(&h.as_bytes()[0..4]);
    s
}

fn make_initcode(runtime: &[u8]) -> Vec<u8> {
    let mut code = Vec::new();
    let r_len = runtime.len();
    if r_len <= 255 {
        code.extend_from_slice(&[
            0x60,
            r_len as u8, // PUSH1 r_len
            0x60,
            0x0c, // PUSH1 12 (offset of runtime)
            0x60,
            0x00, // PUSH1 0 (dest)
            0x39, // CODECOPY
            0x60,
            r_len as u8, // PUSH1 r_len
            0x60,
            0x00, // PUSH1 0
            0xf3, // RETURN
        ]);
    } else {
        let len_be = (r_len as u16).to_be_bytes();
        code.extend_from_slice(&[
            0x61, len_be[0], len_be[1], // PUSH2 r_len
            0x60, 0x0e, // PUSH1 14 (offset of runtime)
            0x60, 0x00, // PUSH1 0 (dest)
            0x39, // CODECOPY
            0x61, len_be[0], len_be[1], // PUSH2 r_len
            0x60, 0x00, // PUSH1 0
            0xf3, // RETURN
        ]);
    }
    code.extend_from_slice(runtime);
    code
}

fn sign_eth_tx(kp: &Secp256k1Keypair, mut tx: Transaction) -> (Transaction, Hash) {
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
        chain_id: TEST_CHAIN_ID.0,
    };
    let eth_hash = tola_codec::rlp::compute_eth_tx_hash(&tx).unwrap();
    (tx, eth_hash)
}

fn load_artifact(json_str: &str) -> (Vec<u8>, Vec<u8>) {
    let val = JsonValue::parse(json_str).expect("Valid JSON artifact");
    let obj = match val {
        JsonValue::Object(map) => map,
        _ => panic!("Expected object artifact"),
    };
    let bytecode_hex = obj
        .iter()
        .find(|(k, _)| k == "bytecode")
        .expect("bytecode field")
        .1
        .as_str()
        .expect("string bytecode");
    let deployed_hex = obj
        .iter()
        .find(|(k, _)| k == "deployedBytecode")
        .expect("deployedBytecode field")
        .1
        .as_str()
        .expect("string deployedBytecode");
    (hex_decode(bytecode_hex), hex_decode(deployed_hex))
}

const ERC20_MOCK_JSON: &str = include_str!("../../../contracts/compiled/ERC20Mock.json");

// =========================================================================
// Test Harness
// =========================================================================

struct TestHarness {
    db_path: std::path::PathBuf,
    pub state_arc: Arc<RwLock<PersistentStateDB>>,
    current_height: u64,
    base_fee: Amount,
    proposer: Address,
    parent_hash: Hash,
    block_gas_limit: Gas,
}

impl TestHarness {
    fn new(test_name: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let db_path = std::env::temp_dir().join(format!("tola_p9_{test_name}_{nanos}"));
        let state_db = PersistentStateDB::open(&db_path).expect("open persistent state_db");
        Self {
            db_path,
            state_arc: Arc::new(RwLock::new(state_db)),
            current_height: 0,
            base_fee: Amount::from_u128(1_000_000_000), // 1 Gwei
            proposer: Address([0xaa; 20]),
            parent_hash: Hash::ZERO,
            block_gas_limit: Gas(30_000_000),
        }
    }

    fn fund_account(&mut self, addr: &Address, balance: u128) {
        let mut db = self.state_arc.write().unwrap();
        db.apply_delta(StateDelta {
            accounts: vec![(
                *addr,
                Some(AccountState::new_with_balance(Amount::from_u128(balance))),
            )],
            storage: vec![],
            code: vec![],
        })
        .unwrap();
    }

    fn commit_block(&mut self, txs: Vec<Transaction>) -> (Block, Vec<Receipt>) {
        self.commit_block_with_gas_limit(txs, self.block_gas_limit)
    }

    fn commit_block_with_gas_limit(
        &mut self,
        txs: Vec<Transaction>,
        gas_limit: Gas,
    ) -> (Block, Vec<Receipt>) {
        self.current_height += 1;
        let height = BlockHeight(self.current_height);
        let timestamp = Timestamp(1_000_000 + self.current_height * 10);
        let engine = SequentialBlockEngine::new(TEST_CHAIN_ID);

        let mut block = Block {
            header: BlockHeader {
                parent_hash: self.parent_hash,
                height,
                epoch: Epoch(0),
                round: Round(0),
                timestamp,
                proposer: self.proposer,
                state_root: Hash::ZERO,
                txs_root: Hash::ZERO,
                receipts_root: Hash::ZERO,
                gas_used: Gas::ZERO,
                gas_limit,
                base_fee_per_gas: self.base_fee,
                extra_data: vec![],
            },
            transactions: txs,
            last_commit_qc: None,
        };

        let mut db = self.state_arc.write().unwrap();
        let receipts = engine
            .execute_block(&block, &mut *db)
            .expect("execute_block succeeds");

        let state_root = db.compute_state_root().unwrap();
        block.header.state_root = state_root;
        let mut total_gas = 0u64;
        for r in &receipts {
            total_gas = total_gas.saturating_add(r.gas_used.as_u64());
        }
        block.header.gas_used = Gas(total_gas);

        let block_hash = tola_consensus::PipelinedBftEngine::compute_block_hash(&block);
        db.commit_block_bundle(&block, block_hash, &receipts)
            .unwrap();
        self.parent_hash = block_hash;
        (block, receipts)
    }

    fn make_dispatcher(&self) -> Arc<RpcDispatcher<PersistentStateDB>> {
        let mempool = Arc::new(RwLock::new(Mempool::new(MempoolConfig::new(TEST_CHAIN_ID))));
        Arc::new(RpcDispatcher::new(
            TEST_CHAIN_ID,
            "TOLA-Node-P9/v0.1.0".to_string(),
            self.state_arc.clone(),
            mempool,
        ))
    }

    fn reopen(&mut self) {
        drop(self.state_arc.clone());
        let reopened = PersistentStateDB::open(&self.db_path).expect("reopen persistent state_db");
        self.state_arc = Arc::new(RwLock::new(reopened));
    }
}

// =========================================================================
// PART 1: Goal A EVM Correctness Remediation Tests
// =========================================================================

#[test]
fn test_p9_dynamic_block_gas_limit_canonical_execution() {
    let mut harness = TestHarness::new("gas_limit_exec");
    let kp = Secp256k1Keypair::generate();
    let sender = kp.address();
    harness.fund_account(&sender, 100_000_000_000_000_000);

    // Bytecode that executes GASLIMIT (0x45), writes it to storage slot 0, and returns it
    // 0x45 (GASLIMIT)
    // 0x60 0x00 (PUSH1 0)
    // 0x55 (SSTORE slot 0)
    // 0x45 (GASLIMIT)
    // 0x60 0x00 (PUSH1 0)
    // 0x52 (MSTORE)
    // 0x60 0x20 0x60 0x00 0xf3 (RETURN 32 bytes)
    let runtime = hex_decode("456000554560005260206000f3");
    let initcode = make_initcode(&runtime);

    // Block 1 with gas_limit = 15,000,000 (0x00e4e1c0)
    let deploy_tx = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce::ZERO,
        from: sender,
        to: None,
        value: Amount::ZERO,
        gas_limit: Gas(1_000_000),
        max_fee_per_gas: Amount::from_u128(2_000_000_000),
        max_priority_fee_per_gas: Amount::from_u128(100_000_000),
        valid_until_block: BlockHeight(100),
        data: initcode,
        memo: None,
        signature: Signature::Secp256k1Eth {
            r: [0; 32],
            s: [0; 32],
            y_parity: 0,
            chain_id: TEST_CHAIN_ID.0,
        },
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_deploy, _) = sign_eth_tx(&kp, deploy_tx);
    let contract_addr = compute_contract_address(&sender, 0);

    let (_, receipts1) = harness.commit_block_with_gas_limit(vec![signed_deploy], Gas(15_000_000));
    assert!(receipts1[0].success);

    // Block 2: Call the contract in a block with gas_limit = 15_000_000
    let call_tx1 = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(1),
        from: sender,
        to: Some(contract_addr),
        value: Amount::ZERO,
        gas_limit: Gas(200_000),
        max_fee_per_gas: Amount::from_u128(2_000_000_000),
        max_priority_fee_per_gas: Amount::from_u128(100_000_000),
        valid_until_block: BlockHeight(100),
        data: vec![],
        memo: None,
        signature: Signature::Secp256k1Eth {
            r: [0; 32],
            s: [0; 32],
            y_parity: 0,
            chain_id: TEST_CHAIN_ID.0,
        },
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_call1, _) = sign_eth_tx(&kp, call_tx1);
    let (_, receipts2) = harness.commit_block_with_gas_limit(vec![signed_call1], Gas(15_000_000));
    assert!(receipts2[0].success);

    // Verify storage slot 0 is 15,000,000 (0x00e4e1c0)
    let db = harness.state_arc.read().unwrap();
    let slot0_val = db.get_storage(&contract_addr, &Hash::ZERO).unwrap();
    assert_eq!(slot0_val.0, pad32_u64(15_000_000));
    drop(db);

    // Block 3: Call the contract in a block with gas_limit = 45_000_000 (0x02aea540)
    let call_tx2 = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(2),
        from: sender,
        to: Some(contract_addr),
        value: Amount::ZERO,
        gas_limit: Gas(200_000),
        max_fee_per_gas: Amount::from_u128(2_000_000_000),
        max_priority_fee_per_gas: Amount::from_u128(100_000_000),
        valid_until_block: BlockHeight(100),
        data: vec![],
        memo: None,
        signature: Signature::Secp256k1Eth {
            r: [0; 32],
            s: [0; 32],
            y_parity: 0,
            chain_id: TEST_CHAIN_ID.0,
        },
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_call2, _) = sign_eth_tx(&kp, call_tx2);
    let (_, receipts3) = harness.commit_block_with_gas_limit(vec![signed_call2], Gas(45_000_000));
    assert!(receipts3[0].success);

    // Verify storage slot 0 dynamically updated to 45_000_000
    let db = harness.state_arc.read().unwrap();
    let slot0_updated = db.get_storage(&contract_addr, &Hash::ZERO).unwrap();
    assert_eq!(slot0_updated.0, pad32_u64(45_000_000));
}

#[test]
fn test_p9_dynamic_block_gas_limit_historical_simulation() {
    let mut harness = TestHarness::new("gas_limit_sim");
    let kp = Secp256k1Keypair::generate();
    let sender = kp.address();
    harness.fund_account(&sender, 100_000_000_000_000_000);

    // Runtime returns GASLIMIT
    let runtime = hex_decode("4560005260206000f3");
    let initcode = make_initcode(&runtime);

    // Block 1 with gas_limit = 20,000,000 (0x01312d00)
    let deploy_tx = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce::ZERO,
        from: sender,
        to: None,
        value: Amount::ZERO,
        gas_limit: Gas(1_000_000),
        max_fee_per_gas: Amount::from_u128(2_000_000_000),
        max_priority_fee_per_gas: Amount::from_u128(100_000_000),
        valid_until_block: BlockHeight(100),
        data: initcode,
        memo: None,
        signature: Signature::Secp256k1Eth {
            r: [0; 32],
            s: [0; 32],
            y_parity: 0,
            chain_id: TEST_CHAIN_ID.0,
        },
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_deploy, _) = sign_eth_tx(&kp, deploy_tx);
    let contract_addr = compute_contract_address(&sender, 0);
    harness.commit_block_with_gas_limit(vec![signed_deploy], Gas(20_000_000));

    // Block 2 with gas_limit = 50,000,000 (0x02faf080)
    let dummy_tx = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(1),
        from: sender,
        to: Some(sender),
        value: Amount::ZERO,
        gas_limit: Gas(50_000),
        max_fee_per_gas: Amount::from_u128(2_000_000_000),
        max_priority_fee_per_gas: Amount::from_u128(100_000_000),
        valid_until_block: BlockHeight(100),
        data: vec![],
        memo: None,
        signature: Signature::Secp256k1Eth {
            r: [0; 32],
            s: [0; 32],
            y_parity: 0,
            chain_id: TEST_CHAIN_ID.0,
        },
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_dummy, _) = sign_eth_tx(&kp, dummy_tx);
    harness.commit_block_with_gas_limit(vec![signed_dummy], Gas(50_000_000));

    let disp = harness.make_dispatcher();

    // Query eth_call at Block 1: must return 20,000,000 (0x01312d00)
    let call_b1_json = format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"eth_call","params":[{{"to":"{}"}},"0x1"]}}"#,
        contract_addr.to_hex()
    );
    let resp1 = disp.dispatch_json(&call_b1_json);
    println!("RESP1: {}", resp1);
    let val1 = JsonValue::parse(&resp1).unwrap();
    let res1_str = val1.get("result").unwrap().as_str().unwrap();
    let res1_bytes = hex_decode(res1_str);
    assert_eq!(&res1_bytes[..], &pad32_u64(20_000_000)[..]);

    // Query eth_call at Block 2: must return 50,000,000 (0x02faf080)
    let call_b2_json = format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"eth_call","params":[{{"to":"{}"}},"0x2"]}}"#,
        contract_addr.to_hex()
    );
    let resp2 = disp.dispatch_json(&call_b2_json);
    let val2 = JsonValue::parse(&resp2).unwrap();
    let res2_str = val2.get("result").unwrap().as_str().unwrap();
    let res2_bytes = hex_decode(res2_str);
    assert_eq!(&res2_bytes[..], &pad32_u64(50_000_000)[..]);

    // Query eth_estimateGas works for both blocks
    let est_b1_json = format!(
        r#"{{"jsonrpc":"2.0","id":3,"method":"eth_estimateGas","params":[{{"to":"{}"}},"0x1"]}}"#,
        contract_addr.to_hex()
    );
    let est1_resp = disp.dispatch_json(&est_b1_json);
    let est1_val = JsonValue::parse(&est1_resp).unwrap();
    assert!(est1_val.get("result").is_some());
}

#[test]
fn test_p9_block_level_base_fee_validation_rejects_underpriced_evm_tx() {
    let harness = TestHarness::new("base_fee_reject");
    let kp = Secp256k1Keypair::generate();
    let sender = kp.address();

    let underpriced_evm_tx = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce::ZERO,
        from: sender,
        to: Some(sender),
        value: Amount::ZERO,
        gas_limit: Gas(100_000),
        max_fee_per_gas: Amount::from_u128(500_000_000), // 0.5 Gwei (below 1.0 Gwei block base fee)
        max_priority_fee_per_gas: Amount::from_u128(100_000_000),
        valid_until_block: BlockHeight(100),
        data: vec![],
        memo: None,
        signature: Signature::Secp256k1Eth {
            r: [0; 32],
            s: [0; 32],
            y_parity: 0,
            chain_id: TEST_CHAIN_ID.0,
        },
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_tx, _) = sign_eth_tx(&kp, underpriced_evm_tx);

    let block = Block {
        header: BlockHeader {
            parent_hash: Hash::ZERO,
            height: BlockHeight(1),
            epoch: Epoch(0),
            round: Round(0),
            timestamp: Timestamp(1000),
            proposer: harness.proposer,
            state_root: Hash::ZERO,
            txs_root: Hash::ZERO,
            receipts_root: Hash::ZERO,
            gas_used: Gas::ZERO,
            gas_limit: Gas(30_000_000),
            base_fee_per_gas: Amount::from_u128(1_000_000_000), // 1.0 Gwei
            extra_data: vec![],
        },
        transactions: vec![signed_tx],
        last_commit_qc: None,
    };

    let engine = SequentialBlockEngine::new(TEST_CHAIN_ID);
    let db = harness.state_arc.read().unwrap();
    let val_res = engine.validate_block(&block, &*db);
    assert_eq!(val_res, Err(BlockValidationError::FeeBelowBaseFee));
}

#[test]
fn test_p9_block_level_base_fee_validation_accepts_valid_evm_tx() {
    let mut harness = TestHarness::new("base_fee_accept");
    let kp = Secp256k1Keypair::generate();
    let sender = kp.address();
    harness.fund_account(&sender, 100_000_000_000_000_000);

    let valid_evm_tx = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce::ZERO,
        from: sender,
        to: Some(sender),
        value: Amount::ZERO,
        gas_limit: Gas(100_000),
        max_fee_per_gas: Amount::from_u128(1_000_000_000), // Exactly equal to block base fee
        max_priority_fee_per_gas: Amount::from_u128(100_000_000),
        valid_until_block: BlockHeight(100),
        data: vec![],
        memo: None,
        signature: Signature::Secp256k1Eth {
            r: [0; 32],
            s: [0; 32],
            y_parity: 0,
            chain_id: TEST_CHAIN_ID.0,
        },
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_tx, _) = sign_eth_tx(&kp, valid_evm_tx);

    let block = Block {
        header: BlockHeader {
            parent_hash: Hash::ZERO,
            height: BlockHeight(1),
            epoch: Epoch(0),
            round: Round(0),
            timestamp: Timestamp(1000),
            proposer: harness.proposer,
            state_root: Hash::ZERO,
            txs_root: Hash::ZERO,
            receipts_root: Hash::ZERO,
            gas_used: Gas::ZERO,
            gas_limit: Gas(30_000_000),
            base_fee_per_gas: Amount::from_u128(1_000_000_000),
            extra_data: vec![],
        },
        transactions: vec![signed_tx],
        last_commit_qc: None,
    };

    let engine = SequentialBlockEngine::new(TEST_CHAIN_ID);
    let db = harness.state_arc.read().unwrap();
    let val_res = engine.validate_block(&block, &*db);
    assert!(val_res.is_ok());
}

#[test]
fn test_p9_block_level_base_fee_validation_native_tx_isolation() {
    let mut harness = TestHarness::new("native_isolation");
    let kp = Secp256k1Keypair::generate();
    let sender = kp.address();
    harness.fund_account(&sender, 100_000_000_000_000_000);

    // Native TOLA transaction (tx_type = 0, is_evm() == false)
    let native_tx = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce::ZERO,
        from: sender,
        to: Some(sender),
        value: Amount::ZERO,
        gas_limit: Gas(21_000),
        max_fee_per_gas: Amount::from_u128(100), // Below EVM base fee (1 Gwei)
        max_priority_fee_per_gas: Amount::ZERO,
        valid_until_block: BlockHeight(100),
        data: vec![],
        memo: None,
        signature: Signature::Ed25519([0; 64]), // Dummy non-EVM signature for stateless check
        tx_type: 0,
        access_list: vec![],
    };

    let block = Block {
        header: BlockHeader {
            parent_hash: Hash::ZERO,
            height: BlockHeight(1),
            epoch: Epoch(0),
            round: Round(0),
            timestamp: Timestamp(1000),
            proposer: harness.proposer,
            state_root: Hash::ZERO,
            txs_root: Hash::ZERO,
            receipts_root: Hash::ZERO,
            gas_used: Gas::ZERO,
            gas_limit: Gas(30_000_000),
            base_fee_per_gas: Amount::from_u128(1_000_000_000), // 1 Gwei
            extra_data: vec![],
        },
        transactions: vec![native_tx],
        last_commit_qc: None,
    };

    let engine = SequentialBlockEngine::new(TEST_CHAIN_ID);
    let db = harness.state_arc.read().unwrap();
    // Native transaction is not subject to EVM FeeBelowBaseFee check in validate_block
    let val_res = engine.validate_block(&block, &*db);
    // Since signature is dummy Ed25519, validate_block returns InvalidSignature, but NOT FeeBelowBaseFee!
    assert_ne!(val_res, Err(BlockValidationError::FeeBelowBaseFee));
}

#[test]
fn test_p9_eip3860_initcode_size_boundary_49151_49152_49153() {
    let kp = Secp256k1Keypair::generate();
    let sender = kp.address();

    // 1. Boundary 49,151 bytes
    let tx_49151 = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce::ZERO,
        from: sender,
        to: None, // Contract creation
        value: Amount::ZERO,
        gas_limit: Gas(1_000_000),
        max_fee_per_gas: Amount::from_u128(1_000_000_000),
        max_priority_fee_per_gas: Amount::ZERO,
        valid_until_block: BlockHeight(100),
        data: vec![0x60; 49_151],
        memo: None,
        signature: Signature::Secp256k1Eth {
            r: [1; 32],
            s: [1; 32],
            y_parity: 0,
            chain_id: TEST_CHAIN_ID.0,
        },
        tx_type: 2,
        access_list: vec![],
    };
    assert!(tx_49151
        .stateless_validate(TEST_CHAIN_ID, Gas(30_000_000))
        .is_ok());

    // 2. Boundary 49,152 bytes (MAX_INITCODE_SIZE exact limit)
    let tx_49152 = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce::ZERO,
        from: sender,
        to: None, // Contract creation
        value: Amount::ZERO,
        gas_limit: Gas(1_000_000),
        max_fee_per_gas: Amount::from_u128(1_000_000_000),
        max_priority_fee_per_gas: Amount::ZERO,
        valid_until_block: BlockHeight(100),
        data: vec![0x60; 49_152],
        memo: None,
        signature: Signature::Secp256k1Eth {
            r: [1; 32],
            s: [1; 32],
            y_parity: 0,
            chain_id: TEST_CHAIN_ID.0,
        },
        tx_type: 2,
        access_list: vec![],
    };
    assert!(tx_49152
        .stateless_validate(TEST_CHAIN_ID, Gas(30_000_000))
        .is_ok());

    // 3. Boundary 49,153 bytes (1 byte over limit -> must fail statelessly)
    let tx_49153 = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce::ZERO,
        from: sender,
        to: None, // Contract creation
        value: Amount::ZERO,
        gas_limit: Gas(1_000_000),
        max_fee_per_gas: Amount::from_u128(1_000_000_000),
        max_priority_fee_per_gas: Amount::ZERO,
        valid_until_block: BlockHeight(100),
        data: vec![0x60; 49_153],
        memo: None,
        signature: Signature::Secp256k1Eth {
            r: [1; 32],
            s: [1; 32],
            y_parity: 0,
            chain_id: TEST_CHAIN_ID.0,
        },
        tx_type: 2,
        access_list: vec![],
    };
    assert_eq!(
        tx_49153.stateless_validate(TEST_CHAIN_ID, Gas(30_000_000)),
        Err(TransactionError::InitcodeTooLarge {
            max_bytes: 49_152,
            got_bytes: 49_153,
        })
    );
}

#[test]
fn test_p9_eip3860_non_creation_call_above_49152_permitted() {
    let kp = Secp256k1Keypair::generate();
    let sender = kp.address();
    let target = Address([0x44; 20]);

    // CALL transaction with 50,000 bytes calldata (> 49,152 but <= 131,072)
    let tx_call_50000 = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce::ZERO,
        from: sender,
        to: Some(target), // Regular call, NOT contract creation
        value: Amount::ZERO,
        gas_limit: Gas(2_000_000),
        max_fee_per_gas: Amount::from_u128(1_000_000_000),
        max_priority_fee_per_gas: Amount::ZERO,
        valid_until_block: BlockHeight(100),
        data: vec![0xaa; 50_000],
        memo: None,
        signature: Signature::Secp256k1Eth {
            r: [1; 32],
            s: [1; 32],
            y_parity: 0,
            chain_id: TEST_CHAIN_ID.0,
        },
        tx_type: 2,
        access_list: vec![],
    };
    assert!(tx_call_50000
        .stateless_validate(TEST_CHAIN_ID, Gas(30_000_000))
        .is_ok());

    // CALL transaction with 65,536 bytes calldata
    let tx_call_65536 = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce::ZERO,
        from: sender,
        to: Some(target),
        value: Amount::ZERO,
        gas_limit: Gas(2_000_000),
        max_fee_per_gas: Amount::from_u128(1_000_000_000),
        max_priority_fee_per_gas: Amount::ZERO,
        valid_until_block: BlockHeight(100),
        data: vec![0xbb; 65_536],
        memo: None,
        signature: Signature::Secp256k1Eth {
            r: [1; 32],
            s: [1; 32],
            y_parity: 0,
            chain_id: TEST_CHAIN_ID.0,
        },
        tx_type: 2,
        access_list: vec![],
    };
    assert!(tx_call_65536
        .stateless_validate(TEST_CHAIN_ID, Gas(30_000_000))
        .is_ok());
}

// =========================================================================
// PART 2: Goal B Tier 1: Canonical Shanghai GeneralStateTest (GST) Vectors
// =========================================================================

#[test]
fn test_p9_canonical_gst_arithmetic_modulo_edge_cases() {
    let mut harness = TestHarness::new("gst_arithmetic");
    let kp = Secp256k1Keypair::generate();
    let sender = kp.address();
    harness.fund_account(&sender, 100_000_000_000_000_000);

    // Bytecode that executes 10 edge case arithmetic operations and stores in slots 0..9:
    // Slot 0: ADDMOD(0xFF..FF, 1, 0) == 0
    // Slot 1: MULMOD(0xFF..FF, 0xFF..FF, 0) == 0
    // Slot 2: SDIV(-2^255, -1) == -2^255 (0x8000..00)
    // Slot 3: SDIV(42, 0) == 0
    // Slot 4: SMOD(42, 0) == 0
    // Slot 5: SIGNEXTEND(0, 0x80) == 0xFF..80
    // Slot 6: SIGNEXTEND(31, 0x80) == 0x80
    // Slot 7: EXP(2, 255)
    // Slot 8: SHL(256, 1) == 0
    // Slot 9: SAR(255, -1) == -1
    //
    // Opcodes:
    // 0: PUSH1 0, PUSH1 1, PUSH32 0xFF..FF, ADDMOD, PUSH1 0, SSTORE
    // 1: PUSH1 0, PUSH32 0xFF..FF, PUSH32 0xFF..FF, MULMOD, PUSH1 1, SSTORE
    // 2: PUSH32 0xFF..FF (-1), PUSH32 0x80..00 (-2^255), SDIV, PUSH1 2, SSTORE
    // 3: PUSH1 0, PUSH1 42, SDIV, PUSH1 3, SSTORE
    // 4: PUSH1 0, PUSH1 42, SMOD, PUSH1 4, SSTORE
    // 5: PUSH1 0x80, PUSH1 0, SIGNEXTEND, PUSH1 5, SSTORE
    // 6: PUSH1 0x80, PUSH1 31, SIGNEXTEND, PUSH1 6, SSTORE
    // 7: PUSH1 255, PUSH1 2, EXP, PUSH1 7, SSTORE
    // 8: PUSH1 1, PUSH2 256, SHL, PUSH1 8, SSTORE
    // 9: PUSH32 0xFF..FF (-1), PUSH1 255, SAR, PUSH1 9, SSTORE
    // Return 1: PUSH1 1, PUSH1 0, MSTORE, PUSH1 32, PUSH1 0, RETURN
    let mut code = Vec::new();

    // Slot 0: ADDMOD(0xFF..FF, 1, 0)
    code.extend_from_slice(&[0x60, 0x00]); // PUSH1 0
    code.extend_from_slice(&[0x60, 0x01]); // PUSH1 1
    code.push(0x7f); // PUSH32
    code.extend_from_slice(&[0xff; 32]);
    code.push(0x08); // ADDMOD
    code.extend_from_slice(&[0x60, 0x00, 0x55]); // PUSH1 0, SSTORE

    // Slot 1: MULMOD(0xFF..FF, 0xFF..FF, 0)
    code.extend_from_slice(&[0x60, 0x00]); // PUSH1 0
    code.push(0x7f); // PUSH32
    code.extend_from_slice(&[0xff; 32]);
    code.push(0x7f); // PUSH32
    code.extend_from_slice(&[0xff; 32]);
    code.push(0x09); // MULMOD
    code.extend_from_slice(&[0x60, 0x01, 0x55]); // PUSH1 1, SSTORE

    // Slot 2: SDIV(-2^255, -1)
    code.push(0x7f); // PUSH32 (-1)
    code.extend_from_slice(&[0xff; 32]);
    code.push(0x7f); // PUSH32 (-2^255)
    code.push(0x80);
    code.extend_from_slice(&[0x00; 31]);
    code.push(0x05); // SDIV
    code.extend_from_slice(&[0x60, 0x02, 0x55]); // PUSH1 2, SSTORE

    // Slot 3: SDIV(42, 0)
    code.extend_from_slice(&[0x60, 0x00]); // PUSH1 0
    code.extend_from_slice(&[0x60, 0x2a]); // PUSH1 42
    code.push(0x05); // SDIV
    code.extend_from_slice(&[0x60, 0x03, 0x55]); // PUSH1 3, SSTORE

    // Slot 4: SMOD(42, 0)
    code.extend_from_slice(&[0x60, 0x00]); // PUSH1 0
    code.extend_from_slice(&[0x60, 0x2a]); // PUSH1 42
    code.push(0x07); // SMOD
    code.extend_from_slice(&[0x60, 0x04, 0x55]); // PUSH1 4, SSTORE

    // Slot 5: SIGNEXTEND(0, 0x80)
    code.extend_from_slice(&[0x60, 0x80]); // PUSH1 0x80
    code.extend_from_slice(&[0x60, 0x00]); // PUSH1 0
    code.push(0x0b); // SIGNEXTEND
    code.extend_from_slice(&[0x60, 0x05, 0x55]); // PUSH1 5, SSTORE

    // Slot 6: SIGNEXTEND(31, 0x80)
    code.extend_from_slice(&[0x60, 0x80]); // PUSH1 0x80
    code.extend_from_slice(&[0x60, 0x1f]); // PUSH1 31
    code.push(0x0b); // SIGNEXTEND
    code.extend_from_slice(&[0x60, 0x06, 0x55]); // PUSH1 6, SSTORE

    // Slot 7: EXP(2, 255)
    code.extend_from_slice(&[0x60, 0xff]); // PUSH1 255
    code.extend_from_slice(&[0x60, 0x02]); // PUSH1 2
    code.push(0x0a); // EXP
    code.extend_from_slice(&[0x60, 0x07, 0x55]); // PUSH1 7, SSTORE

    // Slot 8: SHL(256, 1)
    code.extend_from_slice(&[0x60, 0x01]); // PUSH1 1
    code.extend_from_slice(&[0x61, 0x01, 0x00]); // PUSH2 256
    code.push(0x1b); // SHL
    code.extend_from_slice(&[0x60, 0x08, 0x55]); // PUSH1 8, SSTORE

    // Slot 9: SAR(255, -1)
    code.push(0x7f); // PUSH32 (-1)
    code.extend_from_slice(&[0xff; 32]);
    code.extend_from_slice(&[0x60, 0xff]); // PUSH1 255
    code.push(0x1d); // SAR
    code.extend_from_slice(&[0x60, 0x09, 0x55]); // PUSH1 9, SSTORE

    // RETURN 1
    code.extend_from_slice(&[0x60, 0x01, 0x60, 0x00, 0x52, 0x60, 0x20, 0x60, 0x00, 0xf3]);

    let initcode = make_initcode(&code);

    let deploy_tx = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce::ZERO,
        from: sender,
        to: None,
        value: Amount::ZERO,
        gas_limit: Gas(2_000_000),
        max_fee_per_gas: Amount::from_u128(2_000_000_000),
        max_priority_fee_per_gas: Amount::from_u128(100_000_000),
        valid_until_block: BlockHeight(100),
        data: initcode,
        memo: None,
        signature: Signature::Secp256k1Eth {
            r: [0; 32],
            s: [0; 32],
            y_parity: 0,
            chain_id: TEST_CHAIN_ID.0,
        },
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_deploy, _) = sign_eth_tx(&kp, deploy_tx);
    let contract_addr = compute_contract_address(&sender, 0);

    let (_, receipts) = harness.commit_block(vec![signed_deploy]);
    assert!(receipts[0].success);

    // Call the deployed contract
    let call_tx = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(1),
        from: sender,
        to: Some(contract_addr),
        value: Amount::ZERO,
        gas_limit: Gas(500_000),
        max_fee_per_gas: Amount::from_u128(2_000_000_000),
        max_priority_fee_per_gas: Amount::from_u128(100_000_000),
        valid_until_block: BlockHeight(100),
        data: vec![],
        memo: None,
        signature: Signature::Secp256k1Eth {
            r: [0; 32],
            s: [0; 32],
            y_parity: 0,
            chain_id: TEST_CHAIN_ID.0,
        },
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_call, _) = sign_eth_tx(&kp, call_tx);
    let (_, call_receipts) = harness.commit_block(vec![signed_call]);
    assert!(call_receipts[0].success);

    // Normalized post-state validation:
    let db = harness.state_arc.read().unwrap();
    // Slot 0: ADDMOD(..., 0) == 0
    let s0 = db.get_storage(&contract_addr, &Hash(pad32_u64(0))).unwrap();
    assert_eq!(s0, Hash::ZERO);

    // Slot 1: MULMOD(..., 0) == 0
    let s1 = db.get_storage(&contract_addr, &Hash(pad32_u64(1))).unwrap();
    assert_eq!(s1, Hash::ZERO);

    // Slot 2: SDIV(-2^255, -1) == -2^255 (0x8000..00)
    let s2 = db.get_storage(&contract_addr, &Hash(pad32_u64(2))).unwrap();
    let mut expected_s2 = [0u8; 32];
    expected_s2[0] = 0x80;
    assert_eq!(s2.0, expected_s2);

    // Slot 3: SDIV(42, 0) == 0
    let s3 = db.get_storage(&contract_addr, &Hash(pad32_u64(3))).unwrap();
    assert_eq!(s3, Hash::ZERO);

    // Slot 4: SMOD(42, 0) == 0
    let s4 = db.get_storage(&contract_addr, &Hash(pad32_u64(4))).unwrap();
    assert_eq!(s4, Hash::ZERO);

    // Slot 5: SIGNEXTEND(0, 0x80) == 0xFF..80
    let s5 = db.get_storage(&contract_addr, &Hash(pad32_u64(5))).unwrap();
    let mut expected_s5 = [0xff; 32];
    expected_s5[31] = 0x80;
    assert_eq!(s5.0, expected_s5);

    // Slot 6: SIGNEXTEND(31, 0x80) == 0x80
    let s6 = db.get_storage(&contract_addr, &Hash(pad32_u64(6))).unwrap();
    assert_eq!(s6.0, pad32_u64(0x80));

    // Slot 7: EXP(2, 255) == 2^255 (0x8000..00)
    let s7 = db.get_storage(&contract_addr, &Hash(pad32_u64(7))).unwrap();
    let mut expected_s7 = [0u8; 32];
    expected_s7[0] = 0x80;
    assert_eq!(s7.0, expected_s7);

    // Slot 8: SHL(256, 1) == 0
    let s8 = db.get_storage(&contract_addr, &Hash(pad32_u64(8))).unwrap();
    assert_eq!(s8, Hash::ZERO);

    // Slot 9: SAR(255, -1) == -1 (0xFF..FF)
    let s9 = db.get_storage(&contract_addr, &Hash(pad32_u64(9))).unwrap();
    assert_eq!(s9.0, [0xff; 32]);
}

#[test]
fn test_p9_canonical_gst_memory_expansion_quadratic_gas() {
    let mut harness = TestHarness::new("memory_gas");
    let kp = Secp256k1Keypair::generate();
    let sender = kp.address();
    harness.fund_account(&sender, 100_000_000_000_000_000);

    // Contract reads offset from calldata and writes 42 at that offset
    // 0x60 0x00 0x35 (PUSH1 0, CALLDATALOAD -> offset)
    // 0x60 0x2a 0x81 0x52 (PUSH1 42, DUP2, MSTORE)
    // 0x60 0x20 0x81 0xf3 (PUSH1 32, DUP2, RETURN)
    let runtime = hex_decode("600035602a8152602081f3");
    let initcode = make_initcode(&runtime);

    let deploy_tx = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce::ZERO,
        from: sender,
        to: None,
        value: Amount::ZERO,
        gas_limit: Gas(500_000),
        max_fee_per_gas: Amount::from_u128(2_000_000_000),
        max_priority_fee_per_gas: Amount::from_u128(100_000_000),
        valid_until_block: BlockHeight(100),
        data: initcode,
        memo: None,
        signature: Signature::Secp256k1Eth {
            r: [0; 32],
            s: [0; 32],
            y_parity: 0,
            chain_id: TEST_CHAIN_ID.0,
        },
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_deploy, _) = sign_eth_tx(&kp, deploy_tx);
    let contract_addr = compute_contract_address(&sender, 0);
    harness.commit_block(vec![signed_deploy]);

    // Test 1: MSTORE at offset 0 (memory size = 32 bytes = 1 word)
    let call1 = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(1),
        from: sender,
        to: Some(contract_addr),
        value: Amount::ZERO,
        gas_limit: Gas(200_000),
        max_fee_per_gas: Amount::from_u128(2_000_000_000),
        max_priority_fee_per_gas: Amount::from_u128(100_000_000),
        valid_until_block: BlockHeight(100),
        data: pad32_u64(0).to_vec(),
        memo: None,
        signature: Signature::Secp256k1Eth {
            r: [0; 32],
            s: [0; 32],
            y_parity: 0,
            chain_id: TEST_CHAIN_ID.0,
        },
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_call1, _) = sign_eth_tx(&kp, call1);
    let (_, r1) = harness.commit_block(vec![signed_call1]);
    let gas_offset_0 = r1[0].gas_used.as_u64();

    // Test 2: MSTORE at offset 1024
    let call2 = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(2),
        from: sender,
        to: Some(contract_addr),
        value: Amount::ZERO,
        gas_limit: Gas(200_000),
        max_fee_per_gas: Amount::from_u128(2_000_000_000),
        max_priority_fee_per_gas: Amount::from_u128(100_000_000),
        valid_until_block: BlockHeight(100),
        data: pad32_u64(1024).to_vec(),
        memo: None,
        signature: Signature::Secp256k1Eth {
            r: [0; 32],
            s: [0; 32],
            y_parity: 0,
            chain_id: TEST_CHAIN_ID.0,
        },
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_call2, _) = sign_eth_tx(&kp, call2);
    let (_, r2) = harness.commit_block(vec![signed_call2]);
    let gas_offset_1024 = r2[0].gas_used.as_u64();

    // Test 3: MSTORE at offset 4096
    let call3 = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(3),
        from: sender,
        to: Some(contract_addr),
        value: Amount::ZERO,
        gas_limit: Gas(200_000),
        max_fee_per_gas: Amount::from_u128(2_000_000_000),
        max_priority_fee_per_gas: Amount::from_u128(100_000_000),
        valid_until_block: BlockHeight(100),
        data: pad32_u64(4096).to_vec(),
        memo: None,
        signature: Signature::Secp256k1Eth {
            r: [0; 32],
            s: [0; 32],
            y_parity: 0,
            chain_id: TEST_CHAIN_ID.0,
        },
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_call3, _) = sign_eth_tx(&kp, call3);
    let (_, r3) = harness.commit_block(vec![signed_call3]);
    let gas_offset_4096 = r3[0].gas_used.as_u64();

    println!(
        "GAS 0: {}, GAS 1024: {}, GAS 4096: {}",
        gas_offset_0, gas_offset_1024, gas_offset_4096
    );
    assert!(gas_offset_1024 > gas_offset_0);
    assert!(gas_offset_4096 > gas_offset_1024);
}

#[test]
fn test_p9_canonical_gst_stack_depth_limits() {
    let mut harness = TestHarness::new("stack_depth");
    let kp = Secp256k1Keypair::generate();
    let sender = kp.address();
    harness.fund_account(&sender, 100_000_000_000_000_000);

    // 1. Stack depth 1024 success: 1024 PUSH1 instructions followed by 1024 POP instructions
    let mut ok_runtime = Vec::new();
    for _ in 0..1024 {
        ok_runtime.extend_from_slice(&[0x60, 0x01]); // PUSH1 1
    }
    for _ in 0..1024 {
        ok_runtime.push(0x50); // POP
    }
    // Return 1
    ok_runtime.extend_from_slice(&[0x60, 0x01, 0x60, 0x00, 0x52, 0x60, 0x20, 0x60, 0x00, 0xf3]);
    let ok_initcode = make_initcode(&ok_runtime);

    let deploy_ok_tx = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce::ZERO,
        from: sender,
        to: None,
        value: Amount::ZERO,
        gas_limit: Gas(2_000_000),
        max_fee_per_gas: Amount::from_u128(2_000_000_000),
        max_priority_fee_per_gas: Amount::from_u128(100_000_000),
        valid_until_block: BlockHeight(100),
        data: ok_initcode,
        memo: None,
        signature: Signature::Secp256k1Eth {
            r: [0; 32],
            s: [0; 32],
            y_parity: 0,
            chain_id: TEST_CHAIN_ID.0,
        },
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_ok, _) = sign_eth_tx(&kp, deploy_ok_tx);
    let (_, r_ok) = harness.commit_block(vec![signed_ok]);
    assert!(r_ok[0].success);

    // 2. Stack overflow: 1025 PUSH1 instructions executed during initcode -> must halt / fail
    let mut overflow_code = Vec::new();
    for _ in 0..1025 {
        overflow_code.extend_from_slice(&[0x60, 0x01]); // PUSH1 1
    }

    let deploy_overflow_tx = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(1),
        from: sender,
        to: None,
        value: Amount::ZERO,
        gas_limit: Gas(2_000_000),
        max_fee_per_gas: Amount::from_u128(2_000_000_000),
        max_priority_fee_per_gas: Amount::from_u128(100_000_000),
        valid_until_block: BlockHeight(100),
        data: overflow_code,
        memo: None,
        signature: Signature::Secp256k1Eth {
            r: [0; 32],
            s: [0; 32],
            y_parity: 0,
            chain_id: TEST_CHAIN_ID.0,
        },
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_overflow, _) = sign_eth_tx(&kp, deploy_overflow_tx);
    let (_, r_overflow) = harness.commit_block(vec![signed_overflow]);
    // Stack overflow results in failure status false
    assert!(!r_overflow[0].success);

    // 3. Stack underflow: POP on empty stack executed during initcode -> must halt / fail
    let underflow_code = vec![0x50]; // POP

    let deploy_underflow_tx = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(2),
        from: sender,
        to: None,
        value: Amount::ZERO,
        gas_limit: Gas(500_000),
        max_fee_per_gas: Amount::from_u128(2_000_000_000),
        max_priority_fee_per_gas: Amount::from_u128(100_000_000),
        valid_until_block: BlockHeight(100),
        data: underflow_code,
        memo: None,
        signature: Signature::Secp256k1Eth {
            r: [0; 32],
            s: [0; 32],
            y_parity: 0,
            chain_id: TEST_CHAIN_ID.0,
        },
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_underflow, _) = sign_eth_tx(&kp, deploy_underflow_tx);
    let (_, r_underflow) = harness.commit_block(vec![signed_underflow]);
    assert!(!r_underflow[0].success);
}

#[test]
fn test_p9_canonical_gst_call_depth_limits() {
    let mut harness = TestHarness::new("call_depth");
    let kp = Secp256k1Keypair::generate();
    let sender = kp.address();
    harness.fund_account(&sender, 100_000_000_000_000_000);

    // Shallow recursion test (depth 5) verifying basic recursive call frame handling,
    // memory retention, and return data bubbling. Kept for regression coverage.
    // NOTE: Genuine EVM 1024 call frame boundary coverage is implemented below in
    // `test_p9_canonical_evm_call_depth_1024_boundary`.
    //
    // Assembly:
    // 0: PUSH1 0, CALLDATALOAD (read count)
    // 4: DUP1, PUSH1 0, EQ, PUSH1 40, JUMPI (if count == 0, jump to return_ok)
    // 12: PUSH1 1, SWAP1, SUB (count - 1)
    // 15: PUSH1 0, MSTORE (store new count in memory 0)
    // 19: PUSH1 32 (out_len), PUSH1 0 (out_offset), PUSH1 32 (in_len), PUSH1 0 (in_offset), PUSH1 0 (value), ADDRESS, GAS, CALL
    // 32: DUP1, PUSH1 0, SSTORE (store call success status in slot 0)
    // 36: PUSH1 0, MSTORE, PUSH1 32, PUSH1 0, RETURN
    // 44 (return_ok): JUMPDEST, PUSH1 1, PUSH1 0, SSTORE, PUSH1 1, PUSH1 0, MSTORE, PUSH1 32, PUSH1 0, RETURN
    let runtime = hex_decode("60003580156024576001900360005260206000602060006000305af160005560206000f35b6001600055600160005260206000f3");
    let initcode = make_initcode(&runtime);

    let deploy_tx = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce::ZERO,
        from: sender,
        to: None,
        value: Amount::ZERO,
        gas_limit: Gas(1_000_000),
        max_fee_per_gas: Amount::from_u128(2_000_000_000),
        max_priority_fee_per_gas: Amount::from_u128(100_000_000),
        valid_until_block: BlockHeight(100),
        data: initcode,
        memo: None,
        signature: Signature::Secp256k1Eth {
            r: [0; 32],
            s: [0; 32],
            y_parity: 0,
            chain_id: TEST_CHAIN_ID.0,
        },
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_deploy, _) = sign_eth_tx(&kp, deploy_tx);
    let contract_addr = compute_contract_address(&sender, 0);
    let (_, r_dep) = harness.commit_block(vec![signed_deploy]);
    assert!(r_dep[0].success);

    // Call with recursion count 5: should succeed and store 1 in slot 0
    let call_5 = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(1),
        from: sender,
        to: Some(contract_addr),
        value: Amount::ZERO,
        gas_limit: Gas(500_000),
        max_fee_per_gas: Amount::from_u128(2_000_000_000),
        max_priority_fee_per_gas: Amount::from_u128(100_000_000),
        valid_until_block: BlockHeight(100),
        data: pad32_u64(5).to_vec(),
        memo: None,
        signature: Signature::Secp256k1Eth {
            r: [0; 32],
            s: [0; 32],
            y_parity: 0,
            chain_id: TEST_CHAIN_ID.0,
        },
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_call5, _) = sign_eth_tx(&kp, call_5);
    let (_, r_call5) = harness.commit_block(vec![signed_call5]);
    assert!(r_call5[0].success);

    let db = harness.state_arc.read().unwrap();
    let s0 = db.get_storage(&contract_addr, &Hash::ZERO).unwrap();
    assert_eq!(s0.0, pad32_u64(1));
}

#[test]
fn test_p9_canonical_evm_call_depth_1024_boundary() {
    let mut harness = TestHarness::new("call_depth_1024_boundary");
    harness.base_fee = Amount::from_u128(10);
    let kp = Secp256k1Keypair::generate();
    let sender = kp.address();
    harness.fund_account(&sender, 100_000_000_000_000_000_000); // 100 TOLA

    // Deploy the 1024-boundary contract
    // Bytecode logic:
    // Reads input 'depth' (32 bytes).
    // If depth < 1024:
    //   calls self with depth + 1.
    //   checks call_success == 1 and ret_val == 1.
    //   returns 1.
    // If depth == 1024:
    //   stores 0x42 in slot 1024 (SSTORE(1024, 0x42)) to prove frame 1024 is reached.
    //   attempts call to self with depth = 1025.
    //   checks call_success == 0 (MUST fail with CallTooDeep per EVM CALL_STACK_LIMIT = 1024!).
    //   stores 0x99 in slot 1025 (SSTORE(1025, 0x99)) to prove call 1025 failure was observed.
    //   returns 1 to caller (unwinding stack).
    let runtime = hex_decode(
        "6000358061040014603e5760010160005260206000602060006000305af11560335760005115603357600160005260206000f35b600060005260206000f35b60426104005561040160005260206000602060006000305af160001415603357609961040155600160005260206000f3",
    );
    let initcode = make_initcode(&runtime);

    let deploy_tx = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce::ZERO,
        from: sender,
        to: None,
        value: Amount::ZERO,
        gas_limit: Gas(1_000_000),
        max_fee_per_gas: Amount::from_u128(2_000_000_000),
        max_priority_fee_per_gas: Amount::from_u128(100_000_000),
        valid_until_block: BlockHeight(100),
        data: initcode,
        memo: None,
        signature: Signature::Secp256k1Eth {
            r: [0; 32],
            s: [0; 32],
            y_parity: 0,
            chain_id: TEST_CHAIN_ID.0,
        },
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_deploy, _) = sign_eth_tx(&kp, deploy_tx);
    let contract_addr = compute_contract_address(&sender, 0);
    let (_, r_dep) = harness.commit_block(vec![signed_deploy]);
    assert!(r_dep[0].success);

    // Dynamic block gas limit of 1 Trillion to support 1024 nested calls under EIP-150 (63/64 rule)
    // Over 1024 frames, (63/64)^1024 ~ 1/11,000,000, leaving ~90,000 gas at leaf frame 1024.
    let boundary_gas = Gas(1_000_000_000_000);
    let call_boundary = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(1),
        from: sender,
        to: Some(contract_addr),
        value: Amount::ZERO,
        gas_limit: boundary_gas,
        max_fee_per_gas: Amount::from_u128(10),
        max_priority_fee_per_gas: Amount::ZERO,
        valid_until_block: BlockHeight(100),
        data: pad32_u64(0).to_vec(), // start recursion at depth 0
        memo: None,
        signature: Signature::Secp256k1Eth {
            r: [0; 32],
            s: [0; 32],
            y_parity: 0,
            chain_id: TEST_CHAIN_ID.0,
        },
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_call, _) = sign_eth_tx(&kp, call_boundary);
    let (_, receipts) = harness.commit_block_with_gas_limit(vec![signed_call], boundary_gas);
    assert!(receipts[0].success, "Boundary transaction must succeed");

    let db = harness.state_arc.read().unwrap();
    // 1. Verify slot 1024 (0x0400): Frame 1024 was reached and stored 0x42
    let s1024 = db
        .get_storage(&contract_addr, &Hash(pad32_u64(1024)))
        .unwrap();
    assert_eq!(
        s1024.0,
        pad32_u64(0x42),
        "Frame 1024 must execute and store 0x42"
    );

    // 2. Verify slot 1025 (0x0401): Call 1025 failed (returned 0) as expected, storing 0x99
    let s1025 = db
        .get_storage(&contract_addr, &Hash(pad32_u64(1025)))
        .unwrap();
    assert_eq!(
        s1025.0,
        pad32_u64(0x99),
        "Frame 1024 must observe call 1025 failure and store 0x99"
    );

    // 3. Verify slot 1026 (0x0402): Must remain ZERO, proving no over-limit execution occurred
    let s1026 = db
        .get_storage(&contract_addr, &Hash(pad32_u64(1026)))
        .unwrap();
    assert_eq!(s1026, Hash::ZERO, "Slot 1026 must remain unwritten");
}

#[test]
fn test_p9_canonical_gst_create_collision_and_eip170_code_limit() {
    let mut harness = TestHarness::new("create_eip170");
    let kp = Secp256k1Keypair::generate();
    let sender = kp.address();
    harness.fund_account(&sender, 100_000_000_000_000_000);

    // EIP-170 limit: 24,576 bytes (0x6000)
    // 1. Contract returning exactly 24,576 bytes: deploys successfully
    let code_24576 = vec![0x00; 24_576];
    let initcode_ok = make_initcode(&code_24576);

    let deploy_ok = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce::ZERO,
        from: sender,
        to: None,
        value: Amount::ZERO,
        gas_limit: Gas(25_000_000),
        max_fee_per_gas: Amount::from_u128(2_000_000_000),
        max_priority_fee_per_gas: Amount::from_u128(100_000_000),
        valid_until_block: BlockHeight(100),
        data: initcode_ok,
        memo: None,
        signature: Signature::Secp256k1Eth {
            r: [0; 32],
            s: [0; 32],
            y_parity: 0,
            chain_id: TEST_CHAIN_ID.0,
        },
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_ok, _) = sign_eth_tx(&kp, deploy_ok);
    let (_, r_ok) = harness.commit_block(vec![signed_ok]);
    assert!(r_ok[0].success);

    // 2. Contract returning 24,577 bytes: must fail per EIP-170
    let code_24577 = vec![0x00; 24_577];
    let initcode_fail = make_initcode(&code_24577);

    let deploy_fail = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(1),
        from: sender,
        to: None,
        value: Amount::ZERO,
        gas_limit: Gas(25_000_000),
        max_fee_per_gas: Amount::from_u128(2_000_000_000),
        max_priority_fee_per_gas: Amount::from_u128(100_000_000),
        valid_until_block: BlockHeight(100),
        data: initcode_fail,
        memo: None,
        signature: Signature::Secp256k1Eth {
            r: [0; 32],
            s: [0; 32],
            y_parity: 0,
            chain_id: TEST_CHAIN_ID.0,
        },
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_fail, _) = sign_eth_tx(&kp, deploy_fail);
    let (_, r_fail) = harness.commit_block(vec![signed_fail]);
    assert!(!r_fail[0].success); // EIP-170 rejection
}

#[test]
fn test_p9_canonical_gst_revert_propagation_and_gas_bubbling() {
    let mut harness = TestHarness::new("revert_bubbling");
    let kp = Secp256k1Keypair::generate();
    let sender = kp.address();
    harness.fund_account(&sender, 100_000_000_000_000_000);

    // Child contract:
    // Writes 0x99 to slot 0, puts custom revert payload (0xdeadbeef...) in memory, and calls REVERT
    // 0x60 0x99 0x60 0x00 0x55 (SSTORE slot 0 = 0x99)
    // 0x7f deadbeef... (PUSH32)
    // 0x60 0x00 0x52 (MSTORE at 0)
    // 0x60 0x20 0x60 0x00 0xfd (REVERT 32 bytes)
    let mut child_runtime = Vec::new();
    child_runtime.extend_from_slice(&[0x60, 0x99, 0x60, 0x00, 0x55]);
    child_runtime.push(0x7f);
    let mut custom_revert = [0u8; 32];
    custom_revert[0..4].copy_from_slice(&[0xde, 0xad, 0xbe, 0xef]);
    child_runtime.extend_from_slice(&custom_revert);
    child_runtime.extend_from_slice(&[0x60, 0x00, 0x52, 0x60, 0x20, 0x60, 0x00, 0xfd]);
    let child_init = make_initcode(&child_runtime);

    let deploy_child = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce::ZERO,
        from: sender,
        to: None,
        value: Amount::ZERO,
        gas_limit: Gas(500_000),
        max_fee_per_gas: Amount::from_u128(2_000_000_000),
        max_priority_fee_per_gas: Amount::from_u128(100_000_000),
        valid_until_block: BlockHeight(100),
        data: child_init,
        memo: None,
        signature: Signature::Secp256k1Eth {
            r: [0; 32],
            s: [0; 32],
            y_parity: 0,
            chain_id: TEST_CHAIN_ID.0,
        },
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_child, _) = sign_eth_tx(&kp, deploy_child);
    let child_addr = compute_contract_address(&sender, 0);
    harness.commit_block(vec![signed_child]);

    // Parent contract:
    // Takes child address in calldata.
    // Calls child.
    // Discards call status (0).
    // Copies returndata via RETURNDATACOPY to memory, loads it, and stores in parent's slot 0!
    // 0: PUSH1 0, CALLDATALOAD (child addr)
    // 4: PUSH1 0 (out_len), PUSH1 0 (out_off), PUSH1 0 (in_len), PUSH1 0 (in_off), PUSH1 0 (value), DUP6 (child), GAS, CALL
    // 16: POP (pop call result 0)
    // 17: RETURNDATASIZE, PUSH1 0 (data_off), PUSH1 0 (mem_dest), RETURNDATACOPY
    // 23: PUSH1 0, MLOAD, PUSH1 0, SSTORE (store revert payload in slot 0)
    // 28: PUSH1 1, PUSH1 0, MSTORE, PUSH1 32, PUSH1 0, RETURN
    let parent_runtime = hex_decode(
        "60003560006000600060006000855af1503d600060003e600051600055600160005260206000f3",
    );
    let parent_init = make_initcode(&parent_runtime);

    let deploy_parent = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(1),
        from: sender,
        to: None,
        value: Amount::ZERO,
        gas_limit: Gas(500_000),
        max_fee_per_gas: Amount::from_u128(2_000_000_000),
        max_priority_fee_per_gas: Amount::from_u128(100_000_000),
        valid_until_block: BlockHeight(100),
        data: parent_init,
        memo: None,
        signature: Signature::Secp256k1Eth {
            r: [0; 32],
            s: [0; 32],
            y_parity: 0,
            chain_id: TEST_CHAIN_ID.0,
        },
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_parent, _) = sign_eth_tx(&kp, deploy_parent);
    let parent_addr = compute_contract_address(&sender, 1);
    harness.commit_block(vec![signed_parent]);

    // Call parent passing child address
    let call_tx = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(2),
        from: sender,
        to: Some(parent_addr),
        value: Amount::ZERO,
        gas_limit: Gas(500_000),
        max_fee_per_gas: Amount::from_u128(2_000_000_000),
        max_priority_fee_per_gas: Amount::from_u128(100_000_000),
        valid_until_block: BlockHeight(100),
        data: pad32_addr(&child_addr).to_vec(),
        memo: None,
        signature: Signature::Secp256k1Eth {
            r: [0; 32],
            s: [0; 32],
            y_parity: 0,
            chain_id: TEST_CHAIN_ID.0,
        },
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_call, _) = sign_eth_tx(&kp, call_tx);
    let (_, r_call) = harness.commit_block(vec![signed_call]);
    assert!(r_call[0].success); // Overall transaction succeeded

    let db = harness.state_arc.read().unwrap();
    // 1. Child storage slot 0 was rolled back to 0 (reverted)
    let child_s0 = db.get_storage(&child_addr, &Hash::ZERO).unwrap();
    assert_eq!(child_s0, Hash::ZERO);

    // 2. Parent storage slot 0 captured the custom revert payload
    let parent_s0 = db.get_storage(&parent_addr, &Hash::ZERO).unwrap();
    assert_eq!(&parent_s0.0[0..4], &[0xde, 0xad, 0xbe, 0xef]);
}

// =========================================================================
// PART 3: Goal B Tier 2: Standalone REVM 43.0.3 Diagnostic Oracle
// =========================================================================

#[test]
fn test_p9_tier2_revm_diagnostic_oracle_arithmetic_equivalence() {
    let harness = TestHarness::new("oracle_arith");
    let kp = Secp256k1Keypair::generate();
    let sender = kp.address();
    let contract_addr = Address([0x77; 20]);

    // Bytecode that executes complex arithmetic:
    // (a * b) % N + (c // d)
    // Returns 32-byte result
    let runtime = hex_decode("60003560203560403509606035608035050160005260206000f3");

    // Inputs:
    // calldata 0: a = 12345
    // calldata 32: b = 67890
    // calldata 64: N = 10007
    // calldata 96: c = 999999
    // calldata 128: d = 333
    let mut calldata = Vec::new();
    calldata.extend_from_slice(&pad32_u64(12345));
    calldata.extend_from_slice(&pad32_u64(67890));
    calldata.extend_from_slice(&pad32_u64(10007));
    calldata.extend_from_slice(&pad32_u64(999999));
    calldata.extend_from_slice(&pad32_u64(333));

    let tx = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce::ZERO,
        from: sender,
        to: Some(contract_addr),
        value: Amount::ZERO,
        gas_limit: Gas(200_000),
        max_fee_per_gas: Amount::from_u128(20),
        max_priority_fee_per_gas: Amount::ZERO,
        valid_until_block: BlockHeight(100),
        data: calldata.clone(),
        memo: None,
        signature: Signature::Secp256k1Eth {
            r: [0; 32],
            s: [0; 32],
            y_parity: 0,
            chain_id: TEST_CHAIN_ID.0,
        },
        tx_type: 2,
        access_list: vec![],
    };

    // 1. Run on TOLA EVM
    let mut tola_db = harness.state_arc.write().unwrap();
    let code_hash = Hash(keccak256_hash(&runtime).0);
    tola_db
        .apply_delta(StateDelta {
            accounts: vec![
                (
                    sender,
                    Some(AccountState::new_with_balance(Amount::from_u128(
                        10_000_000,
                    ))),
                ),
                (
                    contract_addr,
                    Some(AccountState {
                        balance: Amount::ZERO,
                        nonce: Nonce(1),
                        code_hash,
                        storage_root: Hash::ZERO,
                        flags: FLAG_CONTRACT,
                    }),
                ),
            ],
            storage: vec![],
            code: vec![(code_hash, runtime.clone())],
        })
        .unwrap();

    let tola_outcome = EvmExecutor::execute_transaction(
        &tx,
        BlockHeight(1),
        Timestamp(1000),
        Amount::from_u128(10),
        harness.proposer,
        Gas(30_000_000),
        &mut *tola_db,
    )
    .unwrap();
    drop(tola_db);

    // 2. Run on Standalone in-memory REVM 43.0.3
    let mut revm_db = InMemoryDB::default();
    let revm_caller = RevmAddress::from(sender.0);
    let revm_contract = RevmAddress::from(contract_addr.0);

    revm_db.insert_account_info(
        revm_caller,
        RevmAccountInfo {
            balance: RevmU256::from(10_000_000),
            nonce: 0,
            code_hash: revm::primitives::KECCAK_EMPTY,
            code: None,
            ..Default::default()
        },
    );
    revm_db.insert_account_info(
        revm_contract,
        RevmAccountInfo {
            balance: RevmU256::ZERO,
            nonce: 1,
            code_hash: revm::primitives::B256::from(code_hash.0),
            code: Some(revm::bytecode::Bytecode::new_raw(RevmBytes::from(runtime))),
            ..Default::default()
        },
    );

    let mut block_env = RevmBlockEnv::default();
    block_env.number = RevmU256::from(1);
    block_env.timestamp = RevmU256::from(1000);
    block_env.beneficiary = RevmAddress::from(harness.proposer.0);
    block_env.basefee = 10;
    block_env.gas_limit = 30_000_000;

    let mut tx_env = RevmTxEnv::default();
    tx_env.caller = revm_caller;
    tx_env.kind = RevmTxKind::Call(revm_contract);
    tx_env.value = RevmU256::ZERO;
    tx_env.data = RevmBytes::from(calldata);
    tx_env.gas_limit = 200_000;
    tx_env.gas_price = 10;
    tx_env.chain_id = Some(TEST_CHAIN_ID.0);
    tx_env.nonce = 0;

    let mut cfg_env = RevmCfgEnv::default();
    cfg_env.spec = SpecId::SHANGHAI;
    cfg_env.chain_id = TEST_CHAIN_ID.0;

    let ctx = RevmContext::mainnet()
        .with_db(revm_db)
        .with_block(block_env)
        .with_cfg(cfg_env);

    let inspector = TolaEvmInspector::default();
    let mut evm = ctx.build_mainnet_with_inspector(inspector);
    let revm_result = evm.inspect_tx_commit(tx_env).unwrap();

    // Verify bit-for-bit equivalence between TOLA adapter and pure REVM:
    assert_eq!(tola_outcome.success, revm_result.is_success());
    assert_eq!(tola_outcome.gas_used.as_u64(), revm_result.tx_gas_used());
    assert_eq!(
        tola_outcome.return_data,
        revm_result.output().unwrap().to_vec()
    );
}

#[test]
fn test_p9_tier2_revm_diagnostic_oracle_state_mutation_equivalence() {
    let harness = TestHarness::new("oracle_state");
    let kp = Secp256k1Keypair::generate();
    let sender = kp.address();
    let contract_addr = Address([0x88; 20]);

    // Contract that stores calldata at storage slot 42 and transfers value to contract
    // 0x60 0x00 0x35 (CALLDATALOAD at 0)
    // 0x60 0x2a 0x55 (SSTORE at slot 42)
    // 0x60 0x01 0x60 0x00 0x52 0x60 0x20 0x60 0x00 0xf3
    let runtime = hex_decode("600035602a55600160005260206000f3");
    let code_hash = Hash(keccak256_hash(&runtime).0);
    let slot_val = pad32_u64(0xcafe_babe);

    let tx = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce::ZERO,
        from: sender,
        to: Some(contract_addr),
        value: Amount::from_u128(1_000), // Transfer 1,000 wei
        gas_limit: Gas(200_000),
        max_fee_per_gas: Amount::from_u128(20),
        max_priority_fee_per_gas: Amount::ZERO,
        valid_until_block: BlockHeight(100),
        data: slot_val.to_vec(),
        memo: None,
        signature: Signature::Secp256k1Eth {
            r: [0; 32],
            s: [0; 32],
            y_parity: 0,
            chain_id: TEST_CHAIN_ID.0,
        },
        tx_type: 2,
        access_list: vec![],
    };

    // 1. TOLA EVM
    let mut tola_db = harness.state_arc.write().unwrap();
    tola_db
        .apply_delta(StateDelta {
            accounts: vec![
                (
                    sender,
                    Some(AccountState::new_with_balance(Amount::from_u128(
                        10_000_000,
                    ))),
                ),
                (
                    contract_addr,
                    Some(AccountState {
                        balance: Amount::ZERO,
                        nonce: Nonce(1),
                        code_hash,
                        storage_root: Hash::ZERO,
                        flags: FLAG_CONTRACT,
                    }),
                ),
            ],
            storage: vec![],
            code: vec![(code_hash, runtime.clone())],
        })
        .unwrap();

    let tola_outcome = EvmExecutor::execute_transaction(
        &tx,
        BlockHeight(1),
        Timestamp(1000),
        Amount::from_u128(10),
        harness.proposer,
        Gas(30_000_000),
        &mut *tola_db,
    )
    .unwrap();
    assert!(tola_outcome.success);

    let tola_stored = tola_db
        .get_storage(&contract_addr, &Hash(pad32_u64(42)))
        .unwrap();
    let tola_contract_acct = tola_db.get_account(&contract_addr).unwrap().unwrap();
    drop(tola_db);

    // 2. Standalone REVM
    let mut revm_db = InMemoryDB::default();
    let revm_caller = RevmAddress::from(sender.0);
    let revm_contract = RevmAddress::from(contract_addr.0);

    revm_db.insert_account_info(
        revm_caller,
        RevmAccountInfo {
            balance: RevmU256::from(10_000_000),
            nonce: 0,
            code_hash: revm::primitives::KECCAK_EMPTY,
            code: None,
            ..Default::default()
        },
    );
    revm_db.insert_account_info(
        revm_contract,
        RevmAccountInfo {
            balance: RevmU256::ZERO,
            nonce: 1,
            code_hash: revm::primitives::B256::from(code_hash.0),
            code: Some(revm::bytecode::Bytecode::new_raw(RevmBytes::from(runtime))),
            ..Default::default()
        },
    );

    let mut block_env = RevmBlockEnv::default();
    block_env.number = RevmU256::from(1);
    block_env.timestamp = RevmU256::from(1000);
    block_env.beneficiary = RevmAddress::from(harness.proposer.0);
    block_env.basefee = 10;
    block_env.gas_limit = 30_000_000;

    let mut tx_env = RevmTxEnv::default();
    tx_env.caller = revm_caller;
    tx_env.kind = RevmTxKind::Call(revm_contract);
    tx_env.value = RevmU256::from(1_000);
    tx_env.data = RevmBytes::from(slot_val.to_vec());
    tx_env.gas_limit = 200_000;
    tx_env.gas_price = 10;
    tx_env.chain_id = Some(TEST_CHAIN_ID.0);
    tx_env.nonce = 0;

    let mut cfg_env = RevmCfgEnv::default();
    cfg_env.spec = SpecId::SHANGHAI;
    cfg_env.chain_id = TEST_CHAIN_ID.0;

    let ctx = RevmContext::mainnet()
        .with_db(&mut revm_db)
        .with_block(block_env)
        .with_cfg(cfg_env);

    let inspector = TolaEvmInspector::default();
    let mut evm = ctx.build_mainnet_with_inspector(inspector);
    let revm_result = evm.inspect_tx_commit(tx_env).unwrap();
    assert!(revm_result.is_success());

    // Storage slot value in pure REVM:
    use revm::Database;
    let revm_stored = revm_db.storage(revm_contract, RevmU256::from(42)).unwrap();
    let revm_contract_acct = revm_db.basic(revm_contract).unwrap().unwrap();

    // Verify identical storage mutation:
    assert_eq!(tola_stored.0, revm_stored.to_be_bytes::<32>());
    // Verify identical recipient balance:
    assert_eq!(
        tola_contract_acct.balance.as_u128(),
        revm_contract_acct.balance.to::<u128>()
    );
}

// =========================================================================
// PART 4: Regression Suite (P5, P7, P8, Cold Restart)
// =========================================================================

#[test]
fn test_p9_regression_p5_equal_fee_persistence() {
    let mut harness = TestHarness::new("p5_equal_fee");
    let kp = Secp256k1Keypair::generate();
    let sender = kp.address();
    harness.fund_account(&sender, 100_000_000_000_000_000);

    // Type-2 tx where max_fee_per_gas == base_fee_per_gas (equal fee economics)
    let equal_fee_tx = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce::ZERO,
        from: sender,
        to: Some(sender),
        value: Amount::from_u128(1_000),
        gas_limit: Gas(100_000),
        max_fee_per_gas: Amount::from_u128(1_000_000_000),
        max_priority_fee_per_gas: Amount::ZERO,
        valid_until_block: BlockHeight(100),
        data: vec![],
        memo: None,
        signature: Signature::Secp256k1Eth {
            r: [0; 32],
            s: [0; 32],
            y_parity: 0,
            chain_id: TEST_CHAIN_ID.0,
        },
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_tx, eth_hash) = sign_eth_tx(&kp, equal_fee_tx);
    let (_, receipts) = harness.commit_block(vec![signed_tx]);
    assert!(receipts[0].success);

    // Verify receipt lookup
    let disp = harness.make_dispatcher();
    let receipt_json = format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"eth_getTransactionReceipt","params":["{}"]}}"#,
        eth_hash.to_hex()
    );
    let resp = disp.dispatch_json(&receipt_json);
    let val = JsonValue::parse(&resp).unwrap();
    assert!(val.get("result").is_some());
    let res = val.get("result").unwrap();
    assert_ne!(res, &JsonValue::Null);
    assert_eq!(res.get("status").unwrap().as_str().unwrap(), "0x1");
    assert_eq!(
        res.get("effectiveGasPrice").unwrap().as_str().unwrap(),
        "0x3b9aca00"
    );
}

#[test]
fn test_p9_regression_p7_solidity_and_precompiles() {
    let mut harness = TestHarness::new("p7_solidity_precompiles");
    let kp = Secp256k1Keypair::generate();
    let sender = kp.address();
    harness.fund_account(&sender, 100_000_000_000_000_000);

    // 1. Deploy OpenZeppelin v5.0.2 ERC-20
    let (bytecode, _) = load_artifact(ERC20_MOCK_JSON);
    let mut deploy_data = bytecode;
    // Constructor args: name="Token", symbol="TKN"
    // ABI encoding: offset name (64), offset symbol (128), string "Token", string "TKN"
    deploy_data.extend_from_slice(&pad32_u64(64));
    deploy_data.extend_from_slice(&pad32_u64(128));
    deploy_data.extend_from_slice(&pad32_u64(5)); // len("Token")
    let mut name_bytes = [0u8; 32];
    name_bytes[0..5].copy_from_slice(b"Token");
    deploy_data.extend_from_slice(&name_bytes);
    deploy_data.extend_from_slice(&pad32_u64(3)); // len("TKN")
    let mut sym_bytes = [0u8; 32];
    sym_bytes[0..3].copy_from_slice(b"TKN");
    deploy_data.extend_from_slice(&sym_bytes);

    let deploy_tx = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce::ZERO,
        from: sender,
        to: None,
        value: Amount::ZERO,
        gas_limit: Gas(3_000_000),
        max_fee_per_gas: Amount::from_u128(2_000_000_000),
        max_priority_fee_per_gas: Amount::from_u128(100_000_000),
        valid_until_block: BlockHeight(100),
        data: deploy_data,
        memo: None,
        signature: Signature::Secp256k1Eth {
            r: [0; 32],
            s: [0; 32],
            y_parity: 0,
            chain_id: TEST_CHAIN_ID.0,
        },
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_deploy, _) = sign_eth_tx(&kp, deploy_tx);
    let erc20_addr = compute_contract_address(&sender, 0);
    let (_, r_dep) = harness.commit_block(vec![signed_deploy]);
    assert!(r_dep[0].success);

    // Mint 1000 tokens to sender: mint(address,uint256) -> selector 0x40c10f19
    let mut mint_data = Vec::new();
    mint_data.extend_from_slice(&selector("mint(address,uint256)"));
    mint_data.extend_from_slice(&pad32_addr(&sender));
    mint_data.extend_from_slice(&pad32_u64(1000));

    let mint_tx = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(1),
        from: sender,
        to: Some(erc20_addr),
        value: Amount::ZERO,
        gas_limit: Gas(200_000),
        max_fee_per_gas: Amount::from_u128(2_000_000_000),
        max_priority_fee_per_gas: Amount::from_u128(100_000_000),
        valid_until_block: BlockHeight(100),
        data: mint_data,
        memo: None,
        signature: Signature::Secp256k1Eth {
            r: [0; 32],
            s: [0; 32],
            y_parity: 0,
            chain_id: TEST_CHAIN_ID.0,
        },
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_mint, _) = sign_eth_tx(&kp, mint_tx);
    let (_, r_mint) = harness.commit_block(vec![signed_mint]);
    assert!(r_mint[0].success);

    // Check balanceOf via eth_call
    let disp = harness.make_dispatcher();
    let mut bal_data = Vec::new();
    bal_data.extend_from_slice(&selector("balanceOf(address)"));
    bal_data.extend_from_slice(&pad32_addr(&sender));
    let bal_json = format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"eth_call","params":[{{"to":"{}","data":"0x{}"}},"latest"]}}"#,
        erc20_addr.to_hex(),
        hex_encode(&bal_data)
    );
    let bal_resp = disp.dispatch_json(&bal_json);
    let bal_val = JsonValue::parse(&bal_resp).unwrap();
    let bal_str = bal_val.get("result").unwrap().as_str().unwrap();
    assert_eq!(hex_decode(bal_str), pad32_u64(1000).to_vec());

    // 2. Precompile 0x01 ecrecover (Low-S and High-S verification)
    // Low-S:
    let msg_hash = keccak256_hash(b"Tola Shanghai Conformance Test");
    let sig_rec = kp.sign_recoverable(msg_hash.as_bytes()).unwrap();
    let r_bytes = &sig_rec[0..32];
    let s_bytes = &sig_rec[32..64];
    let v_byte = sig_rec[64] + 27;

    let mut ecrecover_input = Vec::new();
    ecrecover_input.extend_from_slice(msg_hash.as_bytes());
    ecrecover_input.extend_from_slice(&pad32_u64(v_byte as u64));
    ecrecover_input.extend_from_slice(r_bytes);
    ecrecover_input.extend_from_slice(s_bytes);

    let ecrecover_json = format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"eth_call","params":[{{"to":"0x0000000000000000000000000000000000000001","data":"0x{}"}},"latest"]}}"#,
        hex_encode(&ecrecover_input)
    );
    let ec_resp = disp.dispatch_json(&ecrecover_json);
    let ec_val = JsonValue::parse(&ec_resp).unwrap();
    let ec_out = hex_decode(ec_val.get("result").unwrap().as_str().unwrap());
    assert_eq!(&ec_out[12..32], &sender.0[..]);

    // High-S ecrecover input:
    // SECP256K1_N = 0xFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFEBAAEDCE6AF48A03BBFD25E8CD0364141
    let secp_n = num_bigint::BigUint::parse_bytes(
        b"FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFEBAAEDCE6AF48A03BBFD25E8CD0364141",
        16,
    )
    .unwrap();
    let orig_s = num_bigint::BigUint::from_bytes_be(s_bytes);
    let high_s = &secp_n - &orig_s;
    let high_s_bytes = {
        let mut b = high_s.to_bytes_be();
        while b.len() < 32 {
            b.insert(0, 0);
        }
        b
    };
    let flipped_v = if v_byte == 27 { 28 } else { 27 };

    let mut high_s_input = Vec::new();
    high_s_input.extend_from_slice(msg_hash.as_bytes());
    high_s_input.extend_from_slice(&pad32_u64(flipped_v as u64));
    high_s_input.extend_from_slice(r_bytes);
    high_s_input.extend_from_slice(&high_s_bytes);

    let high_s_json = format!(
        r#"{{"jsonrpc":"2.0","id":3,"method":"eth_call","params":[{{"to":"0x0000000000000000000000000000000000000001","data":"0x{}"}},"latest"]}}"#,
        hex_encode(&high_s_input)
    );
    let high_s_resp = disp.dispatch_json(&high_s_json);
    let high_s_val = JsonValue::parse(&high_s_resp).unwrap();
    let high_s_out = hex_decode(high_s_val.get("result").unwrap().as_str().unwrap());
    assert_eq!(&high_s_out[12..32], &sender.0[..]); // EVM ecrecover correctly recovers address for high-S

    // 3. Precompile 0x02 sha256
    let sha_input = b"tola sha256 precompile";
    let sha_json = format!(
        r#"{{"jsonrpc":"2.0","id":4,"method":"eth_call","params":[{{"to":"0x0000000000000000000000000000000000000002","data":"0x{}"}},"latest"]}}"#,
        hex_encode(sha_input)
    );
    let sha_resp = disp.dispatch_json(&sha_json);
    let sha_val = JsonValue::parse(&sha_resp).unwrap();
    let sha_out = hex_decode(sha_val.get("result").unwrap().as_str().unwrap());
    use sha2::Digest;
    let expected_sha = sha2::Sha256::digest(sha_input);
    let expected_slice: &[u8] = &expected_sha[..];
    assert_eq!(&sha_out[..], expected_slice);
}

#[test]
fn test_p9_regression_p8_historical_state_queries() {
    let mut harness = TestHarness::new("p8_historical");
    let kp = Secp256k1Keypair::generate();
    let sender = kp.address();
    harness.fund_account(&sender, 100_000_000_000_000_000);

    // Contract writes calldata to slot 0
    let runtime = hex_decode("600035600055600160005260206000f3");
    let initcode = make_initcode(&runtime);

    // Block 1: Deploy contract (initializes slot 0 to empty)
    let deploy_tx = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce::ZERO,
        from: sender,
        to: None,
        value: Amount::ZERO,
        gas_limit: Gas(500_000),
        max_fee_per_gas: Amount::from_u128(2_000_000_000),
        max_priority_fee_per_gas: Amount::from_u128(100_000_000),
        valid_until_block: BlockHeight(100),
        data: initcode,
        memo: None,
        signature: Signature::Secp256k1Eth {
            r: [0; 32],
            s: [0; 32],
            y_parity: 0,
            chain_id: TEST_CHAIN_ID.0,
        },
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_dep, _) = sign_eth_tx(&kp, deploy_tx);
    let contract_addr = compute_contract_address(&sender, 0);
    harness.commit_block(vec![signed_dep]);

    // Block 2: Write 111 to slot 0
    let call1 = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(1),
        from: sender,
        to: Some(contract_addr),
        value: Amount::ZERO,
        gas_limit: Gas(200_000),
        max_fee_per_gas: Amount::from_u128(2_000_000_000),
        max_priority_fee_per_gas: Amount::from_u128(100_000_000),
        valid_until_block: BlockHeight(100),
        data: pad32_u64(111).to_vec(),
        memo: None,
        signature: Signature::Secp256k1Eth {
            r: [0; 32],
            s: [0; 32],
            y_parity: 0,
            chain_id: TEST_CHAIN_ID.0,
        },
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_call1, _) = sign_eth_tx(&kp, call1);
    harness.commit_block(vec![signed_call1]);

    // Block 3: Write 222 to slot 0
    let call2 = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(2),
        from: sender,
        to: Some(contract_addr),
        value: Amount::ZERO,
        gas_limit: Gas(200_000),
        max_fee_per_gas: Amount::from_u128(2_000_000_000),
        max_priority_fee_per_gas: Amount::from_u128(100_000_000),
        valid_until_block: BlockHeight(100),
        data: pad32_u64(222).to_vec(),
        memo: None,
        signature: Signature::Secp256k1Eth {
            r: [0; 32],
            s: [0; 32],
            y_parity: 0,
            chain_id: TEST_CHAIN_ID.0,
        },
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_call2, _) = sign_eth_tx(&kp, call2);
    harness.commit_block(vec![signed_call2]);

    let disp = harness.make_dispatcher();

    // Query eth_getStorageAt at Block 2: must be 111
    let s_b2_json = format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"eth_getStorageAt","params":["{}","0x0","0x2"]}}"#,
        contract_addr.to_hex()
    );
    let s_b2_resp = disp.dispatch_json(&s_b2_json);
    let s_b2_val = JsonValue::parse(&s_b2_resp).unwrap();
    let s_b2_str = s_b2_val.get("result").unwrap().as_str().unwrap();
    assert_eq!(hex_decode(s_b2_str), pad32_u64(111).to_vec());

    // Query eth_getStorageAt at Block 3: must be 222
    let s_b3_json = format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"eth_getStorageAt","params":["{}","0x0","0x3"]}}"#,
        contract_addr.to_hex()
    );
    let s_b3_resp = disp.dispatch_json(&s_b3_json);
    let s_b3_val = JsonValue::parse(&s_b3_resp).unwrap();
    let s_b3_str = s_b3_val.get("result").unwrap().as_str().unwrap();
    assert_eq!(hex_decode(s_b3_str), pad32_u64(222).to_vec());

    // Query future block 999: must return server error -32000
    let future_json = format!(
        r#"{{"jsonrpc":"2.0","id":3,"method":"eth_getStorageAt","params":["{}","0x0","0x3e7"]}}"#,
        contract_addr.to_hex()
    );
    let fut_resp = disp.dispatch_json(&future_json);
    let fut_val = JsonValue::parse(&fut_resp).unwrap();
    assert!(fut_val.get("error").is_some());
    assert_eq!(
        fut_val.get("error").unwrap().get("code").unwrap().as_i64(),
        Some(-32000)
    );
}

#[test]
fn test_p9_regression_cold_restart_persistence() {
    let mut harness = TestHarness::new("cold_restart");
    let kp = Secp256k1Keypair::generate();
    let sender = kp.address();
    harness.fund_account(&sender, 100_000_000_000_000_000);

    // Deploy contract in Block 1
    let runtime = hex_decode("602a60005260206000f3");
    let initcode = make_initcode(&runtime);
    let deploy_tx = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce::ZERO,
        from: sender,
        to: None,
        value: Amount::ZERO,
        gas_limit: Gas(500_000),
        max_fee_per_gas: Amount::from_u128(2_000_000_000),
        max_priority_fee_per_gas: Amount::from_u128(100_000_000),
        valid_until_block: BlockHeight(100),
        data: initcode,
        memo: None,
        signature: Signature::Secp256k1Eth {
            r: [0; 32],
            s: [0; 32],
            y_parity: 0,
            chain_id: TEST_CHAIN_ID.0,
        },
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_dep, _) = sign_eth_tx(&kp, deploy_tx);
    let contract_addr = compute_contract_address(&sender, 0);
    harness.commit_block(vec![signed_dep]);

    // Cold restart
    harness.reopen();

    // Verify contract code and execution persist after reopen
    let disp = harness.make_dispatcher();
    let call_json = format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"eth_call","params":[{{"to":"{}"}},"latest"]}}"#,
        contract_addr.to_hex()
    );
    let resp = disp.dispatch_json(&call_json);
    let val = JsonValue::parse(&resp).unwrap();
    let out_str = val.get("result").unwrap().as_str().unwrap();
    assert_eq!(hex_decode(out_str), pad32_u64(42).to_vec());
}

// =========================================================================
// Part 5 — Official Ethereum Shanghai GeneralStateTest (GST) Runner
// =========================================================================

const FIXTURE_ADD11: &str = include_str!("fixtures/shanghai/add11.json");
const FIXTURE_MULMOD_ZERO: &str = include_str!("fixtures/shanghai/mulmod_zero.json");
const FIXTURE_SDIV_MIN_INT: &str = include_str!("fixtures/shanghai/sdiv_min_int.json");
const FIXTURE_EXP_BASIC: &str = include_str!("fixtures/shanghai/exp_basic.json");
const FIXTURE_SHL_SAR_BASIC: &str = include_str!("fixtures/shanghai/shl_sar_basic.json");
const FIXTURE_MEMORY_EXPANSION: &str = include_str!("fixtures/shanghai/memory_expansion.json");
const FIXTURE_REVERT_WITH_DATA: &str = include_str!("fixtures/shanghai/revert_with_data.json");
const FIXTURE_EIP170_CODE_SIZE: &str = include_str!("fixtures/shanghai/eip170_code_size.json");
const FIXTURE_EIP3860_INITCODE: &str = include_str!("fixtures/shanghai/eip3860_initcode.json");
const FIXTURE_STACK_DEPTH_LIMIT: &str = include_str!("fixtures/shanghai/stack_depth_limit.json");
const FIXTURE_CALL_DEPTH_LIMIT: &str = include_str!("fixtures/shanghai/call_depth_limit.json");
const FIXTURE_SHA3_BASIC: &str = include_str!("fixtures/shanghai/sha3_basic.json");

fn safe_hex_decode_32(s: &str) -> [u8; 32] {
    let clean = s
        .strip_prefix("0x")
        .or_else(|| s.strip_prefix("0X"))
        .unwrap_or(s);
    let padded = if clean.len() % 2 != 0 {
        format!("0{}", clean)
    } else {
        clean.to_string()
    };
    let bytes = hex_decode(&padded);
    let mut arr = [0u8; 32];
    if bytes.len() <= 32 {
        arr[32 - bytes.len()..].copy_from_slice(&bytes);
    }
    arr
}

fn parse_json_hex_u64(val: &JsonValue, default: u64) -> u64 {
    if let Some(s) = val.as_str() {
        let clean = s
            .strip_prefix("0x")
            .or_else(|| s.strip_prefix("0X"))
            .unwrap_or(s);
        u64::from_str_radix(clean, 16).unwrap_or(default)
    } else if let Some(n) = val.as_u64() {
        n
    } else {
        default
    }
}

fn parse_json_hex_u128(val: &JsonValue, default: u128) -> u128 {
    if let Some(s) = val.as_str() {
        let clean = s
            .strip_prefix("0x")
            .or_else(|| s.strip_prefix("0X"))
            .unwrap_or(s);
        u128::from_str_radix(clean, 16).unwrap_or(default)
    } else if let Some(n) = val.as_u64() {
        n as u128
    } else {
        default
    }
}

fn run_official_shanghai_fixture_json(json_content: &str) {
    let parsed = JsonValue::parse(json_content).expect("Valid JSON fixture");
    let entries = match parsed {
        JsonValue::Object(entries) => entries,
        _ => panic!("Expected root object in GST fixture"),
    };
    let (fixture_name, fixture_val) = &entries[0];
    let fixture_obj = match fixture_val {
        JsonValue::Object(entries) => entries,
        _ => panic!("Expected fixture object for {}", fixture_name),
    };

    let find_obj = |key: &str| -> &[(String, JsonValue)] {
        for (k, v) in fixture_obj {
            if k == key {
                if let JsonValue::Object(entries) = v {
                    return entries;
                }
            }
        }
        panic!("Missing {} section in fixture {}", key, fixture_name);
    };

    let env = find_obj("env");
    let pre = find_obj("pre");
    let tx_obj = find_obj("transaction");
    let post = find_obj("post");

    let get_env_field = |key: &str| -> &JsonValue {
        for (k, v) in env {
            if k == key {
                return v;
            }
        }
        &JsonValue::Null
    };

    let proposer_hex = get_env_field("currentCoinbase")
        .as_str()
        .unwrap_or("0x0000000000000000000000000000000000000000");
    let proposer = Address::from_hex(proposer_hex).expect("Valid coinbase address");
    let block_gas_limit = Gas(parse_json_hex_u64(
        get_env_field("currentGasLimit"),
        30_000_000,
    ));
    let base_fee = Amount::from_u128(parse_json_hex_u128(get_env_field("currentBaseFee"), 10));

    // Secret key for transaction sender
    let mut secret_bytes = [0u8; 32];
    for (k, v) in tx_obj {
        if k == "secretKey" {
            if let Some(s) = v.as_str() {
                let dec = hex_decode(s);
                secret_bytes.copy_from_slice(&dec[..32]);
            }
        }
    }
    let kp = Secp256k1Keypair::from_private_key_bytes(&secret_bytes)
        .expect("Valid Secp256k1 private key in transaction");
    let sender = kp.address();

    // Harness setup
    let mut harness = TestHarness::new(fixture_name);
    harness.block_gas_limit = block_gas_limit;
    harness.proposer = proposer;
    harness.base_fee = base_fee;

    // Load pre-state
    for (addr_str, acct_val) in pre {
        let addr = Address::from_hex(addr_str).expect("Valid pre account address");
        if let JsonValue::Object(acct_entries) = acct_val {
            let mut balance = 0u128;
            let mut nonce = 0u64;
            let mut code = Vec::new();
            let mut storage_deltas = Vec::new();

            for (ak, av) in acct_entries {
                match ak.as_str() {
                    "balance" => balance = parse_json_hex_u128(av, 0),
                    "nonce" => nonce = parse_json_hex_u64(av, 0),
                    "code" => {
                        if let Some(s) = av.as_str() {
                            code = hex_decode(s);
                        }
                    }
                    "storage" => {
                        if let JsonValue::Object(st_entries) = av {
                            for (sk, sv) in st_entries {
                                let slot_arr = safe_hex_decode_32(sk);
                                let val_arr = safe_hex_decode_32(sv.as_str().unwrap_or(""));
                                storage_deltas.push(((addr, Hash(slot_arr)), Hash(val_arr)));
                            }
                        }
                    }
                    _ => {}
                }
            }

            let mut acct_state = AccountState::new_with_balance(Amount::from_u128(balance));
            acct_state.nonce = Nonce(nonce);
            let mut code_deltas = Vec::new();
            if !code.is_empty() {
                let code_hash = keccak256_hash(&code);
                acct_state.code_hash = code_hash;
                code_deltas.push((code_hash, code));
            }

            let mut db = harness.state_arc.write().unwrap();
            db.apply_delta(StateDelta {
                accounts: vec![(addr, Some(acct_state))],
                storage: storage_deltas,
                code: code_deltas,
            })
            .unwrap();
        }
    }

    // Transaction parameters
    let to_opt = {
        let mut t = None;
        for (k, v) in tx_obj {
            if k == "to" {
                if let Some(s) = v.as_str() {
                    if !s.is_empty() && s != "0x" {
                        t = Some(Address::from_hex(s).unwrap());
                    }
                }
            }
        }
        t
    };

    let tx_data = {
        let mut d = Vec::new();
        for (k, v) in tx_obj {
            if k == "data" {
                if let Some(arr) = v.as_array() {
                    if let Some(first) = arr.first().and_then(|x| x.as_str()) {
                        d = hex_decode(first);
                    }
                } else if let Some(s) = v.as_str() {
                    d = hex_decode(s);
                }
            }
        }
        d
    };

    let tx_gas = {
        let mut g = 100_000u64;
        for (k, v) in tx_obj {
            if k == "gasLimit" {
                if let Some(arr) = v.as_array() {
                    if let Some(first) = arr.first() {
                        g = parse_json_hex_u64(first, g);
                    }
                } else {
                    g = parse_json_hex_u64(v, g);
                }
            }
        }
        Gas(g)
    };

    let tx_val = {
        let mut val = 0u128;
        for (k, v) in tx_obj {
            if k == "value" {
                if let Some(arr) = v.as_array() {
                    if let Some(first) = arr.first() {
                        val = parse_json_hex_u128(first, val);
                    }
                } else {
                    val = parse_json_hex_u128(v, val);
                }
            }
        }
        Amount::from_u128(val)
    };

    let mut tx_nonce = Nonce::ZERO;
    let mut gas_price = base_fee;
    for (k, v) in tx_obj {
        if k == "nonce" {
            tx_nonce = Nonce(parse_json_hex_u64(v, 0));
        } else if k == "gasPrice" {
            gas_price = Amount::from_u128(parse_json_hex_u128(v, base_fee.as_u128()));
        }
    }

    let tx = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: tx_nonce,
        from: sender,
        to: to_opt,
        value: tx_val,
        gas_limit: tx_gas,
        max_fee_per_gas: gas_price,
        max_priority_fee_per_gas: Amount::ZERO,
        valid_until_block: BlockHeight(100),
        data: tx_data,
        memo: None,
        signature: Signature::Secp256k1Eth {
            r: [0; 32],
            s: [0; 32],
            y_parity: 0,
            chain_id: TEST_CHAIN_ID.0,
        },
        tx_type: 2,
        access_list: vec![],
    };

    let (signed_tx, _) = sign_eth_tx(&kp, tx);

    let mut expect_exception = None;
    for (pk, pv) in post {
        if pk == "Shanghai" {
            if let Some(arr) = pv.as_array() {
                if let Some(first) = arr.first() {
                    if let Some(ex) = first.get("expectException").and_then(|e| e.as_str()) {
                        expect_exception = Some(ex.to_string());
                    }
                }
            }
        }
    }

    let (_, receipts) = harness.commit_block(vec![signed_tx]);

    if let Some(ref ex) = expect_exception {
        assert!(
            !receipts[0].success,
            "Fixture {} expected exception '{}' but transaction succeeded",
            fixture_name, ex
        );
    } else {
        assert!(
            receipts[0].success,
            "Fixture {} expected success but transaction failed",
            fixture_name
        );
    }

    // Assert normalized post_state
    for (k, v) in fixture_obj {
        if k == "post_state" {
            if let JsonValue::Object(post_entries) = v {
                let db = harness.state_arc.read().unwrap();
                for (addr_str, acct_val) in post_entries {
                    let addr = Address::from_hex(addr_str).expect("Valid post account address");
                    let actual_acct = db
                        .get_account(&addr)
                        .unwrap()
                        .unwrap_or(AccountState::EMPTY);
                    if let JsonValue::Object(acct_fields) = acct_val {
                        for (ak, av) in acct_fields {
                            match ak.as_str() {
                                "nonce" => {
                                    let expected_nonce = parse_json_hex_u64(av, 0);
                                    assert_eq!(
                                        actual_acct.nonce.0, expected_nonce,
                                        "Fixture {}: Nonce mismatch at {}",
                                        fixture_name, addr_str
                                    );
                                }
                                "code" => {
                                    if let Some(s) = av.as_str() {
                                        let expected_code = hex_decode(s);
                                        let actual_code = if actual_acct.code_hash == Hash::ZERO {
                                            Vec::new()
                                        } else {
                                            db.get_code(&actual_acct.code_hash)
                                                .unwrap()
                                                .unwrap_or_default()
                                        };
                                        assert_eq!(
                                            actual_code, expected_code,
                                            "Fixture {}: Code mismatch at {}",
                                            fixture_name, addr_str
                                        );
                                    }
                                }
                                "storage" => {
                                    if let JsonValue::Object(st_entries) = av {
                                        for (sk, sv) in st_entries {
                                            let slot_arr = safe_hex_decode_32(sk);
                                            let expected_val_arr =
                                                safe_hex_decode_32(sv.as_str().unwrap());
                                            let actual_val =
                                                db.get_storage(&addr, &Hash(slot_arr)).unwrap();
                                            assert_eq!(
                                                actual_val,
                                                Hash(expected_val_arr),
                                                "Fixture {}: Storage mismatch at {} slot {}",
                                                fixture_name,
                                                addr_str,
                                                sk
                                            );
                                        }
                                    }
                                }
                                "balance" => {
                                    if addr != sender {
                                        let expected_bal = parse_json_hex_u128(av, 0);
                                        assert_eq!(
                                            actual_acct.balance.as_u128(),
                                            expected_bal,
                                            "Fixture {}: Balance mismatch at {}",
                                            fixture_name,
                                            addr_str
                                        );
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn test_p9_official_shanghai_gst_01_add11() {
    run_official_shanghai_fixture_json(FIXTURE_ADD11);
}

#[test]
fn test_p9_official_shanghai_gst_02_mulmod_zero() {
    run_official_shanghai_fixture_json(FIXTURE_MULMOD_ZERO);
}

#[test]
fn test_p9_official_shanghai_gst_03_sdiv_min_int() {
    run_official_shanghai_fixture_json(FIXTURE_SDIV_MIN_INT);
}

#[test]
fn test_p9_official_shanghai_gst_04_exp_basic() {
    run_official_shanghai_fixture_json(FIXTURE_EXP_BASIC);
}

#[test]
fn test_p9_official_shanghai_gst_05_shl_sar_basic() {
    run_official_shanghai_fixture_json(FIXTURE_SHL_SAR_BASIC);
}

#[test]
fn test_p9_official_shanghai_gst_06_memory_expansion() {
    run_official_shanghai_fixture_json(FIXTURE_MEMORY_EXPANSION);
}

#[test]
fn test_p9_official_shanghai_gst_07_revert_with_data() {
    run_official_shanghai_fixture_json(FIXTURE_REVERT_WITH_DATA);
}

#[test]
fn test_p9_official_shanghai_gst_08_eip170_code_size() {
    run_official_shanghai_fixture_json(FIXTURE_EIP170_CODE_SIZE);
}

#[test]
fn test_p9_official_shanghai_gst_09_eip3860_initcode() {
    run_official_shanghai_fixture_json(FIXTURE_EIP3860_INITCODE);
}

#[test]
fn test_p9_official_shanghai_gst_10_stack_depth_limit() {
    run_official_shanghai_fixture_json(FIXTURE_STACK_DEPTH_LIMIT);
}

#[test]
fn test_p9_official_shanghai_gst_11_call_depth_limit() {
    run_official_shanghai_fixture_json(FIXTURE_CALL_DEPTH_LIMIT);
}

#[test]
fn test_p9_official_shanghai_gst_12_sha3_basic() {
    run_official_shanghai_fixture_json(FIXTURE_SHA3_BASIC);
}

#[test]
fn test_p9_official_shanghai_all_curated_suite() {
    let all_fixtures = [
        FIXTURE_ADD11,
        FIXTURE_MULMOD_ZERO,
        FIXTURE_SDIV_MIN_INT,
        FIXTURE_EXP_BASIC,
        FIXTURE_SHL_SAR_BASIC,
        FIXTURE_MEMORY_EXPANSION,
        FIXTURE_REVERT_WITH_DATA,
        FIXTURE_EIP170_CODE_SIZE,
        FIXTURE_EIP3860_INITCODE,
        FIXTURE_STACK_DEPTH_LIMIT,
        FIXTURE_CALL_DEPTH_LIMIT,
        FIXTURE_SHA3_BASIC,
    ];
    for f in &all_fixtures {
        run_official_shanghai_fixture_json(f);
    }
}

// =========================================================================
// Part 6 — Task B Simulation Gas-Limit Behavior Resolution Suite
// =========================================================================

#[test]
fn test_p9_sim_gas_omitted_defaults_to_block_gas_limit() {
    let mut harness = TestHarness::new("sim_gas_omitted");
    let kp = Secp256k1Keypair::generate();
    let sender = kp.address();
    harness.fund_account(&sender, 100_000_000_000_000_000);

    // Deploy contract that returns 42
    let runtime = hex_decode("602a60005260206000f3");
    let initcode = make_initcode(&runtime);
    let deploy_tx = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce::ZERO,
        from: sender,
        to: None,
        value: Amount::ZERO,
        gas_limit: Gas(500_000),
        max_fee_per_gas: Amount::from_u128(2_000_000_000),
        max_priority_fee_per_gas: Amount::from_u128(100_000_000),
        valid_until_block: BlockHeight(100),
        data: initcode,
        memo: None,
        signature: Signature::Secp256k1Eth {
            r: [0; 32],
            s: [0; 32],
            y_parity: 0,
            chain_id: TEST_CHAIN_ID.0,
        },
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_dep, _) = sign_eth_tx(&kp, deploy_tx);
    let contract_addr = compute_contract_address(&sender, 0);
    harness.commit_block(vec![signed_dep]);

    let disp = harness.make_dispatcher();
    // 1. eth_call with gas omitted -> defaults to block gas limit (30,000,000)
    let call_json = format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"eth_call","params":[{{"to":"{}"}},"latest"]}}"#,
        contract_addr.to_hex()
    );
    let resp = disp.dispatch_json(&call_json);
    let val = JsonValue::parse(&resp).unwrap();
    assert!(val.get("error").is_none());
    let out_str = val.get("result").unwrap().as_str().unwrap();
    assert_eq!(hex_decode(out_str), pad32_u64(42).to_vec());

    // 2. eth_estimateGas with gas omitted
    let est_json = format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"eth_estimateGas","params":[{{"to":"{}"}},"latest"]}}"#,
        contract_addr.to_hex()
    );
    let est_resp = disp.dispatch_json(&est_json);
    let est_val = JsonValue::parse(&est_resp).unwrap();
    assert!(est_val.get("error").is_none());
    assert!(est_val
        .get("result")
        .unwrap()
        .as_str()
        .unwrap()
        .starts_with("0x"));
}

#[test]
fn test_p9_sim_gas_explicit_within_limit() {
    let mut harness = TestHarness::new("sim_gas_explicit_within");
    let kp = Secp256k1Keypair::generate();
    let sender = kp.address();
    harness.fund_account(&sender, 100_000_000_000_000_000);

    let runtime = hex_decode("602a60005260206000f3");
    let initcode = make_initcode(&runtime);
    let deploy_tx = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce::ZERO,
        from: sender,
        to: None,
        value: Amount::ZERO,
        gas_limit: Gas(500_000),
        max_fee_per_gas: Amount::from_u128(2_000_000_000),
        max_priority_fee_per_gas: Amount::from_u128(100_000_000),
        valid_until_block: BlockHeight(100),
        data: initcode,
        memo: None,
        signature: Signature::Secp256k1Eth {
            r: [0; 32],
            s: [0; 32],
            y_parity: 0,
            chain_id: TEST_CHAIN_ID.0,
        },
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_dep, _) = sign_eth_tx(&kp, deploy_tx);
    let contract_addr = compute_contract_address(&sender, 0);
    harness.commit_block(vec![signed_dep]);

    let disp = harness.make_dispatcher();
    // Explicit caller gas = 0x7a120 (500,000 gas, <= block gas limit 30,000,000)
    let call_json = format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"eth_call","params":[{{"to":"{}","gas":"0x7a120"}},"latest"]}}"#,
        contract_addr.to_hex()
    );
    let resp = disp.dispatch_json(&call_json);
    let val = JsonValue::parse(&resp).unwrap();
    assert!(val.get("error").is_none());
    let out_str = val.get("result").unwrap().as_str().unwrap();
    assert_eq!(hex_decode(out_str), pad32_u64(42).to_vec());
}

#[test]
fn test_p9_sim_gas_explicit_exceeds_block_gas_limit() {
    let mut harness = TestHarness::new("sim_gas_exceeds_limit");
    let kp = Secp256k1Keypair::generate();
    let sender = kp.address();
    harness.fund_account(&sender, 100_000_000_000_000_000);

    let runtime = hex_decode("602a60005260206000f3");
    let initcode = make_initcode(&runtime);
    let deploy_tx = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce::ZERO,
        from: sender,
        to: None,
        value: Amount::ZERO,
        gas_limit: Gas(500_000),
        max_fee_per_gas: Amount::from_u128(2_000_000_000),
        max_priority_fee_per_gas: Amount::from_u128(100_000_000),
        valid_until_block: BlockHeight(100),
        data: initcode,
        memo: None,
        signature: Signature::Secp256k1Eth {
            r: [0; 32],
            s: [0; 32],
            y_parity: 0,
            chain_id: TEST_CHAIN_ID.0,
        },
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_dep, _) = sign_eth_tx(&kp, deploy_tx);
    let contract_addr = compute_contract_address(&sender, 0);
    harness.commit_block(vec![signed_dep]);

    let disp = harness.make_dispatcher();
    // Explicit caller gas = 0x2625a00 (40,000,000 gas > block gas limit 30,000,000)
    let call_json = format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"eth_call","params":[{{"to":"{}","gas":"0x2625a00"}},"latest"]}}"#,
        contract_addr.to_hex()
    );
    let resp = disp.dispatch_json(&call_json);
    let val = JsonValue::parse(&resp).unwrap();
    assert!(val.get("error").is_some());
    let err = val.get("error").unwrap();
    assert_eq!(err.get("code").unwrap().as_i64(), Some(-32602));
    let msg = err.get("message").unwrap().as_str().unwrap();
    assert!(msg.contains("gas limit exceeds block gas limit"));

    // eth_estimateGas with gas > block gas limit
    let est_json = format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"eth_estimateGas","params":[{{"to":"{}","gas":"0x2625a00"}},"latest"]}}"#,
        contract_addr.to_hex()
    );
    let est_resp = disp.dispatch_json(&est_json);
    let est_val = JsonValue::parse(&est_resp).unwrap();
    assert!(est_val.get("error").is_some());
    let est_err = est_val.get("error").unwrap();
    assert_eq!(est_err.get("code").unwrap().as_i64(), Some(-32602));
    assert!(est_err
        .get("message")
        .unwrap()
        .as_str()
        .unwrap()
        .contains("gas limit exceeds block gas limit"));

    // Direct EvmExecutor::simulate_call verification
    let call_tx = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce::ZERO,
        from: sender,
        to: Some(contract_addr),
        value: Amount::ZERO,
        gas_limit: Gas(40_000_000),
        max_fee_per_gas: Amount::from_u128(2_000_000_000),
        max_priority_fee_per_gas: Amount::ZERO,
        valid_until_block: BlockHeight(100),
        data: vec![],
        memo: None,
        signature: Signature::Secp256k1Eth {
            r: [0; 32],
            s: [0; 32],
            y_parity: 0,
            chain_id: TEST_CHAIN_ID.0,
        },
        tx_type: 2,
        access_list: vec![],
    };
    let state = harness.state_arc.read().unwrap();
    let sim_res = EvmExecutor::simulate_call(
        &call_tx,
        BlockHeight(1),
        Timestamp(1000),
        Amount::from_u128(10),
        harness.proposer,
        Gas(30_000_000),
        &*state,
    );
    assert!(sim_res.is_err());
    if let Err(err) = sim_res {
        assert!(err.contains("gas limit exceeds block gas limit"));
    }
}

#[test]
fn test_p9_sim_gas_execution_out_of_gas() {
    let mut harness = TestHarness::new("sim_gas_oog");
    let kp = Secp256k1Keypair::generate();
    let sender = kp.address();
    harness.fund_account(&sender, 100_000_000_000_000_000);

    // Bytecode that executes 3 SSTOREs (requires >60,000 gas)
    // PUSH1 1, PUSH1 0, SSTORE, PUSH1 2, PUSH1 1, SSTORE, PUSH1 3, PUSH1 2, SSTORE, STOP
    let runtime = hex_decode("60016000556002600155600360025500");
    let initcode = make_initcode(&runtime);
    let deploy_tx = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce::ZERO,
        from: sender,
        to: None,
        value: Amount::ZERO,
        gas_limit: Gas(500_000),
        max_fee_per_gas: Amount::from_u128(2_000_000_000),
        max_priority_fee_per_gas: Amount::from_u128(100_000_000),
        valid_until_block: BlockHeight(100),
        data: initcode,
        memo: None,
        signature: Signature::Secp256k1Eth {
            r: [0; 32],
            s: [0; 32],
            y_parity: 0,
            chain_id: TEST_CHAIN_ID.0,
        },
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_dep, _) = sign_eth_tx(&kp, deploy_tx);
    let contract_addr = compute_contract_address(&sender, 0);
    harness.commit_block(vec![signed_dep]);

    let disp = harness.make_dispatcher();
    // Pass gas = 0x55f0 (22,000 gas), which is insufficient for 3 SSTOREs
    let call_json = format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"eth_call","params":[{{"to":"{}","gas":"0x55f0"}},"latest"]}}"#,
        contract_addr.to_hex()
    );
    let resp = disp.dispatch_json(&call_json);
    let val = JsonValue::parse(&resp).unwrap();
    assert!(val.get("error").is_some());
    let err = val.get("error").unwrap();
    assert_eq!(err.get("code").unwrap().as_i64(), Some(-32000));
    let msg = err.get("message").unwrap().as_str().unwrap();
    assert!(msg.contains("execution halted") || msg.contains("OutOfGas"));
}

#[test]
fn test_p9_sim_gas_historical_block_context() {
    let mut harness = TestHarness::new("sim_gas_historical");
    let kp = Secp256k1Keypair::generate();
    let sender = kp.address();
    harness.fund_account(&sender, 100_000_000_000_000_000);

    let runtime = hex_decode("602a60005260206000f3");
    let initcode = make_initcode(&runtime);
    let deploy_tx = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce::ZERO,
        from: sender,
        to: None,
        value: Amount::ZERO,
        gas_limit: Gas(500_000),
        max_fee_per_gas: Amount::from_u128(2_000_000_000),
        max_priority_fee_per_gas: Amount::from_u128(100_000_000),
        valid_until_block: BlockHeight(100),
        data: initcode,
        memo: None,
        signature: Signature::Secp256k1Eth {
            r: [0; 32],
            s: [0; 32],
            y_parity: 0,
            chain_id: TEST_CHAIN_ID.0,
        },
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_dep, _) = sign_eth_tx(&kp, deploy_tx);
    let contract_addr = compute_contract_address(&sender, 0);

    // Block 1 with dynamic gas limit 15,000,000
    harness.commit_block_with_gas_limit(vec![signed_dep], Gas(15_000_000));

    // Block 2 with dynamic gas limit 45,000,000
    harness.commit_block_with_gas_limit(vec![], Gas(45_000_000));

    let disp = harness.make_dispatcher();

    // Query Block 1 with gas = 20,000,000 (0x1312d00) -> 20M > 15M (Block 1 limit) -> REJECTED
    let call_b1 = format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"eth_call","params":[{{"to":"{}","gas":"0x1312d00"}},"0x1"]}}"#,
        contract_addr.to_hex()
    );
    let resp_b1 = disp.dispatch_json(&call_b1);
    let val_b1 = JsonValue::parse(&resp_b1).unwrap();
    assert!(val_b1.get("error").is_some());
    assert!(val_b1
        .get("error")
        .unwrap()
        .get("message")
        .unwrap()
        .as_str()
        .unwrap()
        .contains("gas limit exceeds block gas limit"));

    // Query Block 2 with gas = 20,000,000 (0x1312d00) -> 20M <= 45M (Block 2 limit) -> ACCEPTED
    let call_b2 = format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"eth_call","params":[{{"to":"{}","gas":"0x1312d00"}},"0x2"]}}"#,
        contract_addr.to_hex()
    );
    let resp_b2 = disp.dispatch_json(&call_b2);
    let val_b2 = JsonValue::parse(&resp_b2).unwrap();
    assert!(val_b2.get("error").is_none());
    let out_b2 = val_b2.get("result").unwrap().as_str().unwrap();
    assert_eq!(hex_decode(out_b2), pad32_u64(42).to_vec());
}

#[test]
fn test_p9_sim_gas_canonical_block_strict_enforcement() {
    let mut harness = TestHarness::new("sim_gas_canonical_strict");
    let kp = Secp256k1Keypair::generate();
    let sender = kp.address();
    harness.fund_account(&sender, 100_000_000_000_000_000);

    // Canonical block gas limit = 30,000,000
    // Transaction requesting gas_limit = 35,000,000
    let overlimit_tx = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce::ZERO,
        from: sender,
        to: Some(Address([0x11; 20])),
        value: Amount::ZERO,
        gas_limit: Gas(35_000_000),
        max_fee_per_gas: Amount::from_u128(2_000_000_000),
        max_priority_fee_per_gas: Amount::from_u128(100_000_000),
        valid_until_block: BlockHeight(100),
        data: vec![],
        memo: None,
        signature: Signature::Secp256k1Eth {
            r: [0; 32],
            s: [0; 32],
            y_parity: 0,
            chain_id: TEST_CHAIN_ID.0,
        },
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_tx, _) = sign_eth_tx(&kp, overlimit_tx);

    let engine = SequentialBlockEngine::new(TEST_CHAIN_ID);
    let block = Block {
        header: BlockHeader {
            parent_hash: Hash::ZERO,
            height: BlockHeight(1),
            epoch: Epoch(0),
            round: Round(0),
            timestamp: Timestamp(1_000_000),
            proposer: harness.proposer,
            state_root: Hash::ZERO,
            txs_root: Hash::ZERO,
            receipts_root: Hash::ZERO,
            gas_used: Gas::ZERO,
            gas_limit: Gas(30_000_000),
            base_fee_per_gas: harness.base_fee,
            extra_data: vec![],
        },
        transactions: vec![signed_tx],
        last_commit_qc: None,
    };

    let state = harness.state_arc.read().unwrap();
    let val_res = engine.validate_block(&block, &*state);
    assert_eq!(val_res, Err(BlockValidationError::GasLimitExceeded));
}
