//! TOLA EVM Phase 7 (P7) Solidity & Precompile Compatibility Integration Test Suite
//!
//! Verifies:
//! 1. Real Solidity 0.8.24 compiler-generated artifacts (committed under `contracts/compiled/`).
//! 2. EVM Shanghai target compatibility (`--evm-version shanghai`).
//! 3. OpenZeppelin Contracts v5.0.2 (ERC-20, ERC-721, ERC-1155, UUPS Upgradeable).
//! 4. Canonical EIP-1967 proxy storage slot (0x360894a13ba1a3210667c828492db98dca3e2076cc3735a920a3ca505d382bbc).
//! 5. Solidity 0.8+ custom errors and revert payload decoding.
//! 6. Ethereum Shanghai precompiles 0x01 through 0x09 against independent authoritative oracles.
//! 7. Precompile gas reconciliation and OOG sub-call containment.
//! 8. Real contract state persistence across cold database restarts.

use std::sync::{Arc, RwLock};
use tola_core::{BlockEngine, StateDB, StateDelta};
use tola_crypto::{keccak256_hash, Secp256k1Keypair};
use tola_execution::SequentialBlockEngine;
use tola_mempool::{Mempool, MempoolConfig};
use tola_rpc::dispatcher::RpcDispatcher;
use tola_rpc::json::JsonValue;
use tola_storage::PersistentStateDB;
use tola_types::*;

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

fn compute_contract_address(sender: &Address, nonce: u64) -> Address {
    tola_codec::rlp::compute_contract_address(sender, nonce)
}

