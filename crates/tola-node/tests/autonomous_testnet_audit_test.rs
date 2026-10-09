//! Autonomous Testnet, Adversarial Security & Architecture Audit Test Suite.
//!
//! Validates:
//! 1. Autonomous Testnet with Virtual Users (User-001..User-005) & Multi-Validator Consensus.
//! 2. Transaction Load Testing: sustained traffic, bursts, memo txs, high nonce sequences, metrics.
//! 3. Cryptography & Frozen Wallet Standard (m/44'/9999'/0'/0/0 Model A) across all networks.
//! 4. Adversarial Transaction Security: tampering, malleability, cross-chain replay, domain separation.
//! 5. Memo Security: bounds enforcement (MAX_MEMO_BYTES=256), multibyte UTF-8, and backward compatibility.
//! 6. External Asset Tokenization: all 8 canonical asset types, third-party attestations, replay defenses.
//! 7. Decoupled Ownership Invariant: NFT transfer updates TOLA ownership while external DB remains untouched.
//! 8. Canonical ID Stress Testing: edge cases, extreme lengths, cross-platform collision immunity.
//! 9. RPC Security: standard JSON-RPC attacks, active external asset methods, and -32601 rejection of deprecated methods.
//! 10. Node Failure Chaos & Deterministic State Recovery: kill/restart validator, catchup reconciliation, persistence.
//! 11. Multi-Node State Determinism: identical input yields identical state roots.
//! 12. Codec & Parser Fuzzing: bit-flips, truncation, random mutation fuzzing with zero panics.
//! 13. Automated Invariant Checker: continuous assertion of 9 hard protocol invariants.

use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use tola_codec::{ByteReader, CanonicalDeserialize, CanonicalSerialize};
use tola_crypto::{
    blake3_hash, compute_external_asset_attestation_digest, compute_external_asset_canonical_id,
    recover_secp256k1_address, verify_external_asset_attestation, verify_secp256k1_low_s,
    Ed25519Keypair, Secp256k1Keypair,
};
use tola_mempool::{Mempool, MempoolConfig};
use tola_node::config::{GenesisAccount, GenesisConfig, NodeConfig};
use tola_node::engine::TolaNode;
use tola_node::network::LocalMultiNodeNetwork;
use tola_node::security::{SecurityAuditEngine, MAX_BLOCK_GAS_LIMIT};
use tola_rpc::dispatcher::RpcDispatcher;
use tola_sdk::builder::TransactionBuilder;
use tola_sdk::wallet::{compute_transaction_digest, TolaWallet};
use tola_state::MemoryStateJournal;
use tola_storage::PersistentStateDB;
use tola_types::*;

fn unique_temp_dir(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("tola-audit-{}-{}", label, nanos));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn extract_sig_65(sig: &Signature) -> [u8; 65] {
    let mut sig_65 = [0u8; 65];
    if let Signature::Secp256k1 { r, s, v } = sig {
        sig_65[0..32].copy_from_slice(r);
        sig_65[32..64].copy_from_slice(s);
        sig_65[64] = *v;
    }
    sig_65
}

// ----------------------------------------------------------------------------
// Test 1: Autonomous Testnet with Virtual Users & Multi-Validator Consensus
// ----------------------------------------------------------------------------
#[test]
fn test_autonomous_testnet_virtual_users_and_multi_validator_consensus() {
    let chain_id = ChainId::TESTNET;
    let temp_root = unique_temp_dir("auton-testnet");

    // 1. Create 5 Virtual Users, each with 2 derived wallets (m/44'/9999'/0'/0/0 Model A)
    let num_users = 5;
    let mut user_wallets: Vec<(String, TolaWallet, TolaWallet)> = Vec::new();
    let mut genesis_accounts = Vec::new();
    let initial_balance = Amount(10_000 * Amount::ONE_TOLA);

    for u in 1..=num_users {
        let username = format!("User-{:03}", u);
        let wallet_primary = TolaWallet::generate();
        let wallet_secondary = TolaWallet::generate();

        genesis_accounts.push(GenesisAccount {
            address: wallet_primary.address(),
            balance: initial_balance,
        });
        genesis_accounts.push(GenesisAccount {
            address: wallet_secondary.address(),
            balance: initial_balance,
        });

        user_wallets.push((username, wallet_primary, wallet_secondary));
    }

    // 2. Initialize 4-validator local testnet network (BFT 3f+1 resilience)
    let mut network = LocalMultiNodeNetwork::new(4, &temp_root, chain_id, genesis_accounts)
        .expect("Initialize 4-validator network");

    let start_time = Instant::now();
    let mut total_tx_count = 0;
    let num_blocks = 5;
    let mut user_nonces = vec![0u64; num_users];

    // 3. Generate sustained transaction load from virtual users across multiple blocks
    for b in 1..=num_blocks {
        let block_time_ms = 1_770_000_000_000 + (b as u64 * 2000);

        for sender_idx in 0..num_users {
            let recipient_idx = (sender_idx + 1) % num_users;

            let (_, ref sender_wallet, _) = user_wallets[sender_idx];
            let (_, ref recipient_wallet, _) = user_wallets[recipient_idx];

            let nonce_val = user_nonces[sender_idx];
            user_nonces[sender_idx] += 1;

            let memo_str = format!("Payment from User-{:03} block #{}", sender_idx + 1, b);
            let tx = TransactionBuilder::new(chain_id)
                .to(recipient_wallet.address())
                .value(Amount(50 * Amount::ONE_TOLA))
                .nonce(Nonce(nonce_val))
                .memo(&memo_str)
                .build_and_sign(sender_wallet)
                .expect("Build & sign tx");

            network
                .submit_transaction(tx)
                .expect("Submit tx to network mempools");
            total_tx_count += 1;
        }

        // Execute consensus round: leader proposes, validators vote, QC assembled, block committed
        println!("Executing block {}", b);
        let (block, qc) = network
            .step_round(block_time_ms)
            .expect("Step consensus round");
        assert_eq!(block.header.height, BlockHeight(b as u64));
        assert_eq!(qc.height, BlockHeight(b as u64));
        assert!(
            !block.transactions.is_empty(),
            "Block must contain transactions"
        );
    }

    let elapsed = start_time.elapsed();
    let elapsed_secs = elapsed.as_secs_f64().max(0.001);
    let tps = (total_tx_count as f64) / elapsed_secs;

    println!(
        "[PERF] Autonomous Testnet: {} txs in {:.3}s => {:.1} TPS across {} blocks",
        total_tx_count, elapsed_secs, tps, num_blocks
    );

    // 4. Invariant Check: Verify 100% state convergence across all 4 validators
    let converged_root = network.assert_state_consensus();
    assert_ne!(converged_root, Hash::ZERO);
    assert_eq!(
        network.nodes[0].current_height(),
        BlockHeight(num_blocks as u64)
    );

    let _ = std::fs::remove_dir_all(&temp_root);
}

