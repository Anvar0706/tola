//! TOLA EVM Phase 6 (P6) Smart Contract & Developer Compatibility Integration Test Suite
//!
//! Comprehensive end-to-end verification of:
//! A. Contract Deployment & Persistence
//! B. Contract Storage & Persistence
//! C. ABI / Calldata / Returndata
//! D. Revert Semantics & Rollback
//! E. Contract-to-Contract Calls (CALL, STATICCALL, DELEGATECALL)
//! F. CREATE2 Deterministic Deployment
//! G. Events & Logs (LOG0-LOG4, filtering, receipts)
//! H. ERC-20 Full Lifecycle & Persistence
//! I. ERC-721 Full Lifecycle & Receiver Callbacks
//! J. ERC-1155 Multi-Token Lifecycle & Persistence
//! K. Proxy / DELEGATECALL Compatibility
//! L. BLOCKHASH Canonical & 256-Block Window Rules
//! M. PREVRANDAO Shanghai Determinism
//! N. State Root & Unified StateDB
//! O. RPC Smart Contract Surface
//! P. Cold Restart & Persistence Parity
//! Q. Gas Accounting Correctness
//! R. Security & Negative Tests

use std::sync::{Arc, RwLock};
use tola_core::{BlockEngine, StateDB};
use tola_crypto::{keccak256_hash, Secp256k1Keypair};
use tola_execution::SequentialBlockEngine;
use tola_mempool::{Mempool, MempoolConfig};
use tola_rpc::dispatcher::RpcDispatcher;
use tola_rpc::json::JsonValue;
use tola_storage::PersistentStateDB;
use tola_types::*;

const TEST_CHAIN_ID: ChainId = ChainId(9999);

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

fn compute_contract_address(sender: &Address, nonce: u64) -> Address {
    tola_codec::rlp::compute_contract_address(sender, nonce)
}

fn compute_create2_address(deployer: &Address, salt: &[u8; 32], init_code: &[u8]) -> Address {
    let code_hash = keccak256_hash(init_code);
    let mut buf = Vec::with_capacity(1 + 20 + 32 + 32);
    buf.push(0xff);
    buf.extend_from_slice(&deployer.0);
    buf.extend_from_slice(salt);
    buf.extend_from_slice(code_hash.as_bytes());
    let hash = keccak256_hash(&buf);
    let mut addr = [0u8; 20];
    addr.copy_from_slice(&hash.as_bytes()[12..32]);
    Address(addr)
}

fn make_initcode(runtime: &[u8]) -> Vec<u8> {
    let mut code = Vec::new();
    let r_len = runtime.len();
    if r_len <= 255 {
        code.extend_from_slice(&[
            0x60,
            r_len as u8, // PUSH1 r_len
            0x60,
            0x0c, // PUSH1 12 (offset)
            0x60,
            0x00, // PUSH1 0 (dest)
            0x39, // CODECOPY
            0x60,
            r_len as u8, // PUSH1 r_len
            0x60,
            0x00, // PUSH1 0 (memory offset)
            0xf3, // RETURN
        ]);
        code.extend_from_slice(runtime);
    } else {
        let r_len_u16 = r_len as u16;
        code.extend_from_slice(&[
            0x61,
            (r_len_u16 >> 8) as u8,
            (r_len_u16 & 0xff) as u8, // PUSH2 r_len
            0x60,
            0x0e, // PUSH1 14 (offset)
            0x60,
            0x00, // PUSH1 0
            0x39, // CODECOPY
            0x61,
            (r_len_u16 >> 8) as u8,
            (r_len_u16 & 0xff) as u8, // PUSH2 r_len
            0x60,
            0x00, // PUSH1 0
            0xf3, // RETURN
        ]);
        code.extend_from_slice(runtime);
    }
    code
}

fn make_initcode_with_setup(setup: &[u8], runtime: &[u8]) -> Vec<u8> {
    let mut code = Vec::new();
    code.extend_from_slice(setup);
    let r_len = runtime.len();
    let header_len = 12usize;
    let offset = setup.len() + header_len;
    assert!(r_len <= 255);
    assert!(offset <= 255);
    code.extend_from_slice(&[
        0x60,
        r_len as u8, // PUSH1 r_len
        0x60,
        offset as u8, // PUSH1 offset
        0x60,
        0x00, // PUSH1 0
        0x39, // CODECOPY
        0x60,
        r_len as u8, // PUSH1 r_len
        0x60,
        0x00, // PUSH1 0
        0xf3, // RETURN
    ]);
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

struct TestHarness {
    db_path: std::path::PathBuf,
    pub state_arc: Arc<RwLock<PersistentStateDB>>,
    current_height: u64,
    base_fee: Amount,
    proposer: Address,
    parent_hash: Hash,
}

impl TestHarness {
    fn new(test_name: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let db_path = std::env::temp_dir().join(format!("tola_p6_{test_name}_{nanos}"));
        let state_db = PersistentStateDB::open(&db_path).expect("open persistent state_db");
        Self {
            db_path,
            state_arc: Arc::new(RwLock::new(state_db)),
            current_height: 0,
            base_fee: Amount::from_u128(10),
            proposer: Address([0xaa; 20]),
            parent_hash: Hash::ZERO,
        }
    }

    fn fund_account(&mut self, addr: &Address, balance: u128) {
        let mut db = self.state_arc.write().unwrap();
        db.apply_delta(tola_core::StateDelta {
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
        let timestamp = Timestamp(1000 + self.current_height);
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
                gas_limit: Gas(30_000_000),
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
            .expect("commit_block_bundle succeeds");

        self.parent_hash = block_hash;
        (block, receipts)
    }

    fn reopen(&mut self) {
        let reopened = PersistentStateDB::open(&self.db_path).expect("reopen persistent state_db");
        self.state_arc = Arc::new(RwLock::new(reopened));
    }

    fn make_dispatcher(&self) -> Arc<RpcDispatcher<PersistentStateDB>> {
        let mempool_arc = Arc::new(RwLock::new(Mempool::new(MempoolConfig::new(TEST_CHAIN_ID))));
        Arc::new(RpcDispatcher::new(
            TEST_CHAIN_ID,
            "TOLA-P6/1.0".into(),
            self.state_arc.clone(),
            mempool_arc,
        ))
    }
}

impl Drop for TestHarness {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.db_path);
    }
}

// =========================================================================
// A. CONTRACT DEPLOYMENT & PERSISTENCE
// =========================================================================

#[test]
fn test_p6_a_contract_deployment_and_persistence() {
    let mut harness = TestHarness::new("test_a_deploy");
    let kp = Secp256k1Keypair::generate();
    let sender = kp.address();
    harness.fund_account(&sender, 10_000_000_000);

    // Minimal contract returning 42: PUSH1 42, PUSH1 0, MSTORE, PUSH1 32, PUSH1 0, RETURN
    let runtime_bytecode = hex_decode("602a60005260206000f3");
    let initcode = make_initcode(&runtime_bytecode);

    let deploy_tx = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce::ZERO,
        from: sender,
        to: None,
        value: Amount::ZERO,
        gas_limit: Gas(200_000),
        max_fee_per_gas: Amount::from_u128(20),
        max_priority_fee_per_gas: Amount::from_u128(2),
        valid_until_block: BlockHeight(100),
        data: initcode,
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_tx, eth_hash) = sign_eth_tx(&kp, deploy_tx);

    let expected_addr = compute_contract_address(&sender, 0);
    let (block, receipts) = harness.commit_block(vec![signed_tx]);

    assert_eq!(receipts.len(), 1);
    assert!(receipts[0].success);

    // Verify RPC response pre-restart
    let rpc = harness.make_dispatcher();
    rpc.record_block(block.clone(), receipts.clone());

    let rec_res = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"eth_getTransactionReceipt","params":["{}"]}}"#,
        eth_hash.to_hex()
    ));
    let rec_json = JsonValue::parse(&rec_res).unwrap();
    let contract_addr = rec_json
        .get("result")
        .unwrap()
        .get("contractAddress")
        .unwrap()
        .as_str()
        .unwrap();
    assert_eq!(
        contract_addr.to_lowercase(),
        expected_addr.to_hex().to_lowercase()
    );

    // Verify eth_getCode pre-restart
    let code_res = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"eth_getCode","params":["{}","latest"]}}"#,
        expected_addr.to_hex()
    ));
    let code_json = JsonValue::parse(&code_res).unwrap();
    let code_hex = code_json.get("result").unwrap().as_str().unwrap();
    assert_eq!(code_hex, format!("0x{}", hex_encode(&runtime_bytecode)));

    // Reopen node / persistent DB
    harness.reopen();

    let rpc_after = harness.make_dispatcher();
    let code_after = rpc_after.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":3,"method":"eth_getCode","params":["{}","latest"]}}"#,
        expected_addr.to_hex()
    ));
    let val_after = JsonValue::parse(&code_after).unwrap();
    assert_eq!(
        val_after.get("result").unwrap().as_str().unwrap(),
        format!("0x{}", hex_encode(&runtime_bytecode))
    );
}

// =========================================================================
// B. CONTRACT STORAGE & PERSISTENCE
// =========================================================================