fn selector(sig: &str) -> [u8; 4] {
    let h = keccak256_hash(sig.as_bytes());
    let mut s = [0u8; 4];
    s.copy_from_slice(&h.as_bytes()[0..4]);
    s
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

fn pad32_bool(b: bool) -> [u8; 32] {
    let mut buf = [0u8; 32];
    if b {
        buf[31] = 1;
    }
    buf
}

fn encode_dynamic_string(s: &str) -> Vec<u8> {
    let bytes = s.as_bytes();
    let len = bytes.len();
    let mut out = Vec::new();
    out.extend_from_slice(&pad32_u128(len as u128));
    out.extend_from_slice(bytes);
    let remainder = len % 32;
    if remainder != 0 {
        out.extend(vec![0u8; 32 - remainder]);
    }
    out
}

fn encode_dynamic_bytes(bytes: &[u8]) -> Vec<u8> {
    let len = bytes.len();
    let mut out = Vec::new();
    out.extend_from_slice(&pad32_u128(len as u128));
    out.extend_from_slice(bytes);
    let remainder = len % 32;
    if remainder != 0 {
        out.extend(vec![0u8; 32 - remainder]);
    }
    out
}

// =========================================================================
// Test Harness
// =========================================================================

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
        let db_path = std::env::temp_dir().join(format!("tola_p7_{test_name}_{nanos}"));
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
            .unwrap();
        self.parent_hash = block_hash;
        (block, receipts)
    }

    fn make_dispatcher(&self) -> Arc<RpcDispatcher<PersistentStateDB>> {
        let mempool = Arc::new(RwLock::new(Mempool::new(MempoolConfig::new(TEST_CHAIN_ID))));
        Arc::new(RpcDispatcher::new(
            TEST_CHAIN_ID,
            "TOLA-Node-P7/v0.1.0".to_string(),
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

// Helper to load committed artifact
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

// Embedded committed artifacts
const MINIMAL_STATE_JSON: &str = include_str!("../../../contracts/compiled/MinimalState.json");
const ERC20_MOCK_JSON: &str = include_str!("../../../contracts/compiled/ERC20Mock.json");
const ERC721_MOCK_JSON: &str = include_str!("../../../contracts/compiled/ERC721Mock.json");
const ERC721_RECEIVER_MOCK_JSON: &str =
    include_str!("../../../contracts/compiled/ERC721ReceiverMock.json");
const ERC1155_MOCK_JSON: &str = include_str!("../../../contracts/compiled/ERC1155Mock.json");
const ERC1155_RECEIVER_MOCK_JSON: &str =
    include_str!("../../../contracts/compiled/ERC1155ReceiverMock.json");
const PRECOMPILE_CALLER_JSON: &str =
    include_str!("../../../contracts/compiled/PrecompileCaller.json");
const CALL_ROUTER_JSON: &str = include_str!("../../../contracts/compiled/CallRouter.json");
const TARGET_CONTRACT_JSON: &str = include_str!("../../../contracts/compiled/TargetContract.json");
const CUSTOM_ERROR_TEST_JSON: &str =
    include_str!("../../../contracts/compiled/CustomErrorTest.json");
const EIP1967_PROXY_WRAPPER_JSON: &str =
    include_str!("../../../contracts/compiled/EIP1967ProxyWrapper.json");
const UUPS_IMPL_MOCK_JSON: &str =
    include_str!("../../../contracts/compiled/UUPSImplementationMock.json");
const UUPS_IMPL_V2_MOCK_JSON: &str =
    include_str!("../../../contracts/compiled/UUPSImplementationV2Mock.json");

// =========================================================================
// 1. Precompiles Complete Shanghai Matrix (0x01 - 0x09)
// =========================================================================

#[test]
fn test_p7_precompiles_complete_shanghai_matrix() {
    let mut harness = TestHarness::new("precompiles_matrix");
    let kp = Secp256k1Keypair::generate();
    let sender = kp.address();
    harness.fund_account(&sender, 100_000_000_000);
    let rpc = harness.make_dispatcher();

    // 0x01: ecrecover
    // Test 1: Valid signature recovery against independent k256 oracle
    {
        let msg = b"TOLA Network P7 Precompile Oracle";
        let msg_hash = keccak256_hash(msg);
        let sig = kp.sign_recoverable(msg_hash.as_bytes()).unwrap();

        let mut input = Vec::with_capacity(128);
        input.extend_from_slice(msg_hash.as_bytes());
        input.extend_from_slice(&pad32_u128(sig[64] as u128 + 27)); // v
        input.extend_from_slice(&sig[0..32]); // r
        input.extend_from_slice(&sig[32..64]); // s

        let res = rpc.dispatch_json(&format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"eth_call","params":[{{ "to": "0x0000000000000000000000000000000000000001", "data": "0x{}" }}]}}"#,
            hex_encode(&input)
        ));
        let expected_addr_padded = pad32_addr(&sender);
        assert!(res.contains(&hex_encode(&expected_addr_padded)));

        // Test 2: Invalid v (v=0) returns empty returndata
        let mut bad_v_input = input.clone();
        bad_v_input[32..64].copy_from_slice(&[0u8; 32]);
        let res_bad_v = rpc.dispatch_json(&format!(
            r#"{{"jsonrpc":"2.0","id":2,"method":"eth_call","params":[{{ "to": "0x0000000000000000000000000000000000000001", "data": "0x{}" }}]}}"#,
            hex_encode(&bad_v_input)
        ));
        assert!(res_bad_v.contains(r#""result":"0x""#));

        // Test 3: High-S recovery per EIP-2 / Yellow Paper semantics
        // Precompile 0x01 accepts high-S (s > N/2) when s < N, recovering sender with flipped v
        let curve_n_bytes =
            hex_decode("fffffffffffffffffffffffffffffffebaaedce6af48a03bbfd25e8cd0364141");
        let curve_n = num_bigint::BigUint::from_bytes_be(&curve_n_bytes);
        let s_low = num_bigint::BigUint::from_bytes_be(&sig[32..64]);
        let s_high = &curve_n - &s_low;
        let s_high_vec = s_high.to_bytes_be();
        let mut s_high_bytes = [0u8; 32];
        s_high_bytes[32 - s_high_vec.len()..].copy_from_slice(&s_high_vec);
        let v_orig = sig[64] + 27;
        let v_high = if v_orig == 27 { 28u8 } else { 27u8 };

        let mut high_s_input = Vec::with_capacity(128);
        high_s_input.extend_from_slice(msg_hash.as_bytes());
        high_s_input.extend_from_slice(&pad32_u128(v_high as u128));
        high_s_input.extend_from_slice(&sig[0..32]);
        high_s_input.extend_from_slice(&s_high_bytes);

        let res_high_s = rpc.dispatch_json(&format!(
            r#"{{"jsonrpc":"2.0","id":3,"method":"eth_call","params":[{{ "to": "0x0000000000000000000000000000000000000001", "data": "0x{}" }}]}}"#,
            hex_encode(&high_s_input)
        ));
        assert!(res_high_s.contains(&hex_encode(&expected_addr_padded)));
    }

    // 0x02: SHA-256 against independent sha2::Sha256 oracle
    {
        use sha2::Digest;
        let test_data = b"TOLA Protocol SHA-256 Independent Verification";
        let mut hasher = sha2::Sha256::new();
        hasher.update(test_data);
        let expected_sha256 = hasher.finalize();

        let res = rpc.dispatch_json(&format!(
            r#"{{"jsonrpc":"2.0","id":3,"method":"eth_call","params":[{{ "to": "0x0000000000000000000000000000000000000002", "data": "0x{}" }}]}}"#,
            hex_encode(test_data)
        ));
        assert!(res.contains(&hex_encode(&expected_sha256)));
    }

    // 0x03: RIPEMD-160 against independent ripemd::Ripemd160 oracle
    {
        use ripemd::Digest;
        let test_data = b"TOLA Protocol RIPEMD-160 Verification";
        let mut hasher = ripemd::Ripemd160::new();
        hasher.update(test_data);
        let ripemd_hash = hasher.finalize();
        let mut expected_padded = [0u8; 32];
        expected_padded[12..32].copy_from_slice(&ripemd_hash);

        let res = rpc.dispatch_json(&format!(
            r#"{{"jsonrpc":"2.0","id":4,"method":"eth_call","params":[{{ "to": "0x0000000000000000000000000000000000000003", "data": "0x{}" }}]}}"#,
            hex_encode(test_data)
        ));
        assert!(res.contains(&hex_encode(&expected_padded)));
    }

    // 0x04: identity precompile
    {
        let test_payload =
            hex_decode("0123456789abcdefdeadbeefcafebabe112233445566778899aabbccddeeff");
        let res = rpc.dispatch_json(&format!(
            r#"{{"jsonrpc":"2.0","id":5,"method":"eth_call","params":[{{ "to": "0x0000000000000000000000000000000000000004", "data": "0x{}" }}]}}"#,
            hex_encode(&test_payload)
        ));
        assert!(res.contains(&hex_encode(&test_payload)));
    }

    // 0x05: modexp against independent num_bigint::BigUint oracle
    {
        use num_bigint::BigUint;
        let b = BigUint::from(3u32);
        let e = BigUint::from(5u32);
        let m = BigUint::from(7u32);
        let expected_modexp = b.modpow(&e, &m); // 3^5 % 7 = 243 % 7 = 5

        let mut input = Vec::new();
        input.extend_from_slice(&pad32_u128(1)); // Bsize = 1
        input.extend_from_slice(&pad32_u128(1)); // Esize = 1
        input.extend_from_slice(&pad32_u128(1)); // Msize = 1
        input.push(3); // Base
        input.push(5); // Exp
        input.push(7); // Mod

        let res = rpc.dispatch_json(&format!(
            r#"{{"jsonrpc":"2.0","id":6,"method":"eth_call","params":[{{ "to": "0x0000000000000000000000000000000000000005", "data": "0x{}" }}]}}"#,
            hex_encode(&input)
        ));
        let expected_byte = expected_modexp.to_bytes_be();
        assert!(res.contains(&hex_encode(&expected_byte)));
    }

    // 0x06: alt_bn128 ecAdd
    // Point at infinity (0,0) + Point at infinity (0,0) = (0,0)
    {
        let zero_points = vec![0u8; 128];
        let res = rpc.dispatch_json(&format!(
            r#"{{"jsonrpc":"2.0","id":7,"method":"eth_call","params":[{{ "to": "0x0000000000000000000000000000000000000006", "data": "0x{}" }}]}}"#,
            hex_encode(&zero_points)
        ));
        assert!(res.contains(&hex_encode(&zero_points[..64])));
    }

    // 0x07: alt_bn128 ecMul
    // Generator point (1, 2) * 1 = (1, 2)
    {
        let mut input = Vec::new();
        input.extend_from_slice(&pad32_u128(1)); // X = 1
        input.extend_from_slice(&pad32_u128(2)); // Y = 2
        input.extend_from_slice(&pad32_u128(1)); // Scalar = 1

        let res = rpc.dispatch_json(&format!(
            r#"{{"jsonrpc":"2.0","id":8,"method":"eth_call","params":[{{ "to": "0x0000000000000000000000000000000000000007", "data": "0x{}" }}]}}"#,
            hex_encode(&input)
        ));
        let mut expected = Vec::new();
        expected.extend_from_slice(&pad32_u128(1));
        expected.extend_from_slice(&pad32_u128(2));
        assert!(res.contains(&hex_encode(&expected)));
    }

    // 0x08: alt_bn128 ecPairing
    // Empty input returns 1 per EIP-197
    {
        let res = rpc.dispatch_json(
            r#"{"jsonrpc":"2.0","id":9,"method":"eth_call","params":[{"to":"0x0000000000000000000000000000000000000008","data":"0x"}]}"#,
        );
        let expected_one = pad32_u128(1);
        assert!(res.contains(&hex_encode(&expected_one)));
    }

    // 0x09: blake2f compression function
    // Verified with valid rounds and compression inputs
    {
        // 4 bytes rounds + 64 bytes h + 128 bytes m + 16 bytes t + 1 byte f = 213 bytes
        let mut input = vec![0u8; 213];
        input[3] = 12; // 12 rounds
        input[212] = 1; // f = 1 (final block)

        let res = rpc.dispatch_json(&format!(
            r#"{{"jsonrpc":"2.0","id":10,"method":"eth_call","params":[{{ "to": "0x0000000000000000000000000000000000000009", "data": "0x{}" }}]}}"#,
            hex_encode(&input)
        ));
        // Verify valid 64-byte return
        let val = JsonValue::parse(&res).unwrap();
        let ret = val.get("result").unwrap().as_str().unwrap();
        assert_eq!(ret.len(), 2 + 128); // 0x + 64 bytes hex
    }
}

// =========================================================================
// 2. Minimal State Contract Lifecycle (Real Solc Bytecode)
// =========================================================================

#[test]
fn test_p7_solidity_minimal_contract_lifecycle() {
    let mut harness = TestHarness::new("minimal_state");
    let kp = Secp256k1Keypair::generate();
    let sender = kp.address();
    let initial_balance = 10_000_000_000u128;
    harness.fund_account(&sender, initial_balance);

    let (creation_code, runtime_code) = load_artifact(MINIMAL_STATE_JSON);

    // Constructor parameters: (uint256 12345, address sender, bool true)
    let mut initcode = creation_code;
    initcode.extend_from_slice(&pad32_u128(12345));
    initcode.extend_from_slice(&pad32_addr(&sender));
    initcode.extend_from_slice(&pad32_bool(true));

    let deploy_tx = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce::ZERO,
        from: sender,
        to: None,
        value: Amount::ZERO,
        gas_limit: Gas(300_000),
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

    let (b1, r1) = harness.commit_block(vec![signed_deploy]);
    assert!(r1[0].success);

    // Verify runtime code in Table::Code
    let rpc = harness.make_dispatcher();
    rpc.record_block(b1, r1);
    let code_res = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"eth_getCode","params":["{}","latest"]}}"#,
        contract_addr.to_hex()
    ));
    assert!(code_res.contains(&hex_encode(&runtime_code)));

    // Verify getters via eth_call
    // getValue(): 0x20965255
    let res_val = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"eth_call","params":[{{ "to": "{}", "data": "0x{}" }}]}}"#,
        contract_addr.to_hex(),
        hex_encode(&selector("getValue()"))
    ));
    assert!(res_val.contains(&hex_encode(&pad32_u128(12345))));

    // getOwner(): 0x893d20e8
    let res_owner = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":3,"method":"eth_call","params":[{{ "to": "{}", "data": "0x{}" }}]}}"#,
        contract_addr.to_hex(),
        hex_encode(&selector("getOwner()"))
    ));
    assert!(res_owner.contains(&hex_encode(&pad32_addr(&sender))));

    // getFlag(): 0x51cff8d9
    let res_flag = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":4,"method":"eth_call","params":[{{ "to": "{}", "data": "0x{}" }}]}}"#,
        contract_addr.to_hex(),
        hex_encode(&selector("getFlag()"))
    ));
    assert!(res_flag.contains(&hex_encode(&pad32_bool(true))));

    // Mutate state via setValue(99999)
    let mut call_data = selector("setValue(uint256)").to_vec();
    call_data.extend_from_slice(&pad32_u128(99999));

    let mutate_tx = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(1),
        from: sender,
        to: Some(contract_addr),
        value: Amount::ZERO,
        gas_limit: Gas(100_000),
        max_fee_per_gas: Amount::from_u128(20),
        max_priority_fee_per_gas: Amount::from_u128(2),
        valid_until_block: BlockHeight(100),
        data: call_data,
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_mutate, _) = sign_eth_tx(&kp, mutate_tx);
    let (b2, r2) = harness.commit_block(vec![signed_mutate]);
    assert!(r2[0].success);
    rpc.record_block(b2, r2);

    // Verify updated value
    let res_val_after = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":5,"method":"eth_call","params":[{{ "to": "{}", "data": "0x{}" }}]}}"#,
        contract_addr.to_hex(),
        hex_encode(&selector("getValue()"))
    ));
    assert!(res_val_after.contains(&hex_encode(&pad32_u128(99999))));
}

