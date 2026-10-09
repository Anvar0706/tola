//! Integration tests for TOLA EVM Phase 2 (P2):
//! eth_getLogs, eth_newFilter, eth_getFilterChanges, and eth_uninstallFilter.

use std::sync::{Arc, RwLock};
use tola_core::{BlockEngine, StateDB};
use tola_crypto::Secp256k1Keypair;
use tola_execution::SequentialBlockEngine;
use tola_mempool::{Mempool, MempoolConfig};
use tola_rpc::dispatcher::{compute_transaction_id, RpcDispatcher};
use tola_rpc::json::JsonValue;
use tola_storage::PersistentStateDB;
use tola_types::*;

const TEST_CHAIN_ID: ChainId = ChainId(9999);

fn make_signed_eth_tx(
    kp: &Secp256k1Keypair,
    nonce: u64,
    to: Option<Address>,
    data: Vec<u8>,
) -> (Transaction, Hash) {
    let mut tx = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(nonce),
        from: kp.address(),
        to,
        value: Amount::ZERO,
        gas_limit: Gas(100_000),
        max_fee_per_gas: Amount::from_u128(1_000_000_000),
        max_priority_fee_per_gas: Amount::from_u128(100_000_000),
        valid_until_block: BlockHeight(100),
        data,
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
        chain_id: TEST_CHAIN_ID.0,
    };
    let eth_hash = tola_codec::rlp::compute_eth_tx_hash(&tx).unwrap();
    (tx, eth_hash)
}

fn make_native_tx(from: Address, nonce: u64, to: Address) -> Transaction {
    Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(nonce),
        from,
        to: Some(to),
        value: Amount::from_u128(10),
        gas_limit: Gas(21_000),
        max_fee_per_gas: Amount::from_u128(1_000_000_000),
        max_priority_fee_per_gas: Amount::ZERO,
        valid_until_block: BlockHeight(100),
        data: vec![],
        memo: None,
        signature: Signature::Ed25519([0x55; 64]),
        tx_type: 0,
        access_list: vec![],
    }
}

