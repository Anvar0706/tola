//! TOLA EVM Phase 8 (P8) Historical State & Block-Tag Query Fidelity Integration Test Suite
//!
//! Comprehensive end-to-end verification of:
//! Test A — Historical Native Balance Fidelity
//! Test B — Historical Contract Code Fidelity
//! Test C — Historical Storage Fidelity
//! Test D — Historical eth_call Simulation
//! Test E — Historical Nonce Fidelity
//! Test F — Historical Gas Estimation
//! Test G — Named Block Tags Fidelity (latest, earliest, pending, safe, finalized)
//! Test H — Historical Execution Opcode Fidelity (NUMBER, TIMESTAMP, BASEFEE, COINBASE, BLOCKHASH)
//! Test I — BLOCKHASH 256-Block Window Anchored to Historical Target
//! Test J — Cold Restart Historical State Invariance
//! Test K — Future Block Rejection (-32000 error, no fallback)
//! Test L — Non-Existent Account / Slot Historical Consistency
//! Test M — Storage Zeroing Invariance
//! Test N — Multiple Contracts Independent Historical States
//! Test O — EIP-1898 blockNumber and blockHash Object Queries
//! Test P — Historical Multi-Step Call Trace / Nested Call
//! Test Q — Unified State Root Monotonic Determinism
//! Test R — Mempool Pending Overlay Isolation
//! Test S — Atomic Block Bundle Commit Invariance

use std::sync::{Arc, RwLock};
use tola_codec::CanonicalSerialize;
use tola_core::{BlockEngine, StateDB};
use tola_crypto::Secp256k1Keypair;
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

fn pad32_u64(val: u64) -> [u8; 32] {
    let mut buf = [0u8; 32];
    buf[24..32].copy_from_slice(&val.to_be_bytes());
    buf
}

fn pad32_u128(val: u128) -> [u8; 32] {
    let mut buf = [0u8; 32];
    buf[16..32].copy_from_slice(&val.to_be_bytes());
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

fn make_initcode(runtime: &[u8]) -> Vec<u8> {
    let mut code = Vec::new();
    let r_len = runtime.len();
    assert!(r_len <= 255);
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
        let db_path = std::env::temp_dir().join(format!("tola_p8_{test_name}_{nanos}"));
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
        let timestamp = Timestamp(1_700_000_000 + self.current_height * 2);
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
            "TOLA-P8/1.0".into(),
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

// Bytecode for StorageStore contract (read/write arbitrary slot):
// Write: CALLDATASIZE == 64 -> SSTORE(slot=calldataload(0), val=calldataload(32))
// Read: CALLDATASIZE == 32 -> SLOAD(slot=calldataload(0)) -> MSTORE(0, val) -> RETURN(0, 32)
// Hex: 36602014600f5760203560003555005b6000355460005260206000f3
fn storage_contract_runtime() -> Vec<u8> {
    hex_decode("36602014600f5760203560003555005b6000355460005260206000f3")
}

// Bytecode for Environment Reader contract (returns NUMBER, TIMESTAMP, BASEFEE, COINBASE):
// Hex: 4360005242602052486040524160605260806000f3
fn env_reader_runtime() -> Vec<u8> {
    hex_decode("4360005242602052486040524160605260806000f3")
}

// Bytecode for BLOCKHASH Reader contract (returns BLOCKHASH(calldataload(0))):
// Hex: 6000354060005260206000f3
fn blockhash_reader_runtime() -> Vec<u8> {
    hex_decode("6000354060005260206000f3")
}

// Bytecode for Nested Caller contract (Contract A staticcalls Contract B):
// Calldata: [0..32: Target Address], [32..64: Slot]
// Hex: 60206020600037602060206020600060003563000f4240fa60206020f3
fn nested_caller_runtime() -> Vec<u8> {
    hex_decode("60206020600037602060206020600060003563000f4240fa60206020f3")
}

// =========================================================================
// TEST A — Historical Native Balance Fidelity
// =========================================================================

#[test]
fn test_p8_a_historical_native_balance_fidelity() {
    let mut harness = TestHarness::new("test_a_bal");
    let kp_a = Secp256k1Keypair::generate();
    let addr_a = kp_a.address();
    let addr_b = Address([0xbb; 20]);
    let addr_c = Address([0xcc; 20]);

    // Initial fund at Block 1
    harness.fund_account(&addr_a, 1_000_000_000);
    let (b1, r1) = harness.commit_block(vec![]);
    let rpc = harness.make_dispatcher();
    rpc.record_block(b1, r1);

    // Block 2: A sends 400_000_000 to B
    let tx_transfer1 = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce::ZERO,
        from: addr_a,
        to: Some(addr_b),
        value: Amount::from_u128(400_000_000),
        gas_limit: Gas(21_000),
        max_fee_per_gas: Amount::from_u128(10),
        max_priority_fee_per_gas: Amount::ZERO,
        valid_until_block: BlockHeight(100),
        data: vec![],
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_tx1, _) = sign_eth_tx(&kp_a, tx_transfer1);
    let (b2, r2) = harness.commit_block(vec![signed_tx1]);
    rpc.record_block(b2, r2);

    // Block 3: A sends 100_000_000 to C
    let tx_transfer2 = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(1),
        from: addr_a,
        to: Some(addr_c),
        value: Amount::from_u128(100_000_000),
        gas_limit: Gas(21_000),
        max_fee_per_gas: Amount::from_u128(10),
        max_priority_fee_per_gas: Amount::ZERO,
        valid_until_block: BlockHeight(100),
        data: vec![],
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_tx2, _) = sign_eth_tx(&kp_a, tx_transfer2);
    let (b3, r3) = harness.commit_block(vec![signed_tx2]);
    rpc.record_block(b3, r3);

    // 1. Query Account A balance at Block 1: exactly 1_000_000_000
    let res_b1 = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"eth_getBalance","params":["{}","0x1"]}}"#,
        addr_a.to_hex()
    ));
    assert!(res_b1.contains(&format!("0x{:x}", 1_000_000_000u64)));

    // 2. Query Account A balance at Block 2: 1_000_000_000 - 400_000_000 - gas_used * 10
    let gas_cost1 = 21_000u128 * 10;
    let expected_b2 = 1_000_000_000u128 - 400_000_000u128 - gas_cost1;
    let res_b2 = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"eth_getBalance","params":["{}","0x2"]}}"#,
        addr_a.to_hex()
    ));
    assert!(res_b2.contains(&format!("0x{:x}", expected_b2)));

    // 3. Query Account A balance at Block 3: expected_b2 - 100_000_000 - gas_cost2
    let gas_cost2 = 21_000u128 * 10;
    let expected_b3 = expected_b2 - 100_000_000u128 - gas_cost2;
    let res_b3 = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":3,"method":"eth_getBalance","params":["{}","0x3"]}}"#,
        addr_a.to_hex()
    ));
    assert!(res_b3.contains(&format!("0x{:x}", expected_b3)));

    // 4. Query Account A balance at "latest": matches Block 3
    let res_latest = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":4,"method":"eth_getBalance","params":["{}","latest"]}}"#,
        addr_a.to_hex()
    ));
    assert!(res_latest.contains(&format!("0x{:x}", expected_b3)));

    // 5. Query Account B balance at Block 1: 0x0; at Block 2: 400_000_000
    let res_b_b1 = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":5,"method":"eth_getBalance","params":["{}","0x1"]}}"#,
        addr_b.to_hex()
    ));
    assert!(res_b_b1.contains("\"0x0\""));

    let res_b_b2 = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":6,"method":"eth_getBalance","params":["{}","0x2"]}}"#,
        addr_b.to_hex()
    ));
    assert!(res_b_b2.contains(&format!("0x{:x}", 400_000_000u64)));
}