// =========================================================================
// 3. OpenZeppelin ERC-20 Mock (Real OpenZeppelin v5.0.2)
// =========================================================================

#[test]
fn test_p7_solidity_openzeppelin_erc20() {
    let mut harness = TestHarness::new("oz_erc20");
    let kp_owner = Secp256k1Keypair::generate();
    let owner = kp_owner.address();
    let kp_bob = Secp256k1Keypair::generate();
    let bob = kp_bob.address();
    let kp_charlie = Secp256k1Keypair::generate();
    let charlie = kp_charlie.address();

    harness.fund_account(&owner, 10_000_000_000);
    harness.fund_account(&bob, 10_000_000_000);
    harness.fund_account(&charlie, 10_000_000_000);

    let (creation_code, _) = load_artifact(ERC20_MOCK_JSON);

    // Constructor: string name, string symbol, address initialAccount, uint256 initialBalance
    let mut initcode = creation_code;
    // Head: 4 words
    initcode.extend_from_slice(&pad32_u128(128)); // offset to name
    let name_bytes = encode_dynamic_string("TolaToken");
    let offset_symbol = 128 + name_bytes.len();
    initcode.extend_from_slice(&pad32_u128(offset_symbol as u128));
    initcode.extend_from_slice(&pad32_addr(&owner));
    initcode.extend_from_slice(&pad32_u128(1_000_000));
    // Tail:
    initcode.extend_from_slice(&name_bytes);
    initcode.extend_from_slice(&encode_dynamic_string("TOLA"));

    let deploy_tx = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce::ZERO,
        from: owner,
        to: None,
        value: Amount::ZERO,
        gas_limit: Gas(1_500_000),
        max_fee_per_gas: Amount::from_u128(20),
        max_priority_fee_per_gas: Amount::from_u128(2),
        valid_until_block: BlockHeight(100),
        data: initcode,
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

    // 1. Verify totalSupply() == 1,000,000
    let res_supply = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"eth_call","params":[{{ "to": "{}", "data": "0x{}" }}]}}"#,
        token_addr.to_hex(),
        hex_encode(&selector("totalSupply()"))
    ));
    assert!(res_supply.contains(&hex_encode(&pad32_u128(1_000_000))));

    // 2. Transfer: transfer(bob, 250_000)
    let mut transfer_data = selector("transfer(address,uint256)").to_vec();
    transfer_data.extend_from_slice(&pad32_addr(&bob));
    transfer_data.extend_from_slice(&pad32_u128(250_000));

    let tx_transfer = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(1),
        from: owner,
        to: Some(token_addr),
        value: Amount::ZERO,
        gas_limit: Gas(200_000),
        max_fee_per_gas: Amount::from_u128(20),
        max_priority_fee_per_gas: Amount::from_u128(2),
        valid_until_block: BlockHeight(100),
        data: transfer_data,
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_transfer, _) = sign_eth_tx(&kp_owner, tx_transfer);
    let (b2, r2) = harness.commit_block(vec![signed_transfer]);
    assert!(r2[0].success);
    // Verify Transfer log emitted
    assert_eq!(r2[0].logs.len(), 1);
    assert_eq!(
        r2[0].logs[0].topics[0],
        Hash(keccak256_hash(b"Transfer(address,address,uint256)").0)
    );

    rpc.record_block(b2, r2);

    // 3. Verify balanceOf(bob) == 250_000
    let mut bal_bob_data = selector("balanceOf(address)").to_vec();
    bal_bob_data.extend_from_slice(&pad32_addr(&bob));
    let res_bal_bob = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"eth_call","params":[{{ "to": "{}", "data": "0x{}" }}]}}"#,
        token_addr.to_hex(),
        hex_encode(&bal_bob_data)
    ));
    assert!(res_bal_bob.contains(&hex_encode(&pad32_u128(250_000))));

    // 4. Insufficient balance custom error: transfer(charlie, 999_999_999) from bob
    let mut overspend_data = selector("transfer(address,uint256)").to_vec();
    overspend_data.extend_from_slice(&pad32_addr(&charlie));
    overspend_data.extend_from_slice(&pad32_u128(999_999_999));

    let tx_overspend = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce::ZERO,
        from: bob,
        to: Some(token_addr),
        value: Amount::ZERO,
        gas_limit: Gas(200_000),
        max_fee_per_gas: Amount::from_u128(20),
        max_priority_fee_per_gas: Amount::from_u128(2),
        valid_until_block: BlockHeight(100),
        data: overspend_data,
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_overspend, _) = sign_eth_tx(&kp_bob, tx_overspend);
    let (_b3, r3) = harness.commit_block(vec![signed_overspend]);
    assert!(!r3[0].success); // Reverts!

    // Verify OpenZeppelin ERC20InsufficientBalance selector
    // keccak256("ERC20InsufficientBalance(address,uint256,uint256)")[0..4] = 0xe450d38c
    let err_selector = selector("ERC20InsufficientBalance(address,uint256,uint256)");
    assert_eq!(r3[0].return_data[0..4], err_selector);
}

// =========================================================================
// 4. OpenZeppelin ERC-721 Mock & Receiver Callback
// =========================================================================

#[test]
fn test_p7_solidity_openzeppelin_erc721() {
    let mut harness = TestHarness::new("oz_erc721");
    let kp = Secp256k1Keypair::generate();
    let deployer = kp.address();
    harness.fund_account(&deployer, 10_000_000_000);

    let (creation_721, _) = load_artifact(ERC721_MOCK_JSON);
    let (creation_recv, _) = load_artifact(ERC721_RECEIVER_MOCK_JSON);

    // Deploy ERC721Mock: name="TolaNFT", symbol="TNFT"
    let mut initcode_721 = creation_721;
    initcode_721.extend_from_slice(&pad32_u128(64)); // offset name
    let name_bytes = encode_dynamic_string("TolaNFT");
    initcode_721.extend_from_slice(&pad32_u128((64 + name_bytes.len()) as u128));
    initcode_721.extend_from_slice(&name_bytes);
    initcode_721.extend_from_slice(&encode_dynamic_string("TNFT"));

    let deploy_721 = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce::ZERO,
        from: deployer,
        to: None,
        value: Amount::ZERO,
        gas_limit: Gas(2_000_000),
        max_fee_per_gas: Amount::from_u128(20),
        max_priority_fee_per_gas: Amount::from_u128(2),
        valid_until_block: BlockHeight(100),
        data: initcode_721,
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_721, _) = sign_eth_tx(&kp, deploy_721);
    let nft_addr = compute_contract_address(&deployer, 0);

    // Deploy ERC721ReceiverMock
    let deploy_recv = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(1),
        from: deployer,
        to: None,
        value: Amount::ZERO,
        gas_limit: Gas(500_000),
        max_fee_per_gas: Amount::from_u128(20),
        max_priority_fee_per_gas: Amount::from_u128(2),
        valid_until_block: BlockHeight(100),
        data: creation_recv,
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_recv, _) = sign_eth_tx(&kp, deploy_recv);
    let recv_addr = compute_contract_address(&deployer, 1);

    let (b1, r1) = harness.commit_block(vec![signed_721, signed_recv]);
    assert!(r1[0].success);
    assert!(r1[1].success);

    let rpc = harness.make_dispatcher();
    rpc.record_block(b1, r1);

    // Mint token 1 to Receiver contract with URI: safeMint(to, tokenId, uri)
    let mut mint_data = selector("safeMint(address,uint256,string)").to_vec();
    mint_data.extend_from_slice(&pad32_addr(&recv_addr));
    mint_data.extend_from_slice(&pad32_u128(1));
    mint_data.extend_from_slice(&pad32_u128(96)); // offset uri
    mint_data.extend_from_slice(&encode_dynamic_string("ipfs://tola-metadata-1"));

    let tx_mint = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(2),
        from: deployer,
        to: Some(nft_addr),
        value: Amount::ZERO,
        gas_limit: Gas(300_000),
        max_fee_per_gas: Amount::from_u128(20),
        max_priority_fee_per_gas: Amount::from_u128(2),
        valid_until_block: BlockHeight(100),
        data: mint_data,
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_mint, _) = sign_eth_tx(&kp, tx_mint);
    let (b2, r2) = harness.commit_block(vec![signed_mint]);
    assert!(r2[0].success);
    rpc.record_block(b2, r2);

    // Verify ownerOf(1) == recv_addr
    let mut owner_of_data = selector("ownerOf(uint256)").to_vec();
    owner_of_data.extend_from_slice(&pad32_u128(1));
    let res_owner = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"eth_call","params":[{{ "to": "{}", "data": "0x{}" }}]}}"#,
        nft_addr.to_hex(),
        hex_encode(&owner_of_data)
    ));
    assert!(res_owner.contains(&hex_encode(&pad32_addr(&recv_addr))));
}

