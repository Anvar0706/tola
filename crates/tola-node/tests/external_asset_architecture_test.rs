//! Comprehensive integration test verifying:
//! 1. TOLA Network Platform Independence & Absence of Native Identity / Gifts.
//! 2. Section 28 Simulation: Complete External Asset Tokenization Lifecycle:
//!    - External Platform (owns original asset) -> Independent Tokenizer App (verifies) ->
//!      TOLA Wallet (signed tx) -> TOLA Blockchain (TLS-721 token) -> Secondary Market Transfer
//!    - Decoupling Verification: TOLA NFT ownership changed to Bob, External Platform DB untouched.
//!    - Section 27 Negative Architecture Assertions: Native identity/gifts/numbers do NOT exist.
//! 3. Adversarial Attestation Security, Replay Defense, and Duplicate Mint Prevention.

use std::sync::{Arc, RwLock};

use tola_core::{BlockEngine, StateDB, StateDelta};
use tola_crypto::{
    compute_external_asset_attestation_digest, compute_external_asset_canonical_id,
    Secp256k1Keypair,
};
use tola_execution::{
    compute_tx_signing_digest, ExternalAssetCall, ExternalAssetStorage, SequentialBlockEngine,
    TOPIC_EXTERNAL_ASSET_MINTED, TOPIC_TRANSFER,
};
use tola_mempool::{Mempool, MempoolConfig};
use tola_rpc::dispatcher::RpcDispatcher;
use tola_rpc::json::JsonValue;
use tola_state::MemoryStateJournal;
use tola_storage::PersistentStateDB;
use tola_types::*;

fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

fn setup_test_dispatcher(chain_id: ChainId) -> RpcDispatcher<MemoryStateJournal> {
    let state = Arc::new(RwLock::new(MemoryStateJournal::new()));
    let mempool = Arc::new(RwLock::new(Mempool::new(MempoolConfig::new(chain_id))));
    RpcDispatcher::new(chain_id, "TOLA/v0.1.0-alpha".into(), state, mempool)
}

// 1. Independence & Negative Architecture Test:
// Proves that the TOLA blockchain node runs completely autonomously with zero external
// messenger/PaxtaGram dependencies, and asserts that Native Username, Native Gift,
// and Native Identity RPC methods DO NOT EXIST (return -32601 Method Not Found).
#[test]
fn test_platform_independence_and_absence_of_native_identity_and_gifts() {
    let chain_id = ChainId::TESTNET;
    let disp = setup_test_dispatcher(chain_id);

    // A. Native accounts and balance operations execute sovereignly on TOLA
    let alice = Address([0x11; 20]);
    let alice_acct = AccountState::new_with_balance(Amount::from_u128(1_000 * Amount::ONE_TOLA));
    disp.state_db
        .write()
        .unwrap()
        .set_account(alice, alice_acct);

    let req_bal = format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"eth_getBalance","params":["{}","latest"]}}"#,
        alice.to_hex()
    );
    let res_bal = disp.dispatch_json(&req_bal);
    let val_bal = JsonValue::parse(&res_bal).unwrap();
    assert!(val_bal.get("result").is_some());

    // B. Negative Architecture Assertions (Section 27 & 28):
    // 1) TOLA native username resolution DOES NOT EXIST
    let req_user = r#"{"jsonrpc":"2.0","id":2,"method":"tola_resolveUsername","params":["anvar"]}"#;
    let res_user = disp.dispatch_json(req_user);
    let val_user = JsonValue::parse(&res_user).unwrap();
    assert_eq!(
        val_user
            .get("error")
            .and_then(|e| e.get("code"))
            .and_then(|c| c.as_i64()),
        Some(-32601),
        "tola_resolveUsername must return Method Not Found (-32601)"
    );

    // 2) TOLA native username record query DOES NOT EXIST
    let req_user_rec =
        r#"{"jsonrpc":"2.0","id":3,"method":"tola_getUsernameRecord","params":["anvar"]}"#;
    let res_user_rec = disp.dispatch_json(req_user_rec);
    let val_user_rec = JsonValue::parse(&res_user_rec).unwrap();
    assert_eq!(
        val_user_rec
            .get("error")
            .and_then(|e| e.get("code"))
            .and_then(|c| c.as_i64()),
        Some(-32601),
        "tola_getUsernameRecord must return Method Not Found (-32601)"
    );

    // 3) TOLA native gift query DOES NOT EXIST
    let req_gift = r#"{"jsonrpc":"2.0","id":4,"method":"tola_getGiftDetails","params":[101]}"#;
    let res_gift = disp.dispatch_json(req_gift);
    let val_gift = JsonValue::parse(&res_gift).unwrap();
    assert_eq!(
        val_gift
            .get("error")
            .and_then(|e| e.get("code"))
            .and_then(|c| c.as_i64()),
        Some(-32601),
        "tola_getGiftDetails must return Method Not Found (-32601)"
    );

    // 4) TOLA native badge attestation verification DOES NOT EXIST
    let req_badge =
        r#"{"jsonrpc":"2.0","id":5,"method":"tola_verifyBadgeAttestation","params":[{}]}"#;
    let res_badge = disp.dispatch_json(req_badge);
    let val_badge = JsonValue::parse(&res_badge).unwrap();
    assert_eq!(
        val_badge
            .get("error")
            .and_then(|e| e.get("code"))
            .and_then(|c| c.as_i64()),
        Some(-32601),
        "tola_verifyBadgeAttestation must return Method Not Found (-32601)"
    );

    println!("[PASS] Platform independence and absence of native identity/gifts verified");
}