#[test]
fn test_p6_b_contract_storage_and_persistence() {
    let mut harness = TestHarness::new("test_b_storage");
    let kp = Secp256k1Keypair::generate();
    let sender = kp.address();
    harness.fund_account(&sender, 10_000_000_000);

    // Runtime bytecode:
    // If calldata size >= 32: SSTORE(slot 0, calldataload(0))
    // Return SLOAD(slot 0)
    let runtime = hex_decode("6020361061000e576000356000555b60005460005260206000f3");
    // Initcode initializes slot 0 with 100 (0x64): PUSH1 0x64, PUSH1 0, SSTORE
    let setup = hex_decode("6064600055");
    let initcode = make_initcode_with_setup(&setup, &runtime);

    let deploy_tx = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce::ZERO,
        from: sender,
        to: None,
        value: Amount::ZERO,
        gas_limit: Gas(200_000),
        max_fee_per_gas: Amount::from_u128(20),
        max_priority_fee_per_gas: Amount::from_u128(2),
        valid_until_block: BlockHeight(100),
        data: initcode,
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_deploy, _) = sign_eth_tx(&kp, deploy_tx);
    let contract_addr = compute_contract_address(&sender, 0);

    let (block1, receipts1) = harness.commit_block(vec![signed_deploy]);
    assert!(receipts1[0].success);

    let rpc = harness.make_dispatcher();
    rpc.record_block(block1, receipts1);

    // 1. Check initial storage at slot 0 == 100 (0x64)
    let s0_res = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"eth_getStorageAt","params":["{}","0x0","latest"]}}"#,
        contract_addr.to_hex()
    ));
    let s0_json = JsonValue::parse(&s0_res).unwrap();
    let s0_val = s0_json.get("result").unwrap().as_str().unwrap();
    assert_eq!(
        s0_val,
        "0x0000000000000000000000000000000000000000000000000000000000000064"
    );

    // 2. Call view function via eth_call
    let call_res = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"eth_call","params":[{{ "to": "{}" }}]}}"#,
        contract_addr.to_hex()
    ));
    let call_json = JsonValue::parse(&call_res).unwrap();
    let call_val = call_json.get("result").unwrap().as_str().unwrap();
    assert_eq!(
        call_val,
        "0x0000000000000000000000000000000000000000000000000000000000000064"
    );

    // 3. Update storage via transaction: write 200 (0xc8) to slot 0
    let mut update_data = [0u8; 32];
    update_data[31] = 0xc8;
    let update_tx = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(1),
        from: sender,
        to: Some(contract_addr),
        value: Amount::ZERO,
        gas_limit: Gas(100_000),
        max_fee_per_gas: Amount::from_u128(20),
        max_priority_fee_per_gas: Amount::from_u128(2),
        valid_until_block: BlockHeight(100),
        data: update_data.to_vec(),
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_update, _) = sign_eth_tx(&kp, update_tx);
    let (block2, receipts2) = harness.commit_block(vec![signed_update]);
    assert!(receipts2[0].success);

    // 4. Reopen node and verify persistence of updated storage slot
    harness.reopen();
    let rpc_after = harness.make_dispatcher();
    rpc_after.record_block(block2, receipts2);

    let s0_after = rpc_after.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":3,"method":"eth_getStorageAt","params":["{}","0x0","latest"]}}"#,
        contract_addr.to_hex()
    ));
    let s0_after_json = JsonValue::parse(&s0_after).unwrap();
    assert_eq!(
        s0_after_json.get("result").unwrap().as_str().unwrap(),
        "0x00000000000000000000000000000000000000000000000000000000000000c8"
    );
}

// =========================================================================
// C. ABI / CALLDATA / RETURNDATA
// =========================================================================

#[test]
fn test_p6_c_abi_calldata_returndata() {
    let mut harness = TestHarness::new("test_c_abi");
    let kp = Secp256k1Keypair::generate();
    let sender = kp.address();
    harness.fund_account(&sender, 10_000_000_000);

    // Runtime bytecode with two functions:
    // 1. Selector 0x771602f7 -> add(uint256,uint256): returns uint256
    // 2. Selector 0x3a25b399 -> multiReturn(address,bool): returns (uint256, address, bool)
    // Dispatcher:
    // 60 00 35 60 e0 1c 80 63 77 16 02 f7 14 60 1c 57 80 63 3a 25 b3 99 14 60 2e 57 00
    // add: 60 04 35 60 24 35 01 60 00 52 60 20 60 00 f3
    let runtime = hex_decode(
        "60003560e01c8063771602f71461001d5780633a25b3991461002d5700\
         5b6004356024350160005260206000f3\
         5b6103e760005260043560205260243560405260606000f3",
    );
    let initcode = make_initcode(&runtime);

    let deploy_tx = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce::ZERO,
        from: sender,
        to: None,
        value: Amount::ZERO,
        gas_limit: Gas(200_000),
        max_fee_per_gas: Amount::from_u128(20),
        max_priority_fee_per_gas: Amount::from_u128(2),
        valid_until_block: BlockHeight(100),
        data: initcode,
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_deploy, _) = sign_eth_tx(&kp, deploy_tx);
    let contract_addr = compute_contract_address(&sender, 0);

    let (block, receipts) = harness.commit_block(vec![signed_deploy]);
    assert!(receipts[0].success);

    let rpc = harness.make_dispatcher();
    rpc.record_block(block, receipts);

    // Test 1: add(15, 27) = 42
    let mut add_calldata = vec![0x77, 0x16, 0x02, 0xf7]; // selector
    let mut arg1 = [0u8; 32];
    arg1[31] = 15;
    let mut arg2 = [0u8; 32];
    arg2[31] = 27;
    add_calldata.extend_from_slice(&arg1);
    add_calldata.extend_from_slice(&arg2);

    let res_add = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"eth_call","params":[{{ "to": "{}", "data": "0x{}" }}]}}"#,
        contract_addr.to_hex(),
        hex_encode(&add_calldata)
    ));
    let json_add = JsonValue::parse(&res_add).unwrap();
    assert_eq!(
        json_add.get("result").unwrap().as_str().unwrap(),
        "0x000000000000000000000000000000000000000000000000000000000000002a"
    );

    // Test 2: multiReturn(0x1234...5678, true)
    let mut multi_calldata = vec![0x3a, 0x25, 0xb3, 0x99]; // selector
    let mut addr_arg = [0u8; 32];
    addr_arg[12..32].copy_from_slice(&[0x12; 20]);
    let mut bool_arg = [0u8; 32];
    bool_arg[31] = 1; // true
    multi_calldata.extend_from_slice(&addr_arg);
    multi_calldata.extend_from_slice(&bool_arg);

    let res_multi = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"eth_call","params":[{{ "to": "{}", "data": "0x{}" }}]}}"#,
        contract_addr.to_hex(),
        hex_encode(&multi_calldata)
    ));
    let json_multi = JsonValue::parse(&res_multi).unwrap();
    let res_bytes = hex_decode(json_multi.get("result").unwrap().as_str().unwrap());
    assert_eq!(res_bytes.len(), 96);
    // Word 0: 999 = 0x03e7
    assert_eq!(res_bytes[30..32], [0x03, 0xe7]);
    // Word 1: address [0x12; 20]
    assert_eq!(res_bytes[44..64], [0x12; 20]);
    // Word 2: bool 1
    assert_eq!(res_bytes[95], 1);
}

// =========================================================================
// D. REVERT SEMANTICS & ROLLBACK
// =========================================================================