// =========================================================================
// 5. OpenZeppelin ERC-1155 Multi-Token Mock
// =========================================================================

#[test]
fn test_p7_solidity_openzeppelin_erc1155() {
    let mut harness = TestHarness::new("oz_erc1155");
    let kp = Secp256k1Keypair::generate();
    let deployer = kp.address();
    harness.fund_account(&deployer, 10_000_000_000);

    let (creation_1155, _) = load_artifact(ERC1155_MOCK_JSON);

    // Constructor: string uri
    let mut initcode = creation_1155;
    initcode.extend_from_slice(&pad32_u128(32));
    initcode.extend_from_slice(&encode_dynamic_string(
        "https://tola.network/token/{id}.json",
    ));

    let deploy_tx = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce::ZERO,
        from: deployer,
        to: None,
        value: Amount::ZERO,
        gas_limit: Gas(2_000_000),
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
    let multi_addr = compute_contract_address(&deployer, 0);

    let (b1, r1) = harness.commit_block(vec![signed_deploy]);
    assert!(r1[0].success);

    let rpc = harness.make_dispatcher();
    rpc.record_block(b1, r1);

    // Mint token 7 with amount 500: mint(deployer, 7, 500, "")
    let mut mint_data = selector("mint(address,uint256,uint256,bytes)").to_vec();
    mint_data.extend_from_slice(&pad32_addr(&deployer));
    mint_data.extend_from_slice(&pad32_u128(7));
    mint_data.extend_from_slice(&pad32_u128(500));
    mint_data.extend_from_slice(&pad32_u128(128)); // offset bytes
    mint_data.extend_from_slice(&encode_dynamic_bytes(&[]));

    let tx_mint = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(1),
        from: deployer,
        to: Some(multi_addr),
        value: Amount::ZERO,
        gas_limit: Gas(250_000),
        max_fee_per_gas: Amount::from_u128(20),
        max_priority_fee_per_gas: Amount::from_u128(2),
        valid_until_block: BlockHeight(100),
        data: mint_data,
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_mint, _) = sign_eth_tx(&kp, tx_mint);
    let (b2, r2) = harness.commit_block(vec![signed_mint]);
    assert!(r2[0].success);
    rpc.record_block(b2, r2);

    // Verify balanceOf(deployer, 7) == 500
    let mut bal_data = selector("balanceOf(address,uint256)").to_vec();
    bal_data.extend_from_slice(&pad32_addr(&deployer));
    bal_data.extend_from_slice(&pad32_u128(7));
    let res_bal = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"eth_call","params":[{{ "to": "{}", "data": "0x{}" }}]}}"#,
        multi_addr.to_hex(),
        hex_encode(&bal_data)
    ));
    assert!(res_bal.contains(&hex_encode(&pad32_u128(500))));

    // Deploy ERC1155ReceiverMock
    let (creation_recv, _) = load_artifact(ERC1155_RECEIVER_MOCK_JSON);
    let deploy_recv = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(2),
        from: deployer,
        to: None,
        value: Amount::ZERO,
        gas_limit: Gas(500_000),
        max_fee_per_gas: Amount::from_u128(20),
        max_priority_fee_per_gas: Amount::from_u128(2),
        valid_until_block: BlockHeight(100),
        data: creation_recv,
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_recv, _) = sign_eth_tx(&kp, deploy_recv);
    let recv_addr = compute_contract_address(&deployer, 2);

    let (b3, r3) = harness.commit_block(vec![signed_recv]);
    assert!(r3[0].success);
    rpc.record_block(b3, r3);

    // safeTransferFrom(from, to, id, value, data)
    let mut xfer_data =
        selector("safeTransferFrom(address,address,uint256,uint256,bytes)").to_vec();
    xfer_data.extend_from_slice(&pad32_addr(&deployer));
    xfer_data.extend_from_slice(&pad32_addr(&recv_addr));
    xfer_data.extend_from_slice(&pad32_u128(7));
    xfer_data.extend_from_slice(&pad32_u128(200));
    xfer_data.extend_from_slice(&pad32_u128(160)); // offset of data
    xfer_data.extend_from_slice(&encode_dynamic_bytes(&[]));

    let tx_xfer = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(3),
        from: deployer,
        to: Some(multi_addr),
        value: Amount::ZERO,
        gas_limit: Gas(300_000),
        max_fee_per_gas: Amount::from_u128(20),
        max_priority_fee_per_gas: Amount::from_u128(2),
        valid_until_block: BlockHeight(100),
        data: xfer_data,
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_xfer, _) = sign_eth_tx(&kp, tx_xfer);
    let (b4, r4) = harness.commit_block(vec![signed_xfer]);
    assert!(r4[0].success);
    rpc.record_block(b4, r4);

    // Verify receiver recorded callback: lastId() == 7 and lastValue() == 200
    let res_last_id = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"eth_call","params":[{{ "to": "{}", "data": "0x{}" }}]}}"#,
        recv_addr.to_hex(),
        hex_encode(&selector("lastId()"))
    ));
    assert!(res_last_id.contains(&hex_encode(&pad32_u128(7))));

    let res_last_val = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"eth_call","params":[{{ "to": "{}", "data": "0x{}" }}]}}"#,
        recv_addr.to_hex(),
        hex_encode(&selector("lastValue()"))
    ));
    assert!(res_last_val.contains(&hex_encode(&pad32_u128(200))));
}

// =========================================================================
// 6. Solidity ecrecover Integration
// =========================================================================

