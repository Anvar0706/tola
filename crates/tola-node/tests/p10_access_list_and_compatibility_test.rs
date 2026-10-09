//! TOLA EVM Phase 10 (P10) Ethereum Conformance & Compatibility Integration Test Suite
//!
//! Comprehensive end-to-end verification of:
//! 1. EIP-2930 Type-1 Access List warm storage / gas discounting in REVM
//! 2. EIP-1559 Type-2 Access List execution and state root persistence
//! 3. MAX_ACCESS_LIST_ITEMS = 256 boundary enforcement
//! 4. Intrinsic gas access list metering (2,400 per address, 1,900 per key)
//! 5. Payable `eth_estimateGas` fidelity with non-zero msg.value
//! 6. Full JSON-RPC roundtrip for Type-1 and Type-2 envelopes and receipts

use std::sync::{Arc, RwLock};
use tola_codec::{CanonicalDeserialize, CanonicalSerialize};
use tola_core::{BlockEngine, StateDB, StateDelta};
use tola_crypto::Secp256k1Keypair;
use tola_execution::SequentialBlockEngine;
use tola_mempool::{Mempool, MempoolConfig};
use tola_rpc::dispatcher::RpcDispatcher;
use tola_rpc::json::JsonValue;
use tola_storage::{PersistentStateDB, SegmentStorage, RECORD_BLOCK_BODY};
use tola_types::*;

const TEST_CHAIN_ID: ChainId = ChainId(9999);

struct TestHarness {
    pub state_arc: Arc<RwLock<PersistentStateDB>>,
    pub db_path: std::path::PathBuf,
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
        let db_path = std::env::temp_dir().join(format!("tola_p10_{test_name}_{nanos}"));
        let state_db = PersistentStateDB::open(&db_path).expect("open persistent state_db");
        Self {
            state_arc: Arc::new(RwLock::new(state_db)),
            db_path,
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
                gas_limit: self.block_gas_limit,
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
            "tola/v0.1.0".to_string(),
            self.state_arc.clone(),
            mempool,
        ))
    }
}

fn sign_type1_tx(kp: &Secp256k1Keypair, mut tx: Transaction) -> (Transaction, Hash) {
    tx.tx_type = 1;
    let signing_hash = tola_codec::rlp::compute_type1_signing_hash(&tx);
    let sig_bytes = kp.sign_recoverable(signing_hash.as_bytes()).unwrap();

    let mut r = [0u8; 32];
    let mut s = [0u8; 32];
    r.copy_from_slice(&sig_bytes[0..32]);
    s.copy_from_slice(&sig_bytes[32..64]);
    let y_parity = sig_bytes[64];

    tx.signature = Signature::Secp256k1Eth {
        r,
        s,
        y_parity,
        chain_id: tx.chain_id.0,
    };
    let eth_hash = tola_codec::rlp::compute_eth_tx_hash(&tx).unwrap();
    (tx, eth_hash)
}

fn sign_type2_tx(kp: &Secp256k1Keypair, mut tx: Transaction) -> (Transaction, Hash) {
    tx.tx_type = 2;
    let signing_hash = tola_codec::rlp::compute_type2_signing_hash(&tx);
    let sig_bytes = kp.sign_recoverable(signing_hash.as_bytes()).unwrap();

    let mut r = [0u8; 32];
    let mut s = [0u8; 32];
    r.copy_from_slice(&sig_bytes[0..32]);
    s.copy_from_slice(&sig_bytes[32..64]);
    let y_parity = sig_bytes[64];

    tx.signature = Signature::Secp256k1Eth {
        r,
        s,
        y_parity,
        chain_id: tx.chain_id.0,
    };
    let eth_hash = tola_codec::rlp::compute_eth_tx_hash(&tx).unwrap();
    (tx, eth_hash)
}