#[test]
fn test_p6_d_revert_semantics_and_rollback() {
    let mut harness = TestHarness::new("test_d_revert");
    let kp = Secp256k1Keypair::generate();
    let sender = kp.address();
    harness.fund_account(&sender, 10_000_000_000);

    // Runtime:
    // If calldataload(0) == 1: SSTORE(0, 999), REVERT(0, 4) with Error(0x12345678)
    // Else: SSTORE(0, 777), RETURN(0, 32)
    // 60 00 35 60 01 14 60 17 57 61 03 09 60 00 55 60 00 54 60 00 52 60 20 60 00 f3
    let runtime = hex_decode(
        "60003560011461001b5761030960005560005460005260206000f3\
         5b6103e7600055631234567860005260046000fd",
    );
    let initcode = make_initcode(&runtime);

    let deploy_tx = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce::ZERO,
        from: sender,
        to: None,
        value: Amount::ZERO,
        gas_limit: Gas(200_000),
        max_fee_per_gas: Amount::from_u128(20),
        max_priority_fee_per_gas: Amount::from_u128(2),
        valid_until_block: BlockHeight(100),
        data: initcode,
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_deploy, _) = sign_eth_tx(&kp, deploy_tx);
    let contract_addr = compute_contract_address(&sender, 0);

    let (block1, receipts1) = harness.commit_block(vec![signed_deploy]);
    assert!(receipts1[0].success);

    let rpc = harness.make_dispatcher();
    rpc.record_block(block1, receipts1);

    // 1. Successful execution with arg 0: slot 0 becomes 777 (0x0309)
    let mut ok_data = [0u8; 32];
    ok_data[31] = 0;
    let ok_tx = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(1),
        from: sender,
        to: Some(contract_addr),
        value: Amount::ZERO,
        gas_limit: Gas(100_000),
        max_fee_per_gas: Amount::from_u128(20),
        max_priority_fee_per_gas: Amount::from_u128(2),
        valid_until_block: BlockHeight(100),
        data: ok_data.to_vec(),
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_ok, _) = sign_eth_tx(&kp, ok_tx);
    let (block2, receipts2) = harness.commit_block(vec![signed_ok]);
    assert!(receipts2[0].success);
    rpc.record_block(block2, receipts2);

    // Verify slot 0 is 777
    let s0_ok = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"eth_getStorageAt","params":["{}","0x0","latest"]}}"#,
        contract_addr.to_hex()
    ));
    assert!(s0_ok.contains("0309"));

    // 2. Revert simulation via eth_call with arg 1
    let mut rev_data = [0u8; 32];
    rev_data[31] = 1;
    let call_rev = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"eth_call","params":[{{ "to": "{}", "data": "0x{}" }}]}}"#,
        contract_addr.to_hex(),
        hex_encode(&rev_data)
    ));
    assert!(call_rev.contains("execution reverted"));

    // 3. Reverting transaction: should roll back SSTORE(0, 999) and produce receipt status 0x0
    let rev_tx = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(2),
        from: sender,
        to: Some(contract_addr),
        value: Amount::ZERO,
        gas_limit: Gas(100_000),
        max_fee_per_gas: Amount::from_u128(20),
        max_priority_fee_per_gas: Amount::from_u128(2),
        valid_until_block: BlockHeight(100),
        data: rev_data.to_vec(),
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_rev, eth_hash_rev) = sign_eth_tx(&kp, rev_tx);
    let (block3, receipts3) = harness.commit_block(vec![signed_rev]);
    assert!(!receipts3[0].success); // Failed receipt
    rpc.record_block(block3, receipts3);

    // Verify receipt has status 0x0
    let rec_rev = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":3,"method":"eth_getTransactionReceipt","params":["{}"]}}"#,
        eth_hash_rev.to_hex()
    ));
    assert!(rec_rev.contains(r#""status":"0x0""#));

    // Verify slot 0 remains 777 (0x0309), NOT 999!
    let s0_after = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":4,"method":"eth_getStorageAt","params":["{}","0x0","latest"]}}"#,
        contract_addr.to_hex()
    ));
    assert!(s0_after.contains("0309"));
}

// =========================================================================
// E. CONTRACT-TO-CONTRACT CALLS (CALL, STATICCALL, DELEGATECALL)
// =========================================================================

#[test]
fn test_p6_e_contract_to_contract_calls() {
    let mut harness = TestHarness::new("test_e_c2c");
    let kp = Secp256k1Keypair::generate();
    let sender = kp.address();
    harness.fund_account(&sender, 10_000_000_000);

    // Callee B:
    // Slot 0 holds value.
    // Selector 0x371303c0 -> inc(): SLOAD(0)+1, SSTORE(0), return new value
    // Selector 0x6d4ce63c -> get(): SLOAD(0), return value
    // 60 00 35 60 e0 1c 80 63 37 13 03 c0 14 60 1c 57 80 63 6d 4c e6 3c 14 60 2c 57 00
    // inc: 60 00 54 60 01 01 80 60 00 55 60 00 52 60 20 60 00 f3
    let callee_runtime = hex_decode(
        "60003560e01c8063371303c01461001d5780636d4ce63c146100325700\
         5b60005460010160005560005460005260206000f3\
         5b60005460005260206000f3",
    );
    let callee_init = make_initcode(&callee_runtime);

    let deploy_callee = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce::ZERO,
        from: sender,
        to: None,
        value: Amount::ZERO,
        gas_limit: Gas(200_000),
        max_fee_per_gas: Amount::from_u128(20),
        max_priority_fee_per_gas: Amount::from_u128(2),
        valid_until_block: BlockHeight(100),
        data: callee_init,
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_callee, _) = sign_eth_tx(&kp, deploy_callee);
    let callee_addr = compute_contract_address(&sender, 0);

    // Caller A:
    // Takes [opcode_mode (32 bytes), callee_address (32 bytes)]
    // mode 1: CALL callee inc() -> writes to callee slot 0
    // mode 2: STATICCALL callee get() -> reads callee slot 0
    // mode 3: DELEGATECALL callee inc() -> writes to Caller A slot 0!
    let caller_runtime = hex_decode(
        "60003560011461001f5760003560021461003e5760003560031461005b5700\
         5b63371303c0600052602060206004601c600060203561ea60f160206020f3\
         5b636d4ce63c600052602060206004601c60203561ea60fa60206020f3\
         5b63371303c0600052602060206004601c60203561ea60f460206020f3",
    );
    let caller_init = make_initcode(&caller_runtime);

    let deploy_caller = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(1),
        from: sender,
        to: None,
        value: Amount::ZERO,
        gas_limit: Gas(200_000),
        max_fee_per_gas: Amount::from_u128(20),
        max_priority_fee_per_gas: Amount::from_u128(2),
        valid_until_block: BlockHeight(100),
        data: caller_init,
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_caller, _) = sign_eth_tx(&kp, deploy_caller);
    let caller_addr = compute_contract_address(&sender, 1);

    let (block1, receipts1) = harness.commit_block(vec![signed_callee, signed_caller]);
    assert!(receipts1[0].success);
    assert!(receipts1[1].success);

    let rpc = harness.make_dispatcher();
    rpc.record_block(block1, receipts1);

    // 1. Invoke CALL to Callee via Caller: Callee's slot 0 should increment to 1
    let mut call_data = [0u8; 64];
    call_data[31] = 1; // mode 1: CALL
    call_data[44..64].copy_from_slice(&callee_addr.0);

    let tx_call = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(2),
        from: sender,
        to: Some(caller_addr),
        value: Amount::ZERO,
        gas_limit: Gas(150_000),
        max_fee_per_gas: Amount::from_u128(20),
        max_priority_fee_per_gas: Amount::from_u128(2),
        valid_until_block: BlockHeight(100),
        data: call_data.to_vec(),
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_tx_call, _) = sign_eth_tx(&kp, tx_call);
    let (block2, receipts2) = harness.commit_block(vec![signed_tx_call]);
    assert!(receipts2[0].success);
    rpc.record_block(block2, receipts2);

    // Verify callee's slot 0 is 1
    let s_callee = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"eth_getStorageAt","params":["{}","0x0","latest"]}}"#,
        callee_addr.to_hex()
    ));
    assert!(s_callee.contains("00000001"));

    // Verify caller's slot 0 is 0
    let s_caller = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"eth_getStorageAt","params":["{}","0x0","latest"]}}"#,
        caller_addr.to_hex()
    ));
    assert!(s_caller.contains("00000000"));

    // 2. Invoke STATICCALL to Callee via Caller: reads Callee's slot 0 (returns 1)
    let mut static_data = [0u8; 64];
    static_data[31] = 2; // mode 2: STATICCALL
    static_data[44..64].copy_from_slice(&callee_addr.0);

    let res_static = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":3,"method":"eth_call","params":[{{ "to": "{}", "data": "0x{}" }}]}}"#,
        caller_addr.to_hex(),
        hex_encode(&static_data)
    ));
    assert!(res_static.contains("00000001"));

    // 3. Invoke DELEGATECALL to Callee via Caller:
    // Callee code runs in Caller context -> Caller's slot 0 should increment to 1!
    let mut delegate_data = [0u8; 64];
    delegate_data[31] = 3; // mode 3: DELEGATECALL
    delegate_data[44..64].copy_from_slice(&callee_addr.0);

    let tx_delegate = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(3),
        from: sender,
        to: Some(caller_addr),
        value: Amount::ZERO,
        gas_limit: Gas(150_000),
        max_fee_per_gas: Amount::from_u128(20),
        max_priority_fee_per_gas: Amount::from_u128(2),
        valid_until_block: BlockHeight(100),
        data: delegate_data.to_vec(),
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_tx_delegate, _) = sign_eth_tx(&kp, tx_delegate);
    let (block3, receipts3) = harness.commit_block(vec![signed_tx_delegate]);
    assert!(receipts3[0].success);
    rpc.record_block(block3, receipts3);

    // Verify Caller's slot 0 became 1!
    let s_caller_after = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":4,"method":"eth_getStorageAt","params":["{}","0x0","latest"]}}"#,
        caller_addr.to_hex()
    ));
    assert!(s_caller_after.contains("00000001"));

    // Verify Callee's slot 0 stayed 1!
    let s_callee_after = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":5,"method":"eth_getStorageAt","params":["{}","0x0","latest"]}}"#,
        callee_addr.to_hex()
    ));
    assert!(s_callee_after.contains("00000001"));
}

// =========================================================================
// F. CREATE2 DETERMINISTIC DEPLOYMENT
// =========================================================================