#[test]
fn test_p7_solidity_ecrecover_integration() {
    let mut harness = TestHarness::new("sol_ecrecover");
    let kp = Secp256k1Keypair::generate();
    let deployer = kp.address();
    harness.fund_account(&deployer, 10_000_000_000);

    let (creation_code, _) = load_artifact(PRECOMPILE_CALLER_JSON);

    let deploy_tx = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce::ZERO,
        from: deployer,
        to: None,
        value: Amount::ZERO,
        gas_limit: Gas(1_000_000),
        max_fee_per_gas: Amount::from_u128(20),
        max_priority_fee_per_gas: Amount::from_u128(2),
        valid_until_block: BlockHeight(100),
        data: creation_code,
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_deploy, _) = sign_eth_tx(&kp, deploy_tx);
    let caller_addr = compute_contract_address(&deployer, 0);

    let (b1, r1) = harness.commit_block(vec![signed_deploy]);
    assert!(r1[0].success);

    let rpc = harness.make_dispatcher();
    rpc.record_block(b1, r1);

    // Sign message hash
    let msg_hash = keccak256_hash(b"Solidity ecrecover precompile test payload");
    let sig = kp.sign_recoverable(msg_hash.as_bytes()).unwrap();

    // callEcrecover(bytes32,uint8,bytes32,bytes32)
    let mut call_data = selector("callEcrecover(bytes32,uint8,bytes32,bytes32)").to_vec();
    call_data.extend_from_slice(msg_hash.as_bytes());
    call_data.extend_from_slice(&pad32_u128(sig[64] as u128 + 27));
    call_data.extend_from_slice(&sig[0..32]);
    call_data.extend_from_slice(&sig[32..64]);

    let res = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"eth_call","params":[{{ "to": "{}", "data": "0x{}" }}]}}"#,
        caller_addr.to_hex(),
        hex_encode(&call_data)
    ));
    assert!(res.contains(&hex_encode(&pad32_addr(&deployer))));

    // High-S investigation:
    // Curve order N = 0xFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFEBAAEDCE6AF48A03BBFD25E8CD0364141
    use num_bigint::BigUint;
    let curve_n_bytes =
        hex_decode("fffffffffffffffffffffffffffffffebaaedce6af48a03bbfd25e8cd0364141");
    let curve_n = BigUint::from_bytes_be(&curve_n_bytes);
    let s_low = BigUint::from_bytes_be(&sig[32..64]);
    let s_high = &curve_n - &s_low;
    let s_high_vec = s_high.to_bytes_be();
    let mut s_high_bytes = [0u8; 32];
    s_high_bytes[32 - s_high_vec.len()..].copy_from_slice(&s_high_vec);

    // EIP-2 note: EIP-2 enforced low-S for transaction signatures, but explicitly left
    // the ecrecover precompile (0x01) capable of recovering high-S signatures.
    // For high-S, parity v flips (27 <-> 28)
    let v_orig = sig[64] + 27;
    let v_high = if v_orig == 27 { 28u8 } else { 27u8 };

    // 1. Recover with high-S and flipped v via Solidity ecrecover
    let mut call_data_high_s = selector("callEcrecover(bytes32,uint8,bytes32,bytes32)").to_vec();
    call_data_high_s.extend_from_slice(msg_hash.as_bytes());
    call_data_high_s.extend_from_slice(&pad32_u128(v_high as u128));
    call_data_high_s.extend_from_slice(&sig[0..32]);
    call_data_high_s.extend_from_slice(&s_high_bytes);

    let res_high_s = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"eth_call","params":[{{ "to": "{}", "data": "0x{}" }}]}}"#,
        caller_addr.to_hex(),
        hex_encode(&call_data_high_s)
    ));
    assert!(res_high_s.contains(&hex_encode(&pad32_addr(&deployer))));

    // 2. High-S with unflipped v does NOT recover deployer
    let mut call_data_wrong_v = selector("callEcrecover(bytes32,uint8,bytes32,bytes32)").to_vec();
    call_data_wrong_v.extend_from_slice(msg_hash.as_bytes());
    call_data_wrong_v.extend_from_slice(&pad32_u128(v_orig as u128));
    call_data_wrong_v.extend_from_slice(&sig[0..32]);
    call_data_wrong_v.extend_from_slice(&s_high_bytes);

    let res_wrong_v = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":3,"method":"eth_call","params":[{{ "to": "{}", "data": "0x{}" }}]}}"#,
        caller_addr.to_hex(),
        hex_encode(&call_data_wrong_v)
    ));
    assert!(!res_wrong_v.contains(&hex_encode(&pad32_addr(&deployer))));

    // 3. Out-of-range s (s >= N) returns address(0)
    let s_out_of_range = curve_n.to_bytes_be();
    let mut call_data_oor = selector("callEcrecover(bytes32,uint8,bytes32,bytes32)").to_vec();
    call_data_oor.extend_from_slice(msg_hash.as_bytes());
    call_data_oor.extend_from_slice(&pad32_u128(v_orig as u128));
    call_data_oor.extend_from_slice(&sig[0..32]);
    call_data_oor.extend_from_slice(&s_out_of_range);

    let res_oor = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":4,"method":"eth_call","params":[{{ "to": "{}", "data": "0x{}" }}]}}"#,
        caller_addr.to_hex(),
        hex_encode(&call_data_oor)
    ));
    assert!(res_oor.contains(&hex_encode(&pad32_addr(&Address::ZERO))));
}

// =========================================================================
// 7. Contract-to-Contract Calls (CALL & STATICCALL)
// =========================================================================

#[test]
fn test_p7_solidity_contract_to_contract_calls() {
    let mut harness = TestHarness::new("contract_calls");
    let kp = Secp256k1Keypair::generate();
    let deployer = kp.address();
    harness.fund_account(&deployer, 10_000_000_000);

    let (creation_target, _) = load_artifact(TARGET_CONTRACT_JSON);
    let (creation_router, _) = load_artifact(CALL_ROUTER_JSON);

    // Deploy TargetContract
    let deploy_target = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce::ZERO,
        from: deployer,
        to: None,
        value: Amount::ZERO,
        gas_limit: Gas(500_000),
        max_fee_per_gas: Amount::from_u128(20),
        max_priority_fee_per_gas: Amount::from_u128(2),
        valid_until_block: BlockHeight(100),
        data: creation_target,
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_target, _) = sign_eth_tx(&kp, deploy_target);
    let target_addr = compute_contract_address(&deployer, 0);

    // Deploy CallRouter
    let deploy_router = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(1),
        from: deployer,
        to: None,
        value: Amount::ZERO,
        gas_limit: Gas(500_000),
        max_fee_per_gas: Amount::from_u128(20),
        max_priority_fee_per_gas: Amount::from_u128(2),
        valid_until_block: BlockHeight(100),
        data: creation_router,
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_router, _) = sign_eth_tx(&kp, deploy_router);
    let router_addr = compute_contract_address(&deployer, 1);

    let (b1, r1) = harness.commit_block(vec![signed_target, signed_router]);
    assert!(r1[0].success);
    assert!(r1[1].success);

    let rpc = harness.make_dispatcher();
    rpc.record_block(b1, r1);

    // Call storeValue(888) through router: routeCall(target, data)
    let mut store_data = selector("storeValue(uint256)").to_vec();
    store_data.extend_from_slice(&pad32_u128(888));

    let mut route_call_data = selector("routeCall(address,bytes)").to_vec();
    route_call_data.extend_from_slice(&pad32_addr(&target_addr));
    route_call_data.extend_from_slice(&pad32_u128(64)); // offset
    route_call_data.extend_from_slice(&encode_dynamic_bytes(&store_data));

    let tx_route = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(2),
        from: deployer,
        to: Some(router_addr),
        value: Amount::ZERO,
        gas_limit: Gas(200_000),
        max_fee_per_gas: Amount::from_u128(20),
        max_priority_fee_per_gas: Amount::from_u128(2),
        valid_until_block: BlockHeight(100),
        data: route_call_data,
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_route, _) = sign_eth_tx(&kp, tx_route);
    let (b2, r2) = harness.commit_block(vec![signed_route]);
    assert!(r2[0].success);
    rpc.record_block(b2, r2);

    // Verify inside TargetContract: lastCaller is router_addr and valueStored is 888
    let res_read = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"eth_call","params":[{{ "to": "{}", "data": "0x{}" }}]}}"#,
        target_addr.to_hex(),
        hex_encode(&selector("readValue()"))
    ));
    assert!(res_read.contains(&hex_encode(&pad32_addr(&router_addr))));
    assert!(res_read.contains(&hex_encode(&pad32_u128(888))));
}

// =========================================================================
// 8. Custom Errors and Reverts (Solidity 0.8+)
// =========================================================================