#[test]
fn test_p10_eip2930_access_list_warm_slot_gas_discount() {
    let mut harness = TestHarness::new("access_list_warm_slot");
    let kp = Secp256k1Keypair::generate();
    let sender = kp.address();
    harness.fund_account(&sender, 10_000_000_000_000_000_000); // 10 TOLA

    // Bytecode that executes SLOAD on slot 0:
    // PUSH1 0x00, SLOAD, POP, STOP
    // 6000 54 50 00
    let sload_code = vec![0x60, 0x00, 0x54, 0x50, 0x00];
    let code_hash = tola_crypto::keccak256_hash(&sload_code);
    let contract_addr = Address([0x77; 20]);

    {
        let mut db = harness.state_arc.write().unwrap();
        let mut acct = AccountState::new_with_balance(Amount::ZERO);
        acct.code_hash = code_hash;
        db.apply_delta(StateDelta {
            accounts: vec![(contract_addr, Some(acct))],
            storage: vec![((contract_addr, Hash::ZERO), Hash([0x42; 32]))],
            code: vec![(code_hash, sload_code)],
        })
        .unwrap();
    }

    // Run 1: Without access list (cold slot SLOAD = 2100 gas)
    let tx_no_al = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(0),
        from: sender,
        to: Some(contract_addr),
        value: Amount::ZERO,
        gas_limit: Gas(100_000),
        max_fee_per_gas: Amount::from_u128(1_000_000_000),
        max_priority_fee_per_gas: Amount::from_u128(1_000_000_000),
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
    let (signed_no_al, _) = sign_type2_tx(&kp, tx_no_al);
    let (_, receipts1) = harness.commit_block(vec![signed_no_al]);
    assert!(receipts1[0].success);
    let gas_used_no_al = receipts1[0].gas_used.as_u64();

    // Run 2: With access list containing contract_addr and slot 0
    // SLOAD is warm = 100 gas.
    // Intrinsic gas adds: 2400 (address) + 1900 (storage_key) = 4300 gas.
    // Net execution gas is 2000 gas cheaper (2100 - 100 = 2000 saved inside EVM).
    let tx_with_al = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(1),
        from: sender,
        to: Some(contract_addr),
        value: Amount::ZERO,
        gas_limit: Gas(100_000),
        max_fee_per_gas: Amount::from_u128(1_000_000_000),
        max_priority_fee_per_gas: Amount::from_u128(1_000_000_000),
        valid_until_block: BlockHeight(100),
        data: vec![],
        memo: None,
        signature: Signature::Secp256k1Eth {
            r: [0; 32],
            s: [0; 32],
            y_parity: 0,
            chain_id: TEST_CHAIN_ID.0,
        },
        tx_type: 1,
        access_list: vec![AccessListItem {
            address: contract_addr,
            storage_keys: vec![Hash::ZERO],
        }],
    };
    let (signed_with_al, _) = sign_type1_tx(&kp, tx_with_al);
    let (_, receipts2) = harness.commit_block(vec![signed_with_al]);
    assert!(receipts2[0].success);
    let gas_used_with_al = receipts2[0].gas_used.as_u64();

    // Expected difference:
    // With AL: Base 21,000 + 4,300 (AL intrinsic) + 100 (warm SLOAD) + other opcodes
    // Without AL: Base 21,000 + 2,100 (cold SLOAD) + other opcodes (since contract is warm via tx.to)
    // Difference = 4,300 - 2,000 = +2,300 total gas
    assert_eq!(
        gas_used_with_al,
        gas_used_no_al + 2300,
        "Access list warm SLOAD gas arithmetic must strictly match EVM specification"
    );
}

#[test]
fn test_p10_access_list_size_limit_rejection() {
    let kp = Secp256k1Keypair::generate();
    let sender = kp.address();

    // 256 items: Must succeed
    let valid_al: Vec<AccessListItem> = (0..MAX_ACCESS_LIST_ITEMS)
        .map(|i| AccessListItem {
            address: Address([(i % 256) as u8; 20]),
            storage_keys: vec![],
        })
        .collect();

    let tx_valid = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(0),
        from: sender,
        to: Some(Address([0x11; 20])),
        value: Amount::ZERO,
        gas_limit: Gas(2_000_000),
        max_fee_per_gas: Amount::from_u128(10),
        max_priority_fee_per_gas: Amount::from_u128(10),
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
        access_list: valid_al,
    };
    assert!(
        tx_valid
            .stateless_validate(TEST_CHAIN_ID, Gas(30_000_000))
            .is_ok(),
        "256 access list items must pass stateless validation"
    );

    // 257 items: Must fail with MalformedPayload
    let mut invalid_al = tx_valid.access_list.clone();
    invalid_al.push(AccessListItem {
        address: Address([0xff; 20]),
        storage_keys: vec![],
    });

    let tx_invalid = Transaction {
        access_list: invalid_al,
        ..tx_valid
    };
    let err = tx_invalid
        .stateless_validate(TEST_CHAIN_ID, Gas(30_000_000))
        .expect_err("257 items must fail stateless validation");
    assert_eq!(
        err,
        TransactionError::AccessListTooLarge {
            max_items: MAX_ACCESS_LIST_ITEMS,
            got_items: 257,
        },
        "Must return AccessListTooLarge on exceeding MAX_ACCESS_LIST_ITEMS"
    );
}