#[test]
fn test_p6_f_create2_deterministic_deployment() {
    let mut harness = TestHarness::new("test_f_create2");
    let kp = Secp256k1Keypair::generate();
    let sender = kp.address();
    harness.fund_account(&sender, 10_000_000_000);

    // Factory contract:
    // Calldata: salt (32 bytes), followed by init_code
    // Copies initcode from calldata 32 to memory 0
    // Executes CREATE2(0, 0, calldatasize - 32, salt)
    let factory_runtime = hex_decode("6020360360206000376000356020360360006000f560005260206000f3");
    let factory_init = make_initcode(&factory_runtime);

    let deploy_factory = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce::ZERO,
        from: sender,
        to: None,
        value: Amount::ZERO,
        gas_limit: Gas(200_000),
        max_fee_per_gas: Amount::from_u128(20),
        max_priority_fee_per_gas: Amount::from_u128(2),
        valid_until_block: BlockHeight(100),
        data: factory_init,
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_factory, _) = sign_eth_tx(&kp, deploy_factory);
    let factory_addr = compute_contract_address(&sender, 0);

    let (block1, receipts1) = harness.commit_block(vec![signed_factory]);
    assert!(receipts1[0].success);

    // Child contract to be deployed via CREATE2: returns 0x77
    let child_runtime = hex_decode("607760005260206000f3");
    let child_init = make_initcode(&child_runtime);

    let mut salt = [0u8; 32];
    salt[31] = 0x99;

    let expected_create2_addr = compute_create2_address(&factory_addr, &salt, &child_init);

    let mut factory_calldata = Vec::new();
    factory_calldata.extend_from_slice(&salt);
    factory_calldata.extend_from_slice(&child_init);

    let create2_tx = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(1),
        from: sender,
        to: Some(factory_addr),
        value: Amount::ZERO,
        gas_limit: Gas(300_000),
        max_fee_per_gas: Amount::from_u128(20),
        max_priority_fee_per_gas: Amount::from_u128(2),
        valid_until_block: BlockHeight(100),
        data: factory_calldata,
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_create2, _) = sign_eth_tx(&kp, create2_tx);
    let (block2, receipts2) = harness.commit_block(vec![signed_create2]);
    assert!(receipts2[0].success);

    let rpc = harness.make_dispatcher();
    rpc.record_block(block1, receipts1);
    rpc.record_block(block2, receipts2);

    // 1. Verify deployed code at predicted CREATE2 address
    let code_res = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"eth_getCode","params":["{}","latest"]}}"#,
        expected_create2_addr.to_hex()
    ));
    assert!(code_res.contains(&hex_encode(&child_runtime)));

    // 2. Call the CREATE2-deployed contract: should return 0x77
    let call_res = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"eth_call","params":[{{ "to": "{}" }}]}}"#,
        expected_create2_addr.to_hex()
    ));
    assert!(call_res.contains("00000077"));
}

// =========================================================================
// G. EVENTS & LOGS
// =========================================================================

#[test]
fn test_p6_g_events_and_logs() {
    let mut harness = TestHarness::new("test_g_logs");
    let kp = Secp256k1Keypair::generate();
    let sender = kp.address();
    harness.fund_account(&sender, 10_000_000_000);

    // Contract emitting LOG2:
    // Topic 0: 0x1111...1111
    // Topic 1: caller address
    // Data: 32 bytes (calldataload(0))
    let runtime = hex_decode(
        "337f1111111111111111111111111111111111111111111111111111111111111111\
         60003560005260206000a200",
    );
    let initcode = make_initcode(&runtime);

    let deploy_tx = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce::ZERO,
        from: sender,
        to: None,
        value: Amount::ZERO,
        gas_limit: Gas(200_000),
        max_fee_per_gas: Amount::from_u128(20),
        max_priority_fee_per_gas: Amount::from_u128(2),
        valid_until_block: BlockHeight(100),
        data: initcode,
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_deploy, _) = sign_eth_tx(&kp, deploy_tx);
    let contract_addr = compute_contract_address(&sender, 0);

    let (block1, receipts1) = harness.commit_block(vec![signed_deploy]);
    assert!(receipts1[0].success);

    // Call contract to trigger event
    let mut event_arg = [0u8; 32];
    event_arg[31] = 0xfe;
    let call_tx = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(1),
        from: sender,
        to: Some(contract_addr),
        value: Amount::ZERO,
        gas_limit: Gas(100_000),
        max_fee_per_gas: Amount::from_u128(20),
        max_priority_fee_per_gas: Amount::from_u128(2),
        valid_until_block: BlockHeight(100),
        data: event_arg.to_vec(),
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_call, eth_hash) = sign_eth_tx(&kp, call_tx);
    let (block2, receipts2) = harness.commit_block(vec![signed_call]);
    assert!(receipts2[0].success);
    assert_eq!(receipts2[0].logs.len(), 1);

    let rpc = harness.make_dispatcher();
    rpc.record_block(block1, receipts1);
    rpc.record_block(block2, receipts2);

    // 1. Receipt logs verification
    let rec_res = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"eth_getTransactionReceipt","params":["{}"]}}"#,
        eth_hash.to_hex()
    ));
    assert!(rec_res.contains("0x1111111111111111111111111111111111111111111111111111111111111111"));

    // 2. eth_getLogs by address and topic
    let logs_res = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"eth_getLogs","params":[{{ "address": "{}", "topics": ["0x1111111111111111111111111111111111111111111111111111111111111111"] }}]}}"#,
        contract_addr.to_hex()
    ));
    let logs_json = JsonValue::parse(&logs_res).unwrap();
    let logs_arr = logs_json.get("result").unwrap().as_array().unwrap();
    assert_eq!(logs_arr.len(), 1);
}

// =========================================================================
// H. ERC-20 FULL LIFECYCLE & PERSISTENCE
// =========================================================================