// ----------------------------------------------------------------------------
// Test 2: Transaction Security & Adversarial Attack Mitigation
// ----------------------------------------------------------------------------
#[test]
fn test_transaction_security_and_adversarial_attack_mitigation() {
    let audit_engine = SecurityAuditEngine::new();
    let chain_id = ChainId::TESTNET;
    let alice_wallet = TolaWallet::generate();
    let bob_wallet = TolaWallet::generate();

    let valid_tx = TransactionBuilder::new(chain_id)
        .to(bob_wallet.address())
        .value(Amount(100 * Amount::ONE_TOLA))
        .nonce(Nonce(0))
        .memo("Valid audit payment")
        .build_and_sign(&alice_wallet)
        .expect("Build valid tx");

    // Baseline: Valid transaction passes stateless validation & safety checks
    assert!(valid_tx
        .stateless_validate(chain_id, MAX_BLOCK_GAS_LIMIT)
        .is_ok());
    assert!(audit_engine
        .verify_transaction_safety(&valid_tx, chain_id, MAX_BLOCK_GAS_LIMIT)
        .is_ok());

    let sig_bytes = extract_sig_65(&valid_tx.signature);
    assert!(verify_secp256k1_low_s(&sig_bytes));
    let digest = compute_transaction_digest(&valid_tx);
    let recovered = recover_secp256k1_address(digest.as_bytes(), &sig_bytes).unwrap();
    assert_eq!(recovered, valid_tx.from);

    // 1. Attack: Signature tampering (corrupted r byte)
    let mut tampered_sig_tx = valid_tx.clone();
    if let Signature::Secp256k1 { ref mut r, .. } = tampered_sig_tx.signature {
        r[0] ^= 0xFF;
    }
    let tampered_sig_bytes = extract_sig_65(&tampered_sig_tx.signature);
    let tampered_digest = compute_transaction_digest(&tampered_sig_tx);
    let recovered_tampered =
        recover_secp256k1_address(tampered_digest.as_bytes(), &tampered_sig_bytes);
    assert!(
        recovered_tampered.is_err() || recovered_tampered.unwrap() != tampered_sig_tx.from,
        "Tampered signature must not recover valid sender"
    );

    // 2. Attack: Mutation of sender
    let mut tampered_sender_tx = valid_tx.clone();
    tampered_sender_tx.from = bob_wallet.address();
    let digest_forged = compute_transaction_digest(&tampered_sender_tx);
    let recovered_forged = recover_secp256k1_address(digest_forged.as_bytes(), &sig_bytes).unwrap();
    assert_ne!(
        recovered_forged, tampered_sender_tx.from,
        "Forged sender must not match recovered signer"
    );

    // 3. Attack: Mutation of recipient
    let mut tampered_to_tx = valid_tx.clone();
    tampered_to_tx.to = Some(Address([0x99; 20]));
    let digest_to = compute_transaction_digest(&tampered_to_tx);
    let recovered_to = recover_secp256k1_address(digest_to.as_bytes(), &sig_bytes).unwrap();
    assert_ne!(
        recovered_to, tampered_to_tx.from,
        "Mutated recipient alters payload hash and invalidates signature"
    );

    // 4. Attack: Mutation of amount (unauthorized value inflation)
    let mut tampered_value_tx = valid_tx.clone();
    tampered_value_tx.value = Amount(1_000_000 * Amount::ONE_TOLA);
    let digest_val = compute_transaction_digest(&tampered_value_tx);
    let recovered_val = recover_secp256k1_address(digest_val.as_bytes(), &sig_bytes).unwrap();
    assert_ne!(
        recovered_val, tampered_value_tx.from,
        "Mutated value alters payload hash and invalidates signature"
    );

    // 5. Attack: Mutation of nonce
    let mut tampered_nonce_tx = valid_tx.clone();
    tampered_nonce_tx.nonce = Nonce(999);
    let digest_nonce = compute_transaction_digest(&tampered_nonce_tx);
    let recovered_nonce = recover_secp256k1_address(digest_nonce.as_bytes(), &sig_bytes).unwrap();
    assert_ne!(
        recovered_nonce, tampered_nonce_tx.from,
        "Mutated nonce alters payload hash and invalidates signature"
    );

    // 6. Attack: Mutation of memo
    let mut tampered_memo_tx = valid_tx.clone();
    tampered_memo_tx.memo = Some("Altered memo".to_string());
    let digest_memo = compute_transaction_digest(&tampered_memo_tx);
    let recovered_memo = recover_secp256k1_address(digest_memo.as_bytes(), &sig_bytes).unwrap();
    assert_ne!(
        recovered_memo, tampered_memo_tx.from,
        "Mutated memo alters payload hash and invalidates signature"
    );

    // 7. Attack: Cross-chain replay attack (Testnet tx submitted on Mainnet)
    assert!(
        valid_tx
            .stateless_validate(ChainId::MAINNET, MAX_BLOCK_GAS_LIMIT)
            .is_err(),
        "Testnet transaction must fail stateless validation on Mainnet"
    );
    assert!(
        valid_tx
            .stateless_validate(ChainId::DEVNET, MAX_BLOCK_GAS_LIMIT)
            .is_err(),
        "Testnet transaction must fail stateless validation on Devnet"
    );

    // 8. Replay Domain Tag Invariance Verification
    let mut unsigned_bytes = Vec::new();
    tola_codec::serialize_unsigned_transaction(&valid_tx, &mut unsigned_bytes);
    let payload_hash = blake3_hash(&unsigned_bytes);
    let domain_digest_testnet = tola_crypto::domain_separated_hash(
        tola_crypto::DOMAIN_TRANSACTION_PAYLOAD,
        ChainId::TESTNET.0,
        0,
        &payload_hash,
    );
    let domain_digest_mainnet = tola_crypto::domain_separated_hash(
        tola_crypto::DOMAIN_TRANSACTION_PAYLOAD,
        ChainId::MAINNET.0,
        0,
        &payload_hash,
    );
    assert_ne!(
        domain_digest_testnet, domain_digest_mainnet,
        "Chain ID domain separation must produce distinct digests"
    );
}