#[test]
fn test_p10_intrinsic_gas_access_list_metering() {
    let kp = Secp256k1Keypair::generate();
    let sender = kp.address();

    // Base tx: 21,000 gas
    let mut tx = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(0),
        from: sender,
        to: Some(Address([0x11; 20])),
        value: Amount::ZERO,
        gas_limit: Gas(21_000),
        max_fee_per_gas: Amount::from_u128(10),
        max_priority_fee_per_gas: Amount::from_u128(10),
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
    assert_eq!(tx.intrinsic_gas(), Gas(21_000));

    // Add 1 address (2,400) and 2 storage keys (2 * 1,900 = 3,800)
    tx.access_list.push(AccessListItem {
        address: Address([0x22; 20]),
        storage_keys: vec![Hash([0x01; 32]), Hash([0x02; 32])],
    });
    // Expected: 21,000 + 2,400 + 3,800 = 27,200
    assert_eq!(tx.intrinsic_gas(), Gas(27_200));

    // Validating with gas_limit = 27,199 must fail
    tx.gas_limit = Gas(27_199);
    assert_eq!(
        tx.stateless_validate(TEST_CHAIN_ID, Gas(30_000_000)),
        Err(TransactionError::GasLimitTooLow {
            minimum: 27_200,
            got: 27_199,
        })
    );

    // Validating with gas_limit = 27,200 must succeed
    tx.gas_limit = Gas(27_200);
    assert!(tx
        .stateless_validate(TEST_CHAIN_ID, Gas(30_000_000))
        .is_ok());
}

#[test]
fn test_p10_payable_eth_estimate_gas_accuracy() {
    let harness = TestHarness::new("payable_estimate_gas");
    let disp = harness.make_dispatcher();

    let kp = Secp256k1Keypair::generate();
    let sender = kp.address();
    harness
        .state_arc
        .write()
        .unwrap()
        .apply_delta(StateDelta {
            accounts: vec![(
                sender,
                Some(AccountState::new_with_balance(Amount::from_u128(
                    10_000_000_000_000_000_000,
                ))),
            )],
            storage: vec![],
            code: vec![],
        })
        .unwrap();

    // Payable contract bytecode:
    // CALLVALUE, DUP1, ISZERO, PUSH1 0x0e, JUMPI, PUSH1 0x00, SSTORE, STOP, JUMPDEST, PUSH1 0x00, PUSH1 0x00, REVERT
    // Bytecode: 34 80 15 600e 57 6000 55 00 5b 6000 6000 fd
    let payable_bytecode = vec![
        0x34, 0x80, 0x15, 0x60, 0x0e, 0x57, 0x60, 0x00, 0x55, 0x00, 0x5b, 0x60, 0x00, 0x60, 0x00,
        0xfd,
    ];
    let code_hash = tola_crypto::keccak256_hash(&payable_bytecode);
    let contract_addr = Address([0x88; 20]);

    {
        let mut db = harness.state_arc.write().unwrap();
        let mut acct = AccountState::new_with_balance(Amount::ZERO);
        acct.code_hash = code_hash;
        db.apply_delta(StateDelta {
            accounts: vec![(contract_addr, Some(acct))],
            storage: vec![],
            code: vec![(code_hash, payable_bytecode)],
        })
        .unwrap();
    }

    // 1. eth_estimateGas with value: 0 -> Must REVERT because msg.value is 0!
    let req_zero_val = format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"eth_estimateGas","params":[{{ "from": "{}", "to": "{}", "value": "0x0" }}]}}"#,
        sender.to_hex(),
        contract_addr.to_hex()
    );
    let res_zero_val = disp.dispatch_json(&req_zero_val);
    let val_zero_val = JsonValue::parse(&res_zero_val).unwrap();
    assert!(
        val_zero_val.get("error").is_some(),
        "eth_estimateGas with 0 value must revert as required by bytecode: {res_zero_val}"
    );

    // 2. eth_estimateGas with value: 100 Wei ("0x64") -> Must SUCCEED because msg.value > 0!
    let req_paid = format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"eth_estimateGas","params":[{{ "from": "{}", "to": "{}", "value": "0x64" }}]}}"#,
        sender.to_hex(),
        contract_addr.to_hex()
    );
    let res_paid = disp.dispatch_json(&req_paid);
    let val_paid = JsonValue::parse(&res_paid).unwrap();
    assert!(
        val_paid.get("result").is_some(),
        "eth_estimateGas with non-zero value must succeed: {res_paid}"
    );
    let gas_hex = val_paid.get("result").unwrap().as_str().unwrap();
    assert!(gas_hex.starts_with("0x"));
}