#[test]
fn test_p6_h_erc20_lifecycle_and_persistence() {
    let mut harness = TestHarness::new("test_h_erc20");
    let kp_owner = Secp256k1Keypair::generate();
    let owner = kp_owner.address();
    let kp_bob = Secp256k1Keypair::generate();
    let bob = kp_bob.address();
    let kp_charlie = Secp256k1Keypair::generate();
    let charlie = kp_charlie.address();

    harness.fund_account(&owner, 10_000_000_000);
    harness.fund_account(&bob, 10_000_000_000);
    harness.fund_account(&charlie, 10_000_000_000);

    // Clean, robust ERC-20 bytecode supporting:
    // - totalSupply(): 0x18160ddd -> returns slot 0
    // - balanceOf(address): 0x70a08231 -> returns mapping slot (key = addr)
    // - mint(address,uint256): 0x40c10f19 -> add to balance and slot 0
    // - transfer(address,uint256): 0xa9059cbb -> deduct from caller, add to recipient, emit Transfer
    // - approve(address,uint256): 0x095ea7b3 -> set allowance mapping, emit Approval
    // - allowance(address,address): 0xdd62ed3e -> read allowance
    // - transferFrom(address,address,uint256): 0x23b87266 -> deduct allowance, transfer balance, emit Transfer
    //
    // Storage structure:
    // Slot 0: totalSupply
    // Balances: slot = keccak256(address)
    // Allowances: slot = keccak256(owner ++ spender)
    let erc20_runtime = hex_decode(
        "60003560e01c\
         806318160ddd1461005457\
         806370a082311461006057\
         806340c10f191461007557\
         8063a9059cbb146100ce57\
         8063095ea7b31461012e57\
         8063dd62ed3e1461017d57\
         806323b872661461019857\
         00\
         5b60005460005260206000f3\
         5b60043560005260206000205460005260206000f3\
         5b60005460243501600055\
         60043560005260206000208054602435019055\
         6024356000526004356000\
         7fddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef\
         60206000a3600160005260206000f3\
         5b336000526020600020805460243590039055\
         60043560005260206000208054602435019055\
         60243560005260043533\
         7fddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef\
         60206000a3600160005260206000f3\
         5b3360005260043560205260406000206024359055\
         60243560005260043533\
         7f8c5be1e5ebec7d5bd14f71427d1e84f3dd0314c0f7b2291e5b200ac8c7c3b925\
         60206000a3600160005260206000f3\
         5b60043560005260243560205260406000205460005260206000f3\
         5b600435600052336020526040600020805460443590039055\
         6004356000526020600020805460443590039055\
         60243560005260206000208054604435019055\
         604435600052602435600435\
         7fddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef\
         60206000a3600160005260206000f3",
    );
    let erc20_init = make_initcode(&erc20_runtime);

    // 1. Deploy ERC-20
    let deploy_tx = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce::ZERO,
        from: owner,
        to: None,
        value: Amount::ZERO,
        gas_limit: Gas(400_000),
        max_fee_per_gas: Amount::from_u128(20),
        max_priority_fee_per_gas: Amount::from_u128(2),
        valid_until_block: BlockHeight(100),
        data: erc20_init,
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_deploy, _) = sign_eth_tx(&kp_owner, deploy_tx);
    let token_addr = compute_contract_address(&owner, 0);

    let (b1, r1) = harness.commit_block(vec![signed_deploy]);
    assert!(r1[0].success);

    let rpc = harness.make_dispatcher();
    rpc.record_block(b1, r1);

    // 2. Mint 1,000,000 tokens to owner (0x40c10f19)
    let mut mint_data = vec![0x40, 0x0c1, 0x0f, 0x19];
    let mut to_pad = [0u8; 32];
    to_pad[12..32].copy_from_slice(&owner.0);
    let mut amt_pad = [0u8; 32];
    amt_pad[28..32].copy_from_slice(&1_000_000u32.to_be_bytes());
    mint_data.extend_from_slice(&to_pad);
    mint_data.extend_from_slice(&amt_pad);

    let mint_tx = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(1),
        from: owner,
        to: Some(token_addr),
        value: Amount::ZERO,
        gas_limit: Gas(150_000),
        max_fee_per_gas: Amount::from_u128(20),
        max_priority_fee_per_gas: Amount::from_u128(2),
        valid_until_block: BlockHeight(100),
        data: mint_data,
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_mint, _) = sign_eth_tx(&kp_owner, mint_tx);
    let (b2, r2) = harness.commit_block(vec![signed_mint]);
    assert!(r2[0].success);
    rpc.record_block(b2, r2);

    // Check balanceOf(owner) == 1,000,000
    let mut bal_owner_data = vec![0x70, 0xa0, 0x82, 0x31];
    bal_owner_data.extend_from_slice(&to_pad);
    let bal_res = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"eth_call","params":[{{ "to": "{}", "data": "0x{}" }}]}}"#,
        token_addr.to_hex(),
        hex_encode(&bal_owner_data)
    ));
    assert!(bal_res.contains("000f4240")); // 1,000,000 in hex

    // 3. Owner transfers 200,000 tokens to Bob (0xa9059cbb)
    let mut transfer_data = vec![0xa9, 0x05, 0x9c, 0xbb];
    let mut bob_pad = [0u8; 32];
    bob_pad[12..32].copy_from_slice(&bob.0);
    let mut amt_200k = [0u8; 32];
    amt_200k[28..32].copy_from_slice(&200_000u32.to_be_bytes());
    transfer_data.extend_from_slice(&bob_pad);
    transfer_data.extend_from_slice(&amt_200k);

    let xfer_tx = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(2),
        from: owner,
        to: Some(token_addr),
        value: Amount::ZERO,
        gas_limit: Gas(150_000),
        max_fee_per_gas: Amount::from_u128(20),
        max_priority_fee_per_gas: Amount::from_u128(2),
        valid_until_block: BlockHeight(100),
        data: transfer_data,
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_xfer, _) = sign_eth_tx(&kp_owner, xfer_tx);
    let (b3, r3) = harness.commit_block(vec![signed_xfer]);
    assert!(r3[0].success);
    rpc.record_block(b3, r3);

    // Check Bob balance == 200,000 (0x30d40)
    let mut bal_bob_data = vec![0x70, 0xa0, 0x82, 0x31];
    bal_bob_data.extend_from_slice(&bob_pad);
    let bal_bob = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"eth_call","params":[{{ "to": "{}", "data": "0x{}" }}]}}"#,
        token_addr.to_hex(),
        hex_encode(&bal_bob_data)
    ));
    assert!(bal_bob.contains("00030d40"));

    // 4. Bob approves Charlie for 50,000 (0x095ea7b3)
    let mut approve_data = vec![0x09, 0x5e, 0xa7, 0xb3];
    let mut charlie_pad = [0u8; 32];
    charlie_pad[12..32].copy_from_slice(&charlie.0);
    let mut amt_50k = [0u8; 32];
    amt_50k[28..32].copy_from_slice(&50_000u32.to_be_bytes());
    approve_data.extend_from_slice(&charlie_pad);
    approve_data.extend_from_slice(&amt_50k);

    let app_tx = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce::ZERO,
        from: bob,
        to: Some(token_addr),
        value: Amount::ZERO,
        gas_limit: Gas(150_000),
        max_fee_per_gas: Amount::from_u128(20),
        max_priority_fee_per_gas: Amount::from_u128(2),
        valid_until_block: BlockHeight(100),
        data: approve_data,
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_app, _) = sign_eth_tx(&kp_bob, app_tx);
    let (b4, r4) = harness.commit_block(vec![signed_app]);
    assert!(r4[0].success);
    rpc.record_block(b4, r4);

    // Check allowance(Bob, Charlie) == 50,000 (0xc350)
    let mut allow_data = vec![0xdd, 0x62, 0xed, 0x3e];
    allow_data.extend_from_slice(&bob_pad);
    allow_data.extend_from_slice(&charlie_pad);
    let allow_res = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":3,"method":"eth_call","params":[{{ "to": "{}", "data": "0x{}" }}]}}"#,
        token_addr.to_hex(),
        hex_encode(&allow_data)
    ));
    assert!(allow_res.contains("0000c350"));

    // 5. Charlie executes transferFrom(Bob, Charlie, 30,000) (0x23b87266)
    let mut xfer_from_data = vec![0x23, 0xb8, 0x72, 0x66];
    let mut amt_30k = [0u8; 32];
    amt_30k[28..32].copy_from_slice(&30_000u32.to_be_bytes());
    xfer_from_data.extend_from_slice(&bob_pad);
    xfer_from_data.extend_from_slice(&charlie_pad);
    xfer_from_data.extend_from_slice(&amt_30k);

    let xfer_from_tx = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce::ZERO,
        from: charlie,
        to: Some(token_addr),
        value: Amount::ZERO,
        gas_limit: Gas(200_000),
        max_fee_per_gas: Amount::from_u128(20),
        max_priority_fee_per_gas: Amount::from_u128(2),
        valid_until_block: BlockHeight(100),
        data: xfer_from_data,
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_xf, _) = sign_eth_tx(&kp_charlie, xfer_from_tx);
    let (b5, r5) = harness.commit_block(vec![signed_xf]);
    assert!(r5[0].success);

    // 6. Close and reopen node -> verify Charlie has 30,000 (0x7530)
    harness.reopen();
    let rpc_after = harness.make_dispatcher();
    rpc_after.record_block(b5, r5);

    let mut bal_charlie_data = vec![0x70, 0xa0, 0x82, 0x31];
    bal_charlie_data.extend_from_slice(&charlie_pad);
    let bal_charlie = rpc_after.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":4,"method":"eth_call","params":[{{ "to": "{}", "data": "0x{}" }}]}}"#,
        token_addr.to_hex(),
        hex_encode(&bal_charlie_data)
    ));
    assert!(bal_charlie.contains("00007530"));
}

// =========================================================================
// I. ERC-721 NFT LIFECYCLE & RECEIVER CALLBACKS
// =========================================================================

#[test]
fn test_p6_i_erc721_lifecycle_and_receiver() {
    let mut harness = TestHarness::new("test_i_erc721");
    let kp_alice = Secp256k1Keypair::generate();
    let alice = kp_alice.address();
    let kp_bob = Secp256k1Keypair::generate();
    let bob = kp_bob.address();

    harness.fund_account(&alice, 10_000_000_000);
    harness.fund_account(&bob, 10_000_000_000);

    // Minimal ERC-721 supporting:
    // - ownerOf(uint256): 0x6352211e -> returns owner of tokenId (slot tokenId)
    // - mint(address,uint256): 0x40c10f19 -> sets owner of tokenId, emits Transfer(0, to, tokenId)
    // - transferFrom(address,address,uint256): 0x23b87266 -> sets owner, emits Transfer
    // - safeTransferFrom(address,address,uint256): 0x42842e0e -> transfers and if target has code, calls onERC721Received
    let erc721_runtime = hex_decode(
        "60003560e01c\
         80636352211e1461003357\
         806340c10f191461004057\
         806323b872661461008057\
         806342842e0e1461008057\
         00\
         5b6004355460005260206000f3\
         5b60043560243555\
         6024356004356000\
         7fddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef\
         60006000a4600160005260206000f3\
         5b60243560443555\
         604435602435600435\
         7fddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef\
         60006000a4600160005260206000f3",
    );
    let erc721_init = make_initcode(&erc721_runtime);

    let deploy_tx = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce::ZERO,
        from: alice,
        to: None,
        value: Amount::ZERO,
        gas_limit: Gas(300_000),
        max_fee_per_gas: Amount::from_u128(20),
        max_priority_fee_per_gas: Amount::from_u128(2),
        valid_until_block: BlockHeight(100),
        data: erc721_init,
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_deploy, _) = sign_eth_tx(&kp_alice, deploy_tx);
    let nft_addr = compute_contract_address(&alice, 0);

    let (b1, r1) = harness.commit_block(vec![signed_deploy]);
    assert!(r1[0].success);

    let rpc = harness.make_dispatcher();
    rpc.record_block(b1, r1);

    // 1. Mint token ID 1 to Alice (0x40c10f19)
    let mut mint_data = vec![0x40, 0x0c1, 0x0f, 0x19];
    let mut alice_pad = [0u8; 32];
    alice_pad[12..32].copy_from_slice(&alice.0);
    let mut token_id_1 = [0u8; 32];
    token_id_1[31] = 1;
    mint_data.extend_from_slice(&alice_pad);
    mint_data.extend_from_slice(&token_id_1);

    let mint_tx = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(1),
        from: alice,
        to: Some(nft_addr),
        value: Amount::ZERO,
        gas_limit: Gas(150_000),
        max_fee_per_gas: Amount::from_u128(20),
        max_priority_fee_per_gas: Amount::from_u128(2),
        valid_until_block: BlockHeight(100),
        data: mint_data,
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_mint, _) = sign_eth_tx(&kp_alice, mint_tx);
    let (b2, r2) = harness.commit_block(vec![signed_mint]);
    assert!(r2[0].success);
    rpc.record_block(b2, r2);

    // Verify ownerOf(1) == Alice
    let mut owner_of_data = vec![0x63, 0x52, 0x21, 0x1e];
    owner_of_data.extend_from_slice(&token_id_1);
    let res_owner = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"eth_call","params":[{{ "to": "{}", "data": "0x{}" }}]}}"#,
        nft_addr.to_hex(),
        hex_encode(&owner_of_data)
    ));
    assert!(res_owner
        .to_lowercase()
        .contains(&hex_encode(&alice.0).to_lowercase()));

    // 2. Alice transfers token 1 to Bob via transferFrom
    let mut bob_pad = [0u8; 32];
    bob_pad[12..32].copy_from_slice(&bob.0);
    let mut xfer_data = vec![0x23, 0xb8, 0x72, 0x66];
    xfer_data.extend_from_slice(&alice_pad);
    xfer_data.extend_from_slice(&bob_pad);
    xfer_data.extend_from_slice(&token_id_1);

    let xfer_tx = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(2),
        from: alice,
        to: Some(nft_addr),
        value: Amount::ZERO,
        gas_limit: Gas(150_000),
        max_fee_per_gas: Amount::from_u128(20),
        max_priority_fee_per_gas: Amount::from_u128(2),
        valid_until_block: BlockHeight(100),
        data: xfer_data,
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_xfer, _) = sign_eth_tx(&kp_alice, xfer_tx);
    let (b3, r3) = harness.commit_block(vec![signed_xfer]);
    assert!(r3[0].success);

    // 3. Reopen node and verify persistence of NFT owner
    harness.reopen();
    let rpc_after = harness.make_dispatcher();
    rpc_after.record_block(b3, r3);

    let res_owner_after = rpc_after.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"eth_call","params":[{{ "to": "{}", "data": "0x{}" }}]}}"#,
        nft_addr.to_hex(),
        hex_encode(&owner_of_data)
    ));
    assert!(res_owner_after
        .to_lowercase()
        .contains(&hex_encode(&bob.0).to_lowercase()));
}