#[test]
fn test_p7_solidity_custom_errors_and_reverts() {
    let mut harness = TestHarness::new("custom_errors");
    let kp = Secp256k1Keypair::generate();
    let deployer = kp.address();
    harness.fund_account(&deployer, 10_000_000_000);

    let (creation_code, _) = load_artifact(CUSTOM_ERROR_TEST_JSON);

    // Constructor: uint256 required = 1000
    let mut initcode = creation_code;
    initcode.extend_from_slice(&pad32_u128(1000));

    let deploy_tx = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce::ZERO,
        from: deployer,
        to: None,
        value: Amount::ZERO,
        gas_limit: Gas(500_000),
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

    // Error selector for: Unauthorized(address,uint256)
    let err_selector = selector("Unauthorized(address,uint256)");

    // 1. eth_call revert with custom error
    let mut test_call = selector("testUnauthorized(uint256)").to_vec();
    test_call.extend_from_slice(&pad32_u128(500)); // provided < required (500 < 1000)

    let res_revert = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"eth_call","params":[{{ "to": "{}", "data": "0x{}" }}]}}"#,
        contract_addr.to_hex(),
        hex_encode(&test_call)
    ));
    assert!(res_revert.contains("revert"));
    assert!(res_revert.contains(&hex_encode(&err_selector)));

    // 2. Committed transaction revert with custom error
    let mut tx_revert_data = selector("triggerRevertTx(uint256)").to_vec();
    tx_revert_data.extend_from_slice(&pad32_u128(500));

    let tx_fail = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(1),
        from: deployer,
        to: Some(contract_addr),
        value: Amount::ZERO,
        gas_limit: Gas(100_000),
        max_fee_per_gas: Amount::from_u128(20),
        max_priority_fee_per_gas: Amount::from_u128(2),
        valid_until_block: BlockHeight(100),
        data: tx_revert_data,
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_fail, _) = sign_eth_tx(&kp, tx_fail);
    let (_b2, r2) = harness.commit_block(vec![signed_fail]);
    assert!(!r2[0].success);
    assert_eq!(r2[0].return_data[0..4], err_selector);
    assert_eq!(&r2[0].return_data[36..68], &pad32_u128(1000));
}

// =========================================================================
// 9. Canonical EIP-1967 OpenZeppelin UUPS Proxy
// =========================================================================

#[test]
fn test_p7_solidity_eip1967_openzeppelin_proxy() {
    let mut harness = TestHarness::new("eip1967_proxy");
    let kp = Secp256k1Keypair::generate();
    let owner = kp.address();
    harness.fund_account(&owner, 10_000_000_000);

    let (creation_v1, _) = load_artifact(UUPS_IMPL_MOCK_JSON);
    let (creation_proxy, _) = load_artifact(EIP1967_PROXY_WRAPPER_JSON);
    let (creation_v2, _) = load_artifact(UUPS_IMPL_V2_MOCK_JSON);

    // 1. Deploy Logic V1
    let deploy_v1 = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce::ZERO,
        from: owner,
        to: None,
        value: Amount::ZERO,
        gas_limit: Gas(1_000_000),
        max_fee_per_gas: Amount::from_u128(20),
        max_priority_fee_per_gas: Amount::from_u128(2),
        valid_until_block: BlockHeight(100),
        data: creation_v1,
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_v1, _) = sign_eth_tx(&kp, deploy_v1);
    let v1_addr = compute_contract_address(&owner, 0);

    // 2. Prepare Proxy Init Data: initialize(owner, 42)
    let mut init_calldata = selector("initialize(address,uint256)").to_vec();
    init_calldata.extend_from_slice(&pad32_addr(&owner));
    init_calldata.extend_from_slice(&pad32_u128(42));

    // Deploy Proxy(logic, data)
    let mut initcode_proxy = creation_proxy;
    initcode_proxy.extend_from_slice(&pad32_addr(&v1_addr));
    initcode_proxy.extend_from_slice(&pad32_u128(64)); // offset
    initcode_proxy.extend_from_slice(&encode_dynamic_bytes(&init_calldata));

    let deploy_proxy = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(1),
        from: owner,
        to: None,
        value: Amount::ZERO,
        gas_limit: Gas(1_000_000),
        max_fee_per_gas: Amount::from_u128(20),
        max_priority_fee_per_gas: Amount::from_u128(2),
        valid_until_block: BlockHeight(100),
        data: initcode_proxy,
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_proxy, _) = sign_eth_tx(&kp, deploy_proxy);
    let proxy_addr = compute_contract_address(&owner, 1);

    let (b1, r1) = harness.commit_block(vec![signed_v1, signed_proxy]);
    assert!(r1[0].success);
    assert!(r1[1].success);

    let rpc = harness.make_dispatcher();
    rpc.record_block(b1, r1);

    // 3. Verify canonical EIP-1967 slot on proxy:
    // bytes32(uint256(keccak256("eip1967.proxy.implementation")) - 1)
    let eip1967_slot_hex = "0x360894a13ba1a3210667c828492db98dca3e2076cc3735a920a3ca505d382bbc";
    let slot_res = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"eth_getStorageAt","params":["{}","{}","latest"]}}"#,
        proxy_addr.to_hex(),
        eip1967_slot_hex
    ));
    assert!(slot_res.contains(&hex_encode(&pad32_addr(&v1_addr))));

    // 4. Verify proxy storage at slot 0 (value is 42)
    let val_slot = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"eth_getStorageAt","params":["{}","0x0","latest"]}}"#,
        proxy_addr.to_hex()
    ));
    assert!(val_slot.contains(&hex_encode(&pad32_u128(42))));

    // 5. Verify logic contract storage slot 0 is 0 (storage isolation)
    let logic_slot = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":3,"method":"eth_getStorageAt","params":["{}","0x0","latest"]}}"#,
        v1_addr.to_hex()
    ));
    assert!(logic_slot.contains(&hex_encode(&pad32_u128(0))));

    // 6. Mutate storage through proxy: setValue(100)
    let mut set_val_data = selector("setValue(uint256)").to_vec();
    set_val_data.extend_from_slice(&pad32_u128(100));

    let tx_set = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(2),
        from: owner,
        to: Some(proxy_addr),
        value: Amount::ZERO,
        gas_limit: Gas(200_000),
        max_fee_per_gas: Amount::from_u128(20),
        max_priority_fee_per_gas: Amount::from_u128(2),
        valid_until_block: BlockHeight(100),
        data: set_val_data,
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_set, _) = sign_eth_tx(&kp, tx_set);
    let (b2, r2) = harness.commit_block(vec![signed_set]);
    assert!(r2[0].success);
    rpc.record_block(b2, r2);

    let val_slot_100 = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":4,"method":"eth_getStorageAt","params":["{}","0x0","latest"]}}"#,
        proxy_addr.to_hex()
    ));
    assert!(val_slot_100.contains(&hex_encode(&pad32_u128(100))));

    // 7. Deploy V2 and execute UUPS upgradeToAndCall(v2, "")
    let deploy_v2 = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(3),
        from: owner,
        to: None,
        value: Amount::ZERO,
        gas_limit: Gas(1_000_000),
        max_fee_per_gas: Amount::from_u128(20),
        max_priority_fee_per_gas: Amount::from_u128(2),
        valid_until_block: BlockHeight(100),
        data: creation_v2,
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_v2, _) = sign_eth_tx(&kp, deploy_v2);
    let v2_addr = compute_contract_address(&owner, 3);

    let mut upgrade_data = selector("upgradeToAndCall(address,bytes)").to_vec();
    upgrade_data.extend_from_slice(&pad32_addr(&v2_addr));
    upgrade_data.extend_from_slice(&pad32_u128(64));
    upgrade_data.extend_from_slice(&encode_dynamic_bytes(&[]));

    let tx_upgrade = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(4),
        from: owner,
        to: Some(proxy_addr),
        value: Amount::ZERO,
        gas_limit: Gas(300_000),
        max_fee_per_gas: Amount::from_u128(20),
        max_priority_fee_per_gas: Amount::from_u128(2),
        valid_until_block: BlockHeight(100),
        data: upgrade_data,
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_upgrade, _) = sign_eth_tx(&kp, tx_upgrade);
    let (b3, r3) = harness.commit_block(vec![signed_v2, signed_upgrade]);
    assert!(r3[0].success);
    assert!(r3[1].success);
    rpc.record_block(b3, r3);

    // Verify proxy implementation slot updated to v2_addr
    let slot_v2 = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":5,"method":"eth_getStorageAt","params":["{}","{}","latest"]}}"#,
        proxy_addr.to_hex(),
        eip1967_slot_hex
    ));
    assert!(slot_v2.contains(&hex_encode(&pad32_addr(&v2_addr))));

    // 8. Execute new V2 function: multiplyValue(3)
    let mut mult_data = selector("multiplyValue(uint256)").to_vec();
    mult_data.extend_from_slice(&pad32_u128(3));

    let tx_mult = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(5),
        from: owner,
        to: Some(proxy_addr),
        value: Amount::ZERO,
        gas_limit: Gas(200_000),
        max_fee_per_gas: Amount::from_u128(20),
        max_priority_fee_per_gas: Amount::from_u128(2),
        valid_until_block: BlockHeight(100),
        data: mult_data,
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_mult, _) = sign_eth_tx(&kp, tx_mult);
    let (b4, r4) = harness.commit_block(vec![signed_mult]);
    assert!(r4[0].success);
    rpc.record_block(b4, r4);

    // Verify proxy slot 0 is now 300 (100 * 3)
    let val_slot_300 = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":6,"method":"eth_getStorageAt","params":["{}","0x0","latest"]}}"#,
        proxy_addr.to_hex()
    ));
    assert!(val_slot_300.contains(&hex_encode(&pad32_u128(300))));
}