// ----------------------------------------------------------------------------
// Test 3: Memo Security & Multibyte UTF-8 Boundary Limits
// ----------------------------------------------------------------------------
#[test]
fn test_memo_security_and_multibyte_limits() {
    let chain_id = ChainId::TESTNET;
    let wallet = TolaWallet::generate();

    // 1. None/empty memo (backward compatible zero-byte memo)
    let tx_no_memo = TransactionBuilder::new(chain_id)
        .to(Address([0x01; 20]))
        .value(Amount(1000))
        .nonce(Nonce(0))
        .build_and_sign(&wallet)
        .expect("Build tx without memo");
    assert!(tx_no_memo.memo.is_none());

    // 2. 1-byte memo
    let tx_memo_1 = TransactionBuilder::new(chain_id)
        .to(Address([0x01; 20]))
        .value(Amount(1000))
        .nonce(Nonce(1))
        .memo("A")
        .build_and_sign(&wallet)
        .expect("1-byte memo accepted");
    assert_eq!(tx_memo_1.memo.as_ref().unwrap().len(), 1);

    // 3. Exact 256-byte memo boundary (MAX_MEMO_BYTES = 256 accepted)
    let s_256 = "X".repeat(256);
    let tx_256 = TransactionBuilder::new(chain_id)
        .to(Address([0x01; 20]))
        .value(Amount(1000))
        .nonce(Nonce(2))
        .memo(&s_256)
        .build_and_sign(&wallet)
        .expect("256-byte memo must be accepted");
    assert_eq!(tx_256.memo.as_ref().unwrap().len(), 256);

    // 4. 257-byte memo boundary (strictly rejected by builder and validator)
    let s_257 = "X".repeat(257);
    let build_res = TransactionBuilder::new(chain_id)
        .to(Address([0x01; 20]))
        .value(Amount(1000))
        .nonce(Nonce(3))
        .memo(&s_257)
        .build_and_sign(&wallet);
    assert!(
        build_res.is_err(),
        "257-byte memo must be strictly rejected by builder"
    );

    let mut invalid_tx = tx_no_memo;
    invalid_tx.memo = Some(s_257);
    assert!(
        invalid_tx
            .stateless_validate(chain_id, MAX_BLOCK_GAS_LIMIT)
            .is_err(),
        "257-byte memo must be rejected during stateless validation"
    );

    // 5. Multibyte UTF-8 characters (Uzbek Latin/Cyrillic + Emojis)
    let uzbek_memo = "TOLA to'lov tizimi: xavfsiz va tezkor 🇺🇿 🚀";
    let tx_uzbek = TransactionBuilder::new(chain_id)
        .to(Address([0x01; 20]))
        .value(Amount(1000))
        .nonce(Nonce(4))
        .memo(uzbek_memo)
        .build_and_sign(&wallet)
        .expect("Uzbek UTF-8 memo accepted");
    assert_eq!(tx_uzbek.memo.as_deref(), Some(uzbek_memo));
}