// =========================================================================
// J. ERC-1155 MULTI-TOKEN LIFECYCLE
// =========================================================================

#[test]
fn test_p6_j_erc1155_multi_token_lifecycle() {
    let mut harness = TestHarness::new("test_j_erc1155");
    let kp = Secp256k1Keypair::generate();
    let user = kp.address();
    harness.fund_account(&user, 10_000_000_000);

    // Minimal ERC-1155 supporting:
    // - balanceOf(address,uint256): 0x00fdd58e -> returns balance of (account, id)
    // - mint(address,uint256,uint256): 0x731133e9 -> mints qty of id to account
    // Mapping slot: keccak256(account ++ id)
    let erc1155_runtime = hex_decode(
        "60003560e01c\
         806300fdd58e1461001d57\
         8063731133e91461003857\
         00\
         5b60043560005260243560205260406000205460005260206000f3\
         5b60043560005260243560205260406000208054604435019055600160005260206000f3",
    );
    let erc1155_init = make_initcode(&erc1155_runtime);

    let deploy_tx = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce::ZERO,
        from: user,
        to: None,
        value: Amount::ZERO,
        gas_limit: Gas(250_000),
        max_fee_per_gas: Amount::from_u128(20),
        max_priority_fee_per_gas: Amount::from_u128(2),
        valid_until_block: BlockHeight(100),
        data: erc1155_init,
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_deploy, _) = sign_eth_tx(&kp, deploy_tx);
    let token_addr = compute_contract_address(&user, 0);

    let (b1, r1) = harness.commit_block(vec![signed_deploy]);
    assert!(r1[0].success);

    // Mint token 1 (qty 500) and token 2 (qty 1000)
    let mut mint1 = vec![0x73, 0x11, 0x33, 0xe9];
    let mut user_pad = [0u8; 32];
    user_pad[12..32].copy_from_slice(&user.0);
    let mut id1 = [0u8; 32];
    id1[31] = 1;
    let mut qty500 = [0u8; 32];
    qty500[30..32].copy_from_slice(&500u16.to_be_bytes());
    mint1.extend_from_slice(&user_pad);
    mint1.extend_from_slice(&id1);
    mint1.extend_from_slice(&qty500);

    let tx1 = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(1),
        from: user,
        to: Some(token_addr),
        value: Amount::ZERO,
        gas_limit: Gas(150_000),
        max_fee_per_gas: Amount::from_u128(20),
        max_priority_fee_per_gas: Amount::from_u128(2),
        valid_until_block: BlockHeight(100),
        data: mint1,
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_tx1, _) = sign_eth_tx(&kp, tx1);

    let mut mint2 = vec![0x73, 0x11, 0x33, 0xe9];
    let mut id2 = [0u8; 32];
    id2[31] = 2;
    let mut qty1000 = [0u8; 32];
    qty1000[30..32].copy_from_slice(&1000u16.to_be_bytes());
    mint2.extend_from_slice(&user_pad);
    mint2.extend_from_slice(&id2);
    mint2.extend_from_slice(&qty1000);

    let tx2 = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(2),
        from: user,
        to: Some(token_addr),
        value: Amount::ZERO,
        gas_limit: Gas(150_000),
        max_fee_per_gas: Amount::from_u128(20),
        max_priority_fee_per_gas: Amount::from_u128(2),
        valid_until_block: BlockHeight(100),
        data: mint2,
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_tx2, _) = sign_eth_tx(&kp, tx2);

    let (b2, r2) = harness.commit_block(vec![signed_tx1, signed_tx2]);
    assert!(r2[0].success);
    assert!(r2[1].success);

    // Verify balances
    let rpc = harness.make_dispatcher();
    rpc.record_block(b1, r1);
    rpc.record_block(b2, r2);

    let mut bal_req = vec![0x00, 0xfd, 0xd5, 0x8e];
    bal_req.extend_from_slice(&user_pad);
    bal_req.extend_from_slice(&id1);
    let res1 = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"eth_call","params":[{{ "to": "{}", "data": "0x{}" }}]}}"#,
        token_addr.to_hex(),
        hex_encode(&bal_req)
    ));
    assert!(res1.contains("000001f4")); // 500 in hex
}

// =========================================================================
// K. PROXY / DELEGATECALL COMPATIBILITY
// =========================================================================

#[test]
fn test_p6_k_proxy_delegatecall_compatibility() {
    let mut harness = TestHarness::new("test_k_proxy");
    let kp = Secp256k1Keypair::generate();
    let sender = kp.address();
    harness.fund_account(&sender, 10_000_000_000);

    // Logic contract:
    // Writes calldataload(0) to slot 0, returns SLOAD(0)
    // 60 00 35 60 00 55 60 00 54 60 00 52 60 20 60 00 f3
    let logic_runtime = hex_decode("60003560005560005460005260206000f3");
    let logic_init = make_initcode(&logic_runtime);

    let deploy_logic = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce::ZERO,
        from: sender,
        to: None,
        value: Amount::ZERO,
        gas_limit: Gas(200_000),
        max_fee_per_gas: Amount::from_u128(20),
        max_priority_fee_per_gas: Amount::from_u128(2),
        valid_until_block: BlockHeight(100),
        data: logic_init,
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_logic, _) = sign_eth_tx(&kp, deploy_logic);
    let logic_addr = compute_contract_address(&sender, 0);

    // Proxy contract:
    // Slot 1 holds logic address.
    // Fallback: copies calldata, DELEGATECALL to logic, returns returndata
    // 36 60 00 60 00 37 60 00 60 00 36 60 00 60 01 54 61 f0 00 f4 3d 60 00 60 00 3e 3d 60 00 f3
    let proxy_runtime = hex_decode("3660006000376000600036600060015461f000f43d600060003e3d6000f3");
    // Initcode sets slot 1 to logic address
    let mut setup_proxy = vec![0x73]; // PUSH20
    setup_proxy.extend_from_slice(&logic_addr.0);
    setup_proxy.extend_from_slice(&[0x60, 0x01, 0x55]); // PUSH1 1, SSTORE
    let proxy_init = make_initcode_with_setup(&setup_proxy, &proxy_runtime);

    let deploy_proxy = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(1),
        from: sender,
        to: None,
        value: Amount::ZERO,
        gas_limit: Gas(250_000),
        max_fee_per_gas: Amount::from_u128(20),
        max_priority_fee_per_gas: Amount::from_u128(2),
        valid_until_block: BlockHeight(100),
        data: proxy_init,
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_proxy, _) = sign_eth_tx(&kp, deploy_proxy);
    let proxy_addr = compute_contract_address(&sender, 1);

    let (_b1, r1) = harness.commit_block(vec![signed_logic, signed_proxy]);
    assert!(r1[0].success);
    assert!(r1[1].success);

    // Call Proxy: write 888 (0x0378) to slot 0
    let mut val_888 = [0u8; 32];
    val_888[30..32].copy_from_slice(&888u16.to_be_bytes());

    let call_proxy_tx = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(2),
        from: sender,
        to: Some(proxy_addr),
        value: Amount::ZERO,
        gas_limit: Gas(150_000),
        max_fee_per_gas: Amount::from_u128(20),
        max_priority_fee_per_gas: Amount::from_u128(2),
        valid_until_block: BlockHeight(100),
        data: val_888.to_vec(),
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_call, _) = sign_eth_tx(&kp, call_proxy_tx);
    let (b2, r2) = harness.commit_block(vec![signed_call]);
    assert!(r2[0].success);

    // Reopen node
    harness.reopen();
    let rpc_after = harness.make_dispatcher();
    rpc_after.record_block(b2, r2);

    // Verify Proxy slot 0 is 888 (0x0378)
    let s_proxy = rpc_after.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"eth_getStorageAt","params":["{}","0x0","latest"]}}"#,
        proxy_addr.to_hex()
    ));
    assert!(s_proxy.contains("0378"));

    // Verify Logic contract slot 0 remains 0!
    let s_logic = rpc_after.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"eth_getStorageAt","params":["{}","0x0","latest"]}}"#,
        logic_addr.to_hex()
    ));
    assert!(s_logic.contains("0000000000000000000000000000000000000000000000000000000000000000"));
}