// =========================================================================
// TEST B — Historical Contract Code Fidelity
// =========================================================================

#[test]
fn test_p8_b_historical_contract_code_fidelity() {
    let mut harness = TestHarness::new("test_b_code");
    let kp = Secp256k1Keypair::generate();
    let deployer = kp.address();
    harness.fund_account(&deployer, 1_000_000_000);

    // Block 1: Ordinary empty block
    let (b1, r1) = harness.commit_block(vec![]);
    let rpc = harness.make_dispatcher();
    rpc.record_block(b1, r1);

    // Block 2: Deploy contract
    let runtime = storage_contract_runtime();
    let initcode = make_initcode(&runtime);
    let deploy_tx = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce::ZERO,
        from: deployer,
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
    let contract_addr = compute_contract_address(&deployer, 0);
    let (b2, r2) = harness.commit_block(vec![signed_deploy]);
    assert!(r2[0].success);
    rpc.record_block(b2, r2);

    // Block 3: Subsequent block
    let (b3, r3) = harness.commit_block(vec![]);
    rpc.record_block(b3, r3);

    // 1. Query eth_getCode at Block 1: returns "0x"
    let code_b1 = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"eth_getCode","params":["{}","0x1"]}}"#,
        contract_addr.to_hex()
    ));
    assert!(code_b1.contains("\"0x\""));

    // 2. Query eth_getCode at Block 2: returns full runtime bytecode
    let code_b2 = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"eth_getCode","params":["{}","0x2"]}}"#,
        contract_addr.to_hex()
    ));
    assert!(code_b2.contains(&hex_encode(&runtime)));

    // 3. Query eth_getCode at Block 3: returns full runtime bytecode
    let code_b3 = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":3,"method":"eth_getCode","params":["{}","0x3"]}}"#,
        contract_addr.to_hex()
    ));
    assert!(code_b3.contains(&hex_encode(&runtime)));

    // 4. Query eth_getCode at "latest": returns full runtime bytecode
    let code_latest = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":4,"method":"eth_getCode","params":["{}","latest"]}}"#,
        contract_addr.to_hex()
    ));
    assert!(code_latest.contains(&hex_encode(&runtime)));
}

// =========================================================================
// TEST C — Historical Storage Fidelity
// =========================================================================

#[test]
fn test_p8_c_historical_storage_fidelity() {
    let mut harness = TestHarness::new("test_c_storage");
    let kp = Secp256k1Keypair::generate();
    let deployer = kp.address();
    harness.fund_account(&deployer, 1_000_000_000);

    // Block 1: Deploy StorageStore contract
    let runtime = storage_contract_runtime();
    let initcode = make_initcode(&runtime);
    let deploy_tx = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce::ZERO,
        from: deployer,
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
    let contract_addr = compute_contract_address(&deployer, 0);
    let (b1, r1) = harness.commit_block(vec![signed_deploy]);
    assert!(r1[0].success);
    let rpc = harness.make_dispatcher();
    rpc.record_block(b1, r1);

    let slot = pad32_u64(0x01);
    let val_b2 = pad32_u128(0x1111_1111);
    let val_b3 = pad32_u128(0x2222_2222);
    let val_b4 = pad32_u128(0x3333_3333);

    // Block 2: Set slot 1 to val_b2
    let mut data_b2 = Vec::new();
    data_b2.extend_from_slice(&slot);
    data_b2.extend_from_slice(&val_b2);
    let tx_b2 = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(1),
        from: deployer,
        to: Some(contract_addr),
        value: Amount::ZERO,
        gas_limit: Gas(200_000),
        max_fee_per_gas: Amount::from_u128(20),
        max_priority_fee_per_gas: Amount::from_u128(2),
        valid_until_block: BlockHeight(100),
        data: data_b2,
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_b2, _) = sign_eth_tx(&kp, tx_b2);
    let (b2, r2) = harness.commit_block(vec![signed_b2]);
    assert!(r2[0].success);
    rpc.record_block(b2, r2);

    // Block 3: Set slot 1 to val_b3
    let mut data_b3 = Vec::new();
    data_b3.extend_from_slice(&slot);
    data_b3.extend_from_slice(&val_b3);
    let tx_b3 = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(2),
        from: deployer,
        to: Some(contract_addr),
        value: Amount::ZERO,
        gas_limit: Gas(200_000),
        max_fee_per_gas: Amount::from_u128(20),
        max_priority_fee_per_gas: Amount::from_u128(2),
        valid_until_block: BlockHeight(100),
        data: data_b3,
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_b3, _) = sign_eth_tx(&kp, tx_b3);
    let (b3, r3) = harness.commit_block(vec![signed_b3]);
    assert!(r3[0].success);
    rpc.record_block(b3, r3);

    // Block 4: Set slot 1 to val_b4
    let mut data_b4 = Vec::new();
    data_b4.extend_from_slice(&slot);
    data_b4.extend_from_slice(&val_b4);
    let tx_b4 = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(3),
        from: deployer,
        to: Some(contract_addr),
        value: Amount::ZERO,
        gas_limit: Gas(200_000),
        max_fee_per_gas: Amount::from_u128(20),
        max_priority_fee_per_gas: Amount::from_u128(2),
        valid_until_block: BlockHeight(100),
        data: data_b4,
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_b4, _) = sign_eth_tx(&kp, tx_b4);
    let (b4, r4) = harness.commit_block(vec![signed_b4]);
    assert!(r4[0].success);
    rpc.record_block(b4, r4);

    let slot_hex = format!("0x{}", hex_encode(&slot));

    // Query eth_getStorageAt at Block 1: returns zero hash
    let s_b1 = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"eth_getStorageAt","params":["{}","{}","0x1"]}}"#,
        contract_addr.to_hex(),
        slot_hex
    ));
    assert!(s_b1.contains(&Hash::ZERO.to_hex()));

    // Query eth_getStorageAt at Block 2: returns val_b2
    let s_b2 = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"eth_getStorageAt","params":["{}","{}","0x2"]}}"#,
        contract_addr.to_hex(),
        slot_hex
    ));
    assert!(s_b2.contains(&hex_encode(&val_b2)));

    // Query eth_getStorageAt at Block 3: returns val_b3
    let s_b3 = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":3,"method":"eth_getStorageAt","params":["{}","{}","0x3"]}}"#,
        contract_addr.to_hex(),
        slot_hex
    ));
    assert!(s_b3.contains(&hex_encode(&val_b3)));

    // Query eth_getStorageAt at Block 4: returns val_b4
    let s_b4 = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":4,"method":"eth_getStorageAt","params":["{}","{}","0x4"]}}"#,
        contract_addr.to_hex(),
        slot_hex
    ));
    assert!(s_b4.contains(&hex_encode(&val_b4)));

    // Query eth_getStorageAt at "latest": returns val_b4
    let s_latest = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":5,"method":"eth_getStorageAt","params":["{}","{}","latest"]}}"#,
        contract_addr.to_hex(),
        slot_hex
    ));
    assert!(s_latest.contains(&hex_encode(&val_b4)));
}