// =========================================================================
// 10. Cold Restart Persistence with OpenZeppelin ERC-20
// =========================================================================

#[test]
fn test_p7_solidity_cold_restart_persistence() {
    let mut harness = TestHarness::new("cold_restart");
    let kp = Secp256k1Keypair::generate();
    let deployer = kp.address();
    let kp_bob = Secp256k1Keypair::generate();
    let bob = kp_bob.address();
    let kp_charlie = Secp256k1Keypair::generate();
    let charlie = kp_charlie.address();

    harness.fund_account(&deployer, 10_000_000_000);
    harness.fund_account(&bob, 10_000_000_000);

    let (creation_code, _) = load_artifact(ERC20_MOCK_JSON);

    // Deploy ERC20: initialBalance = 5_000_000
    let mut initcode = creation_code;
    initcode.extend_from_slice(&pad32_u128(128));
    let name_bytes = encode_dynamic_string("PersistentToken");
    initcode.extend_from_slice(&pad32_u128((128 + name_bytes.len()) as u128));
    initcode.extend_from_slice(&pad32_addr(&deployer));
    initcode.extend_from_slice(&pad32_u128(5_000_000));
    initcode.extend_from_slice(&name_bytes);
    initcode.extend_from_slice(&encode_dynamic_string("PERST"));

    let deploy_tx = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce::ZERO,
        from: deployer,
        to: None,
        value: Amount::ZERO,
        gas_limit: Gas(1_500_000),
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
    let token_addr = compute_contract_address(&deployer, 0);

    let (_b1, r1) = harness.commit_block(vec![signed_deploy]);
    assert!(r1[0].success);

    // Transfer 1_000_000 to bob
    let mut transfer_data = selector("transfer(address,uint256)").to_vec();
    transfer_data.extend_from_slice(&pad32_addr(&bob));
    transfer_data.extend_from_slice(&pad32_u128(1_000_000));

    let tx_transfer = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(1),
        from: deployer,
        to: Some(token_addr),
        value: Amount::ZERO,
        gas_limit: Gas(200_000),
        max_fee_per_gas: Amount::from_u128(20),
        max_priority_fee_per_gas: Amount::from_u128(2),
        valid_until_block: BlockHeight(100),
        data: transfer_data,
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_transfer, _) = sign_eth_tx(&kp, tx_transfer);
    let (_b2, r2) = harness.commit_block(vec![signed_transfer]);
    assert!(r2[0].success);

    // DROP STATE DB AND REOPEN FRESH FROM DISK
    harness.reopen();
    let rpc_after = harness.make_dispatcher();

    // Verify state on cold database: balanceOf(bob) == 1_000_000
    let mut bal_bob_data = selector("balanceOf(address)").to_vec();
    bal_bob_data.extend_from_slice(&pad32_addr(&bob));
    let res_bal_bob = rpc_after.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"eth_call","params":[{{ "to": "{}", "data": "0x{}" }}]}}"#,
        token_addr.to_hex(),
        hex_encode(&bal_bob_data)
    ));
    assert!(res_bal_bob.contains(&hex_encode(&pad32_u128(1_000_000))));

    // Submit subsequent transaction after restart: bob transfers 200_000 to charlie
    let mut transfer_charlie = selector("transfer(address,uint256)").to_vec();
    transfer_charlie.extend_from_slice(&pad32_addr(&charlie));
    transfer_charlie.extend_from_slice(&pad32_u128(200_000));

    let tx_post = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce::ZERO,
        from: bob,
        to: Some(token_addr),
        value: Amount::ZERO,
        gas_limit: Gas(200_000),
        max_fee_per_gas: Amount::from_u128(20),
        max_priority_fee_per_gas: Amount::from_u128(2),
        valid_until_block: BlockHeight(100),
        data: transfer_charlie,
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_post, _) = sign_eth_tx(&kp_bob, tx_post);
    let (_b3, r3) = harness.commit_block(vec![signed_post]);
    assert!(r3[0].success);

    // Verify final balance on charlie == 200_000
    let mut bal_charlie_data = selector("balanceOf(address)").to_vec();
    bal_charlie_data.extend_from_slice(&pad32_addr(&charlie));
    let res_bal_charlie = rpc_after.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"eth_call","params":[{{ "to": "{}", "data": "0x{}" }}]}}"#,
        token_addr.to_hex(),
        hex_encode(&bal_charlie_data)
    ));
    assert!(res_bal_charlie.contains(&hex_encode(&pad32_u128(200_000))));
}

// =========================================================================
// 11. Precompile Out-of-Gas (OOG) Containment
// =========================================================================