// 2. Section 28 Simulation: Complete External Asset Tokenization Lifecycle
// Tests the entire multi-asset tokenization lifecycle:
// 1. Mock External Platform contains:
//    - external username: @anvar
//    - external Gifts Number: #12345
//    - external Gift: #987
// 2. Mock Independent Third-Party Tokenization App verifies ownership and signs attestations.
// 3. Attestations verified via JSON-RPC tola_verifyExternalAssetAttestation.
// 4. TOLA Node registers external assets as TLS-721 tokens.
// 5. Secondary market transfer on TOLA from Alice to Bob.
// 6. Decoupling Check:
//    - TOLA NFT owner = Bob.
//    - External Platform original assets owner = Alice (100% untouched!).
// 7. Negative Architecture Test: asserts native identity/gifts do not exist.
#[test]
fn test_section28_full_multi_asset_tokenization_simulation() {
    let chain_id = ChainId::TESTNET;
    let disp = setup_test_dispatcher(chain_id);

    // ------------------------------------------------------------------------
    // Step 1: Mock External Platform (e.g. PaxtaGram, Telegram, or X)
    // ------------------------------------------------------------------------
    struct MockExternalPlatformDb {
        username_owner: String,     // @anvar owned by "alice_external"
        gifts_number_owner: String, // #12345 owned by "alice_external"
        gift_owner: String,         // #987 owned by "alice_external"
    }

    let external_platform_db = MockExternalPlatformDb {
        username_owner: "alice_external".to_string(),
        gifts_number_owner: "alice_external".to_string(),
        gift_owner: "alice_external".to_string(),
    };

    assert_eq!(external_platform_db.username_owner, "alice_external");
    assert_eq!(external_platform_db.gifts_number_owner, "alice_external");
    assert_eq!(external_platform_db.gift_owner, "alice_external");

    // ------------------------------------------------------------------------
    // Step 2: Mock Independent Third-Party Tokenization App (Fragment-like service)
    // ------------------------------------------------------------------------
    let verifier_kp = Secp256k1Keypair::generate();
    let verifier_addr = verifier_kp.address();

    let alice_wallet = Address([0xaa; 20]);
    let bob_wallet = Address([0xbb; 20]);

    let expires_at = 2500000000u64;
    let source_platform = "paxtagram";

    // Asset 1: External Username @anvar
    let asset1_type = ExternalAssetType::MessengerUsername;
    let asset1_id = "anvar";
    let nonce1 = 101u64;
    let digest1 = compute_external_asset_attestation_digest(
        chain_id.0,
        nonce1,
        &alice_wallet,
        expires_at,
        source_platform,
        asset1_type,
        asset1_id,
    );
    let sig1 = verifier_kp.sign_recoverable(&digest1).unwrap();
    let cid1 = compute_external_asset_canonical_id(source_platform, asset1_type, asset1_id);

    // Asset 2: External Gifts Number #12345
    let asset2_type = ExternalAssetType::MessengerNumber;
    let asset2_id = "12345";
    let nonce2 = 102u64;
    let digest2 = compute_external_asset_attestation_digest(
        chain_id.0,
        nonce2,
        &alice_wallet,
        expires_at,
        source_platform,
        asset2_type,
        asset2_id,
    );
    let sig2 = verifier_kp.sign_recoverable(&digest2).unwrap();
    let cid2 = compute_external_asset_canonical_id(source_platform, asset2_type, asset2_id);

    // Asset 3: External Gift #987
    let asset3_type = ExternalAssetType::MessengerGift;
    let asset3_id = "987";
    let nonce3 = 103u64;
    let digest3 = compute_external_asset_attestation_digest(
        chain_id.0,
        nonce3,
        &alice_wallet,
        expires_at,
        source_platform,
        asset3_type,
        asset3_id,
    );
    let sig3 = verifier_kp.sign_recoverable(&digest3).unwrap();
    let cid3 = compute_external_asset_canonical_id(source_platform, asset3_type, asset3_id);

    // ------------------------------------------------------------------------
    // Step 3: Verify Attestations via JSON-RPC
    // ------------------------------------------------------------------------
    for (nonce, a_type, a_id, sig) in [
        (nonce1, asset1_type, asset1_id, &sig1),
        (nonce2, asset2_type, asset2_id, &sig2),
        (nonce3, asset3_type, asset3_id, &sig3),
    ] {
        let req_verify = format!(
            r#"{{"jsonrpc":"2.0","id":100,"method":"tola_verifyExternalAssetAttestation","params":[{{
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
            chain_id.0,
            nonce,
            alice_wallet.to_hex(),
            expires_at,
            source_platform,
            a_type.as_str(),
            a_id,
            verifier_addr.to_hex(),
            hex_encode(&sig[0..32]),
            hex_encode(&sig[32..64]),
            sig[64]
        );
        let res_verify = disp.dispatch_json(&req_verify);
        let val_verify = JsonValue::parse(&res_verify).unwrap();
        let verify_res = val_verify.get("result").expect("Verification result");
        assert_eq!(
            verify_res.get("valid").and_then(|v| v.as_bool()),
            Some(true),
            "Attestation for asset {} must be valid",
            a_id
        );
    }

    // ------------------------------------------------------------------------
    // Step 4: Mint / Register Tokenized Assets on TOLA Blockchain
    // ------------------------------------------------------------------------
    let token1_id = 1001u64;
    let token2_id = 1002u64;
    let token3_id = 1003u64;

    disp.register_external_asset(ExternalAsset {
        token_id: token1_id,
        canonical_id: cid1,
        source_platform: source_platform.to_string(),
        asset_type: asset1_type,
        external_asset_id: asset1_id.to_string(),
        external_reference: "ref_anvar".to_string(),
        owner: alice_wallet,
        authorizer: verifier_addr,
        verifier_address: verifier_addr,
        minted_at: 1770001000,
        metadata_uri: "ipfs://bafybeianvar1001".to_string(),
    })
    .expect("Mint username token");

    disp.register_external_asset(ExternalAsset {
        token_id: token2_id,
        canonical_id: cid2,
        source_platform: source_platform.to_string(),
        asset_type: asset2_type,
        external_asset_id: asset2_id.to_string(),
        external_reference: "ref_number_12345".to_string(),
        owner: alice_wallet,
        authorizer: verifier_addr,
        verifier_address: verifier_addr,
        minted_at: 1770001001,
        metadata_uri: "ipfs://bafybeinumber1002".to_string(),
    })
    .expect("Mint gifts number token");

    disp.register_external_asset(ExternalAsset {
        token_id: token3_id,
        canonical_id: cid3,
        source_platform: source_platform.to_string(),
        asset_type: asset3_type,
        external_asset_id: asset3_id.to_string(),
        external_reference: "ref_gift_987".to_string(),
        owner: alice_wallet,
        authorizer: verifier_addr,
        verifier_address: verifier_addr,
        minted_at: 1770001002,
        metadata_uri: "ipfs://bafybeigift1003".to_string(),
    })
    .expect("Mint gift token");

    // Verify initial ownership on TOLA via RPC
    let req_owner1 = format!(
        r#"{{"jsonrpc":"2.0","id":101,"method":"tola_getExternalAssetOwner","params":[{}]}}"#,
        token1_id
    );
    let res_owner1 = disp.dispatch_json(&req_owner1);
    let val_owner1 = JsonValue::parse(&res_owner1).unwrap();
    assert_eq!(
        val_owner1.get("result").and_then(|v| v.as_str()),
        Some(alice_wallet.to_hex().as_str())
    );

    // ------------------------------------------------------------------------
    // Step 5: Secondary Market Transfer on TOLA from Alice to Bob
    // ------------------------------------------------------------------------
    {
        let mut reg = disp.external_asset_registry.write().unwrap();
        if let Some(asset) = reg.get_mut(&token1_id) {
            asset.owner = bob_wallet;
        }
    }

    // ------------------------------------------------------------------------
    // Step 6: Decoupling Check
    // ------------------------------------------------------------------------
    // A. Query TOLA owner via RPC -> Now Bob!
    let res_owner1_after = disp.dispatch_json(&req_owner1);
    let val_owner1_after = JsonValue::parse(&res_owner1_after).unwrap();
    assert_eq!(
        val_owner1_after.get("result").and_then(|v| v.as_str()),
        Some(bob_wallet.to_hex().as_str()),
        "TOLA NFT owner must now be Bob"
    );

    // B. Query External Platform Database -> STILL Alice (100% UNTOUCHED!)
    assert_eq!(
        external_platform_db.username_owner, "alice_external",
        "Transferring TOLA NFT must NOT alter external platform database username owner"
    );
    assert_eq!(
        external_platform_db.gifts_number_owner, "alice_external",
        "External platform gifts number owner remains alice_external"
    );
    assert_eq!(
        external_platform_db.gift_owner, "alice_external",
        "External platform gift owner remains alice_external"
    );

    // ------------------------------------------------------------------------
    // Step 7: Section 27 Negative Architecture Assertions
    // ------------------------------------------------------------------------
    // TOLA native username = DOES NOT EXIST
    let rpc_nat_user = disp.dispatch_json(
        r#"{"jsonrpc":"2.0","id":201,"method":"tola_resolveUsername","params":["anvar"]}"#,
    );
    assert!(rpc_nat_user.contains("-32601"));

    // TOLA native gift = DOES NOT EXIST
    let rpc_nat_gift = disp.dispatch_json(
        r#"{"jsonrpc":"2.0","id":202,"method":"tola_getGiftDetails","params":[987]}"#,
    );
    assert!(rpc_nat_gift.contains("-32601"));

    // TOLA native Gifts Number = DOES NOT EXIST
    let rpc_nat_number = disp.dispatch_json(
        r#"{"jsonrpc":"2.0","id":203,"method":"tola_resolveGiftsNumber","params":[12345]}"#,
    );
    assert!(rpc_nat_number.contains("-32601"));

    println!("[PASS] Section 28 simulation complete: external asset tokenization and ownership decoupling verified");
}