// =========================================================================
// TEST D — Historical eth_call Simulation
// =========================================================================

#[test]
fn test_p8_d_historical_eth_call_simulation() {
    let mut harness = TestHarness::new("test_d_call");
    let kp = Secp256k1Keypair::generate();
    let deployer = kp.address();
    harness.fund_account(&deployer, 1_000_000_000);

    // Deploy StorageStore
    let runtime = storage_contract_runtime();
    let initcode = make_initcode(&runtime);
    let deploy_tx = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce::ZERO,
        from: deployer,
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
    let contract_addr = compute_contract_address(&deployer, 0);
    let (b1, r1) = harness.commit_block(vec![signed_deploy]);
    assert!(r1[0].success);
    let rpc = harness.make_dispatcher();
    rpc.record_block(b1, r1);

    let slot = pad32_u64(0x42);
    let val_b2 = pad32_u128(100);
    let val_b3 = pad32_u128(200);

    // Block 2: write 100
    let mut d2 = slot.to_vec();
    d2.extend_from_slice(&val_b2);
    let tx_b2 = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(1),
        from: deployer,
        to: Some(contract_addr),
        value: Amount::ZERO,
        gas_limit: Gas(200_000),
        max_fee_per_gas: Amount::from_u128(20),
        max_priority_fee_per_gas: Amount::from_u128(2),
        valid_until_block: BlockHeight(100),
        data: d2,
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_b2, _) = sign_eth_tx(&kp, tx_b2);
    let (b2, r2) = harness.commit_block(vec![signed_b2]);
    assert!(r2[0].success);
    rpc.record_block(b2, r2);

    // Block 3: write 200
    let mut d3 = slot.to_vec();
    d3.extend_from_slice(&val_b3);
    let tx_b3 = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(2),
        from: deployer,
        to: Some(contract_addr),
        value: Amount::ZERO,
        gas_limit: Gas(200_000),
        max_fee_per_gas: Amount::from_u128(20),
        max_priority_fee_per_gas: Amount::from_u128(2),
        valid_until_block: BlockHeight(100),
        data: d3,
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_b3, _) = sign_eth_tx(&kp, tx_b3);
    let (b3, r3) = harness.commit_block(vec![signed_b3]);
    assert!(r3[0].success);
    rpc.record_block(b3, r3);

    // Call data to read slot 0x42: exactly 32 bytes of slot
    let read_data_hex = hex_encode(&slot);

    // 1. eth_call at Block 2: returns 100
    let call_b2 = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"eth_call","params":[{{ "to": "{}", "data": "0x{}" }}, "0x2"]}}"#,
        contract_addr.to_hex(),
        read_data_hex
    ));
    assert!(call_b2.contains(&hex_encode(&val_b2)));

    // 2. eth_call at Block 3: returns 200
    let call_b3 = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"eth_call","params":[{{ "to": "{}", "data": "0x{}" }}, "0x3"]}}"#,
        contract_addr.to_hex(),
        read_data_hex
    ));
    assert!(call_b3.contains(&hex_encode(&val_b3)));

    // 3. eth_call at "latest": returns 200
    let call_latest = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":3,"method":"eth_call","params":[{{ "to": "{}", "data": "0x{}" }}, "latest"]}}"#,
        contract_addr.to_hex(),
        read_data_hex
    ));
    assert!(call_latest.contains(&hex_encode(&val_b3)));
}

// =========================================================================
// TEST E — Historical Nonce Fidelity
// =========================================================================

#[test]
fn test_p8_e_historical_nonce_fidelity() {
    let mut harness = TestHarness::new("test_e_nonce");
    let kp = Secp256k1Keypair::generate();
    let addr = kp.address();
    let dest = Address([0x12; 20]);
    harness.fund_account(&addr, 1_000_000_000);

    // Block 1: tx 1 -> nonce becomes 1
    let tx1 = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce::ZERO,
        from: addr,
        to: Some(dest),
        value: Amount::from_u128(1_000),
        gas_limit: Gas(21_000),
        max_fee_per_gas: Amount::from_u128(10),
        max_priority_fee_per_gas: Amount::ZERO,
        valid_until_block: BlockHeight(100),
        data: vec![],
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_tx1, _) = sign_eth_tx(&kp, tx1);
    let (b1, r1) = harness.commit_block(vec![signed_tx1]);
    let rpc = harness.make_dispatcher();
    rpc.record_block(b1, r1);

    // Block 2: tx 2 -> nonce becomes 2
    let tx2 = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(1),
        from: addr,
        to: Some(dest),
        value: Amount::from_u128(1_000),
        gas_limit: Gas(21_000),
        max_fee_per_gas: Amount::from_u128(10),
        max_priority_fee_per_gas: Amount::ZERO,
        valid_until_block: BlockHeight(100),
        data: vec![],
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_tx2, _) = sign_eth_tx(&kp, tx2);
    let (b2, r2) = harness.commit_block(vec![signed_tx2]);
    rpc.record_block(b2, r2);

    // Block 3: tx 3 -> nonce becomes 3
    let tx3 = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(2),
        from: addr,
        to: Some(dest),
        value: Amount::from_u128(1_000),
        gas_limit: Gas(21_000),
        max_fee_per_gas: Amount::from_u128(10),
        max_priority_fee_per_gas: Amount::ZERO,
        valid_until_block: BlockHeight(100),
        data: vec![],
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_tx3, _) = sign_eth_tx(&kp, tx3);
    let (b3, r3) = harness.commit_block(vec![signed_tx3]);
    rpc.record_block(b3, r3);

    // 1. eth_getTransactionCount at Block 1: 0x1
    let n_b1 = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"eth_getTransactionCount","params":["{}","0x1"]}}"#,
        addr.to_hex()
    ));
    assert!(n_b1.contains("\"0x1\""));

    // 2. eth_getTransactionCount at Block 2: 0x2
    let n_b2 = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"eth_getTransactionCount","params":["{}","0x2"]}}"#,
        addr.to_hex()
    ));
    assert!(n_b2.contains("\"0x2\""));

    // 3. eth_getTransactionCount at Block 3: 0x3
    let n_b3 = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":3,"method":"eth_getTransactionCount","params":["{}","0x3"]}}"#,
        addr.to_hex()
    ));
    assert!(n_b3.contains("\"0x3\""));

    // 4. eth_getTransactionCount at "latest": 0x3
    let n_latest = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":4,"method":"eth_getTransactionCount","params":["{}","latest"]}}"#,
        addr.to_hex()
    ));
    assert!(n_latest.contains("\"0x3\""));
}