#[test]
fn test_p2_logs_query_ranges_address_and_topics() {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let temp_dir = std::env::temp_dir().join(format!("tola_p2_logs_query_{nanos}"));

    let state = PersistentStateDB::open(&temp_dir).unwrap();
    let state_arc = Arc::new(RwLock::new(state));
    let mempool_arc = Arc::new(RwLock::new(Mempool::new(MempoolConfig::new(TEST_CHAIN_ID))));
    let rpc = RpcDispatcher::new(TEST_CHAIN_ID, "TOLA/test".into(), state_arc, mempool_arc);

    let kp = Secp256k1Keypair::generate();
    let contract_a = Address([0x11; 20]);
    let contract_b = Address([0x22; 20]);

    let event_transfer = Hash([0xaa; 32]);
    let event_approval = Hash([0xbb; 32]);
    let topic_alice = Hash([0x10; 32]);
    let topic_bob = Hash([0x20; 32]);

    // Block 1: Tx 1 (Contract A, Transfer, Alice, Bob)
    let (tx1, eth_h1) = make_signed_eth_tx(&kp, 0, Some(contract_a), vec![0x01]);
    let tx1_id = compute_transaction_id(&tx1);
    let r1 = Receipt {
        tx_id: tx1_id,
        success: true,
        gas_used: Gas(21_000),
        cumulative_gas_used: Gas(21_000),
        logs: vec![Log {
            address: contract_a,
            topics: vec![event_transfer, topic_alice, topic_bob],
            data: vec![0x11, 0x22],
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
    rpc.record_block(b1, vec![r1]);

    // Block 2: Tx 2 (Contract B, Approval, Alice)
    let (tx2, _eth_h2) = make_signed_eth_tx(&kp, 1, Some(contract_b), vec![0x02]);
    let tx2_id = compute_transaction_id(&tx2);
    let r2 = Receipt {
        tx_id: tx2_id,
        success: true,
        gas_used: Gas(21_000),
        cumulative_gas_used: Gas(21_000),
        logs: vec![Log {
            address: contract_b,
            topics: vec![event_approval, topic_alice],
            data: vec![0x33],
        }],
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
            gas_used: Gas(21_000),
            gas_limit: Gas(30_000_000),
            base_fee_per_gas: Amount::from_u128(1_000_000_000),
            extra_data: vec![],
        },
        transactions: vec![tx2],
        last_commit_qc: None,
    };
    rpc.record_block(b2, vec![r2]);

    // 1. Tag queries: "earliest" to "latest"
    let req_all = r#"{"jsonrpc":"2.0","id":1,"method":"eth_getLogs","params":[{"fromBlock":"earliest","toBlock":"latest"}]}"#;
    let res_all = rpc.dispatch_json(req_all);
    let val_all = JsonValue::parse(&res_all).unwrap();
    let logs_all = val_all.get("result").unwrap().as_array().unwrap();
    assert_eq!(logs_all.len(), 2);
    assert_eq!(
        logs_all[0].get("transactionHash").unwrap().as_str(),
        Some(eth_h1.to_hex().as_str())
    );

    // 2. Address filter: Contract A
    let req_addr = format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"eth_getLogs","params":[{{
            "fromBlock": "0x1",
            "toBlock": "0x2",
            "address": "{}"
        }}]}}"#,
        contract_a.to_hex()
    );
    let res_addr = rpc.dispatch_json(&req_addr);
    let val_addr = JsonValue::parse(&res_addr).unwrap();
    let logs_addr = val_addr.get("result").unwrap().as_array().unwrap();
    assert_eq!(logs_addr.len(), 1);
    assert_eq!(
        logs_addr[0].get("address").unwrap().as_str(),
        Some(contract_a.to_hex().as_str())
    );

    // 3. Topics with wildcard: [Transfer, null, Bob]
    let req_wild = format!(
        r#"{{"jsonrpc":"2.0","id":3,"method":"eth_getLogs","params":[{{
            "fromBlock": "0x1",
            "toBlock": "0x2",
            "topics": ["{}", null, "{}"]
        }}]}}"#,
        event_transfer.to_hex(),
        topic_bob.to_hex()
    );
    let res_wild = rpc.dispatch_json(&req_wild);
    let val_wild = JsonValue::parse(&res_wild).unwrap();
    let logs_wild = val_wild.get("result").unwrap().as_array().unwrap();
    assert_eq!(logs_wild.len(), 1);

    // 4. Topics OR condition: [[Transfer, Approval]]
    let req_or = format!(
        r#"{{"jsonrpc":"2.0","id":4,"method":"eth_getLogs","params":[{{
            "fromBlock": "0x1",
            "toBlock": "0x2",
            "topics": [["{}", "{}"]]
        }}]}}"#,
        event_transfer.to_hex(),
        event_approval.to_hex()
    );
    let res_or = rpc.dispatch_json(&req_or);
    let val_or = JsonValue::parse(&res_or).unwrap();
    let logs_or = val_or.get("result").unwrap().as_array().unwrap();
    assert_eq!(logs_or.len(), 2);
}