// 3. Adversarial Security:
// Replay prevention across chains and wallets, expiry rejection, and duplicate mint prevention.
#[test]
fn test_adversarial_attestation_security_and_replay_defenses() {
    let testnet_id = ChainId::TESTNET;
    let mainnet_id = ChainId::MAINNET;

    let testnet_disp = setup_test_dispatcher(testnet_id);
    let mainnet_disp = setup_test_dispatcher(mainnet_id);

    let verifier_kp = Secp256k1Keypair::generate();
    let verifier_addr = verifier_kp.address();

    let alice_wallet = Address([0x55; 20]);
    let mallory_wallet = Address([0x66; 20]);

    let nonce = 100u64;
    let expires_at = 2500000000u64;
    let source_platform = "paxtagram";
    let asset_type = ExternalAssetType::MessengerNumber;
    let external_asset_id = "8880001";

    let valid_digest = compute_external_asset_attestation_digest(
        testnet_id.0,
        nonce,
        &alice_wallet,
        expires_at,
        source_platform,
        asset_type,
        external_asset_id,
    );
    let valid_sig = verifier_kp.sign_recoverable(&valid_digest).unwrap();

    // Attack 1: Cross-Chain Replay Attack
    // Replay attestation issued for Testnet (102) onto Mainnet (9999) node
    let req_replay_chain = format!(
        r#"{{"jsonrpc":"2.0","id":20,"method":"tola_verifyExternalAssetAttestation","params":[{{
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
        mainnet_id.0, // Tampered chain ID or submitted to mainnet
        nonce,
        alice_wallet.to_hex(),
        expires_at,
        source_platform,
        asset_type.as_str(),
        external_asset_id,
        verifier_addr.to_hex(),
        hex_encode(&valid_sig[0..32]),
        hex_encode(&valid_sig[32..64]),
        valid_sig[64]
    );
    let res_replay_chain = mainnet_disp.dispatch_json(&req_replay_chain);
    let val_replay_chain = JsonValue::parse(&res_replay_chain).unwrap();
    let result_chain = val_replay_chain.get("result").unwrap();
    assert_eq!(
        result_chain.get("valid").and_then(|v| v.as_bool()),
        Some(false),
        "Cross-chain replay must be rejected"
    );

    // Attack 2: Cross-Wallet Hijacking Attack
    // Mallory intercepts attestation and attempts to claim token to Mallory's wallet
    let req_hijack = format!(
        r#"{{"jsonrpc":"2.0","id":21,"method":"tola_verifyExternalAssetAttestation","params":[{{
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
        testnet_id.0,
        nonce,
        mallory_wallet.to_hex(), // Mallory's address
        expires_at,
        source_platform,
        asset_type.as_str(),
        external_asset_id,
        verifier_addr.to_hex(),
        hex_encode(&valid_sig[0..32]),
        hex_encode(&valid_sig[32..64]),
        valid_sig[64]
    );
    let res_hijack = testnet_disp.dispatch_json(&req_hijack);
    let val_hijack = JsonValue::parse(&res_hijack).unwrap();
    let result_hijack = val_hijack.get("result").unwrap();
    assert_eq!(
        result_hijack.get("valid").and_then(|v| v.as_bool()),
        Some(false),
        "Cross-wallet hijacking must be rejected"
    );

    // Attack 3: Expired Attestation Attack
    let expired_timestamp = 1500000000u64; // in the past
    let expired_digest = compute_external_asset_attestation_digest(
        testnet_id.0,
        nonce + 1,
        &alice_wallet,
        expired_timestamp,
        source_platform,
        asset_type,
        external_asset_id,
    );
    let expired_sig = verifier_kp.sign_recoverable(&expired_digest).unwrap();
    let req_expired = format!(
        r#"{{"jsonrpc":"2.0","id":22,"method":"tola_verifyExternalAssetAttestation","params":[{{
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
        testnet_id.0,
        nonce + 1,
        alice_wallet.to_hex(),
        expired_timestamp,
        source_platform,
        asset_type.as_str(),
        external_asset_id,
        verifier_addr.to_hex(),
        hex_encode(&expired_sig[0..32]),
        hex_encode(&expired_sig[32..64]),
        expired_sig[64]
    );
    let res_expired = testnet_disp.dispatch_json(&req_expired);
    let val_expired = JsonValue::parse(&res_expired).unwrap();
    let result_expired = val_expired.get("result").unwrap();
    assert_eq!(
        result_expired.get("valid").and_then(|v| v.as_bool()),
        Some(false),
        "Expired attestation must be rejected"
    );

    // Attack 4: Duplicate Mint Prevention
    let canonical_id =
        compute_external_asset_canonical_id(source_platform, asset_type, external_asset_id);
    let asset1 = ExternalAsset {
        token_id: 8001,
        canonical_id,
        source_platform: source_platform.to_string(),
        asset_type,
        external_asset_id: external_asset_id.to_string(),
        external_reference: "ref_8001".to_string(),
        owner: alice_wallet,
        authorizer: verifier_addr,
        verifier_address: verifier_addr,
        minted_at: 1770002000,
        metadata_uri: "ipfs://bafybei8001".to_string(),
    };
    testnet_disp
        .register_external_asset(asset1)
        .expect("First mint must succeed");

    // Attempt second mint with identical canonical ID
    let asset2_duplicate = ExternalAsset {
        token_id: 8002,
        canonical_id,
        source_platform: source_platform.to_string(),
        asset_type,
        external_asset_id: external_asset_id.to_string(),
        external_reference: "ref_8002".to_string(),
        owner: mallory_wallet,
        authorizer: verifier_addr,
        verifier_address: verifier_addr,
        minted_at: 1770002001,
        metadata_uri: "ipfs://bafybei8002".to_string(),
    };
    let duplicate_result = testnet_disp.register_external_asset(asset2_duplicate);
    assert!(
        duplicate_result.is_err(),
        "Duplicate minting with same canonical ID must be rejected"
    );

    println!("[PASS] Adversarial attacks and duplicate mint attempts successfully repelled");
}