// =========================================================================
// TEST F — Historical Gas Estimation
// =========================================================================

#[test]
fn test_p8_f_historical_gas_estimation() {
    let mut harness = TestHarness::new("test_f_est");
    let kp = Secp256k1Keypair::generate();
    let deployer = kp.address();
    harness.fund_account(&deployer, 1_000_000_000);

    // Deploy StorageStore
    let runtime = storage_contract_runtime();
    let initcode = make_initcode(&runtime);
    let deploy_tx = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce::ZERO,
        from: deployer,
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
    let contract_addr = compute_contract_address(&deployer, 0);
    let (b1, r1) = harness.commit_block(vec![signed_deploy]);
    let rpc = harness.make_dispatcher();
    rpc.record_block(b1, r1);

    // Block 2
    let (b2, r2) = harness.commit_block(vec![]);
    rpc.record_block(b2, r2);

    // Estimate gas for reading slot 0x01 at historical Block 1 vs Block 2
    let read_data_hex = hex_encode(&pad32_u64(0x01));
    let est_b1 = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"eth_estimateGas","params":[{{ "to": "{}", "data": "0x{}" }}, "0x1"]}}"#,
        contract_addr.to_hex(),
        read_data_hex
    ));
    assert!(est_b1.contains("0x"));

    let est_b2 = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"eth_estimateGas","params":[{{ "to": "{}", "data": "0x{}" }}, "0x2"]}}"#,
        contract_addr.to_hex(),
        read_data_hex
    ));
    assert!(est_b2.contains("0x"));
}

// =========================================================================
// TEST G — Named Block Tags Fidelity
// =========================================================================

#[test]
fn test_p8_g_named_block_tags_fidelity() {
    let mut harness = TestHarness::new("test_g_tags");
    let kp = Secp256k1Keypair::generate();
    let addr = kp.address();
    harness.fund_account(&addr, 500_000_000);

    let (b1, r1) = harness.commit_block(vec![]);
    let rpc = harness.make_dispatcher();
    rpc.record_block(b1, r1);

    let tags = vec!["latest", "pending", "safe", "finalized"];
    for tag in tags {
        let res = rpc.dispatch_json(&format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"eth_getBalance","params":["{}","{}"]}}"#,
            addr.to_hex(),
            tag
        ));
        assert!(
            res.contains(&format!("0x{:x}", 500_000_000u64)),
            "Tag {} failed: {}",
            tag,
            res
        );
    }

    // Earliest resolves to block 0
    let res_earliest = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"eth_getBalance","params":["{}","earliest"]}}"#,
        addr.to_hex()
    ));
    assert!(res_earliest.contains("\"0x0\""));
}

// =========================================================================
// TEST H — Historical Execution Opcode Fidelity
// =========================================================================

#[test]
fn test_p8_h_historical_execution_opcode_fidelity() {
    let mut harness = TestHarness::new("test_h_opcodes");
    let kp = Secp256k1Keypair::generate();
    let deployer = kp.address();
    harness.fund_account(&deployer, 1_000_000_000);

    // Deploy EnvReader contract at Block 1
    let runtime = env_reader_runtime();
    let initcode = make_initcode(&runtime);
    let deploy_tx = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce::ZERO,
        from: deployer,
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
    let contract_addr = compute_contract_address(&deployer, 0);
    let (b1, r1) = harness.commit_block(vec![signed_deploy]);
    assert!(r1[0].success);
    let rpc = harness.make_dispatcher();
    rpc.record_block(b1.clone(), r1);

    // Block 2: Different timestamp, same proposer
    let (b2, r2) = harness.commit_block(vec![]);
    rpc.record_block(b2.clone(), r2);

    // Block 3: Different timestamp
    let (b3, r3) = harness.commit_block(vec![]);
    rpc.record_block(b3.clone(), r3);

    // Verify simulation at Block 1 reflects Block 1 header context
    let call_b1 = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"eth_call","params":[{{ "to": "{}" }}, "0x1"]}}"#,
        contract_addr.to_hex()
    ));
    let parsed_b1 = JsonValue::parse(&call_b1).unwrap();
    let hex_b1 = parsed_b1.get("result").unwrap().as_str().unwrap();
    let bytes_b1 = hex_decode(hex_b1);
    assert_eq!(bytes_b1.len(), 128);
    // bytes[0..32] == block 1 height (1)
    assert_eq!(&bytes_b1[0..32], &pad32_u64(b1.header.height.0));
    // bytes[32..64] == block 1 timestamp
    assert_eq!(&bytes_b1[32..64], &pad32_u64(b1.header.timestamp.0));
    // bytes[64..96] == block 1 base fee
    assert_eq!(&bytes_b1[64..96], &pad32_u128(b1.header.base_fee_per_gas.0));
    // bytes[96..128] == block 1 proposer
    assert_eq!(&bytes_b1[96..128], &pad32_addr(&b1.header.proposer));

    // Verify simulation at Block 2 reflects Block 2 header context
    let call_b2 = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"eth_call","params":[{{ "to": "{}" }}, "0x2"]}}"#,
        contract_addr.to_hex()
    ));
    let parsed_b2 = JsonValue::parse(&call_b2).unwrap();
    let hex_b2 = parsed_b2.get("result").unwrap().as_str().unwrap();
    let bytes_b2 = hex_decode(hex_b2);
    assert_eq!(&bytes_b2[0..32], &pad32_u64(b2.header.height.0));
    assert_eq!(&bytes_b2[32..64], &pad32_u64(b2.header.timestamp.0));
    assert_eq!(&bytes_b2[64..96], &pad32_u128(b2.header.base_fee_per_gas.0));

    // Verify simulation at Block 3 reflects Block 3 header context
    let call_b3 = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":3,"method":"eth_call","params":[{{ "to": "{}" }}, "0x3"]}}"#,
        contract_addr.to_hex()
    ));
    let parsed_b3 = JsonValue::parse(&call_b3).unwrap();
    let hex_b3 = parsed_b3.get("result").unwrap().as_str().unwrap();
    let bytes_b3 = hex_decode(hex_b3);
    assert_eq!(&bytes_b3[0..32], &pad32_u64(b3.header.height.0));
    assert_eq!(&bytes_b3[32..64], &pad32_u64(b3.header.timestamp.0));
    assert_eq!(&bytes_b3[64..96], &pad32_u128(b3.header.base_fee_per_gas.0));
}