#[test]
fn test_p2_block_hash_queries_and_strict_error_semantics() {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let temp_dir = std::env::temp_dir().join(format!("tola_p2_bh_err_{nanos}"));

    let state = PersistentStateDB::open(&temp_dir).unwrap();
    let state_arc = Arc::new(RwLock::new(state));
    let mempool_arc = Arc::new(RwLock::new(Mempool::new(MempoolConfig::new(TEST_CHAIN_ID))));
    let rpc = RpcDispatcher::new(TEST_CHAIN_ID, "TOLA/test".into(), state_arc, mempool_arc);

    let kp = Secp256k1Keypair::generate();
    let contract = Address([0x33; 20]);
    let (tx, _eth_h) = make_signed_eth_tx(&kp, 0, Some(contract), vec![]);
    let tx_id = compute_transaction_id(&tx);
    let r = Receipt {
        tx_id,
        success: true,
        gas_used: Gas(21_000),
        cumulative_gas_used: Gas(21_000),
        logs: vec![Log {
            address: contract,
            topics: vec![Hash([0x99; 32])],
            data: vec![0xaa],
        }],
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
            state_root: Hash([0x01; 32]),
            txs_root: Hash([0x01; 32]),
            receipts_root: Hash([0x01; 32]),
            gas_used: Gas(21_000),
            gas_limit: Gas(30_000_000),
            base_fee_per_gas: Amount::from_u128(1_000_000_000),
            extra_data: vec![],
        },
        transactions: vec![tx],
        last_commit_qc: None,
    };
    let block_hash = tola_consensus::PipelinedBftEngine::compute_block_hash(&block);
    rpc.record_block(block, vec![r]);

    // 1. Valid blockHash query
    let req_bh = format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"eth_getLogs","params":[{{ "blockHash": "{}" }}]}}"#,
        block_hash.to_hex()
    );
    let res_bh = rpc.dispatch_json(&req_bh);
    let val_bh = JsonValue::parse(&res_bh).unwrap();
    let logs_bh = val_bh.get("result").unwrap().as_array().unwrap();
    assert_eq!(logs_bh.len(), 1);

    // 2. Unknown blockHash -> -32000 "unknown block"
    let unknown_hash = Hash([0xee; 32]);
    let req_unk = format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"eth_getLogs","params":[{{ "blockHash": "{}" }}]}}"#,
        unknown_hash.to_hex()
    );
    let res_unk = rpc.dispatch_json(&req_unk);
    let val_unk = JsonValue::parse(&res_unk).unwrap();
    let err_unk = val_unk.get("error").unwrap();
    assert_eq!(err_unk.get("code").and_then(|v| v.as_i64()), Some(-32000));
    assert_eq!(
        err_unk.get("message").and_then(|v| v.as_str()),
        Some("unknown block")
    );

    // 3. blockHash combined with fromBlock -> -32602
    let req_conflict = format!(
        r#"{{"jsonrpc":"2.0","id":3,"method":"eth_getLogs","params":[{{ "blockHash": "{}", "fromBlock": "0x1" }}]}}"#,
        block_hash.to_hex()
    );
    let res_conflict = rpc.dispatch_json(&req_conflict);
    let val_conflict = JsonValue::parse(&res_conflict).unwrap();
    assert_eq!(
        val_conflict
            .get("error")
            .unwrap()
            .get("code")
            .and_then(|v| v.as_i64()),
        Some(-32602)
    );

    // 4. MAX_LOG_BLOCK_RANGE (2000 blocks limit) -> -32005
    let req_range = r#"{"jsonrpc":"2.0","id":4,"method":"eth_getLogs","params":[{"fromBlock":"0x0","toBlock":"0x800"}]}"#; // 2049 blocks
    let res_range = rpc.dispatch_json(req_range);
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
fn test_p2_cumulative_log_index_and_canonical_eth_tx_hash() {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let temp_dir = std::env::temp_dir().join(format!("tola_p2_cumul_idx_{nanos}"));

    let state = PersistentStateDB::open(&temp_dir).unwrap();
    let state_arc = Arc::new(RwLock::new(state));
    let mempool_arc = Arc::new(RwLock::new(Mempool::new(MempoolConfig::new(TEST_CHAIN_ID))));
    let rpc = RpcDispatcher::new(TEST_CHAIN_ID, "TOLA/test".into(), state_arc, mempool_arc);

    let kp = Secp256k1Keypair::generate();
    let contract = Address([0x55; 20]);
    let event_id = Hash([0x66; 32]);

    // Tx 1 (EVM): Emits 2 logs -> logIndex 0, 1
    let (tx1, eth_h1) = make_signed_eth_tx(&kp, 0, Some(contract), vec![]);
    let tx1_id = compute_transaction_id(&tx1);
    let r1 = Receipt {
        tx_id: tx1_id,
        success: true,
        gas_used: Gas(21_000),
        cumulative_gas_used: Gas(21_000),
        logs: vec![
            Log {
                address: contract,
                topics: vec![event_id],
                data: vec![0x01],
            },
            Log {
                address: contract,
                topics: vec![event_id],
                data: vec![0x02],
            },
        ],
        return_data: vec![],
    };

    // Tx 2 (Native): Emits 1 log -> logIndex 2, transactionHash = tx_id.0.to_hex()
    let tx2 = make_native_tx(Address([0x01; 20]), 0, contract);
    let tx2_id = compute_transaction_id(&tx2);
    let r2 = Receipt {
        tx_id: tx2_id,
        success: true,
        gas_used: Gas(21_000),
        cumulative_gas_used: Gas(42_000),
        logs: vec![Log {
            address: contract,
            topics: vec![event_id],
            data: vec![0x03],
        }],
        return_data: vec![],
    };

    // Tx 3 (EVM): Emits 1 log -> logIndex 3
    let (tx3, eth_h3) = make_signed_eth_tx(&kp, 1, Some(contract), vec![]);
    let tx3_id = compute_transaction_id(&tx3);
    let r3 = Receipt {
        tx_id: tx3_id,
        success: true,
        gas_used: Gas(21_000),
        cumulative_gas_used: Gas(63_000),
        logs: vec![Log {
            address: contract,
            topics: vec![event_id],
            data: vec![0x04],
        }],
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
            state_root: Hash([0x11; 32]),
            txs_root: Hash([0x11; 32]),
            receipts_root: Hash([0x11; 32]),
            gas_used: Gas(63_000),
            gas_limit: Gas(30_000_000),
            base_fee_per_gas: Amount::from_u128(1_000_000_000),
            extra_data: vec![],
        },
        transactions: vec![tx1, tx2, tx3],
        last_commit_qc: None,
    };
    rpc.record_block(block, vec![r1, r2, r3]);

    let req_logs = r#"{"jsonrpc":"2.0","id":10,"method":"eth_getLogs","params":[{"fromBlock":"0x1","toBlock":"0x1"}]}"#;
    let res_logs = rpc.dispatch_json(req_logs);
    let val_logs = JsonValue::parse(&res_logs).unwrap();
    let logs = val_logs.get("result").unwrap().as_array().unwrap();
    assert_eq!(logs.len(), 4);

    // Verify Log 0
    assert_eq!(logs[0].get("logIndex").unwrap().as_str(), Some("0x0"));
    assert_eq!(
        logs[0].get("transactionIndex").unwrap().as_str(),
        Some("0x0")
    );
    assert_eq!(
        logs[0].get("transactionHash").unwrap().as_str(),
        Some(eth_h1.to_hex().as_str())
    );

    // Verify Log 1
    assert_eq!(logs[1].get("logIndex").unwrap().as_str(), Some("0x1"));
    assert_eq!(
        logs[1].get("transactionIndex").unwrap().as_str(),
        Some("0x0")
    );
    assert_eq!(
        logs[1].get("transactionHash").unwrap().as_str(),
        Some(eth_h1.to_hex().as_str())
    );

    // Verify Log 2 (Native Transaction: canonical hash is tx_id.0.to_hex())
    assert_eq!(logs[2].get("logIndex").unwrap().as_str(), Some("0x2"));
    assert_eq!(
        logs[2].get("transactionIndex").unwrap().as_str(),
        Some("0x1")
    );
    assert_eq!(
        logs[2].get("transactionHash").unwrap().as_str(),
        Some(tx2_id.0.to_hex().as_str())
    );

    // Verify Log 3 (Tx 3, cumulative logIndex = 3)
    assert_eq!(logs[3].get("logIndex").unwrap().as_str(), Some("0x3"));
    assert_eq!(
        logs[3].get("transactionIndex").unwrap().as_str(),
        Some("0x2")
    );
    assert_eq!(
        logs[3].get("transactionHash").unwrap().as_str(),
        Some(eth_h3.to_hex().as_str())
    );

    // Verify parity with eth_getTransactionReceipt for Tx 3
    let req_rec3 = format!(
        r#"{{"jsonrpc":"2.0","id":11,"method":"eth_getTransactionReceipt","params":["{}"]}}"#,
        eth_h3.to_hex()
    );
    let res_rec3 = rpc.dispatch_json(&req_rec3);
    let val_rec3 = JsonValue::parse(&res_rec3).unwrap();
    let rec3_logs = val_rec3
        .get("result")
        .unwrap()
        .get("logs")
        .unwrap()
        .as_array()
        .unwrap();
    assert_eq!(rec3_logs.len(), 1);
    assert_eq!(rec3_logs[0].get("logIndex").unwrap().as_str(), Some("0x3"));
}