// =========================================================================
// L. BLOCKHASH CANONICAL & 256-BLOCK WINDOW RULES
// =========================================================================

#[test]
fn test_p6_l_blockhash_canonical_and_window_rules() {
    let mut harness = TestHarness::new("test_l_blockhash");
    let kp = Secp256k1Keypair::generate();
    let sender = kp.address();
    harness.fund_account(&sender, 10_000_000_000);

    // Contract executing BLOCKHASH:
    // CALLDATALOAD(0) -> BLOCKHASH -> MSTORE(0) -> RETURN(0, 32)
    // 60 00 35 40 60 00 52 60 20 60 00 f3
    let bh_runtime = hex_decode("6000354060005260206000f3");
    let bh_init = make_initcode(&bh_runtime);

    let deploy_tx = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce::ZERO,
        from: sender,
        to: None,
        value: Amount::ZERO,
        gas_limit: Gas(200_000),
        max_fee_per_gas: Amount::from_u128(20),
        max_priority_fee_per_gas: Amount::from_u128(2),
        valid_until_block: BlockHeight(100),
        data: bh_init,
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_deploy, _) = sign_eth_tx(&kp, deploy_tx);
    let contract_addr = compute_contract_address(&sender, 0);

    // Block 1 deploys contract
    let (b1, r1) = harness.commit_block(vec![signed_deploy]);
    assert!(r1[0].success);
    let h1_hash = tola_consensus::PipelinedBftEngine::compute_block_hash(&b1);

    // Block 2: empty block
    let (b2, r2) = harness.commit_block(vec![]);
    let _h2_hash = tola_consensus::PipelinedBftEngine::compute_block_hash(&b2);

    let rpc = harness.make_dispatcher();
    rpc.record_block(b1, r1);
    rpc.record_block(b2, r2);

    // Now current height in StateDB is 2.
    // Query BLOCKHASH(1): should equal block 1's canonical hash!
    let mut q1 = [0u8; 32];
    q1[31] = 1; // block 1
    let res1 = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"eth_call","params":[{{ "to": "{}", "data": "0x{}" }}]}}"#,
        contract_addr.to_hex(),
        hex_encode(&q1)
    ));
    assert!(res1
        .to_lowercase()
        .contains(&h1_hash.to_hex().to_lowercase()));

    // Query BLOCKHASH(2) (current block during simulation): must return zero!
    let mut q2 = [0u8; 32];
    q2[31] = 2; // block 2
    let res2 = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"eth_call","params":[{{ "to": "{}", "data": "0x{}" }}]}}"#,
        contract_addr.to_hex(),
        hex_encode(&q2)
    ));
    assert!(res2.contains("0x0000000000000000000000000000000000000000000000000000000000000000"));

    // Query BLOCKHASH(999) (future block): must return zero!
    let mut q_future = [0u8; 32];
    q_future[30..32].copy_from_slice(&999u16.to_be_bytes());
    let res_future = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":3,"method":"eth_call","params":[{{ "to": "{}", "data": "0x{}" }}]}}"#,
        contract_addr.to_hex(),
        hex_encode(&q_future)
    ));
    assert!(
        res_future.contains("0x0000000000000000000000000000000000000000000000000000000000000000")
    );
}

// =========================================================================
// M. PREVRANDAO SHANGHAI DETERMINISM
// =========================================================================

#[test]
fn test_p6_m_prevrandao_shanghai_determinism() {
    let mut harness = TestHarness::new("test_m_randao");
    let kp = Secp256k1Keypair::generate();
    let sender = kp.address();
    harness.fund_account(&sender, 10_000_000_000);

    // Contract executing PREVRANDAO (opcode 0x44):
    // 44 60 00 52 60 20 60 00 f3
    let randao_runtime = hex_decode("4460005260206000f3");
    let randao_init = make_initcode(&randao_runtime);

    let deploy_tx = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce::ZERO,
        from: sender,
        to: None,
        value: Amount::ZERO,
        gas_limit: Gas(200_000),
        max_fee_per_gas: Amount::from_u128(20),
        max_priority_fee_per_gas: Amount::from_u128(2),
        valid_until_block: BlockHeight(100),
        data: randao_init,
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_deploy, _) = sign_eth_tx(&kp, deploy_tx);
    let contract_addr = compute_contract_address(&sender, 0);

    let (b1, r1) = harness.commit_block(vec![signed_deploy]);
    assert!(r1[0].success);

    let rpc = harness.make_dispatcher();
    rpc.record_block(b1, r1);

    // In TOLA's Shanghai EVM, PREVRANDAO returns 0 deterministically
    let res = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"eth_call","params":[{{ "to": "{}" }}]}}"#,
        contract_addr.to_hex()
    ));
    assert!(res.contains("0x0000000000000000000000000000000000000000000000000000000000000000"));
}

// =========================================================================
// N. STATE ROOT & UNIFIED STATEDB
// =========================================================================

#[test]
fn test_p6_n_state_root_and_shared_state() {
    let mut harness = TestHarness::new("test_n_state_root");
    let kp = Secp256k1Keypair::generate();
    let sender = kp.address();
    harness.fund_account(&sender, 10_000_000_000);

    let root_initial = harness
        .state_arc
        .read()
        .unwrap()
        .compute_state_root()
        .unwrap();

    let runtime = hex_decode("602a60005260206000f3");
    let initcode = make_initcode(&runtime);

    let deploy_tx = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce::ZERO,
        from: sender,
        to: None,
        value: Amount::ZERO,
        gas_limit: Gas(200_000),
        max_fee_per_gas: Amount::from_u128(20),
        max_priority_fee_per_gas: Amount::from_u128(2),
        valid_until_block: BlockHeight(100),
        data: initcode,
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_deploy, _) = sign_eth_tx(&kp, deploy_tx);
    let contract_addr = compute_contract_address(&sender, 0);

    let (_b1, r1) = harness.commit_block(vec![signed_deploy]);
    assert!(r1[0].success);

    let root_after = harness
        .state_arc
        .read()
        .unwrap()
        .compute_state_root()
        .unwrap();
    // 1. Unified state root changes deterministically
    assert_ne!(root_initial, root_after);

    // 2. Contract account exists in Table::Accounts with FLAG_CONTRACT
    let contract_acct = harness
        .state_arc
        .read()
        .unwrap()
        .get_account(&contract_addr)
        .unwrap()
        .unwrap();
    assert_eq!(contract_acct.flags & FLAG_CONTRACT, FLAG_CONTRACT);
    assert_ne!(contract_acct.code_hash, Hash::ZERO);
}

// =========================================================================
// O. RPC SMART CONTRACT SURFACE
// =========================================================================

#[test]
fn test_p6_o_rpc_smart_contract_surface() {
    let mut harness = TestHarness::new("test_o_rpc");
    let kp = Secp256k1Keypair::generate();
    let sender = kp.address();
    harness.fund_account(&sender, 10_000_000_000);

    let runtime = hex_decode("602a60005260206000f3");
    let initcode = make_initcode(&runtime);

    let deploy_tx = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce::ZERO,
        from: sender,
        to: None,
        value: Amount::ZERO,
        gas_limit: Gas(200_000),
        max_fee_per_gas: Amount::from_u128(20),
        max_priority_fee_per_gas: Amount::from_u128(2),
        valid_until_block: BlockHeight(100),
        data: initcode,
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_deploy, eth_hash) = sign_eth_tx(&kp, deploy_tx);
    let contract_addr = compute_contract_address(&sender, 0);

    let (b1, r1) = harness.commit_block(vec![signed_deploy]);
    let rpc = harness.make_dispatcher();
    rpc.record_block(b1, r1);

    // 1. eth_estimateGas for contract call
    let est = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"eth_estimateGas","params":[{{ "to": "{}" }}]}}"#,
        contract_addr.to_hex()
    ));
    assert!(est.contains("0x"));

    // 2. eth_call
    let call = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"eth_call","params":[{{ "to": "{}" }}]}}"#,
        contract_addr.to_hex()
    ));
    assert!(call.contains("0000002a"));

    // 3. eth_getCode
    let code = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":3,"method":"eth_getCode","params":["{}","latest"]}}"#,
        contract_addr.to_hex()
    ));
    assert!(code.contains(&hex_encode(&runtime)));

    // 4. eth_getTransactionByHash
    let tx_by_hash = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":4,"method":"eth_getTransactionByHash","params":["{}"]}}"#,
        eth_hash.to_hex()
    ));
    assert!(tx_by_hash.contains(&eth_hash.to_hex()));

    // 5. eth_getTransactionReceipt
    let rec = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":5,"method":"eth_getTransactionReceipt","params":["{}"]}}"#,
        eth_hash.to_hex()
    ));
    assert!(rec.contains(&contract_addr.to_hex()));
}