// =========================================================================
// TEST I — BLOCKHASH 256-Block Window Anchored to Historical Target
// =========================================================================

#[test]
fn test_p8_i_blockhash_256_window_anchored_to_target() {
    let mut harness = TestHarness::new("test_i_blockhash");
    let kp = Secp256k1Keypair::generate();
    let deployer = kp.address();
    harness.fund_account(&deployer, 1_000_000_000);

    // Deploy BlockhashReader contract
    let runtime = blockhash_reader_runtime();
    let initcode = make_initcode(&runtime);
    let deploy_tx = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce::ZERO,
        from: deployer,
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
    let contract_addr = compute_contract_address(&deployer, 0);
    let (b1, r1) = harness.commit_block(vec![signed_deploy]);
    assert!(r1[0].success);
    let rpc = harness.make_dispatcher();
    rpc.record_block(b1.clone(), r1);

    // Commit Block 2, Block 3, Block 4
    let (b2, r2) = harness.commit_block(vec![]);
    rpc.record_block(b2.clone(), r2);
    let (b3, r3) = harness.commit_block(vec![]);
    rpc.record_block(b3.clone(), r3);
    let (b4, r4) = harness.commit_block(vec![]);
    rpc.record_block(b4.clone(), r4);

    let b2_hash = tola_consensus::PipelinedBftEngine::compute_block_hash(&b2);

    // Simulation targeted at Block 3 querying BLOCKHASH(2):
    // Since 2 == 3 - 1, it must return b2_hash!
    let data_query_2 = hex_encode(&pad32_u64(2));
    let call_query_2 = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"eth_call","params":[{{ "to": "{}", "data": "0x{}" }}, "0x3"]}}"#,
        contract_addr.to_hex(),
        data_query_2
    ));
    assert!(
        call_query_2.contains(&b2_hash.to_hex()[2..]),
        "Expected BLOCKHASH(2) at Block 3 to match b2_hash: {}",
        call_query_2
    );

    // Querying BLOCKHASH(3) at Block 3:
    // Per EVM spec, BLOCKHASH cannot query current block height, so returns 0!
    let data_query_3 = hex_encode(&pad32_u64(3));
    let call_query_3 = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"eth_call","params":[{{ "to": "{}", "data": "0x{}" }}, "0x3"]}}"#,
        contract_addr.to_hex(),
        data_query_3
    ));
    assert!(
        call_query_3.contains(&Hash::ZERO.to_hex()),
        "Expected BLOCKHASH(3) at Block 3 to be 0: {}",
        call_query_3
    );

    // Querying BLOCKHASH(4) at Block 3:
    // Future block relative to target block 3 -> returns 0!
    let data_query_4 = hex_encode(&pad32_u64(4));
    let call_query_4 = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":3,"method":"eth_call","params":[{{ "to": "{}", "data": "0x{}" }}, "0x3"]}}"#,
        contract_addr.to_hex(),
        data_query_4
    ));
    assert!(
        call_query_4.contains(&Hash::ZERO.to_hex()),
        "Expected BLOCKHASH(4) at Block 3 to be 0: {}",
        call_query_4
    );
}

// =========================================================================
// TEST J — Cold Restart Historical State Invariance
// =========================================================================

#[test]
fn test_p8_j_cold_restart_historical_state_invariance() {
    let mut harness = TestHarness::new("test_j_restart");
    let kp = Secp256k1Keypair::generate();
    let deployer = kp.address();
    harness.fund_account(&deployer, 1_000_000_000);

    // Block 1: Deploy StorageStore
    let runtime = storage_contract_runtime();
    let initcode = make_initcode(&runtime);
    let deploy_tx = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce::ZERO,
        from: deployer,
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
    let contract_addr = compute_contract_address(&deployer, 0);
    let (b1, r1) = harness.commit_block(vec![signed_deploy]);
    let rpc_pre = harness.make_dispatcher();
    rpc_pre.record_block(b1, r1);

    // Block 2: write slot 0x05 -> 5555
    let slot = pad32_u64(0x05);
    let val_b2 = pad32_u128(5555);
    let mut d2 = slot.to_vec();
    d2.extend_from_slice(&val_b2);
    let tx_b2 = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(1),
        from: deployer,
        to: Some(contract_addr),
        value: Amount::ZERO,
        gas_limit: Gas(200_000),
        max_fee_per_gas: Amount::from_u128(20),
        max_priority_fee_per_gas: Amount::from_u128(2),
        valid_until_block: BlockHeight(100),
        data: d2,
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_b2, _) = sign_eth_tx(&kp, tx_b2);
    let (b2, r2) = harness.commit_block(vec![signed_b2]);
    rpc_pre.record_block(b2, r2);

    // Pre-restart queries
    let slot_hex = format!("0x{}", hex_encode(&slot));
    let pre_s_b1 = rpc_pre.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"eth_getStorageAt","params":["{}","{}","0x1"]}}"#,
        contract_addr.to_hex(),
        slot_hex
    ));
    let pre_s_b2 = rpc_pre.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"eth_getStorageAt","params":["{}","{}","0x2"]}}"#,
        contract_addr.to_hex(),
        slot_hex
    ));

    // DROP IN-MEMORY STATE AND REOPEN FRESH FROM DISK
    harness.reopen();
    let rpc_post = harness.make_dispatcher();

    let post_s_b1 = rpc_post.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"eth_getStorageAt","params":["{}","{}","0x1"]}}"#,
        contract_addr.to_hex(),
        slot_hex
    ));
    let post_s_b2 = rpc_post.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"eth_getStorageAt","params":["{}","{}","0x2"]}}"#,
        contract_addr.to_hex(),
        slot_hex
    ));

    assert_eq!(pre_s_b1, post_s_b1);
    assert_eq!(pre_s_b2, post_s_b2);
    assert!(post_s_b2.contains(&hex_encode(&val_b2)));
}

// =========================================================================
// TEST K — Future Block Rejection
// =========================================================================

