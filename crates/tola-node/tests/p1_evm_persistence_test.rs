//! Integration tests for P1 EVM Receipt Persistence, Restart Fallbacks,
//! and Atomic Block Commitments.

use std::sync::{Arc, RwLock};
use tola_core::{BlockEngine, StateDB};
use tola_crypto::Secp256k1Keypair;
use tola_execution::SequentialBlockEngine;
use tola_mempool::{Mempool, MempoolConfig};
use tola_node::compute_tx_digest;
use tola_rpc::dispatcher::RpcDispatcher;
use tola_rpc::json::JsonValue;
use tola_storage::PersistentStateDB;
use tola_types::*;

fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        use std::fmt::Write;
        write!(&mut s, "{:02x}", b).unwrap();
    }
    s
}

const TEST_CHAIN_ID: ChainId = ChainId(9999);

fn setup_keypair_and_eth_tx(
    nonce: u64,
    to: Option<Address>,
    value: u128,
    data: Vec<u8>,
    max_fee: u128,
    max_priority: u128,
) -> (Secp256k1Keypair, Transaction, Hash) {
    let kp = Secp256k1Keypair::generate();
    let from = kp.address();
    let mut tx = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(nonce),
        from,
        to,
        value: Amount::from_u128(value),
        gas_limit: Gas(100_000),
        max_fee_per_gas: Amount::from_u128(max_fee),
        max_priority_fee_per_gas: Amount::from_u128(max_priority),
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
    let y_parity = sig[64];
    tx.signature = Signature::Secp256k1Eth {
        r,
        s,
        y_parity,
        chain_id: TEST_CHAIN_ID.0,
    };
    let eth_hash = tola_codec::rlp::compute_eth_tx_hash(&tx).unwrap();
    (kp, tx, eth_hash)
}