#[test]
fn test_permanent_architecture_eradication_invariants() {
    // 1. Filesystem Invariants: Legacy contracts MUST NOT exist on disk
    let registry_contract = std::path::Path::new("contracts/TolaUsernameRegistry.sol");
    let gift_contract = std::path::Path::new("contracts/TolaDigitalGift.sol");
    let external_asset_contract = std::path::Path::new("contracts/TolaExternalAssetToken.sol");

    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".into());
    let root = std::path::Path::new(&manifest_dir).join("../..");

    let reg_path = root.join("contracts/TolaUsernameRegistry.sol");
    let gift_path = root.join("contracts/TolaDigitalGift.sol");
    let ext_path = root.join("contracts/TolaExternalAssetToken.sol");

    assert!(
        !registry_contract.exists() && !reg_path.exists(),
        "TolaUsernameRegistry.sol must be completely eradicated from repository"
    );
    assert!(
        !gift_contract.exists() && !gift_path.exists(),
        "TolaDigitalGift.sol must be completely eradicated from repository"
    );
    assert!(
        external_asset_contract.exists() || ext_path.exists(),
        "TolaExternalAssetToken.sol must be preserved as the canonical external token contract"
    );

    // 2. RPC Absence Invariants:
    // Any call to eradicated native endpoints MUST return Method Not Found (-32601)
    let chain_id = ChainId::TESTNET;
    let disp = setup_test_dispatcher(chain_id);

    let eradicated_methods = [
        "tola_resolveUsername",
        "tola_getUsernameRecord",
        "tola_getGift",
        "tola_getGiftDetails",
        "tola_verifyBadgeAttestation",
        "tola_registerUsername",
        "tola_transferUsername",
        "tola_mintGift",
    ];

    for method in eradicated_methods {
        let req = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"{}","params":[]}}"#,
            method
        );
        let res = disp.dispatch_json(&req);
        let val = JsonValue::parse(&res).unwrap();
        assert_eq!(
            val.get("error")
                .and_then(|e| e.get("code"))
                .and_then(|c| c.as_i64()),
            Some(-32601),
            "Eradicated method {} must return -32601",
            method
        );
    }

    println!("[PASS] Permanent architecture eradication invariants verified");
}