#[test]
fn test_p8_k_future_block_rejection() {
    let mut harness = TestHarness::new("test_k_future");
    let kp = Secp256k1Keypair::generate();
    let addr = kp.address();
    harness.fund_account(&addr, 1_000_000);

    let (b1, r1) = harness.commit_block(vec![]);
    let rpc = harness.make_dispatcher();
    rpc.record_block(b1, r1);

    // Queries at height 2 (when chain head is 1)
    let bal_future = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"eth_getBalance","params":["{}","0x2"]}}"#,
        addr.to_hex()
    ));
    assert!(
        bal_future.contains("-32000"),
        "Expected error -32000 for future block: {}",
        bal_future
    );

    let code_future = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"eth_getCode","params":["{}","0x99"]}}"#,
        addr.to_hex()
    ));
    assert!(
        code_future.contains("-32000"),
        "Expected error -32000 for future block: {}",
        code_future
    );

    let storage_future = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":3,"method":"eth_getStorageAt","params":["{}","0x0","0x50"]}}"#,
        addr.to_hex()
    ));
    assert!(
        storage_future.contains("-32000"),
        "Expected error -32000 for future block: {}",
        storage_future
    );

    let call_future = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":4,"method":"eth_call","params":[{{ "to": "{}" }}, "0x50"]}}"#,
        addr.to_hex()
    ));
    assert!(
        call_future.contains("-32000"),
        "Expected error -32000 for future block: {}",
        call_future
    );
}

// =========================================================================
// TEST L — Non-Existent Account / Slot Historical Consistency
// =========================================================================

#[test]
fn test_p8_l_non_existent_account_slot_historical_consistency() {
    let mut harness = TestHarness::new("test_l_nonexistent");
    let (b1, r1) = harness.commit_block(vec![]);
    let rpc = harness.make_dispatcher();
    rpc.record_block(b1, r1);

    let random_addr = Address([0x77; 20]);
    let random_slot = format!("0x{}", hex_encode(&pad32_u64(0x9999)));

    // Non-existent account at block 1
    let bal = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"eth_getBalance","params":["{}","0x1"]}}"#,
        random_addr.to_hex()
    ));
    assert!(bal.contains("\"0x0\""));

    let code = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"eth_getCode","params":["{}","0x1"]}}"#,
        random_addr.to_hex()
    ));
    assert!(code.contains("\"0x\""));

    let count = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":3,"method":"eth_getTransactionCount","params":["{}","0x1"]}}"#,
        random_addr.to_hex()
    ));
    assert!(count.contains("\"0x0\""));

    let storage = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":4,"method":"eth_getStorageAt","params":["{}","{}","0x1"]}}"#,
        random_addr.to_hex(),
        random_slot
    ));
    assert!(storage.contains(&Hash::ZERO.to_hex()));
}

// =========================================================================
// TEST M — Storage Zeroing Invariance
// =========================================================================

#[test]
fn test_p8_m_storage_zeroing_invariance() {
    let mut harness = TestHarness::new("test_m_zeroing");
    let kp = Secp256k1Keypair::generate();
    let deployer = kp.address();
    harness.fund_account(&deployer, 1_000_000_000);

    // Deploy StorageStore
    let runtime = storage_contract_runtime();
    let initcode = make_initcode(&runtime);
    let deploy_tx = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce::ZERO,
        from: deployer,
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
    let contract_addr = compute_contract_address(&deployer, 0);
    let (b1, r1) = harness.commit_block(vec![signed_deploy]);
    let rpc = harness.make_dispatcher();
    rpc.record_block(b1, r1);

    let slot = pad32_u64(0x88);
    let val_non_zero = pad32_u128(0xbeef);
    let val_zero = [0u8; 32];

    // Block 2: set to 0xbeef
    let mut d2 = slot.to_vec();
    d2.extend_from_slice(&val_non_zero);
    let tx_b2 = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(1),
        from: deployer,
        to: Some(contract_addr),
        value: Amount::ZERO,
        gas_limit: Gas(200_000),
        max_fee_per_gas: Amount::from_u128(20),
        max_priority_fee_per_gas: Amount::from_u128(2),
        valid_until_block: BlockHeight(100),
        data: d2,
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_b2, _) = sign_eth_tx(&kp, tx_b2);
    let (b2, r2) = harness.commit_block(vec![signed_b2]);
    rpc.record_block(b2, r2);

    // Block 3: clear to 0
    let mut d3 = slot.to_vec();
    d3.extend_from_slice(&val_zero);
    let tx_b3 = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(2),
        from: deployer,
        to: Some(contract_addr),
        value: Amount::ZERO,
        gas_limit: Gas(200_000),
        max_fee_per_gas: Amount::from_u128(20),
        max_priority_fee_per_gas: Amount::from_u128(2),
        valid_until_block: BlockHeight(100),
        data: d3,
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_b3, _) = sign_eth_tx(&kp, tx_b3);
    let (b3, r3) = harness.commit_block(vec![signed_b3]);
    rpc.record_block(b3, r3);

    let slot_hex = format!("0x{}", hex_encode(&slot));

    // Query at Block 2: returns 0xbeef
    let s_b2 = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"eth_getStorageAt","params":["{}","{}","0x2"]}}"#,
        contract_addr.to_hex(),
        slot_hex
    ));
    assert!(s_b2.contains(&hex_encode(&val_non_zero)));

    // Query at Block 3: returns zero hash
    let s_b3 = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"eth_getStorageAt","params":["{}","{}","0x3"]}}"#,
        contract_addr.to_hex(),
        slot_hex
    ));
    assert!(s_b3.contains(&Hash::ZERO.to_hex()));
}

// =========================================================================
// TEST N — Multiple Contracts Independent Historical States
// =========================================================================