#[test]
fn test_p1_receipt_and_tx_persistence_across_restart() {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let temp_dir = std::env::temp_dir().join(format!("tola_p1_rec_test_{nanos}"));

    let (_kp, tx, eth_hash) = setup_keypair_and_eth_tx(
        0,
        Some(Address([0x88; 20])),
        1000,
        vec![],
        10_000_000_000,
        2_000_000_000,
    );

    let sender = tx.from;
    let base_fee = Amount::from_u128(1_000_000_000); // 1 Gwei

    // ------------------------------------------------------------------------
    // Node 1: Execute, commit bundle, verify pre-restart
    // ------------------------------------------------------------------------
    {
        let mut state = PersistentStateDB::open(&temp_dir).expect("Open persistent state");
        // Fund sender with 1 TOLA
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
                base_fee_per_gas: base_fee,
                extra_data: vec![],
            },
            transactions: vec![tx.clone()],
            last_commit_qc: None,
        };

        let receipts = engine
            .execute_block(&block, &mut state)
            .expect("Block execution succeeds");
        assert_eq!(receipts.len(), 1);
        assert!(receipts[0].success);

        let state_root = state.compute_state_root().unwrap();
        block.header.state_root = state_root;
        let block_hash = tola_consensus::PipelinedBftEngine::compute_block_hash(&block);

        // Atomic commit bundle to disk
        state
            .commit_block_bundle(&block, block_hash, &receipts)
            .expect("Bundle commit succeeds");

        // Verify pre-restart through RpcDispatcher
        let state_arc = Arc::new(RwLock::new(state));
        let mempool_arc = Arc::new(RwLock::new(Mempool::new(MempoolConfig::new(TEST_CHAIN_ID))));
        let rpc = RpcDispatcher::new(TEST_CHAIN_ID, "TOLA/test".into(), state_arc, mempool_arc);
        rpc.record_block(block.clone(), receipts.clone());

        let req_rec = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"eth_getTransactionReceipt","params":["{}"]}}"#,
            eth_hash.to_hex()
        );
        let res_rec = rpc.dispatch_json(&req_rec);
        let val_rec = JsonValue::parse(&res_rec).unwrap();
        assert_eq!(
            val_rec
                .get("result")
                .unwrap()
                .get("status")
                .unwrap()
                .as_str(),
            Some("0x1")
        );
    } // Node 1 shuts down here, state and in-memory caches dropped

    // ------------------------------------------------------------------------
    // Node 2: Simulated Restart: Fresh PersistentStateDB and fresh RpcDispatcher
    // ------------------------------------------------------------------------
    {
        let state_reopened =
            PersistentStateDB::open(&temp_dir).expect("Reopen persistent state across restart");
        let state_arc = Arc::new(RwLock::new(state_reopened));
        let mempool_arc = Arc::new(RwLock::new(Mempool::new(MempoolConfig::new(TEST_CHAIN_ID))));

        // Notice: rpc is brand new, in-memory receipts and blocks_by_hash are completely EMPTY!
        let rpc_restarted =
            RpcDispatcher::new(TEST_CHAIN_ID, "TOLA/test".into(), state_arc, mempool_arc);

        // 1. eth_getTransactionReceipt across restart
        let req_rec = format!(
            r#"{{"jsonrpc":"2.0","id":10,"method":"eth_getTransactionReceipt","params":["{}"]}}"#,
            eth_hash.to_hex()
        );
        let res_rec = rpc_restarted.dispatch_json(&req_rec);
        let val_rec = JsonValue::parse(&res_rec).unwrap();
        let rec_obj = val_rec
            .get("result")
            .expect("Must return receipt result after restart");
        assert_ne!(
            rec_obj,
            &JsonValue::Null,
            "Receipt must NOT be null across restart!"
        );

        assert_eq!(rec_obj.get("status").unwrap().as_str(), Some("0x1"));
        assert_eq!(
            rec_obj.get("transactionHash").unwrap().as_str(),
            Some(eth_hash.to_hex().as_str())
        );
        assert_eq!(rec_obj.get("blockNumber").unwrap().as_str(), Some("0x1"));
        assert_eq!(rec_obj.get("type").unwrap().as_str(), Some("0x2"));
        // Effective gas price = 1 Gwei base + min(2 Gwei tip, 10 - 1 Gwei) = 3 Gwei = 0xb2d05e00
        assert_eq!(
            rec_obj.get("effectiveGasPrice").unwrap().as_str(),
            Some("0xb2d05e00")
        );

        // 2. eth_getTransactionByHash across restart
        let req_tx = format!(
            r#"{{"jsonrpc":"2.0","id":11,"method":"eth_getTransactionByHash","params":["{}"]}}"#,
            eth_hash.to_hex()
        );
        let res_tx = rpc_restarted.dispatch_json(&req_tx);
        let val_tx = JsonValue::parse(&res_tx).unwrap();
        let tx_obj = val_tx
            .get("result")
            .expect("Must return tx result after restart");
        assert_ne!(
            tx_obj,
            &JsonValue::Null,
            "Transaction must NOT be null across restart!"
        );
        assert_eq!(
            tx_obj.get("hash").unwrap().as_str(),
            Some(eth_hash.to_hex().as_str())
        );
        assert_eq!(
            tx_obj.get("from").unwrap().as_str(),
            Some(sender.to_hex().as_str())
        );
        assert_eq!(tx_obj.get("blockNumber").unwrap().as_str(), Some("0x1"));
    }
}