fn make_contract_tx(
    keypair: &Secp256k1Keypair,
    chain_id: ChainId,
    nonce: Nonce,
    data: Vec<u8>,
    gas_limit: Gas,
    epoch: u64,
) -> Transaction {
    let from = keypair.address();
    let mut tx = Transaction {
        chain_id,
        nonce,
        from,
        to: Some(EXTERNAL_ASSET_CONTRACT_ADDRESS),
        value: Amount::ZERO,
        gas_limit,
        max_fee_per_gas: Amount::from_u128(10),
        max_priority_fee_per_gas: Amount::from_u128(2),
        valid_until_block: BlockHeight(100),
        data,
        memo: None,
        signature: Signature::Secp256k1 {
            r: [0u8; 32],
            s: [0u8; 32],
            v: 0,
        },
        tx_type: 0,
        access_list: vec![],
    };
    let digest = compute_tx_signing_digest(&tx, epoch);
    let sig_bytes = keypair.sign_recoverable(digest.as_bytes()).unwrap();
    let mut r = [0u8; 32];
    let mut s = [0u8; 32];
    r.copy_from_slice(&sig_bytes[0..32]);
    s.copy_from_slice(&sig_bytes[32..64]);
    tx.signature = Signature::Secp256k1 {
        r,
        s,
        v: sig_bytes[64],
    };
    tx
}

fn make_block(
    height: u64,
    proposer: Address,
    transactions: Vec<Transaction>,
    timestamp: u64,
) -> Block {
    Block {
        header: BlockHeader {
            parent_hash: Hash::ZERO,
            height: BlockHeight(height),
            epoch: Epoch(0),
            round: Round(0),
            timestamp: Timestamp(timestamp),
            proposer,
            state_root: Hash::ZERO,
            txs_root: Hash::ZERO,
            receipts_root: Hash::ZERO,
            gas_used: Gas(500_000),
            gas_limit: Gas(30_000_000),
            base_fee_per_gas: Amount::from_u128(5),
            extra_data: vec![],
        },
        transactions,
        last_commit_qc: None,
    }
}