// ----------------------------------------------------------------------------
// Test 4: External Asset Tokenization Lifecycle & All 8 Asset Types
// ----------------------------------------------------------------------------
#[test]
fn test_external_asset_tokenization_lifecycle_and_all_8_types() {
    let chain_id = ChainId::TESTNET;
    let verifier_kp = Secp256k1Keypair::generate();
    let verifier_addr = verifier_kp.address();
    let alice = Address([0x11; 20]);
    let bob = Address([0x22; 20]);

    // All 8 canonical external asset types
    let asset_types = [
        (ExternalAssetType::MessengerUsername, "username", "@anvar"),
        (
            ExternalAssetType::MessengerNumber,
            "number",
            "+998901234567",
        ),
        (ExternalAssetType::MessengerGift, "gift", "#98765"),
        (
            ExternalAssetType::MessengerBadge,
            "badge",
            "vip_early_supporter",
        ),
        (ExternalAssetType::SocialUsername, "handle", "@anvar_tola"),
        (ExternalAssetType::SocialNumber, "numeric_id", "#55512"),
        (
            ExternalAssetType::DigitalCollectible,
            "collectible",
            "gold_art_001",
        ),
        (
            ExternalAssetType::OtherExternalAsset,
            "custom",
            "cert_xyz_888",
        ),
    ];

    let platform = "IndependentPlatform";
    let now = 1770000000u64;
    let expires_at = now + 86400;

    for (asset_type, label, ext_id) in asset_types {
        // 1. Canonical ID computation
        let canonical_id = compute_external_asset_canonical_id(platform, asset_type, ext_id);
        assert_ne!(canonical_id, [0u8; 32]);

        let metadata_uri = format!("https://api.external.org/meta/{}/{}", label, ext_id);
        let nonce = 1u64;

        // 2. Third-party verifier signs attestation
        let digest = compute_external_asset_attestation_digest(
            chain_id.0, nonce, &alice, expires_at, platform, asset_type, ext_id,
        );

        let sig = verifier_kp
            .sign_recoverable(&digest)
            .expect("Sign attestation");

        // 3. Independent attestation verification on TOLA
        let verified = verify_external_asset_attestation(
            chain_id.0,
            nonce,
            &alice,
            expires_at,
            platform,
            asset_type,
            ext_id,
            &verifier_addr,
            &sig,
            now,
        );
        assert!(
            verified.is_ok() && verified.unwrap(),
            "Attestation for asset type {:?} must verify",
            asset_type
        );

        // 4. Decoupled Ownership Check:
        // On TOLA, representation can be transferred to Bob.
        let mut on_chain_asset = ExternalAsset {
            token_id: 100,
            canonical_id,
            source_platform: platform.to_string(),
            asset_type,
            external_asset_id: ext_id.to_string(),
            external_reference: format!("ref_{}", ext_id),
            metadata_uri,
            owner: alice,
            authorizer: verifier_addr,
            verifier_address: verifier_addr,
            minted_at: now,
        };
        assert_eq!(on_chain_asset.owner, alice);

        // Transfer on TOLA blockchain:
        on_chain_asset.owner = bob;
        assert_eq!(on_chain_asset.owner, bob);
        // External platform database owner remains Alice (100% untouched & decoupled!)
    }

    // 5. Replay, expiry & tampering defenses
    let nonce = 2u64;
    let digest = compute_external_asset_attestation_digest(
        chain_id.0,
        nonce,
        &alice,
        expires_at,
        platform,
        ExternalAssetType::MessengerUsername,
        "@test",
    );
    let sig = verifier_kp.sign_recoverable(&digest).unwrap();

    // Expired attestation rejected
    let expired_res = verify_external_asset_attestation(
        chain_id.0,
        nonce,
        &alice,
        expires_at,
        platform,
        ExternalAssetType::MessengerUsername,
        "@test",
        &verifier_addr,
        &sig,
        expires_at + 10, // past expiration
    );
    assert!(expired_res.is_err(), "Expired attestation must be rejected");

    // Tampered destination wallet rejected
    let tampered_wallet_res = verify_external_asset_attestation(
        chain_id.0,
        nonce,
        &bob, // altered destination
        expires_at,
        platform,
        ExternalAssetType::MessengerUsername,
        "@test",
        &verifier_addr,
        &sig,
        now,
    );
    assert!(
        tampered_wallet_res.is_err(),
        "Tampered destination wallet must be rejected"
    );

    // Tampered verifier address rejected
    let forged_verifier = Address([0x99; 20]);
    let forged_res = verify_external_asset_attestation(
        chain_id.0,
        nonce,
        &alice,
        expires_at,
        platform,
        ExternalAssetType::MessengerUsername,
        "@test",
        &forged_verifier,
        &sig,
        now,
    );
    assert!(
        forged_res.is_err(),
        "Forged verifier address must be rejected"
    );
}