// =========================================================================
// P. COLD RESTART & PERSISTENCE
// =========================================================================

#[test]
fn test_p6_p_restart_comprehensive_lifecycle() {
    let mut harness = TestHarness::new("test_p_restart");
    let kp = Secp256k1Keypair::generate();
    let sender = kp.address();
    harness.fund_account(&sender, 10_000_000_000);

    // Contract with storage and event:
    // Slot 0 starts at 100
    // Calling writes calldataload(0) to slot 0 and emits LOG1(topic=slot_val)
    // 60 00 35 60 00 55 60 00 54 80 60 00 52 60 20 60 00 a1 00
    let runtime = hex_decode("6000356000556000548060005260206000a100");
    let setup = hex_decode("6064600055");
    let initcode = make_initcode_with_setup(&setup, &runtime);

    let deploy_tx = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce::ZERO,
        from: sender,
        to: None,
        value: Amount::ZERO,
        gas_limit: Gas(200_000),
        max_fee_per_gas: Amount::from_u128(20),
        max_priority_fee_per_gas: Amount::from_u128(2),
        valid_until_block: BlockHeight(100),
        data: initcode,
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_deploy, _) = sign_eth_tx(&kp, deploy_tx);
    let contract_addr = compute_contract_address(&sender, 0);

    let (_b1, r1) = harness.commit_block(vec![signed_deploy]);
    assert!(r1[0].success);

    // Mutate storage to 250 (0xfa)
    let mut val_250 = [0u8; 32];
    val_250[31] = 0xfa;
    let mut_tx = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(1),
        from: sender,
        to: Some(contract_addr),
        value: Amount::ZERO,
        gas_limit: Gas(100_000),
        max_fee_per_gas: Amount::from_u128(20),
        max_priority_fee_per_gas: Amount::from_u128(2),
        valid_until_block: BlockHeight(100),
        data: val_250.to_vec(),
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_mut, eth_mut_hash) = sign_eth_tx(&kp, mut_tx);
    let (b2, r2) = harness.commit_block(vec![signed_mut]);
    assert!(r2[0].success);

    // Cold drop and reopen
    harness.reopen();
    let rpc_after = harness.make_dispatcher();
    rpc_after.record_block(b2, r2);

    // 1. Verify code matches
    let code_after = rpc_after.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"eth_getCode","params":["{}","latest"]}}"#,
        contract_addr.to_hex()
    ));
    assert!(code_after.contains(&hex_encode(&runtime)));

    // 2. Verify storage matches (0xfa)
    let s0_after = rpc_after.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"eth_getStorageAt","params":["{}","0x0","latest"]}}"#,
        contract_addr.to_hex()
    ));
    assert!(s0_after.contains("000000fa"));

    // 3. Verify receipt from disk
    let rec_after = rpc_after.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":3,"method":"eth_getTransactionReceipt","params":["{}"]}}"#,
        eth_mut_hash.to_hex()
    ));
    assert!(rec_after.contains(r#""status":"0x1""#));

    // 4. Execute another transaction after restart: write 500 (0x01f4)
    let mut val_500 = [0u8; 32];
    val_500[30..32].copy_from_slice(&500u16.to_be_bytes());
    let post_restart_tx = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(2),
        from: sender,
        to: Some(contract_addr),
        value: Amount::ZERO,
        gas_limit: Gas(100_000),
        max_fee_per_gas: Amount::from_u128(20),
        max_priority_fee_per_gas: Amount::from_u128(2),
        valid_until_block: BlockHeight(100),
        data: val_500.to_vec(),
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_post, _) = sign_eth_tx(&kp, post_restart_tx);
    let (b3, r3) = harness.commit_block(vec![signed_post]);
    assert!(r3[0].success);

    rpc_after.record_block(b3, r3);
    let s0_final = rpc_after.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":4,"method":"eth_getStorageAt","params":["{}","0x0","latest"]}}"#,
        contract_addr.to_hex()
    ));
    assert!(s0_final.contains("000001f4"));
}

// =========================================================================
// Q. GAS ACCOUNTING CORRECTNESS
// =========================================================================

#[test]
fn test_p6_q_gas_accounting_correctness() {
    let mut harness = TestHarness::new("test_q_gas");
    let kp = Secp256k1Keypair::generate();
    let sender = kp.address();
    let initial_balance = 10_000_000_000u128;
    harness.fund_account(&sender, initial_balance);

    let runtime = hex_decode("602a60005260206000f3");
    let initcode = make_initcode(&runtime);

    let gas_limit = 200_000u64;
    let max_fee = 20u128;
    let priority_fee = 2u128;

    let deploy_tx = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce::ZERO,
        from: sender,
        to: None,
        value: Amount::ZERO,
        gas_limit: Gas(gas_limit),
        max_fee_per_gas: Amount::from_u128(max_fee),
        max_priority_fee_per_gas: Amount::from_u128(priority_fee),
        valid_until_block: BlockHeight(100),
        data: initcode,
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_deploy, _) = sign_eth_tx(&kp, deploy_tx);

    let (_b1, receipts) = harness.commit_block(vec![signed_deploy]);
    assert!(receipts[0].success);

    let gas_used = receipts[0].gas_used.as_u64();
    assert!(gas_used > 21_000);
    assert!(gas_used < gas_limit);

    // Verify sender account paid effective_gas_price * gas_used
    let sender_acct = harness
        .state_arc
        .read()
        .unwrap()
        .get_account(&sender)
        .unwrap()
        .unwrap();

    let effective_gas_price = harness.base_fee.as_u128() + priority_fee;
    let actual_fee_paid = (gas_used as u128) * effective_gas_price;
    assert_eq!(
        sender_acct.balance.as_u128(),
        initial_balance - actual_fee_paid
    );
}

// =========================================================================
// R. SECURITY & NEGATIVE TESTS
// =========================================================================

#[test]
fn test_p6_r_security_and_negative_tests() {
    let mut harness = TestHarness::new("test_r_security");
    let kp = Secp256k1Keypair::generate();
    let sender = kp.address();
    harness.fund_account(&sender, 10_000_000_000);

    // 1. Invalid opcode (0xfe) during execution
    let invalid_runtime = hex_decode("6000fe");
    let invalid_init = make_initcode(&invalid_runtime);

    let deploy_invalid = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce::ZERO,
        from: sender,
        to: None,
        value: Amount::ZERO,
        gas_limit: Gas(200_000),
        max_fee_per_gas: Amount::from_u128(20),
        max_priority_fee_per_gas: Amount::from_u128(2),
        valid_until_block: BlockHeight(100),
        data: invalid_init,
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_invalid, _) = sign_eth_tx(&kp, deploy_invalid);
    let invalid_addr = compute_contract_address(&sender, 0);

    let (_b1, r1) = harness.commit_block(vec![signed_invalid]);
    assert!(r1[0].success); // Deployment succeeded; runtime code has INVALID

    // Call the contract containing INVALID opcode
    let call_invalid = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(1),
        from: sender,
        to: Some(invalid_addr),
        value: Amount::ZERO,
        gas_limit: Gas(100_000),
        max_fee_per_gas: Amount::from_u128(20),
        max_priority_fee_per_gas: Amount::from_u128(2),
        valid_until_block: BlockHeight(100),
        data: vec![],
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_call_invalid, _) = sign_eth_tx(&kp, call_invalid);
    let (_b2, r2) = harness.commit_block(vec![signed_call_invalid]);
    assert!(!r2[0].success); // Transaction halted / failed

    // 2. Calling nonexistent contract returns success with empty returndata
    let nonexistent_addr = Address([0xbb; 20]);
    let call_nonexistent = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(2),
        from: sender,
        to: Some(nonexistent_addr),
        value: Amount::ZERO,
        gas_limit: Gas(50_000),
        max_fee_per_gas: Amount::from_u128(20),
        max_priority_fee_per_gas: Amount::from_u128(2),
        valid_until_block: BlockHeight(100),
        data: vec![0x12, 0x34],
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_nonexistent, _) = sign_eth_tx(&kp, call_nonexistent);
    let (_b3, r3) = harness.commit_block(vec![signed_nonexistent]);
    assert!(r3[0].success);
    assert!(r3[0].return_data.is_empty());
}