#[test]
fn test_p2_polling_filter_lifecycle() {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let temp_dir = std::env::temp_dir().join(format!("tola_p2_filter_life_{nanos}"));

    let state = PersistentStateDB::open(&temp_dir).unwrap();
    let state_arc = Arc::new(RwLock::new(state));
    let mempool_arc = Arc::new(RwLock::new(Mempool::new(MempoolConfig::new(TEST_CHAIN_ID))));
    let rpc = RpcDispatcher::new(TEST_CHAIN_ID, "TOLA/test".into(), state_arc, mempool_arc);

    let kp = Secp256k1Keypair::generate();
    let contract = Address([0x77; 20]);
    let event_id = Hash([0x88; 32]);

    // 1. Create filter with omitted fromBlock -> defaults to current height (0)
    let req_new = r#"{"jsonrpc":"2.0","id":1,"method":"eth_newFilter","params":[{}]}"#;
    let res_new = rpc.dispatch_json(req_new);
    let val_new = JsonValue::parse(&res_new).unwrap();
    let fid = val_new.get("result").unwrap().as_str().unwrap();
    assert_eq!(fid, "0x1");

    // Poll changes on empty chain -> empty
    let req_ch = format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"eth_getFilterChanges","params":["{}"]}}"#,
        fid
    );
    let res_ch = rpc.dispatch_json(&req_ch);
    let val_ch = JsonValue::parse(&res_ch).unwrap();
    assert_eq!(val_ch.get("result").unwrap().as_array().unwrap().len(), 0);

    // 2. Mine Block 1 with a log
    let (tx1, _eth_h1) = make_signed_eth_tx(&kp, 0, Some(contract), vec![]);
    let tx1_id = compute_transaction_id(&tx1);
    let r1 = Receipt {
        tx_id: tx1_id,
        success: true,
        gas_used: Gas(21_000),
        cumulative_gas_used: Gas(21_000),
        logs: vec![Log {
            address: contract,
            topics: vec![event_id],
            data: vec![0x11],
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
    rpc.record_block(b1, vec![r1]);

    // Poll changes -> returns Block 1 log
    let res_ch2 = rpc.dispatch_json(&req_ch);
    let val_ch2 = JsonValue::parse(&res_ch2).unwrap();
    let logs2 = val_ch2.get("result").unwrap().as_array().unwrap();
    assert_eq!(logs2.len(), 1);
    assert_eq!(logs2[0].get("blockNumber").unwrap().as_str(), Some("0x1"));

    // Poll again -> empty
    let res_ch3 = rpc.dispatch_json(&req_ch);
    let val_ch3 = JsonValue::parse(&res_ch3).unwrap();
    assert_eq!(val_ch3.get("result").unwrap().as_array().unwrap().len(), 0);

    // eth_getFilterLogs returns the full history without consuming cursor
    let req_fl = format!(
        r#"{{"jsonrpc":"2.0","id":3,"method":"eth_getFilterLogs","params":["{}"]}}"#,
        fid
    );
    let res_fl = rpc.dispatch_json(&req_fl);
    let val_fl = JsonValue::parse(&res_fl).unwrap();
    assert_eq!(val_fl.get("result").unwrap().as_array().unwrap().len(), 1);

    // 3. Uninstall filter
    let req_un = format!(
        r#"{{"jsonrpc":"2.0","id":4,"method":"eth_uninstallFilter","params":["{}"]}}"#,
        fid
    );
    let res_un = rpc.dispatch_json(&req_un);
    let val_un = JsonValue::parse(&res_un).unwrap();
    assert_eq!(val_un.get("result").unwrap().as_bool(), Some(true));

    // Poll on removed filter -> -32000 "filter not found"
    let res_ch_dead = rpc.dispatch_json(&req_ch);
    let val_ch_dead = JsonValue::parse(&res_ch_dead).unwrap();
    assert_eq!(
        val_ch_dead
            .get("error")
            .unwrap()
            .get("code")
            .and_then(|v| v.as_i64()),
        Some(-32000)
    );

    // Uninstall again -> false
    let res_un2 = rpc.dispatch_json(&req_un);
    let val_un2 = JsonValue::parse(&res_un2).unwrap();
    assert_eq!(val_un2.get("result").unwrap().as_bool(), Some(false));
}

#[test]
fn test_p2_persistent_logs_query_across_node_restart() {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let temp_dir = std::env::temp_dir().join(format!("tola_p2_persist_restart_{nanos}"));

    let kp = Secp256k1Keypair::generate();
    let sender = kp.address();
    let contract = Address([0x88; 20]);
    let event_mint = Hash([0xee; 32]);
    let (tx, eth_h) = make_signed_eth_tx(&kp, 0, Some(contract), vec![0x12, 0x34]);

    // ------------------------------------------------------------------------
    // Node 1: Execute, commit bundle to disk, shut down
    // ------------------------------------------------------------------------
    {
        let mut state = PersistentStateDB::open(&temp_dir).unwrap();
        state
            .apply_delta(tola_core::StateDelta {
                accounts: vec![(
                    sender,
                    Some(AccountState::new_with_balance(Amount::from_u128(
                        1_000_000_000_000_000_000,
                    ))),
                )],
                storage: vec![],
                code: vec![],
            })
            .unwrap();

        let engine = SequentialBlockEngine::new(TEST_CHAIN_ID);
        let mut block = Block {
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
                gas_used: Gas(21_000),
                gas_limit: Gas(30_000_000),
                base_fee_per_gas: Amount::from_u128(1_000_000_000),
                extra_data: vec![],
            },
            transactions: vec![tx.clone()],
            last_commit_qc: None,
        };

        let mut receipts = engine.execute_block(&block, &mut state).unwrap();
        assert_eq!(receipts.len(), 1);

        // Inject log into receipt
        receipts[0].logs.push(Log {
            address: contract,
            topics: vec![event_mint],
            data: vec![0xaa, 0xbb],
        });

        let state_root = state.compute_state_root().unwrap();
        block.header.state_root = state_root;
        let block_hash = tola_consensus::PipelinedBftEngine::compute_block_hash(&block);

        state
            .commit_block_bundle(&block, block_hash, &receipts)
            .expect("Commit bundle to disk");
    } // Node 1 drops state and caches

    // ------------------------------------------------------------------------
    // Node 2: Reopen persistent state from disk with a fresh RpcDispatcher
    // ------------------------------------------------------------------------
    {
        let state_reopened = PersistentStateDB::open(&temp_dir).unwrap();
        let state_arc = Arc::new(RwLock::new(state_reopened));
        let mempool_arc = Arc::new(RwLock::new(Mempool::new(MempoolConfig::new(TEST_CHAIN_ID))));

        // Brand new RPC dispatcher: blocks_by_hash and receipts in-memory caches are completely EMPTY!
        let rpc_restarted =
            RpcDispatcher::new(TEST_CHAIN_ID, "TOLA/test".into(), state_arc, mempool_arc);

        // Query eth_getLogs across restart
        let req_logs = r#"{"jsonrpc":"2.0","id":100,"method":"eth_getLogs","params":[{"fromBlock":"0x1","toBlock":"0x1"}]}"#;
        let res_logs = rpc_restarted.dispatch_json(req_logs);
        let val_logs = JsonValue::parse(&res_logs).unwrap();
        let logs = val_logs.get("result").unwrap().as_array().unwrap();
        assert_eq!(logs.len(), 1);

        let log = &logs[0];
        assert_eq!(
            log.get("address").unwrap().as_str(),
            Some(contract.to_hex().as_str())
        );
        assert_eq!(
            log.get("transactionHash").unwrap().as_str(),
            Some(eth_h.to_hex().as_str())
        );
        assert_eq!(log.get("blockNumber").unwrap().as_str(), Some("0x1"));
        assert_eq!(log.get("transactionIndex").unwrap().as_str(), Some("0x0"));
        assert_eq!(log.get("logIndex").unwrap().as_str(), Some("0x0"));
        assert_eq!(log.get("data").unwrap().as_str(), Some("0xaabb"));
        assert_eq!(log.get("removed").unwrap().as_bool(), Some(false));
    }
}