#[test]
fn test_p10_full_rpc_roundtrip_type1_and_type2_with_access_list() {
    let mut harness = TestHarness::new("full_rpc_roundtrip");
    let disp = harness.make_dispatcher();

    let kp = Secp256k1Keypair::generate();
    let sender = kp.address();
    harness.fund_account(&sender, 10_000_000_000_000_000_000);

    // Construct Type-1 transaction with access list
    let target = Address([0x99; 20]);
    let al_item = AccessListItem {
        address: target,
        storage_keys: vec![Hash([0x42; 32])],
    };
    let tx_type1 = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(0),
        from: sender,
        to: Some(target),
        value: Amount::from_u128(1000),
        gas_limit: Gas(100_000),
        max_fee_per_gas: Amount::from_u128(2_000_000_000),
        max_priority_fee_per_gas: Amount::from_u128(2_000_000_000),
        valid_until_block: BlockHeight(100),
        data: vec![0xca, 0xfe],
        memo: None,
        signature: Signature::Secp256k1Eth {
            r: [0; 32],
            s: [0; 32],
            y_parity: 0,
            chain_id: TEST_CHAIN_ID.0,
        },
        tx_type: 1,
        access_list: vec![al_item.clone()],
    };
    let (signed_type1, eth_hash_type1) = sign_type1_tx(&kp, tx_type1);
    let raw_type1 = tola_codec::rlp::encode_raw_eth_transaction(&signed_type1).unwrap();

    // Construct Type-2 transaction with access list
    let tx_type2 = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(1),
        from: sender,
        to: Some(target),
        value: Amount::from_u128(2000),
        gas_limit: Gas(100_000),
        max_fee_per_gas: Amount::from_u128(2_000_000_000),
        max_priority_fee_per_gas: Amount::from_u128(500_000_000),
        valid_until_block: BlockHeight(100),
        data: vec![0xba, 0xbe],
        memo: None,
        signature: Signature::Secp256k1Eth {
            r: [0; 32],
            s: [0; 32],
            y_parity: 0,
            chain_id: TEST_CHAIN_ID.0,
        },
        tx_type: 2,
        access_list: vec![al_item],
    };
    let (signed_type2, eth_hash_type2) = sign_type2_tx(&kp, tx_type2);
    let raw_type2 = tola_codec::rlp::encode_raw_eth_transaction(&signed_type2).unwrap();

    // Commit both into block 1
    let (block, receipts) = harness.commit_block(vec![signed_type1, signed_type2]);
    disp.record_block(block, receipts);

    // 1. Verify Type-1 transaction RPC format
    let req_t1 = format!(
        r#"{{"jsonrpc":"2.0","id":10,"method":"eth_getTransactionByHash","params":["{}"]}}"#,
        eth_hash_type1.to_hex()
    );
    let res_t1 = disp.dispatch_json(&req_t1);
    let val_t1 = JsonValue::parse(&res_t1).unwrap();
    let tx1_obj = val_t1.get("result").unwrap();
    assert_eq!(tx1_obj.get("type").unwrap().as_str(), Some("0x1"));
    let al1 = tx1_obj.get("accessList").unwrap().as_array().unwrap();
    assert_eq!(al1.len(), 1);
    assert_eq!(
        al1[0].get("address").unwrap().as_str(),
        Some(target.to_hex().as_str())
    );
    let keys1 = al1[0].get("storageKeys").unwrap().as_array().unwrap();
    assert_eq!(keys1.len(), 1);
    assert_eq!(keys1[0].as_str(), Some(Hash([0x42; 32]).to_hex().as_str()));

    // 2. Verify Type-1 receipt RPC format
    let req_r1 = format!(
        r#"{{"jsonrpc":"2.0","id":11,"method":"eth_getTransactionReceipt","params":["{}"]}}"#,
        eth_hash_type1.to_hex()
    );
    let res_r1 = disp.dispatch_json(&req_r1);
    let val_r1 = JsonValue::parse(&res_r1).unwrap();
    let rcpt1_obj = val_r1.get("result").unwrap();
    assert_eq!(rcpt1_obj.get("type").unwrap().as_str(), Some("0x1"));
    assert_eq!(rcpt1_obj.get("status").unwrap().as_str(), Some("0x1"));

    // 3. Verify Type-2 transaction RPC format
    let req_t2 = format!(
        r#"{{"jsonrpc":"2.0","id":20,"method":"eth_getTransactionByHash","params":["{}"]}}"#,
        eth_hash_type2.to_hex()
    );
    let res_t2 = disp.dispatch_json(&req_t2);
    let val_t2 = JsonValue::parse(&res_t2).unwrap();
    let tx2_obj = val_t2.get("result").unwrap();
    assert_eq!(tx2_obj.get("type").unwrap().as_str(), Some("0x2"));
    let al2 = tx2_obj.get("accessList").unwrap().as_array().unwrap();
    assert_eq!(al2.len(), 1);

    // 4. Verify Type-2 receipt RPC format
    let req_r2 = format!(
        r#"{{"jsonrpc":"2.0","id":21,"method":"eth_getTransactionReceipt","params":["{}"]}}"#,
        eth_hash_type2.to_hex()
    );
    let res_r2 = disp.dispatch_json(&req_r2);
    let val_r2 = JsonValue::parse(&res_r2).unwrap();
    let rcpt2_obj = val_r2.get("result").unwrap();
    assert_eq!(rcpt2_obj.get("type").unwrap().as_str(), Some("0x2"));
    assert_eq!(rcpt2_obj.get("status").unwrap().as_str(), Some("0x1"));

    // 5. Verify raw tx decoding round-trip directly from wire bytes
    let (dec_t1, hash1) = tola_codec::rlp::decode_raw_eth_transaction(&raw_type1).unwrap();
    assert_eq!(dec_t1.tx_type, 1);
    assert_eq!(dec_t1.from, sender);
    assert_eq!(hash1, eth_hash_type1);

    let (dec_t2, hash2) = tola_codec::rlp::decode_raw_eth_transaction(&raw_type2).unwrap();
    assert_eq!(dec_t2.tx_type, 2);
    assert_eq!(dec_t2.from, sender);
    assert_eq!(hash2, eth_hash_type2);
}