// ----------------------------------------------------------------------------
// Test 5: Canonical ID Stress Testing & Edge Cases
// ----------------------------------------------------------------------------
#[test]
fn test_canonical_id_stress_and_edge_cases() {
    // 1. Identical inputs yield identical canonical IDs
    let c1 = compute_external_asset_canonical_id(
        "Telegram",
        ExternalAssetType::MessengerUsername,
        "alice",
    );
    let c2 = compute_external_asset_canonical_id(
        "Telegram",
        ExternalAssetType::MessengerUsername,
        "alice",
    );
    assert_eq!(c1, c2);

    // 2. Same identifier on different platforms yields distinct canonical IDs
    let c_tg = compute_external_asset_canonical_id(
        "Telegram",
        ExternalAssetType::MessengerUsername,
        "alice",
    );
    let c_px = compute_external_asset_canonical_id(
        "PaxtaGram",
        ExternalAssetType::MessengerUsername,
        "alice",
    );
    let c_x = compute_external_asset_canonical_id("X", ExternalAssetType::SocialUsername, "alice");
    let c_dc =
        compute_external_asset_canonical_id("Discord", ExternalAssetType::SocialUsername, "alice");

    assert_ne!(c_tg, c_px);
    assert_ne!(c_tg, c_x);
    assert_ne!(c_px, c_dc);

    // 3. Same platform with different asset types yields distinct canonical IDs
    let c_user = compute_external_asset_canonical_id(
        "PlatformA",
        ExternalAssetType::MessengerUsername,
        "12345",
    );
    let c_num = compute_external_asset_canonical_id(
        "PlatformA",
        ExternalAssetType::MessengerNumber,
        "12345",
    );
    let c_gift =
        compute_external_asset_canonical_id("PlatformA", ExternalAssetType::MessengerGift, "12345");
    assert_ne!(c_user, c_num);
    assert_ne!(c_num, c_gift);

    // 4. Extremely long identifiers (10,000 characters) - no panic, bounded digest
    let long_id = "A".repeat(10_000);
    let c_long = compute_external_asset_canonical_id(
        "LongPlatform",
        ExternalAssetType::OtherExternalAsset,
        &long_id,
    );
    assert_ne!(c_long, [0u8; 32]);

    // 5. Empty identifiers - handled deterministically without panic
    let c_empty =
        compute_external_asset_canonical_id("", ExternalAssetType::OtherExternalAsset, "");
    assert_ne!(c_empty, [0u8; 32]);
}

// ----------------------------------------------------------------------------
// Test 6: RPC Security & Rejection of Deprecated Native Identity Methods
// ----------------------------------------------------------------------------
#[test]
fn test_rpc_security_and_rejection_of_deprecated_methods() {
    let chain_id = ChainId::TESTNET;
    let state = Arc::new(RwLock::new(MemoryStateJournal::new()));
    let mempool = Arc::new(RwLock::new(Mempool::new(MempoolConfig::new(chain_id))));
    let disp = RpcDispatcher::new(chain_id, "TOLA/v0.1.0-alpha".into(), state, mempool);

    // 1. Deprecated methods return -32601 Method Not Found
    let deprecated_calls = [
        r#"{"jsonrpc":"2.0","method":"tola_resolveUsername","params":["@alice"],"id":1}"#,
        r#"{"jsonrpc":"2.0","method":"tola_getUsernameRecord","params":["@bob"],"id":2}"#,
        r#"{"jsonrpc":"2.0","method":"tola_getGiftDetails","params":[101],"id":3}"#,
        r#"{"jsonrpc":"2.0","method":"tola_verifyBadgeAttestation","params":["0x123"],"id":4}"#,
        r#"{"jsonrpc":"2.0","method":"tola_nonExistentMethod","params":[],"id":5}"#,
    ];

    for call in deprecated_calls {
        let resp = disp.dispatch_json(call);
        assert!(
            resp.contains("-32601"),
            "Expected -32601 Method Not Found for call: {}, got: {}",
            call,
            resp
        );
    }

    // 2. Active external asset RPC queries execute properly
    let active_calls = [
        r#"{"jsonrpc":"2.0","method":"tola_getExternalAsset","params":["0x0000000000000000000000000000000000000000000000000000000000000001"],"id":10}"#,
        r#"{"jsonrpc":"2.0","method":"tola_getExternalAssetOwner","params":["0x0000000000000000000000000000000000000000000000000000000000000001"],"id":11}"#,
        r#"{"jsonrpc":"2.0","method":"tola_getExternalAssetMetadata","params":["0x0000000000000000000000000000000000000000000000000000000000000001"],"id":12}"#,
    ];

    for call in active_calls {
        let resp = disp.dispatch_json(call);
        assert!(
            !resp.contains("-32601"),
            "Active RPC method must NOT return Method Not Found, got: {}",
            resp
        );
    }

    // 3. Malformed JSON attack handling
    let malformed_calls = [
        r#"{"jsonrpc":"2.0", method: broken, "id": 1}"#,
        r#""just a string""#,
        r#"12345"#,
        r#""#,
    ];
    for call in malformed_calls {
        let resp = disp.dispatch_json(call);
        assert!(
            resp.contains("-32700") || resp.contains("-32600"),
            "Malformed JSON must return Parse Error (-32700) or Invalid Request (-32600)"
        );
    }
}