#[test]
fn test_p8_n_multiple_contracts_independent_historical_states() {
    let mut harness = TestHarness::new("test_n_multi");
    let kp = Secp256k1Keypair::generate();
    let deployer = kp.address();
    harness.fund_account(&deployer, 1_000_000_000);

    let runtime = storage_contract_runtime();
    let initcode = make_initcode(&runtime);

    // Deploy Contract 1 at Block 1
    let deploy1 = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce::ZERO,
        from: deployer,
        to: None,
        value: Amount::ZERO,
        gas_limit: Gas(200_000),
        max_fee_per_gas: Amount::from_u128(20),
        max_priority_fee_per_gas: Amount::from_u128(2),
        valid_until_block: BlockHeight(100),
        data: initcode.clone(),
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_d1, _) = sign_eth_tx(&kp, deploy1);
    let c1_addr = compute_contract_address(&deployer, 0);
    let (b1, r1) = harness.commit_block(vec![signed_d1]);
    let rpc = harness.make_dispatcher();
    rpc.record_block(b1, r1);

    // Deploy Contract 2 at Block 2
    let deploy2 = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(1),
        from: deployer,
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
    let (signed_d2, _) = sign_eth_tx(&kp, deploy2);
    let c2_addr = compute_contract_address(&deployer, 1);
    let (b2, r2) = harness.commit_block(vec![signed_d2]);
    rpc.record_block(b2, r2);

    let slot = pad32_u64(0x01);
    let val_c1 = pad32_u128(111);
    let val_c2 = pad32_u128(222);

    // Block 3: Mutate C1 slot 1 -> 111
    let mut d_c1 = slot.to_vec();
    d_c1.extend_from_slice(&val_c1);
    let tx_c1 = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(2),
        from: deployer,
        to: Some(c1_addr),
        value: Amount::ZERO,
        gas_limit: Gas(200_000),
        max_fee_per_gas: Amount::from_u128(20),
        max_priority_fee_per_gas: Amount::from_u128(2),
        valid_until_block: BlockHeight(100),
        data: d_c1,
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_c1, _) = sign_eth_tx(&kp, tx_c1);
    let (b3, r3) = harness.commit_block(vec![signed_c1]);
    rpc.record_block(b3, r3);

    // Block 4: Mutate C2 slot 1 -> 222
    let mut d_c2 = slot.to_vec();
    d_c2.extend_from_slice(&val_c2);
    let tx_c2 = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(3),
        from: deployer,
        to: Some(c2_addr),
        value: Amount::ZERO,
        gas_limit: Gas(200_000),
        max_fee_per_gas: Amount::from_u128(20),
        max_priority_fee_per_gas: Amount::from_u128(2),
        valid_until_block: BlockHeight(100),
        data: d_c2,
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_c2, _) = sign_eth_tx(&kp, tx_c2);
    let (b4, r4) = harness.commit_block(vec![signed_c2]);
    rpc.record_block(b4, r4);

    let slot_hex = format!("0x{}", hex_encode(&slot));

    // C1 at Block 2 has 0; at Block 3 has 111; at Block 4 has 111
    let c1_b2 = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"eth_getStorageAt","params":["{}","{}","0x2"]}}"#,
        c1_addr.to_hex(),
        slot_hex
    ));
    assert!(c1_b2.contains(&Hash::ZERO.to_hex()));

    let c1_b3 = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"eth_getStorageAt","params":["{}","{}","0x3"]}}"#,
        c1_addr.to_hex(),
        slot_hex
    ));
    assert!(c1_b3.contains(&hex_encode(&val_c1)));

    let c1_b4 = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":3,"method":"eth_getStorageAt","params":["{}","{}","0x4"]}}"#,
        c1_addr.to_hex(),
        slot_hex
    ));
    assert!(c1_b4.contains(&hex_encode(&val_c1)));

    // C2 at Block 3 has 0; at Block 4 has 222
    let c2_b3 = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":4,"method":"eth_getStorageAt","params":["{}","{}","0x3"]}}"#,
        c2_addr.to_hex(),
        slot_hex
    ));
    assert!(c2_b3.contains(&Hash::ZERO.to_hex()));

    let c2_b4 = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":5,"method":"eth_getStorageAt","params":["{}","{}","0x4"]}}"#,
        c2_addr.to_hex(),
        slot_hex
    ));
    assert!(c2_b4.contains(&hex_encode(&val_c2)));
}

// =========================================================================
// TEST O — EIP-1898 blockNumber and blockHash Object Queries
// =========================================================================

#[test]
fn test_p8_o_eip1898_blocknumber_and_blockhash_queries() {
    let mut harness = TestHarness::new("test_o_eip1898");
    let kp = Secp256k1Keypair::generate();
    let addr = kp.address();
    harness.fund_account(&addr, 700_000_000);

    let (b1, r1) = harness.commit_block(vec![]);
    let rpc = harness.make_dispatcher();
    rpc.record_block(b1.clone(), r1);

    let b1_hash = tola_consensus::PipelinedBftEngine::compute_block_hash(&b1);

    // 1. EIP-1898 query with blockNumber object: { "blockNumber": "0x1" }
    let res_obj_num = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"eth_getBalance","params":["{}", {{ "blockNumber": "0x1" }}]}}"#,
        addr.to_hex()
    ));
    assert!(res_obj_num.contains(&format!("0x{:x}", 700_000_000u64)));

    // 2. EIP-1898 query with blockHash object: { "blockHash": "0x...", "requireCanonical": true }
    let res_obj_hash = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"eth_getBalance","params":["{}", {{ "blockHash": "{}", "requireCanonical": true }}]}}"#,
        addr.to_hex(),
        b1_hash.to_hex()
    ));
    assert!(res_obj_hash.contains(&format!("0x{:x}", 700_000_000u64)));

    // 3. EIP-1898 query with unknown blockHash -> error -32000
    let fake_hash = Hash([0xee; 32]);
    let res_obj_fake = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":3,"method":"eth_getBalance","params":["{}", {{ "blockHash": "{}", "requireCanonical": true }}]}}"#,
        addr.to_hex(),
        fake_hash.to_hex()
    ));
    assert!(res_obj_fake.contains("-32000"));
}

// =========================================================================
// TEST P — Historical Multi-Step Call Trace / Nested Call
// =========================================================================

#[test]
fn test_p8_p_historical_nested_call_trace() {
    let mut harness = TestHarness::new("test_p_nested");
    let kp = Secp256k1Keypair::generate();
    let deployer = kp.address();
    harness.fund_account(&deployer, 1_000_000_000);

    // Deploy Contract B (StorageStore) at Block 1
    let runtime_b = storage_contract_runtime();
    let initcode_b = make_initcode(&runtime_b);
    let deploy_b = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce::ZERO,
        from: deployer,
        to: None,
        value: Amount::ZERO,
        gas_limit: Gas(200_000),
        max_fee_per_gas: Amount::from_u128(20),
        max_priority_fee_per_gas: Amount::from_u128(2),
        valid_until_block: BlockHeight(100),
        data: initcode_b,
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_b, _) = sign_eth_tx(&kp, deploy_b);
    let addr_b = compute_contract_address(&deployer, 0);
    let (b1, r1) = harness.commit_block(vec![signed_b]);
    let rpc = harness.make_dispatcher();
    rpc.record_block(b1, r1);

    // Deploy Contract A (NestedCaller) at Block 2
    let runtime_a = nested_caller_runtime();
    let initcode_a = make_initcode(&runtime_a);
    let deploy_a = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(1),
        from: deployer,
        to: None,
        value: Amount::ZERO,
        gas_limit: Gas(200_000),
        max_fee_per_gas: Amount::from_u128(20),
        max_priority_fee_per_gas: Amount::from_u128(2),
        valid_until_block: BlockHeight(100),
        data: initcode_a,
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_a, _) = sign_eth_tx(&kp, deploy_a);
    let addr_a = compute_contract_address(&deployer, 1);
    let (b2, r2) = harness.commit_block(vec![signed_a]);
    rpc.record_block(b2, r2);

    let slot = pad32_u64(0x77);
    let val_b3 = pad32_u128(999);

    // Block 3: Mutate Contract B slot 0x77 -> 999
    let mut d_b3 = slot.to_vec();
    d_b3.extend_from_slice(&val_b3);
    let tx_mutate = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(2),
        from: deployer,
        to: Some(addr_b),
        value: Amount::ZERO,
        gas_limit: Gas(200_000),
        max_fee_per_gas: Amount::from_u128(20),
        max_priority_fee_per_gas: Amount::from_u128(2),
        valid_until_block: BlockHeight(100),
        data: d_b3,
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_mut, _) = sign_eth_tx(&kp, tx_mutate);
    let (b3, r3) = harness.commit_block(vec![signed_mut]);
    rpc.record_block(b3, r3);

    // Query Contract A: data is [addr_b padded to 32 bytes] || [slot 0x77]
    let mut call_data = pad32_addr(&addr_b).to_vec();
    call_data.extend_from_slice(&slot);
    let call_data_hex = hex_encode(&call_data);

    // Simulation at Block 2: Contract B slot 0x77 is 0
    let call_at_b2 = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"eth_call","params":[{{ "to": "{}", "data": "0x{}" }}, "0x2"]}}"#,
        addr_a.to_hex(),
        call_data_hex
    ));
    assert!(call_at_b2.contains(&Hash::ZERO.to_hex()));

    // Simulation at Block 3: Contract B slot 0x77 is 999
    let call_at_b3 = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"eth_call","params":[{{ "to": "{}", "data": "0x{}" }}, "0x3"]}}"#,
        addr_a.to_hex(),
        call_data_hex
    ));
    assert!(call_at_b3.contains(&hex_encode(&val_b3)));
}