#[test]
fn test_p10_cold_restart_preserves_access_list_and_reexecution() {
    let mut harness = TestHarness::new("cold_restart_al");
    let db_path = harness.db_path.clone();

    let kp = Secp256k1Keypair::generate();
    let sender = kp.address();
    harness.fund_account(&sender, 10_000_000_000_000_000_000); // 10 TOLA

    // Deploy contract with storage slot: PUSH1 0x00, SLOAD, POP, STOP
    let sload_code = vec![0x60, 0x00, 0x54, 0x50, 0x00];
    let code_hash = tola_crypto::keccak256_hash(&sload_code);
    let contract_addr = Address([0x77; 20]);
    let target_slot = Hash([0x42; 32]);

    {
        let mut db = harness.state_arc.write().unwrap();
        let mut acct = AccountState::new_with_balance(Amount::ZERO);
        acct.code_hash = code_hash;
        db.apply_delta(StateDelta {
            accounts: vec![(contract_addr, Some(acct))],
            storage: vec![((contract_addr, Hash::ZERO), target_slot)],
            code: vec![(code_hash, sload_code.clone())],
        })
        .unwrap();
    }

    // 1. Build Type-1 transaction with access list specifying contract_addr and slot 0
    let al_item = AccessListItem {
        address: contract_addr,
        storage_keys: vec![Hash::ZERO],
    };
    let tx_type1 = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(0),
        from: sender,
        to: Some(contract_addr),
        value: Amount::ZERO,
        gas_limit: Gas(100_000),
        max_fee_per_gas: Amount::from_u128(2_000_000_000),
        max_priority_fee_per_gas: Amount::from_u128(2_000_000_000),
        valid_until_block: BlockHeight(100),
        data: vec![],
        memo: None,
        signature: Signature::Secp256k1Eth {
            r: [0; 32],
            s: [0; 32],
            y_parity: 0,
            chain_id: TEST_CHAIN_ID.0,
        },
        tx_type: 1,
        access_list: vec![al_item.clone()],
    };
    let (signed_type1, eth_hash_type1) = sign_type1_tx(&kp, tx_type1);

    // 2. Build Type-2 transaction with access list specifying contract_addr and slot 0
    let tx_type2 = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(1),
        from: sender,
        to: Some(contract_addr),
        value: Amount::ZERO,
        gas_limit: Gas(100_000),
        max_fee_per_gas: Amount::from_u128(2_000_000_000),
        max_priority_fee_per_gas: Amount::from_u128(500_000_000),
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
        access_list: vec![al_item],
    };
    let (signed_type2, eth_hash_type2) = sign_type2_tx(&kp, tx_type2);

    // Execute and commit both into Block 1 on disk via MDBX commit_block_bundle
    let (committed_block, committed_receipts) =
        harness.commit_block(vec![signed_type1.clone(), signed_type2.clone()]);
    assert_eq!(committed_receipts.len(), 2);
    assert!(committed_receipts[0].success);
    assert!(committed_receipts[1].success);

    let original_state_root = committed_block.header.state_root;
    let original_receipt0_gas = committed_receipts[0].gas_used;
    let original_receipt1_gas = committed_receipts[1].gas_used;

    // Simulate complete cold restart by dropping harness and all in-memory state
    drop(harness);

    // ------------------------------------------------------------------------
    // COLD RESTART: Reopen disk database from scratch
    // ------------------------------------------------------------------------
    let reopened_db = PersistentStateDB::open(&db_path).expect("reopen state_db across restart");
    let state_arc = Arc::new(RwLock::new(reopened_db));

    // A. Verify Block recovered from Table::BlockBodies directly
    let reloaded_block = state_arc
        .read()
        .unwrap()
        .get_block_by_height(BlockHeight(1))
        .expect("get_block_by_height succeeds")
        .expect("Block 1 must exist on disk");

    assert_eq!(reloaded_block.transactions.len(), 2);

    // Assert Transaction 0 (Type-1) has preserved tx_type and access_list
    let reloaded_tx0 = &reloaded_block.transactions[0];
    assert_eq!(
        reloaded_tx0.tx_type, 1,
        "Tx 0 type must be 1 across cold restart"
    );
    assert_eq!(
        reloaded_tx0.access_list.len(),
        1,
        "Tx 0 access list must not be empty across restart"
    );
    assert_eq!(reloaded_tx0.access_list[0].address, contract_addr);
    assert_eq!(reloaded_tx0.access_list[0].storage_keys, vec![Hash::ZERO]);
    assert_eq!(reloaded_tx0, &signed_type1);

    // Assert Transaction 1 (Type-2) has preserved tx_type and access_list
    let reloaded_tx1 = &reloaded_block.transactions[1];
    assert_eq!(
        reloaded_tx1.tx_type, 2,
        "Tx 1 type must be 2 across cold restart"
    );
    assert_eq!(
        reloaded_tx1.access_list.len(),
        1,
        "Tx 1 access list must not be empty across restart"
    );
    assert_eq!(reloaded_tx1.access_list[0].address, contract_addr);
    assert_eq!(reloaded_tx1.access_list[0].storage_keys, vec![Hash::ZERO]);
    assert_eq!(reloaded_tx1, &signed_type2);

    // B. Verify JSON-RPC queries on fresh RpcDispatcher across restart
    let mempool = Arc::new(RwLock::new(Mempool::new(MempoolConfig::new(TEST_CHAIN_ID))));
    let rpc_reopened = RpcDispatcher::new(
        TEST_CHAIN_ID,
        "tola/v0.1.0".to_string(),
        state_arc.clone(),
        mempool,
    );

    // Query Type-1 transaction by hash
    let req_t1 = format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"eth_getTransactionByHash","params":["{}"]}}"#,
        eth_hash_type1.to_hex()
    );
    let res_t1 = rpc_reopened.dispatch_json(&req_t1);
    let val_t1 = JsonValue::parse(&res_t1).unwrap();
    let tx1_obj = val_t1.get("result").unwrap();
    assert_ne!(
        tx1_obj,
        &JsonValue::Null,
        "Type-1 tx must be found across restart"
    );
    assert_eq!(tx1_obj.get("type").unwrap().as_str(), Some("0x1"));
    let al1 = tx1_obj.get("accessList").unwrap().as_array().unwrap();
    assert_eq!(al1.len(), 1, "RPC accessList must not be empty");
    assert_eq!(
        al1[0].get("address").unwrap().as_str(),
        Some(contract_addr.to_hex().as_str())
    );
    let keys1 = al1[0].get("storageKeys").unwrap().as_array().unwrap();
    assert_eq!(keys1.len(), 1);
    assert_eq!(keys1[0].as_str(), Some(Hash::ZERO.to_hex().as_str()));

    // Query Type-2 transaction by hash
    let req_t2 = format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"eth_getTransactionByHash","params":["{}"]}}"#,
        eth_hash_type2.to_hex()
    );
    let res_t2 = rpc_reopened.dispatch_json(&req_t2);
    let val_t2 = JsonValue::parse(&res_t2).unwrap();
    let tx2_obj = val_t2.get("result").unwrap();
    assert_ne!(
        tx2_obj,
        &JsonValue::Null,
        "Type-2 tx must be found across restart"
    );
    assert_eq!(tx2_obj.get("type").unwrap().as_str(), Some("0x2"));
    let al2 = tx2_obj.get("accessList").unwrap().as_array().unwrap();
    assert_eq!(al2.len(), 1, "RPC accessList must not be empty");
    assert_eq!(
        al2[0].get("address").unwrap().as_str(),
        Some(contract_addr.to_hex().as_str())
    );

    // C. Re-execution parity verification:
    // Create an isolated pre-state clone to re-execute the reloaded block
    let re_exec_dir = std::env::temp_dir().join(format!(
        "tola_p10_reexec_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut re_exec_db = PersistentStateDB::open(&re_exec_dir).expect("open re_exec db");
    let mut acct = AccountState::new_with_balance(Amount::ZERO);
    acct.code_hash = code_hash;
    re_exec_db
        .apply_delta(StateDelta {
            accounts: vec![
                (
                    sender,
                    Some(AccountState::new_with_balance(Amount::from_u128(
                        10_000_000_000_000_000_000,
                    ))),
                ),
                (contract_addr, Some(acct)),
            ],
            storage: vec![((contract_addr, Hash::ZERO), target_slot)],
            code: vec![(code_hash, sload_code)],
        })
        .unwrap();

    let engine = SequentialBlockEngine::new(TEST_CHAIN_ID);
    let re_executed_receipts = engine
        .execute_block(&reloaded_block, &mut re_exec_db)
        .expect("re-execution of reloaded block must succeed");

    assert_eq!(re_executed_receipts.len(), 2);
    assert_eq!(
        re_executed_receipts[0].gas_used, original_receipt0_gas,
        "Type-1 warm gas must match exactly"
    );
    assert_eq!(
        re_executed_receipts[1].gas_used, original_receipt1_gas,
        "Type-2 warm gas must match exactly"
    );
    assert!(re_executed_receipts[0].success);
    assert!(re_executed_receipts[1].success);

    let re_executed_state_root = re_exec_db.compute_state_root().unwrap();
    assert_eq!(
        re_executed_state_root, original_state_root,
        "Re-executed state root must match original committed block state root"
    );

    // Clean up temporary directories
    let _ = std::fs::remove_dir_all(&db_path);
    let _ = std::fs::remove_dir_all(&re_exec_dir);
}

#[test]
fn test_p10_canonical_framing_collision_avoidance_and_cold_restart() {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let seg_dir = std::env::temp_dir().join(format!("tola_p10_collision_restart_{nanos}"));

    // 1. Construct adjacent transactions of all types, including collision chain_id 21_778_260
    let collision_chain_id = ChainId(21_778_260);

    // Tx 0: Unextended legacy EVM transaction
    let tx0_legacy = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(0),
        from: Address([0x11; 20]),
        to: Some(Address([0x22; 20])),
        value: Amount::from_u128(1_000_000),
        gas_limit: Gas(21_000),
        max_fee_per_gas: Amount::from_u128(1_000_000_000),
        max_priority_fee_per_gas: Amount::from_u128(1_000_000_000),
        valid_until_block: BlockHeight(100),
        data: vec![],
        memo: None,
        signature: Signature::Secp256k1Eth {
            r: [0x01; 32],
            s: [0x02; 32],
            y_parity: 0,
            chain_id: TEST_CHAIN_ID.0,
        },
        tx_type: 0,
        access_list: vec![],
    };

    // Tx 1: Follows Tx 0 immediately with chain_id 21_778_260
    // Under flawed magic-peek framing, Tx 0's parser would peek into Tx 1's chain_id
    // and misidentify it as TOLA_TYPED_TX_MAGIC [0x54, 0x4F, 0x4C, 0x01].
    let tx1_collision_chain = Transaction {
        chain_id: collision_chain_id,
        nonce: Nonce(1),
        from: Address([0x33; 20]),
        to: Some(Address([0x44; 20])),
        value: Amount::from_u128(2_000_000),
        gas_limit: Gas(21_000),
        max_fee_per_gas: Amount::from_u128(2_000_000_000),
        max_priority_fee_per_gas: Amount::from_u128(2_000_000_000),
        valid_until_block: BlockHeight(100),
        data: vec![],
        memo: None,
        signature: Signature::Secp256k1Eth {
            r: [0x03; 32],
            s: [0x04; 32],
            y_parity: 1,
            chain_id: collision_chain_id.0,
        },
        tx_type: 0,
        access_list: vec![],
    };

    // Tx 2: Type-1 (EIP-2930) with non-empty access list
    let tx2_type1 = Transaction {
        chain_id: collision_chain_id,
        nonce: Nonce(2),
        from: Address([0x55; 20]),
        to: Some(Address([0x66; 20])),
        value: Amount::from_u128(3_000_000),
        gas_limit: Gas(30_000),
        max_fee_per_gas: Amount::from_u128(3_000_000_000),
        max_priority_fee_per_gas: Amount::from_u128(3_000_000_000),
        valid_until_block: BlockHeight(100),
        data: vec![0x12, 0x34],
        memo: None,
        signature: Signature::Secp256k1Eth {
            r: [0x05; 32],
            s: [0x06; 32],
            y_parity: 0,
            chain_id: collision_chain_id.0,
        },
        tx_type: 1,
        access_list: vec![AccessListItem {
            address: Address([0xaa; 20]),
            storage_keys: vec![Hash([0xbb; 32]), Hash([0xcc; 32])],
        }],
    };

    // Tx 3: Native TOLA transaction (Ed25519)
    let tx3_native = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(3),
        from: Address([0x77; 20]),
        to: Some(Address([0x88; 20])),
        value: Amount::from_u128(4_000_000),
        gas_limit: Gas(21_000),
        max_fee_per_gas: Amount::from_u128(1_000_000_000),
        max_priority_fee_per_gas: Amount::from_u128(1_000_000_000),
        valid_until_block: BlockHeight(100),
        data: vec![],
        memo: Some("Native Memo".into()),
        signature: Signature::Ed25519([0xee; 64]),
        tx_type: 0,
        access_list: vec![],
    };

    // Tx 4: Type-2 (EIP-1559) with non-empty access list
    let tx4_type2 = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(4),
        from: Address([0x99; 20]),
        to: Some(Address([0xaa; 20])),
        value: Amount::from_u128(5_000_000),
        gas_limit: Gas(40_000),
        max_fee_per_gas: Amount::from_u128(4_000_000_000),
        max_priority_fee_per_gas: Amount::from_u128(1_000_000_000),
        valid_until_block: BlockHeight(100),
        data: vec![0xfe, 0xed],
        memo: None,
        signature: Signature::Secp256k1Eth {
            r: [0x07; 32],
            s: [0x08; 32],
            y_parity: 1,
            chain_id: TEST_CHAIN_ID.0,
        },
        tx_type: 2,
        access_list: vec![AccessListItem {
            address: Address([0xdd; 20]),
            storage_keys: vec![Hash([0xee; 32])],
        }],
    };

    let original_block = Block {
        header: BlockHeader {
            parent_hash: Hash::ZERO,
            height: BlockHeight(42),
            epoch: Epoch(1),
            round: Round(2),
            timestamp: Timestamp(1_700_000_000),
            proposer: Address([0x12; 20]),
            state_root: Hash([0x34; 32]),
            txs_root: Hash([0x56; 32]),
            receipts_root: Hash([0x78; 32]),
            gas_used: Gas(133_000),
            gas_limit: Gas(30_000_000),
            base_fee_per_gas: Amount::from_u128(1_000_000_000),
            extra_data: vec![0xaa, 0xbb],
        },
        transactions: vec![
            tx0_legacy.clone(),
            tx1_collision_chain.clone(),
            tx2_type1.clone(),
            tx3_native.clone(),
            tx4_type2.clone(),
        ],
        last_commit_qc: None,
    };

    // 2. Persist to flat segment storage on disk
    let original_block_bytes = original_block.to_canonical_bytes();
    let location = {
        let mut seg_storage = SegmentStorage::open(&seg_dir, 64 * 1024 * 1024)
            .expect("open segment storage for append");
        seg_storage
            .append(RECORD_BLOCK_BODY, &original_block_bytes)
            .expect("append block body to segment")
    };

    // 3. Simulate cold restart: re-open segment storage from scratch
    let reloaded_block = {
        let reopened_seg =
            SegmentStorage::open(&seg_dir, 64 * 1024 * 1024).expect("reopen segment storage");
        let raw_payload = reopened_seg
            .read(&location)
            .expect("read record from segment");
        assert_eq!(raw_payload.len(), original_block_bytes.len());

        Block::deserialize_canonical(&raw_payload)
            .expect("Block with collision chain_id and typed txs must deserialize cleanly across cold restart")
    };

    // 4. Verify all transactions and block structures match bit-for-bit
    assert_eq!(reloaded_block.transactions.len(), 5);
    assert_eq!(reloaded_block.transactions[0], tx0_legacy);
    assert_eq!(reloaded_block.transactions[1], tx1_collision_chain);
    assert_eq!(reloaded_block.transactions[2], tx2_type1);
    assert_eq!(reloaded_block.transactions[3], tx3_native);
    assert_eq!(reloaded_block.transactions[4], tx4_type2);
    assert_eq!(reloaded_block, original_block);

    // Clean up temporary segment storage directory
    let _ = std::fs::remove_dir_all(&seg_dir);
}
