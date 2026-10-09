//! Integration tests for TOLA EVM Phase 3 (P3):
//! eth_accounts, eth_syncing, eth_protocolVersion,
//! eth_getBlockTransactionCountByNumber, eth_getBlockTransactionCountByHash,
//! eth_getTransactionByBlockNumberAndIndex, eth_getTransactionByBlockHashAndIndex,
//! and uncle stub methods.

use std::sync::{Arc, RwLock};
use tola_core::{BlockEngine, StateDB};
use tola_crypto::Secp256k1Keypair;
use tola_execution::SequentialBlockEngine;
use tola_mempool::{Mempool, MempoolConfig};
use tola_rpc::dispatcher::RpcDispatcher;
use tola_rpc::json::JsonValue;
use tola_storage::PersistentStateDB;
use tola_types::*;

const TEST_CHAIN_ID: ChainId = ChainId(9999);

fn make_signed_eth_tx(
    kp: &Secp256k1Keypair,
    nonce: u64,
    to: Option<Address>,
    value: u128,
    data: Vec<u8>,
) -> (Transaction, Hash) {
    let mut tx = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(nonce),
        from: kp.address(),
        to,
        value: Amount::from_u128(value),
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

#[test]
fn test_p3_rpc_compatibility_end_to_end_and_restart() {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let temp_dir = std::env::temp_dir().join(format!("tola_test_p3_{nanos}"));

    let kp1 = Secp256k1Keypair::generate();
    let kp2 = Secp256k1Keypair::generate();
    let addr1 = kp1.address();
    let addr2 = kp2.address();

    let (tx1, eth_hash1) = make_signed_eth_tx(&kp1, 0, Some(addr2), 500, vec![]);
    let (tx2, eth_hash2) = make_signed_eth_tx(&kp2, 0, Some(addr1), 1000, vec![]);

    let block_hash = {
        let mut db = PersistentStateDB::open(&temp_dir).expect("Open db");
        db.apply_delta(tola_core::StateDelta {
            accounts: vec![
                (
                    addr1,
                    Some(AccountState::new_with_balance(Amount::from_u128(
                        10_000_000_000_000_000,
                    ))),
                ),
                (
                    addr2,
                    Some(AccountState::new_with_balance(Amount::from_u128(
                        10_000_000_000_000_000,
                    ))),
                ),
            ],
            storage: vec![],
            code: vec![],
        })
        .unwrap();
        db.commit(BlockHeight(0)).unwrap();

        let engine = SequentialBlockEngine::new(TEST_CHAIN_ID);
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
                gas_used: Gas(42_000),
                gas_limit: Gas(30_000_000),
                base_fee_per_gas: Amount::from_u128(1_000_000_000),
                extra_data: vec![],
            },
            transactions: vec![tx1.clone(), tx2.clone()],
            last_commit_qc: None,
        };

        let receipts = engine
            .execute_block(&block, &mut db)
            .expect("Execute block");
        let hash = tola_consensus::PipelinedBftEngine::compute_block_hash(&block);
        db.commit_block_bundle(&block, hash, &receipts)
            .expect("Commit bundle");
        hash
    };

    // Verify RPC against freshly reopened database (tests disk restart safety)
    {
        let db = PersistentStateDB::open(&temp_dir).expect("Reopen db");
        let state_arc = Arc::new(RwLock::new(db));
        let mempool_arc = Arc::new(RwLock::new(Mempool::new(MempoolConfig::new(TEST_CHAIN_ID))));
        let disp = RpcDispatcher::new(
            TEST_CHAIN_ID,
            "TOLA/v0.1.0-alpha".into(),
            state_arc,
            mempool_arc,
        );
        disp.current_height
            .store(1, std::sync::atomic::Ordering::SeqCst);

        // 1. eth_accounts
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

        // 2. eth_syncing
        let res_syncing =
            disp.dispatch_json(r#"{"jsonrpc":"2.0","id":2,"method":"eth_syncing","params":[]}"#);
        let val_syncing = JsonValue::parse(&res_syncing).unwrap();
        assert_eq!(val_syncing.get("result").unwrap().as_bool(), Some(false));

        // 3. eth_protocolVersion
        let res_proto = disp.dispatch_json(
            r#"{"jsonrpc":"2.0","id":3,"method":"eth_protocolVersion","params":[]}"#,
        );
        let val_proto = JsonValue::parse(&res_proto).unwrap();
        assert_eq!(val_proto.get("result").unwrap().as_str(), Some("0x41"));

        // 4. eth_getBlockTransactionCountByNumber
        let res_count_1 = disp.dispatch_json(r#"{"jsonrpc":"2.0","id":4,"method":"eth_getBlockTransactionCountByNumber","params":["0x1"]}"#);
        let val_count_1 = JsonValue::parse(&res_count_1).unwrap();
        assert_eq!(val_count_1.get("result").unwrap().as_str(), Some("0x2"));

        let res_count_latest = disp.dispatch_json(r#"{"jsonrpc":"2.0","id":5,"method":"eth_getBlockTransactionCountByNumber","params":["latest"]}"#);
        let val_count_latest = JsonValue::parse(&res_count_latest).unwrap();
        assert_eq!(
            val_count_latest.get("result").unwrap().as_str(),
            Some("0x2")
        );

        let res_count_none = disp.dispatch_json(r#"{"jsonrpc":"2.0","id":6,"method":"eth_getBlockTransactionCountByNumber","params":["0x999"]}"#);
        let val_count_none = JsonValue::parse(&res_count_none).unwrap();
        assert!(matches!(
            val_count_none.get("result"),
            Some(JsonValue::Null)
        ));

        // 5. eth_getBlockTransactionCountByHash
        let req_count_hash = format!(
            r#"{{"jsonrpc":"2.0","id":7,"method":"eth_getBlockTransactionCountByHash","params":["{}"]}}"#,
            block_hash.to_hex()
        );
        let res_count_hash = disp.dispatch_json(&req_count_hash);
        let val_count_hash = JsonValue::parse(&res_count_hash).unwrap();
        assert_eq!(val_count_hash.get("result").unwrap().as_str(), Some("0x2"));

        // 6. eth_getTransactionByBlockNumberAndIndex
        let res_tx0 = disp.dispatch_json(r#"{"jsonrpc":"2.0","id":8,"method":"eth_getTransactionByBlockNumberAndIndex","params":["0x1","0x0"]}"#);
        let val_tx0 = JsonValue::parse(&res_tx0).unwrap();
        assert_eq!(
            val_tx0.get("result").unwrap().get("hash").unwrap().as_str(),
            Some(eth_hash1.to_hex().as_str())
        );
        assert_eq!(
            val_tx0
                .get("result")
                .unwrap()
                .get("transactionIndex")
                .unwrap()
                .as_str(),
            Some("0x0")
        );

        let res_tx1 = disp.dispatch_json(r#"{"jsonrpc":"2.0","id":9,"method":"eth_getTransactionByBlockNumberAndIndex","params":["0x1","0x1"]}"#);
        let val_tx1 = JsonValue::parse(&res_tx1).unwrap();
        assert_eq!(
            val_tx1.get("result").unwrap().get("hash").unwrap().as_str(),
            Some(eth_hash2.to_hex().as_str())
        );
        assert_eq!(
            val_tx1
                .get("result")
                .unwrap()
                .get("transactionIndex")
                .unwrap()
                .as_str(),
            Some("0x1")
        );

        let res_tx_oob = disp.dispatch_json(r#"{"jsonrpc":"2.0","id":10,"method":"eth_getTransactionByBlockNumberAndIndex","params":["0x1","0x2"]}"#);
        let val_tx_oob = JsonValue::parse(&res_tx_oob).unwrap();
        assert!(matches!(val_tx_oob.get("result"), Some(JsonValue::Null)));

        // 7. eth_getTransactionByBlockHashAndIndex
        let req_tx_by_hash = format!(
            r#"{{"jsonrpc":"2.0","id":11,"method":"eth_getTransactionByBlockHashAndIndex","params":["{}","0x1"]}}"#,
            block_hash.to_hex()
        );
        let res_tx_by_hash = disp.dispatch_json(&req_tx_by_hash);
        let val_tx_by_hash = JsonValue::parse(&res_tx_by_hash).unwrap();
        assert_eq!(
            val_tx_by_hash
                .get("result")
                .unwrap()
                .get("hash")
                .unwrap()
                .as_str(),
            Some(eth_hash2.to_hex().as_str())
        );

        // 8. Uncle methods
        let req_uncle_cnt_hash = format!(
            r#"{{"jsonrpc":"2.0","id":12,"method":"eth_getUncleCountByBlockHash","params":["{}"]}}"#,
            block_hash.to_hex()
        );
        let res_uncle_cnt_hash = disp.dispatch_json(&req_uncle_cnt_hash);
        let val_uncle_cnt_hash = JsonValue::parse(&res_uncle_cnt_hash).unwrap();
        assert_eq!(
            val_uncle_cnt_hash.get("result").unwrap().as_str(),
            Some("0x0")
        );

        let res_uncle_cnt_num = disp.dispatch_json(r#"{"jsonrpc":"2.0","id":13,"method":"eth_getUncleCountByBlockNumber","params":["0x1"]}"#);
        let val_uncle_cnt_num = JsonValue::parse(&res_uncle_cnt_num).unwrap();
        assert_eq!(
            val_uncle_cnt_num.get("result").unwrap().as_str(),
            Some("0x0")
        );

        let req_uncle_by_hash = format!(
            r#"{{"jsonrpc":"2.0","id":14,"method":"eth_getUncleByBlockHashAndIndex","params":["{}","0x0"]}}"#,
            block_hash.to_hex()
        );
        let res_uncle_by_hash = disp.dispatch_json(&req_uncle_by_hash);
        let val_uncle_by_hash = JsonValue::parse(&res_uncle_by_hash).unwrap();
        assert!(matches!(
            val_uncle_by_hash.get("result"),
            Some(JsonValue::Null)
        ));

        let res_uncle_by_num = disp.dispatch_json(r#"{"jsonrpc":"2.0","id":15,"method":"eth_getUncleByBlockNumberAndIndex","params":["0x1","0x0"]}"#);
        let val_uncle_by_num = JsonValue::parse(&res_uncle_by_num).unwrap();
        assert!(matches!(
            val_uncle_by_num.get("result"),
            Some(JsonValue::Null)
        ));
    }

    let _ = std::fs::remove_dir_all(&temp_dir);
}