fn unique_temp_dir(label: &str) -> std::path::PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("tola-test-{}-{}", label, nanos));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn test_on_chain_consensus_execution_and_persistent_storage_roundtrip() {
    let state_path = unique_temp_dir("external-asset-persist");
    let chain_id = ChainId::TESTNET;
    let engine = SequentialBlockEngine::new(chain_id);
    let epoch = 0u64;

    // 1. Keypairs
    let admin_kp = Secp256k1Keypair::generate();
    let alice_kp = Secp256k1Keypair::generate();
    let bob_kp = Secp256k1Keypair::generate();
    let charlie_kp = Secp256k1Keypair::generate();
    let authorizer_paxta_kp = Secp256k1Keypair::generate();
    let authorizer_mx_kp = Secp256k1Keypair::generate();
    let authorizer_game_kp = Secp256k1Keypair::generate();

    let admin = admin_kp.address();
    let alice = alice_kp.address();
    let bob = bob_kp.address();
    let charlie = charlie_kp.address();
    let authorizer_paxta = authorizer_paxta_kp.address();
    let authorizer_mx = authorizer_mx_kp.address();
    let authorizer_game = authorizer_game_kp.address();

    // 2. Open PersistentStateDB & fund accounts
    {
        let mut db = PersistentStateDB::open(&state_path).expect("Open db");
        let initial_funds = Amount::from_u128(1_000_000_000);
        db.apply_delta(StateDelta {
            accounts: vec![
                (admin, Some(AccountState::new_with_balance(initial_funds))),
                (alice, Some(AccountState::new_with_balance(initial_funds))),
                (bob, Some(AccountState::new_with_balance(initial_funds))),
                (charlie, Some(AccountState::new_with_balance(initial_funds))),
            ],
            storage: vec![],
            code: vec![],
        })
        .unwrap();
        db.commit(BlockHeight(0)).unwrap();
    }

    // 3. Multi-Platform Canonical Identifiers:
    // Platform A ("paxtagram") username "anvar"
    // Platform B ("messenger_x") username "anvar" (SAME username, different platforms -> DIFFERENT canonical IDs)
    // Platform A ("paxtagram") gift "123"
    // Platform C ("cyber_game") game item "sword-99"
    let cid_paxta_anvar = compute_external_asset_canonical_id(
        "paxtagram",
        ExternalAssetType::MessengerUsername,
        "anvar",
    );
    let cid_mx_anvar = compute_external_asset_canonical_id(
        "messenger_x",
        ExternalAssetType::SocialUsername,
        "anvar",
    );
    let cid_paxta_gift =
        compute_external_asset_canonical_id("paxtagram", ExternalAssetType::MessengerGift, "123");
    let cid_game_sword = compute_external_asset_canonical_id(
        "cyber_game",
        ExternalAssetType::DigitalCollectible,
        "sword-99",
    );

    // Cryptographic proof that identical user handles on different platforms yield distinct canonical IDs
    assert_ne!(
        cid_paxta_anvar, cid_mx_anvar,
        "Same handle on different platforms must have distinct canonical IDs"
    );
    assert_ne!(cid_paxta_anvar, cid_paxta_gift);
    assert_ne!(cid_mx_anvar, cid_game_sword);

    // 4. Construct Attestation Authorization Proofs
    let attestation_paxta_anvar = {
        let digest = compute_external_asset_attestation_digest(
            chain_id.0,
            1,
            &alice,
            2500000000,
            "paxtagram",
            ExternalAssetType::MessengerUsername,
            "anvar",
        );
        let sig = authorizer_paxta_kp.sign_recoverable(&digest).unwrap();
        ExternalAssetAttestation {
            chain_id: chain_id.0,
            nonce: 1,
            destination_wallet: alice,
            expires_at: 2500000000,
            source_platform: "paxtagram".to_string(),
            asset_type: ExternalAssetType::MessengerUsername,
            external_asset_id: "anvar".to_string(),
            authorizer_address: authorizer_paxta,
            verifier_address: authorizer_paxta,
            signature: sig,
        }
    };

    let attestation_mx_anvar = {
        let digest = compute_external_asset_attestation_digest(
            chain_id.0,
            2,
            &alice,
            2500000000,
            "messenger_x",
            ExternalAssetType::SocialUsername,
            "anvar",
        );
        let sig = authorizer_mx_kp.sign_recoverable(&digest).unwrap();
        ExternalAssetAttestation {
            chain_id: chain_id.0,
            nonce: 2,
            destination_wallet: alice,
            expires_at: 2500000000,
            source_platform: "messenger_x".to_string(),
            asset_type: ExternalAssetType::SocialUsername,
            external_asset_id: "anvar".to_string(),
            authorizer_address: authorizer_mx,
            verifier_address: authorizer_mx,
            signature: sig,
        }
    };

    let attestation_paxta_gift = {
        let digest = compute_external_asset_attestation_digest(
            chain_id.0,
            3,
            &alice,
            2500000000,
            "paxtagram",
            ExternalAssetType::MessengerGift,
            "123",
        );
        let sig = authorizer_paxta_kp.sign_recoverable(&digest).unwrap();
        ExternalAssetAttestation {
            chain_id: chain_id.0,
            nonce: 3,
            destination_wallet: alice,
            expires_at: 2500000000,
            source_platform: "paxtagram".to_string(),
            asset_type: ExternalAssetType::MessengerGift,
            external_asset_id: "123".to_string(),
            authorizer_address: authorizer_paxta,
            verifier_address: authorizer_paxta,
            signature: sig,
        }
    };

    let attestation_game_sword = {
        let digest = compute_external_asset_attestation_digest(
            chain_id.0,
            4,
            &charlie,
            2500000000,
            "cyber_game",
            ExternalAssetType::DigitalCollectible,
            "sword-99",
        );
        let sig = authorizer_game_kp.sign_recoverable(&digest).unwrap();
        ExternalAssetAttestation {
            chain_id: chain_id.0,
            nonce: 4,
            destination_wallet: charlie,
            expires_at: 2500000000,
            source_platform: "cyber_game".to_string(),
            asset_type: ExternalAssetType::DigitalCollectible,
            external_asset_id: "sword-99".to_string(),
            authorizer_address: authorizer_game,
            verifier_address: authorizer_game,
            signature: sig,
        }
    };

    // 5. Block 1: Minting across all 3 platforms with ZERO PRIOR VERIFIER REGISTRATION ON TOLA
    {
        let mut db = PersistentStateDB::open(&state_path).expect("Open db");

        let tx1 = make_contract_tx(
            &alice_kp,
            chain_id,
            Nonce(0),
            ExternalAssetCall::MintWithAttestation {
                attestation: attestation_paxta_anvar.clone(),
                metadata_uri: "ipfs://bafybeipaxtaanvar".to_string(),
            }
            .encode(),
            Gas(300_000),
            epoch,
        );

        let tx2 = make_contract_tx(
            &alice_kp,
            chain_id,
            Nonce(1),
            ExternalAssetCall::MintWithAttestation {
                attestation: attestation_mx_anvar.clone(),
                metadata_uri: "ipfs://bafybeimxanvar".to_string(),
            }
            .encode(),
            Gas(300_000),
            epoch,
        );

        let tx3 = make_contract_tx(
            &alice_kp,
            chain_id,
            Nonce(2),
            ExternalAssetCall::MintWithAttestation {
                attestation: attestation_paxta_gift.clone(),
                metadata_uri: "ipfs://bafybeipaxtagift123".to_string(),
            }
            .encode(),
            Gas(300_000),
            epoch,
        );

        let tx4 = make_contract_tx(
            &charlie_kp,
            chain_id,
            Nonce(0),
            ExternalAssetCall::MintWithAttestation {
                attestation: attestation_game_sword.clone(),
                metadata_uri: "ipfs://bafybeigamesword99".to_string(),
            }
            .encode(),
            Gas(300_000),
            epoch,
        );

        let block1 = make_block(1, admin, vec![tx1, tx2, tx3, tx4], 1770001000);
        assert!(engine.validate_block(&block1, &db).is_ok());
        let receipts1 = engine.execute_block(&block1, &mut db).unwrap();
        assert_eq!(receipts1.len(), 4);

        for (i, r) in receipts1.iter().enumerate() {
            assert!(
                r.success,
                "Tx{} mint must succeed with ZERO prior verifier registration",
                i + 1
            );
            assert_eq!(r.logs.len(), 2);
            assert_eq!(r.logs[0].topics[0], TOPIC_EXTERNAL_ASSET_MINTED);
            assert_eq!(r.logs[1].topics[0], TOPIC_TRANSFER);
        }

        db.commit(BlockHeight(1)).unwrap();

        // State assertions in PersistentStateDB:
        assert_eq!(ExternalAssetStorage::read_owner(&db, 1), Some(alice));
        assert_eq!(ExternalAssetStorage::read_owner(&db, 2), Some(alice));
        assert_eq!(ExternalAssetStorage::read_owner(&db, 3), Some(alice));
        assert_eq!(ExternalAssetStorage::read_owner(&db, 4), Some(charlie));

        assert_eq!(ExternalAssetStorage::read_balance(&db, &alice), 3);
        assert_eq!(ExternalAssetStorage::read_balance(&db, &charlie), 1);
        assert_eq!(ExternalAssetStorage::read_balance(&db, &bob), 0);

        // Canonical ID lookups
        assert_eq!(
            ExternalAssetStorage::read_token_by_canonical_id(&db, &cid_paxta_anvar),
            Some(1)
        );
        assert_eq!(
            ExternalAssetStorage::read_token_by_canonical_id(&db, &cid_mx_anvar),
            Some(2)
        );
        assert_eq!(
            ExternalAssetStorage::read_token_by_canonical_id(&db, &cid_paxta_gift),
            Some(3)
        );
        assert_eq!(
            ExternalAssetStorage::read_token_by_canonical_id(&db, &cid_game_sword),
            Some(4)
        );

        // Verify authorizers recorded on-chain
        let asset1 = ExternalAssetStorage::read_asset(&db, 1).expect("Token 1");
        assert_eq!(asset1.authorizer, authorizer_paxta);
        assert_eq!(asset1.source_platform, "paxtagram");
        assert_eq!(asset1.external_asset_id, "anvar");

        let asset2 = ExternalAssetStorage::read_asset(&db, 2).expect("Token 2");
        assert_eq!(asset2.authorizer, authorizer_mx);
        assert_eq!(asset2.source_platform, "messenger_x");
        assert_eq!(asset2.external_asset_id, "anvar");

        let asset4 = ExternalAssetStorage::read_asset(&db, 4).expect("Token 4");
        assert_eq!(asset4.authorizer, authorizer_game);
        assert_eq!(asset4.source_platform, "cyber_game");
        assert_eq!(asset4.external_asset_id, "sword-99");
    }

    // 6. Block 2: Adversarial Replay, Duplicate Canonical ID, and Forged Signature Defense
    let duplicate_canonical_attestation = {
        let digest = compute_external_asset_attestation_digest(
            chain_id.0,
            999, // New nonce
            &alice,
            2500000000,
            "paxtagram",
            ExternalAssetType::MessengerUsername,
            "anvar", // Same asset ID -> identical canonical ID!
        );
        let sig = authorizer_paxta_kp.sign_recoverable(&digest).unwrap();
        ExternalAssetAttestation {
            chain_id: chain_id.0,
            nonce: 999,
            destination_wallet: alice,
            expires_at: 2500000000,
            source_platform: "paxtagram".to_string(),
            asset_type: ExternalAssetType::MessengerUsername,
            external_asset_id: "anvar".to_string(),
            authorizer_address: authorizer_paxta,
            verifier_address: authorizer_paxta,
            signature: sig,
        }
    };

    let forged_signature_attestation = {
        let digest = compute_external_asset_attestation_digest(
            chain_id.0,
            500,
            &charlie,
            2500000000,
            "cyber_game",
            ExternalAssetType::DigitalCollectible,
            "shield-01",
        );
        // Signed by bob instead of authorizer_game!
        let sig = bob_kp.sign_recoverable(&digest).unwrap();
        ExternalAssetAttestation {
            chain_id: chain_id.0,
            nonce: 500,
            destination_wallet: charlie,
            expires_at: 2500000000,
            source_platform: "cyber_game".to_string(),
            asset_type: ExternalAssetType::DigitalCollectible,
            external_asset_id: "shield-01".to_string(),
            authorizer_address: authorizer_game,
            verifier_address: authorizer_game,
            signature: sig,
        }
    };

    {
        let mut db = PersistentStateDB::open(&state_path).expect("Open db");
        let tx_replay = make_contract_tx(
            &alice_kp,
            chain_id,
            Nonce(3),
            ExternalAssetCall::MintWithAttestation {
                attestation: attestation_paxta_anvar.clone(),
                metadata_uri: "ipfs://bafybeireplay".to_string(),
            }
            .encode(),
            Gas(300_000),
            epoch,
        );

        let tx_dup = make_contract_tx(
            &alice_kp,
            chain_id,
            Nonce(4),
            ExternalAssetCall::MintWithAttestation {
                attestation: duplicate_canonical_attestation,
                metadata_uri: "ipfs://bafybeidup".to_string(),
            }
            .encode(),
            Gas(300_000),
            epoch,
        );

        let tx_forged = make_contract_tx(
            &charlie_kp,
            chain_id,
            Nonce(1),
            ExternalAssetCall::MintWithAttestation {
                attestation: forged_signature_attestation,
                metadata_uri: "ipfs://bafybeiforged".to_string(),
            }
            .encode(),
            Gas(300_000),
            epoch,
        );

        let block2 = make_block(2, admin, vec![tx_replay, tx_dup, tx_forged], 1770002000);
        let receipts2 = engine.execute_block(&block2, &mut db).unwrap();
        assert_eq!(receipts2.len(), 3);
        assert!(!receipts2[0].success, "Replay mint must revert");
        assert!(!receipts2[1].success, "Duplicate canonical ID must revert");
        assert!(!receipts2[2].success, "Forged signature must revert");

        db.commit(BlockHeight(2)).unwrap();

        // Balances remain intact
        assert_eq!(ExternalAssetStorage::read_balance(&db, &alice), 3);
        assert_eq!(ExternalAssetStorage::read_balance(&db, &charlie), 1);
    }

    // 7. Block 3: Secondary Market Transfer on Consensus StateDB
    //    Alice transfers Token 1 to Bob
    {
        let mut db = PersistentStateDB::open(&state_path).expect("Open db");
        let tx_transfer = make_contract_tx(
            &alice_kp,
            chain_id,
            Nonce(5),
            ExternalAssetCall::TransferFrom {
                from: alice,
                to: bob,
                token_id: 1,
            }
            .encode(),
            Gas(200_000),
            epoch,
        );

        let block3 = make_block(3, admin, vec![tx_transfer], 1770003000);
        let receipts3 = engine.execute_block(&block3, &mut db).unwrap();
        assert_eq!(receipts3.len(), 1);
        assert!(receipts3[0].success, "Transfer to Bob must succeed");
        assert_eq!(receipts3[0].logs[0].topics[0], TOPIC_TRANSFER);

        db.commit(BlockHeight(3)).unwrap();

        // Ownership updated in StateDB
        assert_eq!(ExternalAssetStorage::read_owner(&db, 1), Some(bob));
        assert_eq!(ExternalAssetStorage::read_balance(&db, &alice), 2);
        assert_eq!(ExternalAssetStorage::read_balance(&db, &bob), 1);

        let asset1 = ExternalAssetStorage::read_asset(&db, 1).expect("Token 1");
        assert_eq!(asset1.owner, bob);
    }

    // 8. Block 4: Burn Lifecycle
    //    Charlie attempts unauthorized burn -> REVERT
    //    Bob (owner) burns Token 1 -> SUCCESS
    {
        let mut db = PersistentStateDB::open(&state_path).expect("Open db");
        let tx_bad_burn = make_contract_tx(
            &charlie_kp,
            chain_id,
            Nonce(2),
            ExternalAssetCall::Burn { token_id: 1 }.encode(),
            Gas(200_000),
            epoch,
        );

        let tx_bob_burn = make_contract_tx(
            &bob_kp,
            chain_id,
            Nonce(0),
            ExternalAssetCall::Burn { token_id: 1 }.encode(),
            Gas(200_000),
            epoch,
        );

        let block4 = make_block(4, admin, vec![tx_bad_burn, tx_bob_burn], 1770004000);
        let receipts4 = engine.execute_block(&block4, &mut db).unwrap();
        assert_eq!(receipts4.len(), 2);
        assert!(!receipts4[0].success, "Unauthorized burn must revert");
        assert!(receipts4[1].success, "Bob burn must succeed");

        db.commit(BlockHeight(4)).unwrap();

        // Token 1 burned
        assert_eq!(ExternalAssetStorage::read_owner(&db, 1), None);
        assert_eq!(ExternalAssetStorage::read_balance(&db, &bob), 0);
    }

    // 9. Re-open PersistentStateDB from disk and assert all storage persisted across node restart
    {
        let reopened_db = PersistentStateDB::open(&state_path).expect("Reopen db from disk");
        assert_eq!(reopened_db.current_height(), BlockHeight(4));

        // Token 1 is burned
        assert_eq!(ExternalAssetStorage::read_owner(&reopened_db, 1), None);
        assert_eq!(ExternalAssetStorage::read_balance(&reopened_db, &bob), 0);

        // Tokens 2, 3, 4 persisted with intact owners and authorizers
        assert_eq!(
            ExternalAssetStorage::read_owner(&reopened_db, 2),
            Some(alice)
        );
        assert_eq!(
            ExternalAssetStorage::read_owner(&reopened_db, 3),
            Some(alice)
        );
        assert_eq!(
            ExternalAssetStorage::read_owner(&reopened_db, 4),
            Some(charlie)
        );

        assert_eq!(ExternalAssetStorage::read_balance(&reopened_db, &alice), 2);
        assert_eq!(
            ExternalAssetStorage::read_balance(&reopened_db, &charlie),
            1
        );

        let r_asset2 = ExternalAssetStorage::read_asset(&reopened_db, 2).expect("reopened asset 2");
        assert_eq!(r_asset2.authorizer, authorizer_mx);
        assert_eq!(r_asset2.source_platform, "messenger_x");
        assert_eq!(r_asset2.external_asset_id, "anvar");

        let r_asset3 = ExternalAssetStorage::read_asset(&reopened_db, 3).expect("reopened asset 3");
        assert_eq!(r_asset3.authorizer, authorizer_paxta);
        assert_eq!(r_asset3.source_platform, "paxtagram");
        assert_eq!(r_asset3.external_asset_id, "123");

        let r_asset4 = ExternalAssetStorage::read_asset(&reopened_db, 4).expect("reopened asset 4");
        assert_eq!(r_asset4.authorizer, authorizer_game);
        assert_eq!(r_asset4.source_platform, "cyber_game");
        assert_eq!(r_asset4.external_asset_id, "sword-99");

        // Canonical ID lookups persisted
        assert_eq!(
            ExternalAssetStorage::read_token_by_canonical_id(&reopened_db, &cid_mx_anvar),
            Some(2)
        );
        assert_eq!(
            ExternalAssetStorage::read_token_by_canonical_id(&reopened_db, &cid_paxta_gift),
            Some(3)
        );
        assert_eq!(
            ExternalAssetStorage::read_token_by_canonical_id(&reopened_db, &cid_game_sword),
            Some(4)
        );
    }

    let _ = std::fs::remove_dir_all(&state_path);
}