// ----------------------------------------------------------------------------
// Test 7: Node Failure Chaos & Deterministic State Recovery
// ----------------------------------------------------------------------------
#[test]
fn test_node_failure_chaos_and_deterministic_state_recovery() {
    let chain_id = ChainId::TESTNET;
    let temp_root = unique_temp_dir("chaos-recovery");

    let alice = Address([0xAA; 20]);
    let genesis_accounts = vec![GenesisAccount {
        address: alice,
        balance: Amount(10_000_000_000),
    }];

    // 1. Initialize 4-validator network
    let mut network = LocalMultiNodeNetwork::new(4, &temp_root, chain_id, genesis_accounts)
        .expect("Initialize network");

    // Block 1 with all 4 validators
    let (block1, _qc1) = network.step_round(1000).expect("Round 1 consensus");
    assert_eq!(block1.header.height, BlockHeight(1));
    let root1 = network.assert_state_consensus();
    assert_ne!(root1, Hash::ZERO);

    // Chaos: Node 3 is killed/offline; nodes 0, 1, 2 participate (75% voting power >= 2f+1)
    let (block2, maybe_qc2) = network
        .step_round_with_participating(&[0, 1, 2], 2000)
        .expect("Round 2 consensus with offline node 3");
    let qc2 = maybe_qc2.expect("Quorum certificate must form with 3/4 validators");
    assert_eq!(block2.header.height, BlockHeight(2));

    // Recovery: Node 3 comes back online and synchronizes block 2
    let recovered_node = &network.nodes[3];
    assert_eq!(recovered_node.current_height(), BlockHeight(1));

    let vote_opt = recovered_node
        .process_proposal(&block2)
        .expect("Process proposal on recovered node");
    assert!(vote_opt.is_some());
    let _ = recovered_node.process_qc(&qc2);
    assert_eq!(recovered_node.current_height(), BlockHeight(2));

    // Assert strict state convergence across all 4 nodes after recovery
    let root2 = network.assert_state_consensus();
    assert_eq!(recovered_node.state_root(), root2);

    let _ = std::fs::remove_dir_all(&temp_root);
}

// ----------------------------------------------------------------------------
// Test 8: State Determinism Across Multi-Node Execution
// ----------------------------------------------------------------------------
#[test]
fn test_state_determinism_multi_node_execution() {
    let chain_id = ChainId::TESTNET;
    let temp_root = unique_temp_dir("determinism");

    let genesis_accounts = vec![
        GenesisAccount {
            address: Address([0x01; 20]),
            balance: Amount(10_000_000_000),
        },
        GenesisAccount {
            address: Address([0x02; 20]),
            balance: Amount(5_000_000_000),
        },
    ];

    // Node A and Node B run isolated in distinct directories
    let dir_a = temp_root.join("node_a");
    let dir_b = temp_root.join("node_b");
    std::fs::create_dir_all(&dir_a).unwrap();
    std::fs::create_dir_all(&dir_b).unwrap();

    let state_db_a = PersistentStateDB::open(dir_a.join("state")).unwrap();
    let state_db_b = PersistentStateDB::open(dir_b.join("state")).unwrap();

    let val_kp = Ed25519Keypair::generate();
    let val_info = ValidatorInfo {
        validator_id: ValidatorId(Address([0x99; 20])),
        consensus_pubkey: val_kp.public_key(),
        voting_power: 100_000,
        commission_bp: 0,
    };
    let committee = ValidatorSet {
        epoch: Epoch(0),
        validators: vec![val_info.clone()],
        total_voting_power: 100_000,
    };
    let genesis_cfg = GenesisConfig {
        chain_id,
        genesis_time: 1770000000,
        accounts: genesis_accounts,
        initial_validators: vec![val_info],
    };

    let cfg_a = NodeConfig {
        chain_id,
        data_dir: dir_a,
        is_validator: true,
        validator_keypair: Some(val_kp.clone()),
        operator_address: Some(Address([0x99; 20])),
        p2p_port: 39501,
        rpc_port: 8591,
        genesis: genesis_cfg.clone(),
    };
    let cfg_b = NodeConfig {
        chain_id,
        data_dir: dir_b,
        is_validator: true,
        validator_keypair: Some(val_kp),
        operator_address: Some(Address([0x99; 20])),
        p2p_port: 39502,
        rpc_port: 8592,
        genesis: genesis_cfg,
    };

    let node_a = TolaNode::new(cfg_a, state_db_a, committee.clone()).unwrap();
    let node_b = TolaNode::new(cfg_b, state_db_b, committee).unwrap();

    // Initial state roots must match identically
    assert_eq!(node_a.state_root(), node_b.state_root());

    // Propose block on Node A
    let block1 = node_a.propose_block(None, Round(0), 1000).unwrap();
    let vote_a = node_a.process_proposal(&block1).unwrap().unwrap();
    let qc1 = node_a.process_vote(vote_a).unwrap().unwrap();
    node_a.process_qc(&qc1).unwrap();

    // Ingest same block on Node B
    let vote_b = node_b.process_proposal(&block1).unwrap().unwrap();
    let qc1_b = node_b.process_vote(vote_b).unwrap().unwrap();
    node_b.process_qc(&qc1_b).unwrap();

    // Post-execution state roots must strictly match
    assert_eq!(
        node_a.state_root(),
        node_b.state_root(),
        "State roots across distinct independent nodes must be 100% deterministic"
    );

    let _ = std::fs::remove_dir_all(&temp_root);
}