// =========================================================================
// TEST Q — Unified State Root Monotonic Determinism
// =========================================================================

#[test]
fn test_p8_q_unified_state_root_monotonic_determinism() {
    let mut harness = TestHarness::new("test_q_root");
    let kp = Secp256k1Keypair::generate();
    let addr = kp.address();
    harness.fund_account(&addr, 1_000_000);

    let (b1, r1) = harness.commit_block(vec![]);
    let rpc = harness.make_dispatcher();
    rpc.record_block(b1.clone(), r1);

    let root_before = harness
        .state_arc
        .read()
        .unwrap()
        .compute_state_root()
        .unwrap();

    // Perform dozens of historical reads across multiple methods
    for _ in 0..10 {
        let _ = rpc.dispatch_json(&format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"eth_getBalance","params":["{}","0x1"]}}"#,
            addr.to_hex()
        ));
        let _ = rpc.dispatch_json(&format!(
            r#"{{"jsonrpc":"2.0","id":2,"method":"eth_getCode","params":["{}","0x1"]}}"#,
            addr.to_hex()
        ));
        let _ = rpc.dispatch_json(&format!(
            r#"{{"jsonrpc":"2.0","id":3,"method":"eth_getStorageAt","params":["{}","0x0","0x1"]}}"#,
            addr.to_hex()
        ));
    }

    let root_after = harness
        .state_arc
        .read()
        .unwrap()
        .compute_state_root()
        .unwrap();

    assert_eq!(root_before, root_after);
}

// =========================================================================
// TEST R — Mempool Pending Overlay Isolation
// =========================================================================

#[test]
fn test_p8_r_mempool_pending_overlay_isolation() {
    let mut harness = TestHarness::new("test_r_mempool");
    let kp = Secp256k1Keypair::generate();
    let addr = kp.address();
    let dest = Address([0x44; 20]);
    harness.fund_account(&addr, 10_000_000);

    let (b1, r1) = harness.commit_block(vec![]);
    let rpc = harness.make_dispatcher();
    rpc.record_block(b1, r1);

    // Insert pending transaction into mempool with nonce 0
    let tx_pending = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce::ZERO,
        from: addr,
        to: Some(dest),
        value: Amount::from_u128(1_000),
        gas_limit: Gas(21_000),
        max_fee_per_gas: Amount::from_u128(20),
        max_priority_fee_per_gas: Amount::ZERO,
        valid_until_block: BlockHeight(100),
        data: vec![],
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_pending, _) = sign_eth_tx(&kp, tx_pending);
    {
        let mut mp = rpc.mempool.write().unwrap();
        let state = rpc.state_db.read().unwrap();
        let cur_h = rpc.get_current_height();
        mp.insert(signed_pending, &*state, BlockHeight(cur_h))
            .unwrap();
    }

    // 1. eth_getTransactionCount at "pending": reflects pending nonce (0x1)
    let n_pending = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"eth_getTransactionCount","params":["{}","pending"]}}"#,
        addr.to_hex()
    ));
    assert!(n_pending.contains("\"0x1\""));

    // 2. eth_getTransactionCount at "latest": committed state only (0x0)
    let n_latest = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"eth_getTransactionCount","params":["{}","latest"]}}"#,
        addr.to_hex()
    ));
    assert!(n_latest.contains("\"0x0\""));

    // 3. eth_getTransactionCount at Block 1: committed state only (0x0)
    let n_b1 = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":3,"method":"eth_getTransactionCount","params":["{}","0x1"]}}"#,
        addr.to_hex()
    ));
    assert!(n_b1.contains("\"0x0\""));
}

// =========================================================================
// TEST S — Atomic Block Bundle Commit Invariance
// =========================================================================

#[test]
fn test_p8_s_atomic_block_bundle_commit_invariance() {
    let mut harness = TestHarness::new("test_s_bundle");
    let kp = Secp256k1Keypair::generate();
    let deployer = kp.address();
    harness.fund_account(&deployer, 1_000_000_000);

    let runtime = storage_contract_runtime();
    let initcode = make_initcode(&runtime);
    let deploy_tx = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce::ZERO,
        from: deployer,
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
    let contract_addr = compute_contract_address(&deployer, 0);

    let (b1, r1) = harness.commit_block(vec![signed_deploy]);
    assert!(r1[0].success);

    // Verify all atomic bundle tables were populated for block 1:
    let state_guard = harness.state_arc.read().unwrap();

    // 1. Header is present in Table::Headers
    let hdr = state_guard
        .get_block_header_by_height(BlockHeight(1))
        .unwrap();
    assert!(hdr.is_some());
    assert_eq!(hdr.unwrap().height, BlockHeight(1));

    // 2. Block body is present in Table::BlockBodies
    let b = state_guard.get_block_by_height(BlockHeight(1)).unwrap();
    assert!(b.is_some());
    assert_eq!(b.unwrap().transactions.len(), 1);

    // 3. Receipt is present in Table::Receipts
    let tx_bytes = b1.transactions[0].to_canonical_bytes();
    let tx_id = TransactionId(tola_crypto::blake3_hash(&tx_bytes));
    let rec = state_guard.get_receipt(&tx_id).unwrap();
    assert!(rec.is_some());

    // 4. Contract account exists in Table::Accounts
    let acct = state_guard.get_account(&contract_addr).unwrap();
    assert!(acct.is_some());

    // 5. Code is present in Table::Code
    let code = state_guard.get_code(&acct.unwrap().code_hash).unwrap();
    assert!(code.is_some());
}