#[test]
fn test_p7_precompile_oog_containment() {
    let mut harness = TestHarness::new("precompile_oog");
    let kp = Secp256k1Keypair::generate();
    let deployer = kp.address();
    harness.fund_account(&deployer, 10_000_000_000);

    let (creation_code, _) = load_artifact(PRECOMPILE_CALLER_JSON);

    let deploy_tx = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce::ZERO,
        from: deployer,
        to: None,
        value: Amount::ZERO,
        gas_limit: Gas(1_000_000),
        max_fee_per_gas: Amount::from_u128(20),
        max_priority_fee_per_gas: Amount::from_u128(2),
        valid_until_block: BlockHeight(100),
        data: creation_code,
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_deploy, _) = sign_eth_tx(&kp, deploy_tx);
    let caller_addr = compute_contract_address(&deployer, 0);

    let (b1, r1) = harness.commit_block(vec![signed_deploy]);
    assert!(r1[0].success);

    let rpc = harness.make_dispatcher();
    rpc.record_block(b1, r1);

    // Call callPrecompileWithBoundedGas to target 0x01 (ecrecover requires 3000 gas) with only 500 gas
    let mut call_data = selector("callPrecompileWithBoundedGas(address,bytes,uint256)").to_vec();
    call_data.extend_from_slice(&pad32_addr(&Address([
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1,
    ])));
    call_data.extend_from_slice(&pad32_u128(96)); // offset to bytes
    call_data.extend_from_slice(&pad32_u128(500)); // forwarded gas limit
    call_data.extend_from_slice(&encode_dynamic_bytes(&[0u8; 128]));

    let res = rpc.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"eth_call","params":[{{ "to": "{}", "data": "0x{}" }}]}}"#,
        caller_addr.to_hex(),
        hex_encode(&call_data)
    ));
    // The parent call succeeds and returns (success: false, returndata: "")
    assert!(res.contains(r#""result":"0x"#));
    let val = JsonValue::parse(&res).unwrap();
    let hex_result = val.get("result").unwrap().as_str().unwrap();
    let bytes_res = hex_decode(hex_result);
    // bytes_res[0..32] is bool success -> 0 (false)
    assert_eq!(bytes_res[31], 0);
}

// =========================================================================
// 12. Precompiles Exact Gas Accounting Matrix (0x01 - 0x09)
// =========================================================================

#[test]
fn test_p7_precompiles_exact_gas_accounting_matrix() {
    let mut harness = TestHarness::new("precompiles_gas");
    let kp = Secp256k1Keypair::generate();
    let deployer = kp.address();
    harness.fund_account(&deployer, 100_000_000_000);

    let (creation_code, _) = load_artifact(PRECOMPILE_CALLER_JSON);

    let deploy_tx = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce::ZERO,
        from: deployer,
        to: None,
        value: Amount::ZERO,
        gas_limit: Gas(1_000_000),
        max_fee_per_gas: Amount::from_u128(20),
        max_priority_fee_per_gas: Amount::from_u128(2),
        valid_until_block: BlockHeight(100),
        data: creation_code,
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let (signed_deploy, _) = sign_eth_tx(&kp, deploy_tx);
    let caller_addr = compute_contract_address(&deployer, 0);

    let (b1, r1) = harness.commit_block(vec![signed_deploy]);
    assert!(r1[0].success);

    let rpc = harness.make_dispatcher();
    rpc.record_block(b1, r1);

    let test_bounded_gas = |target_byte: u8, input: &[u8], exact_gas: u64| -> (bool, bool) {
        let make_payload = |gas: u64| {
            let mut target = [0u8; 20];
            target[19] = target_byte;
            let mut call_data =
                selector("callPrecompileWithBoundedGas(address,bytes,uint256)").to_vec();
            call_data.extend_from_slice(&pad32_addr(&Address(target)));
            call_data.extend_from_slice(&pad32_u128(96)); // offset of bytes
            call_data.extend_from_slice(&pad32_u128(gas as u128));
            call_data.extend_from_slice(&encode_dynamic_bytes(input));
            call_data
        };

        // Under-budget: exact_gas - 1
        let payload_under = make_payload(exact_gas.saturating_sub(1));
        let res_under = rpc.dispatch_json(&format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"eth_call","params":[{{ "to": "{}", "data": "0x{}" }}]}}"#,
            caller_addr.to_hex(),
            hex_encode(&payload_under)
        ));
        let val_under = JsonValue::parse(&res_under).unwrap();
        let hex_under = val_under.get("result").unwrap().as_str().unwrap();
        let bytes_under = hex_decode(hex_under);
        let ok_under = bytes_under.get(31).copied().unwrap_or(0) == 1;

        // Exact budget
        let payload_exact = make_payload(exact_gas);
        let res_exact = rpc.dispatch_json(&format!(
            r#"{{"jsonrpc":"2.0","id":2,"method":"eth_call","params":[{{ "to": "{}", "data": "0x{}" }}]}}"#,
            caller_addr.to_hex(),
            hex_encode(&payload_exact)
        ));
        let val_exact = JsonValue::parse(&res_exact).unwrap();
        let hex_exact = val_exact.get("result").unwrap().as_str().unwrap();
        let bytes_exact = hex_decode(hex_exact);
        let ok_exact = bytes_exact.get(31).copied().unwrap_or(0) == 1;

        (ok_under, ok_exact)
    };

    // 1. 0x01 ecrecover: Fixed cost = 3,000 gas
    let msg_hash = keccak256_hash(b"Gas ecrecover test");
    let sig = kp.sign_recoverable(msg_hash.as_bytes()).unwrap();
    let mut input_01 = Vec::with_capacity(128);
    input_01.extend_from_slice(msg_hash.as_bytes());
    input_01.extend_from_slice(&pad32_u128(sig[64] as u128 + 27));
    input_01.extend_from_slice(&sig[0..32]);
    input_01.extend_from_slice(&sig[32..64]);
    let (under_01, exact_01) = test_bounded_gas(1, &input_01, 3_000);
    assert!(!under_01, "0x01 should fail on 2999 gas");
    assert!(exact_01, "0x01 should succeed on 3000 gas");

    // 2. 0x02 sha256: 60 + 12 * words
    // Vector A: 1 word (32 bytes) => 60 + 12 = 72 gas
    let input_02_1w = [0x5au8; 32];
    let (under_02_a, exact_02_a) = test_bounded_gas(2, &input_02_1w, 72);
    assert!(!under_02_a && exact_02_a, "0x02 sha256 1-word gas failed");
    // Vector B: 2 words (46 bytes) => 60 + 24 = 84 gas
    let input_02 = b"TOLA Protocol SHA-256 Independent Verification";
    let words_02 = (input_02.len() as u64).div_ceil(32);
    let expected_gas_02 = 60 + 12 * words_02;
    let (under_02, exact_02) = test_bounded_gas(2, input_02, expected_gas_02);
    assert!(!under_02 && exact_02, "0x02 sha256 2-word gas failed");

    // 3. 0x03 ripemd160: 600 + 120 * words
    // Vector A: 1 word (20 bytes) => 600 + 120 = 720 gas
    let input_03_1w = [0x6bu8; 20];
    let (under_03_a, exact_03_a) = test_bounded_gas(3, &input_03_1w, 720);
    assert!(
        !under_03_a && exact_03_a,
        "0x03 ripemd160 1-word gas failed"
    );
    // Vector B: 2 words (37 bytes) => 600 + 240 = 840 gas
    let input_03 = b"TOLA Protocol RIPEMD-160 Verification";
    let words_03 = (input_03.len() as u64).div_ceil(32);
    let expected_gas_03 = 600 + 120 * words_03;
    let (under_03, exact_03) = test_bounded_gas(3, input_03, expected_gas_03);
    assert!(!under_03 && exact_03, "0x03 ripemd160 2-word gas failed");

    // 4. 0x04 identity: 15 + 3 * words
    // Vector A: 1 word (32 bytes) => 15 + 3 = 18 gas
    let input_04 = [0x42u8; 32];
    let (under_04, exact_04) = test_bounded_gas(4, &input_04, 18);
    assert!(!under_04 && exact_04, "0x04 identity 1-word gas failed");
    // Vector B: 2 words (64 bytes) => 15 + 6 = 21 gas
    let input_04_2w = [0x42u8; 64];
    let (under_04_b, exact_04_b) = test_bounded_gas(4, &input_04_2w, 21);
    assert!(!under_04_b && exact_04_b, "0x04 identity 2-word gas failed");

    // 5. 0x05 modexp: EIP-2565 formula (min 200 gas)
    let mut input_05 = Vec::new();
    input_05.extend_from_slice(&pad32_u128(1));
    input_05.extend_from_slice(&pad32_u128(1));
    input_05.extend_from_slice(&pad32_u128(1));
    input_05.push(3);
    input_05.push(5);
    input_05.push(7);
    let (under_05, exact_05) = test_bounded_gas(5, &input_05, 200);
    assert!(!under_05 && exact_05, "0x05 modexp min gas failed");

    // 6. 0x06 ecAdd: Fixed cost = 150 gas per EIP-1108
    let input_06 = [0u8; 128];
    let (under_06, exact_06) = test_bounded_gas(6, &input_06, 150);
    assert!(!under_06 && exact_06, "0x06 ecAdd gas failed");

    // 7. 0x07 ecMul: Fixed cost = 6,000 gas per EIP-1108
    let input_07 = [0u8; 96];
    let (under_07, exact_07) = test_bounded_gas(7, &input_07, 6_000);
    assert!(!under_07 && exact_07, "0x07 ecMul gas failed");

    // 8. 0x08 ecPairing: EIP-1108 formula (34,000 * k + 45,000)
    // Vector A: k=0 (empty input) => 45,000 gas
    let input_08_empty = [];
    let (under_08_a, exact_08_a) = test_bounded_gas(8, &input_08_empty, 45_000);
    assert!(!under_08_a && exact_08_a, "0x08 ecPairing k=0 gas failed");
    // Vector B: k=1 (192 bytes = 1 pair of G1 + G2, all zeros is valid point at infinity)
    // 34,000 * 1 + 45,000 = 79,000 gas
    let input_08_1pair = [0u8; 192];
    let (under_08_b, exact_08_b) = test_bounded_gas(8, &input_08_1pair, 79_000);
    assert!(!under_08_b && exact_08_b, "0x08 ecPairing k=1 gas failed");

    // 9. 0x09 blake2f: EIP-152 formula (rounds = gas)
    // Vector A: 12 rounds => 12 gas
    let mut input_09_12 = vec![0u8; 213];
    input_09_12[3] = 12; // 12 rounds
    input_09_12[212] = 1; // f = 1
    let (under_09_a, exact_09_a) = test_bounded_gas(9, &input_09_12, 12);
    assert!(
        !under_09_a && exact_09_a,
        "0x09 blake2f 12 rounds gas failed"
    );
    // Vector B: 24 rounds => 24 gas
    let mut input_09_24 = vec![0u8; 213];
    input_09_24[3] = 24; // 24 rounds
    input_09_24[212] = 1; // f = 1
    let (under_09_b, exact_09_b) = test_bounded_gas(9, &input_09_24, 24);
    assert!(
        !under_09_b && exact_09_b,
        "0x09 blake2f 24 rounds gas failed"
    );
}