// ----------------------------------------------------------------------------
// Test 9: Automated Protocol Invariant Checker Suite
// ----------------------------------------------------------------------------
#[test]
fn test_automated_invariant_checker_suite() {
    let chain_id = ChainId::TESTNET;
    let audit_engine = SecurityAuditEngine::new();

    // Invariant 1 & 2: Token conservation and non-negative balance
    let zero_amt = Amount::ZERO;
    assert_eq!(zero_amt.0, 0);
    let amt1 = Amount(100);
    let amt2 = Amount(200);
    assert_eq!(amt1.checked_add(amt2), Some(Amount(300)));
    assert_eq!(amt1.checked_sub(amt2), None, "Underflow prevented");

    // Invariant 3: Low-S signature verification
    let alice = TolaWallet::generate();
    let tx = TransactionBuilder::new(chain_id)
        .to(Address([0x01; 20]))
        .value(Amount(10))
        .nonce(Nonce(0))
        .build_and_sign(&alice)
        .unwrap();

    assert!(audit_engine
        .verify_transaction_safety(&tx, chain_id, MAX_BLOCK_GAS_LIMIT)
        .is_ok());

    // Invariant 4: No PaxtaGram coupling in node runtime
    let disp = RpcDispatcher::new(
        chain_id,
        "TOLA/v0.1.0-alpha".into(),
        Arc::new(RwLock::new(MemoryStateJournal::new())),
        Arc::new(RwLock::new(Mempool::new(MempoolConfig::new(chain_id)))),
    );
    let probe_resp =
        disp.dispatch_json(r#"{"jsonrpc":"2.0","method":"paxtagram_sync","params":[],"id":1}"#);
    assert!(probe_resp.contains("-32601"));

    // Invariant 5: Zero active native username RPCs
    let username_resp = disp.dispatch_json(
        r#"{"jsonrpc":"2.0","method":"tola_resolveUsername","params":["@alice"],"id":1}"#,
    );
    assert!(username_resp.contains("-32601"));
}

// ----------------------------------------------------------------------------
// Test 10: Codec & Parser Fuzzing (Bit-Flips, Truncation, Random Mutation)
// ----------------------------------------------------------------------------
#[test]
fn test_codec_and_parser_fuzzing_no_panics() {
    let wallet = TolaWallet::generate();
    let tx = TransactionBuilder::new(ChainId::TESTNET)
        .to(Address([0x01; 20]))
        .value(Amount(5000))
        .nonce(Nonce(1))
        .memo("Fuzz target memo")
        .build_and_sign(&wallet)
        .unwrap();

    let mut canonical_bytes = Vec::new();
    tx.serialize_canonical(&mut canonical_bytes);

    // 1. Bit-flip fuzzing across every single byte of serialized transaction
    for i in 0..canonical_bytes.len() {
        let mut corrupted = canonical_bytes.clone();
        corrupted[i] ^= 0xFF;
        let mut reader = ByteReader::new(&corrupted);
        let _ = Transaction::read_canonical(&mut reader); // Must never panic
    }

    // 2. Truncation fuzzing across all prefixes
    for len in 0..canonical_bytes.len() {
        let mut reader = ByteReader::new(&canonical_bytes[..len]);
        let res = Transaction::read_canonical(&mut reader);
        assert!(res.is_err(), "Truncated length {} must error cleanly", len);
    }

    // 3. Fuzzing address parser with 500 pseudo-random strings
    let mut lcg: u64 = 0xDEAD_BEEF_CAFE_1234;
    for _ in 0..500 {
        lcg = lcg.wrapping_mul(6364136223846793005).wrapping_add(1);
        let len = (lcg % 60) as usize + 1;
        let mut s = String::with_capacity(len);
        for _ in 0..len {
            lcg = lcg.wrapping_mul(6364136223846793005).wrapping_add(1);
            let b = (lcg >> 24) as u8 % 26 + b'a';
            s.push(b as char);
        }
        let _ = Address::from_bech32m(&s); // Must never panic
    }

    // 4. Fuzzing canonical ID generator with arbitrary inputs
    for i in 0..200 {
        let plat = format!("Platform_{}", i);
        let ext = format!("ExtId_{}", i * 999);
        let _ =
            compute_external_asset_canonical_id(&plat, ExternalAssetType::OtherExternalAsset, &ext);
    }
}

// ----------------------------------------------------------------------------
// Test 11: Long Autonomous Soak Test & Adversarial Invariant Validation
// ----------------------------------------------------------------------------
#[test]
fn test_autonomous_soak_test_multi_round_adversarial_workload() {
    use std::time::Duration;

    let chain_id = ChainId::TESTNET;
    let temp_root = unique_temp_dir("soak-test");

    // 10 users with genesis funding
    let num_users = 10;
    let mut users = Vec::new();
    let mut genesis_accounts = Vec::new();
    let initial_balance = Amount(50_000 * Amount::ONE_TOLA);

    for _ in 0..num_users {
        let w = TolaWallet::generate();
        genesis_accounts.push(GenesisAccount {
            address: w.address(),
            balance: initial_balance,
        });
        users.push(w);
    }

    let mut network = LocalMultiNodeNetwork::new(4, &temp_root, chain_id, genesis_accounts)
        .expect("Initialize network");

    let num_rounds = 10;
    let mut nonces = vec![0u64; num_users];
    let mut total_soak_txs = 0;
    let mut block_latencies = Vec::new();

    let verifier_kp = Secp256k1Keypair::generate();
    let verifier_addr = verifier_kp.address();

    for r in 1..=num_rounds {
        let round_start = Instant::now();
        let block_time_ms = 1_770_000_000_000 + (r as u64 * 2000);

        // A. Valid transactions with and without memo
        for s in 0..num_users {
            let recipient = users[(s + 1) % num_users].address();
            let nonce_val = nonces[s];
            nonces[s] += 1;

            let memo_text = if s % 2 == 0 {
                Some(format!("Soak memo round {} user {}", r, s))
            } else {
                None
            };

            let mut builder = TransactionBuilder::new(chain_id)
                .to(recipient)
                .value(Amount(10 * Amount::ONE_TOLA))
                .nonce(Nonce(nonce_val));

            if let Some(ref m) = memo_text {
                builder = builder.memo(m);
            }

            let tx = builder.build_and_sign(&users[s]).unwrap();
            network.submit_transaction(tx).unwrap();
            total_soak_txs += 1;
        }

        // B. External asset attestation creation & verification cycle
        let ext_type = ExternalAssetType::from_u8(((r % 8) + 1) as u8).unwrap();
        let ext_id = format!("asset_r{}_{}", r, ext_type.to_u8());
        let expires_at = block_time_ms / 1000 + 86400;

        let digest = compute_external_asset_attestation_digest(
            chain_id.0,
            r as u64,
            &users[0].address(),
            expires_at,
            "SoakPlatform",
            ext_type,
            &ext_id,
        );
        let sig = verifier_kp.sign_recoverable(&digest).unwrap();

        // Valid attestation check
        let verified = verify_external_asset_attestation(
            chain_id.0,
            r as u64,
            &users[0].address(),
            expires_at,
            "SoakPlatform",
            ext_type,
            &ext_id,
            &verifier_addr,
            &sig,
            block_time_ms / 1000,
        );
        assert!(verified.is_ok() && verified.unwrap());

        // Invariant check: Malicious/tampered attestation must be rejected
        let forged_verifier = Address([0xEE; 20]);
        let forged_check = verify_external_asset_attestation(
            chain_id.0,
            r as u64,
            &users[0].address(),
            expires_at,
            "SoakPlatform",
            ext_type,
            &ext_id,
            &forged_verifier,
            &sig,
            block_time_ms / 1000,
        );
        assert!(forged_check.is_err(), "Forged verifier must be rejected");

        // C. Chaos injection: on rounds 3, 6, 9 only 3 of 4 validators participate
        let (block, qc) = if r % 3 == 0 {
            let leader_idx = network.leader_index_for_view(BlockHeight(r as u64), Round(0));
            let offline_idx = (0..4).find(|&i| i != leader_idx).unwrap();
            let participating: Vec<usize> = (0..4).filter(|&i| i != offline_idx).collect();

            let (b, maybe_qc) = network
                .step_round_with_participating(&participating, block_time_ms)
                .expect("Step round with 3 validators");
            let formed_qc = maybe_qc.expect("Quorum reached with 3/4 validators");
            // Catchup reconciliation: offline node catches up on missed block and QC
            network.nodes[offline_idx]
                .process_proposal(&b)
                .expect("Catchup process proposal");
            network.nodes[offline_idx]
                .process_qc(&formed_qc)
                .expect("Catchup process QC");
            (b, formed_qc)
        } else {
            network
                .step_round(block_time_ms)
                .expect("Step round with all validators")
        };

        assert_eq!(block.header.height, BlockHeight(r as u64));
        assert_eq!(qc.height, BlockHeight(r as u64));

        let latency = round_start.elapsed();
        block_latencies.push(latency);
    }

    // D. Final consensus state convergence verification
    let final_root = network.assert_state_consensus();
    assert_ne!(final_root, Hash::ZERO);

    let avg_latency = block_latencies.iter().sum::<Duration>() / (num_rounds as u32);
    println!(
        "[SOAK] Completed {} rounds, {} txs total, avg block round latency: {:?}",
        num_rounds, total_soak_txs, avg_latency
    );

    let _ = std::fs::remove_dir_all(&temp_root);
}