#[test]
fn test_p1_reverted_tx_receipt_persistence() {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let temp_dir = std::env::temp_dir().join(format!("tola_p1_rev_test_{nanos}"));

    // Bytecode that reverts immediately: PUSH1 0x00, PUSH1 0x00, REVERT (0x60006000fd)
    let revert_contract_bytes = vec![0x60, 0x00, 0x60, 0x00, 0xfd];
    let revert_contract_hash = tola_crypto::keccak256_hash(&revert_contract_bytes);
    let revert_contract_addr = Address([0x77; 20]);

    let (_kp, tx, eth_hash) = setup_keypair_and_eth_tx(
        0,
        Some(revert_contract_addr),
        0,
        vec![0x12, 0x34], // call data
        10_000_000_000,
        1_000_000_000,
    );

    let sender = tx.from;
    let base_fee = Amount::from_u128(1_000_000_000);

    // Node 1: Execute, commit
    {
        let mut state = PersistentStateDB::open(&temp_dir).expect("Open persistent state");
        let mut acct = AccountState::new_with_balance(Amount::from_u128(1_000_000_000_000_000_000));
        acct.code_hash = revert_contract_hash;

        state
            .apply_delta(tola_core::StateDelta {
                accounts: vec![
                    (
                        sender,
                        Some(AccountState::new_with_balance(Amount::from_u128(
                            1_000_000_000_000_000_000,
                        ))),
                    ),
                    (revert_contract_addr, Some(acct)),
                ],
                storage: vec![],
                code: vec![(revert_contract_hash, revert_contract_bytes)],
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
                base_fee_per_gas: base_fee,
                extra_data: vec![],
            },
            transactions: vec![tx.clone()],
            last_commit_qc: None,
        };

        let receipts = engine
            .execute_block(&block, &mut state)
            .expect("Execution succeeds");
        assert_eq!(receipts.len(), 1);
        assert!(
            !receipts[0].success,
            "Reverting transaction must have success = false"
        );

        let state_root = state.compute_state_root().unwrap();
        block.header.state_root = state_root;
        let block_hash = tola_consensus::PipelinedBftEngine::compute_block_hash(&block);

        state
            .commit_block_bundle(&block, block_hash, &receipts)
            .unwrap();
    }

    // Node 2: Restart
    {
        let state_reopened = PersistentStateDB::open(&temp_dir).unwrap();
        let state_arc = Arc::new(RwLock::new(state_reopened));
        let mempool_arc = Arc::new(RwLock::new(Mempool::new(MempoolConfig::new(TEST_CHAIN_ID))));
        let rpc = RpcDispatcher::new(TEST_CHAIN_ID, "TOLA/test".into(), state_arc, mempool_arc);

        let req_rec = format!(
            r#"{{"jsonrpc":"2.0","id":20,"method":"eth_getTransactionReceipt","params":["{}"]}}"#,
            eth_hash.to_hex()
        );
        let res_rec = rpc.dispatch_json(&req_rec);
        let val_rec = JsonValue::parse(&res_rec).unwrap();
        let rec_obj = val_rec.get("result").unwrap();

        assert_eq!(
            rec_obj.get("status").unwrap().as_str(),
            Some("0x0"),
            "Reverted tx status must be 0x0 across restart"
        );
        assert_eq!(rec_obj.get("contractAddress").unwrap(), &JsonValue::Null);
    }
}

#[test]
fn test_p1_logs_and_bloom_persistence() {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let temp_dir = std::env::temp_dir().join(format!("tola_p1_logs_test_{nanos}"));

    let (_kp, tx, eth_hash) = setup_keypair_and_eth_tx(
        0,
        Some(Address([0x88; 20])),
        0,
        vec![],
        10_000_000_000,
        1_000_000_000,
    );

    let sender = tx.from;
    let base_fee = Amount::from_u128(1_000_000_000);
    let log_addr = Address([0x44; 20]);
    let topic1 = Hash([0xaa; 32]);
    let topic2 = Hash([0xbb; 32]);
    let log_data = vec![0x11, 0x22, 0x33, 0x44];

    // Node 1: Commit with custom event log
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

        let tx_id = compute_tx_digest(&tx);
        let receipt = Receipt {
            tx_id: TransactionId(tx_id),
            success: true,
            gas_used: Gas(35_000),
            cumulative_gas_used: Gas(35_000),
            logs: vec![Log {
                address: log_addr,
                topics: vec![topic1, topic2],
                data: log_data.clone(),
            }],
            return_data: vec![],
        };

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
                gas_used: Gas(35_000),
                gas_limit: Gas(30_000_000),
                base_fee_per_gas: base_fee,
                extra_data: vec![],
            },
            transactions: vec![tx.clone()],
            last_commit_qc: None,
        };

        let state_root = state.compute_state_root().unwrap();
        block.header.state_root = state_root;
        let block_hash = tola_consensus::PipelinedBftEngine::compute_block_hash(&block);

        state
            .commit_block_bundle(&block, block_hash, &[receipt])
            .unwrap();
    }

    // Node 2: Restart and verify logs and logsBloom
    {
        let state_reopened = PersistentStateDB::open(&temp_dir).unwrap();
        let state_arc = Arc::new(RwLock::new(state_reopened));
        let mempool_arc = Arc::new(RwLock::new(Mempool::new(MempoolConfig::new(TEST_CHAIN_ID))));
        let rpc = RpcDispatcher::new(TEST_CHAIN_ID, "TOLA/test".into(), state_arc, mempool_arc);

        let req_rec = format!(
            r#"{{"jsonrpc":"2.0","id":30,"method":"eth_getTransactionReceipt","params":["{}"]}}"#,
            eth_hash.to_hex()
        );
        let res_rec = rpc.dispatch_json(&req_rec);
        let val_rec = JsonValue::parse(&res_rec).unwrap();
        let rec_obj = val_rec.get("result").unwrap();

        let logs = rec_obj.get("logs").unwrap().as_array().unwrap();
        assert_eq!(logs.len(), 1, "Logs array must have 1 log");

        let log_obj = &logs[0];
        assert_eq!(
            log_obj.get("address").unwrap().as_str(),
            Some(log_addr.to_hex().as_str())
        );
        assert_eq!(
            log_obj.get("data").unwrap().as_str(),
            Some(format!("0x{}", hex_encode(&log_data)).as_str())
        );
        let topics = log_obj.get("topics").unwrap().as_array().unwrap();
        assert_eq!(topics.len(), 2);
        assert_eq!(topics[0].as_str(), Some(topic1.to_hex().as_str()));
        assert_eq!(topics[1].as_str(), Some(topic2.to_hex().as_str()));

        // Verify logsBloom is populated and non-empty
        let bloom_str = rec_obj.get("logsBloom").unwrap().as_str().unwrap();
        assert!(bloom_str.starts_with("0x"));
        assert_eq!(bloom_str.len(), 2 + 512); // 256 bytes = 512 hex chars + 0x
        assert_ne!(
            bloom_str,
            &format!("0x{}", "0".repeat(512)),
            "logsBloom must be non-zero for active topics"
        );
    }
}

#[test]
fn test_p1_atomic_commit_and_no_orphan_records() {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let temp_dir = std::env::temp_dir().join(format!("tola_p1_atomic_test_{nanos}"));

    let (_kp, tx, eth_hash) = setup_keypair_and_eth_tx(
        0,
        Some(Address([0x99; 20])),
        500,
        vec![],
        10_000_000_000,
        2_000_000_000,
    );

    let tx_id = TransactionId(compute_tx_digest(&tx));
    let mut state = PersistentStateDB::open(&temp_dir).unwrap();

    // Prior to commit: No receipt, no eth tx lookup, no location
    assert!(state.get_receipt(&tx_id).unwrap().is_none());
    assert!(state.get_eth_tx(&eth_hash).unwrap().is_none());
    assert!(state.get_tx_location(&tx_id).unwrap().is_none());

    let block = Block {
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

    let receipt = Receipt {
        tx_id,
        success: true,
        gas_used: Gas(21_000),
        cumulative_gas_used: Gas(21_000),
        logs: vec![],
        return_data: vec![],
    };

    let block_hash = tola_consensus::PipelinedBftEngine::compute_block_hash(&block);

    // Commit bundle atomically
    state
        .commit_block_bundle(&block, block_hash, std::slice::from_ref(&receipt))
        .expect("Atomic bundle commit succeeds");

    // All entities exist simultaneously
    assert_eq!(state.get_receipt(&tx_id).unwrap(), Some(receipt));
    assert_eq!(state.get_eth_tx(&eth_hash).unwrap(), Some(tx_id));
    assert_eq!(
        state.get_tx_location(&tx_id).unwrap(),
        Some((block_hash, 0))
    );
    assert!(state.get_block_by_hash(&block_hash).unwrap().is_some());
    assert!(state.get_block_by_height(BlockHeight(1)).unwrap().is_some());
}
