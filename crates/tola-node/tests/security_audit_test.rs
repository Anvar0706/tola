//! Adversarial security audit and threat mitigation integration test suite.
//!
//! Validates:
//! 1. TM-01 / TM-02: Byzantine equivocation detection, proof verification, and slashing penalty.
//! 2. TM-04: Eclipse attack defense via anti-Sybil subnet quotas.
//! 3. TM-06: Mempool DDoS defense via gas envelopes and resource limits.
//! 4. TM-07: Cross-chain replay attack prevention and Chain ID isolation.
//! 5. TM-10: Strict low-S signature malleability immunity (BIP-146 / RFC 6979).
//! 6. TM-18: Native token conservation, checked arithmetic, and underflow defense.
//! 7. State Merkle proof cryptographic tamper resistance.

use std::net::IpAddr;

use tola_consensus::qc::compute_vote_hash;
use tola_consensus::registry::{RegistryConfig, ValidatorRegistry};
use tola_consensus::slashing::SlashingEngine;
use tola_core::NetworkError;
use tola_crypto::{recover_secp256k1_address, verify_secp256k1_low_s, Ed25519Keypair};
use tola_p2p::peer::{PeerId, SubnetLimiter};
use tola_storage::jmt::{verify_jmt_proof, PersistentJmt};
use tola_storage::kv::DiskKvStore;
use tola_types::*;

use tola_node::security::{InvariantViolation, SecurityAuditEngine, MAX_BLOCK_GAS_LIMIT};

#[test]
fn test_byzantine_equivocation_detection_and_slashing() {
    let mut registry = ValidatorRegistry::new(RegistryConfig::default());
    let keypair = Ed25519Keypair::generate();
    let val_addr = Address([0x77; 20]);
    let val_id = ValidatorId(val_addr);

    // Register validator with 100,000 TOLA self-stake
    registry
        .register_validator(
            val_addr,
            keypair.public_key(),
            Amount(100_000 * Amount::ONE_TOLA),
            500,
        )
        .expect("Register validator");

    let engine = SlashingEngine::default();
    let height = BlockHeight(42);
    let round = Round(3);
    let hash_a = Hash([0xAA; 32]);
    let hash_b = Hash([0xBB; 32]);

    // Validator maliciously signs two conflicting prevotes for the same height and round
    let msg_a = compute_vote_hash(VoteType::Prevote, height, round, &hash_a);
    let msg_b = compute_vote_hash(VoteType::Prevote, height, round, &hash_b);

    let vote_a = Vote {
        vote_type: VoteType::Prevote,
        height,
        round,
        block_hash: hash_a,
        validator: val_id,
        signature: keypair.sign(&msg_a.0),
    };

    let vote_b = Vote {
        vote_type: VoteType::Prevote,
        height,
        round,
        block_hash: hash_b,
        validator: val_id,
        signature: keypair.sign(&msg_b.0),
    };

    let evidence = SlashingEvidence {
        height,
        round,
        vote_a,
        vote_b,
    };

    // Slashing execution
    let slash_receipt = engine
        .process_slashing(&evidence, &mut registry, BlockHeight(43))
        .expect("Slashing processing must succeed");

    // 5% of 100,000 TOLA = 5,000 TOLA slashed per Decisions D02 and D06
    assert_eq!(
        slash_receipt.slashed_amount,
        Amount(5_000 * Amount::ONE_TOLA)
    );
    assert_eq!(slash_receipt.validator_id, val_id);

    // Verify validator is jailed
    let info = registry.get_validator(&val_id).expect("Validator info");
    assert_eq!(info.status, ValidatorStatus::Jailed);
    assert_eq!(info.total_bonded_stake, Amount(95_000 * Amount::ONE_TOLA));
}

#[test]
fn test_cross_chain_replay_attack_isolation() {
    let audit_engine = SecurityAuditEngine::new();
    let t_chain = ChainId(102); // TOLA Testnet

    let make_tx = |chain: u64| Transaction {
        chain_id: ChainId(chain),
        nonce: Nonce(0),
        from: Address([0x11; 20]),
        to: Some(Address([0x22; 20])),
        value: Amount(100),
        gas_limit: Gas(21_000),
        max_fee_per_gas: Amount(1_000_000_000),
        max_priority_fee_per_gas: Amount(1_000_000_000),
        valid_until_block: BlockHeight(1_000),
        data: vec![],
        memo: None,
        signature: Signature::Secp256k1 {
            r: [0x01; 32],
            s: [0x01; 32],
            v: 0,
        },
        tx_type: 0,
        access_list: vec![],
    };

    // 1. Transaction crafted for Ethereum Mainnet (Chain ID 1)
    let eth_tx = make_tx(1);
    assert!(matches!(
        audit_engine.verify_transaction_safety(&eth_tx, t_chain, MAX_BLOCK_GAS_LIMIT),
        Err(InvariantViolation::InvalidChainId {
            expected: 102,
            actual: 1
        })
    ));

    // 2. Transaction crafted for Polygon (Chain ID 137)
    let poly_tx = make_tx(137);
    assert!(matches!(
        audit_engine.verify_transaction_safety(&poly_tx, t_chain, MAX_BLOCK_GAS_LIMIT),
        Err(InvariantViolation::InvalidChainId {
            expected: 102,
            actual: 137
        })
    ));

    // 3. Genuine TOLA Testnet transaction (Chain ID 102)
    let tola_tx = make_tx(102);
    assert!(audit_engine
        .verify_transaction_safety(&tola_tx, t_chain, MAX_BLOCK_GAS_LIMIT)
        .is_ok());

    // 4. Mainnet (Chain ID 9999) verification & legacy 101 rejection
    let m_chain = ChainId::MAINNET; // ChainId(9999)
    let mainnet_tx = make_tx(9999);
    assert!(audit_engine
        .verify_transaction_safety(&mainnet_tx, m_chain, MAX_BLOCK_GAS_LIMIT)
        .is_ok());

    // Legacy Chain ID 101 rejected on Mainnet 9999
    let legacy_101_tx = make_tx(101);
    assert!(matches!(
        audit_engine.verify_transaction_safety(&legacy_101_tx, m_chain, MAX_BLOCK_GAS_LIMIT),
        Err(InvariantViolation::InvalidChainId {
            expected: 9999,
            actual: 101,
        })
    ));

    // Testnet tx (102) rejected on Mainnet (9999)
    assert!(matches!(
        audit_engine.verify_transaction_safety(&tola_tx, m_chain, MAX_BLOCK_GAS_LIMIT),
        Err(InvariantViolation::InvalidChainId {
            expected: 9999,
            actual: 102,
        })
    ));

    // Mainnet tx (9999) rejected on Testnet (102)
    assert!(matches!(
        audit_engine.verify_transaction_safety(&mainnet_tx, t_chain, MAX_BLOCK_GAS_LIMIT),
        Err(InvariantViolation::InvalidChainId {
            expected: 102,
            actual: 9999,
        })
    ));
}

#[test]
fn test_signature_malleability_high_s_strict_rejection() {
    // Curve order n/2 for Secp256k1:
    // 0x7FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF5D576E7357A4501DDFE92F46681B20A0
    let mut low_s = [0x10u8; 32];
    low_s[0] = 0x10; // definitely < 0x7F...
    let high_s = [0xffu8; 32]; // definitely > 0x7F...

    let mut sig_valid = [0u8; 65];
    sig_valid[0..32].copy_from_slice(&[0x05; 32]);
    sig_valid[32..64].copy_from_slice(&low_s);
    sig_valid[64] = 0;

    let mut sig_malleable = [0u8; 65];
    sig_malleable[0..32].copy_from_slice(&[0x05; 32]);
    sig_malleable[32..64].copy_from_slice(&high_s);
    sig_malleable[64] = 0;

    assert!(
        verify_secp256k1_low_s(&sig_valid),
        "Low-S must be recognized as canonical"
    );
    assert!(
        !verify_secp256k1_low_s(&sig_malleable),
        "High-S must be rejected as malleable"
    );

    let digest = [0x42u8; 32];
    let res = recover_secp256k1_address(&digest, &sig_malleable);
    assert!(matches!(
        res,
        Err(tola_crypto::CryptoError::MalleableSignatureHighS)
    ));
}

#[test]
fn test_token_conservation_and_arithmetic_overflow_immunity() {
    let audit_engine = SecurityAuditEngine::new();
    let initial_genesis = Amount(1_000_000_000_000_000_000_000); // 1,000 TOLA

    // Legitimate state: 950 circulating + 50 burned = 1000
    let circulating = Amount(950_000_000_000_000_000_000);
    let burned = Amount(50_000_000_000_000_000_000);
    assert!(audit_engine
        .verify_supply_conservation(initial_genesis, circulating, burned)
        .is_ok());

    // Unauthorized inflation: 1 wei minted out of thin air
    let fraudulent_circulating = Amount(950_000_000_000_000_001);
    assert!(matches!(
        audit_engine.verify_supply_conservation(initial_genesis, fraudulent_circulating, burned),
        Err(InvariantViolation::SupplyMismatch { .. })
    ));

    // Overflow attempt using u128::MAX
    let max_circulating = Amount(u128::MAX);
    assert!(matches!(
        audit_engine.verify_supply_conservation(initial_genesis, max_circulating, Amount(1)),
        Err(InvariantViolation::SupplyMismatch { .. })
    ));

    // Underflow protection on AccountState debit
    let mut acct = AccountState {
        nonce: Nonce(0),
        balance: Amount(100),
        storage_root: Hash::ZERO,
        code_hash: Hash::ZERO,
        flags: 0,
    };
    assert!(acct.debit(Amount(100)).is_ok());
    assert_eq!(acct.balance, Amount(0));
    // Debit 1 more wei from empty account fails safely
    assert!(acct.debit(Amount(1)).is_err());
    assert_eq!(acct.balance, Amount(0));
}

#[test]
fn test_mempool_ddos_gas_limits_and_resource_bounds() {
    let audit_engine = SecurityAuditEngine::new();
    let chain_id = ChainId(102);

    let base_tx = Transaction {
        chain_id,
        nonce: Nonce(0),
        from: Address([0x11; 20]),
        to: Some(Address([0x22; 20])),
        value: Amount(100),
        gas_limit: Gas(21_000),
        max_fee_per_gas: Amount(1_000_000_000),
        max_priority_fee_per_gas: Amount(1_000_000_000),
        valid_until_block: BlockHeight(1_000),
        data: vec![],
        memo: None,
        signature: Signature::Secp256k1 {
            r: [0x01; 32],
            s: [0x01; 32],
            v: 0,
        },
        tx_type: 0,
        access_list: vec![],
    };

    // 1. Gas limit below intrinsic transfer cost (20,999 < 21,000)
    let mut under_gas_tx = base_tx.clone();
    under_gas_tx.gas_limit = Gas(20_999);
    assert!(matches!(
        audit_engine.verify_transaction_safety(&under_gas_tx, chain_id, MAX_BLOCK_GAS_LIMIT),
        Err(InvariantViolation::InvalidGasParameters(_))
    ));

    // 2. Gas limit exceeding block limit (30,000,001 > 30,000,000)
    let mut over_gas_tx = base_tx.clone();
    over_gas_tx.gas_limit = Gas(30_000_001);
    assert!(matches!(
        audit_engine.verify_transaction_safety(&over_gas_tx, chain_id, MAX_BLOCK_GAS_LIMIT),
        Err(InvariantViolation::InvalidGasParameters(_))
    ));

    // 3. Max priority fee exceeding max fee
    let mut fee_mismatch_tx = base_tx.clone();
    fee_mismatch_tx.max_priority_fee_per_gas = Amount(2_000_000_000);
    fee_mismatch_tx.max_fee_per_gas = Amount(1_000_000_000);
    assert!(matches!(
        audit_engine.verify_transaction_safety(&fee_mismatch_tx, chain_id, MAX_BLOCK_GAS_LIMIT),
        Err(InvariantViolation::InvalidGasParameters(_))
    ));
}

#[test]
fn test_eclipse_defense_anti_sybil_subnet_limiter() {
    let mut limiter = SubnetLimiter::new(2); // Max 2 peers per /24 subnet

    let peer1 = PeerId::random_for_test(1);
    let peer2 = PeerId::random_for_test(2);
    let peer3 = PeerId::random_for_test(3);

    let ip1: IpAddr = "192.168.1.10".parse().unwrap();
    let ip2: IpAddr = "192.168.1.20".parse().unwrap();
    let ip3: IpAddr = "192.168.1.99".parse().unwrap();
    let ip_diff_subnet: IpAddr = "192.168.2.10".parse().unwrap();

    // First two connections from 192.168.1.0/24 accepted
    assert!(
        limiter.register_peer(peer1, ip1).is_ok(),
        "First peer from subnet must be allowed"
    );
    assert!(
        limiter.register_peer(peer2, ip2).is_ok(),
        "Second peer from subnet must be allowed"
    );

    // Third connection from same /24 rejected (Eclipse attack defense)
    assert_eq!(
        limiter.register_peer(peer3, ip3),
        Err(NetworkError::SubnetLimitExceeded),
        "Third peer from same /24 subnet must be rejected"
    );

    // Peer from different subnet 192.168.2.0/24 accepted
    assert!(
        limiter.register_peer(peer3, ip_diff_subnet).is_ok(),
        "Peer from different subnet must be allowed"
    );

    // Releasing peer1 opens quota slot for ip3
    limiter.unregister_peer(&peer1);
    assert!(
        limiter.register_peer(peer1, ip1).is_ok(),
        "Peer must be allowed after quota slot is freed"
    );
}

#[test]
fn test_jmt_cryptographic_proof_tampering_rejected() {
    let mut db = DiskKvStore::open_transient();
    let addr = Address([0x77; 20]);
    let state = AccountState {
        nonce: Nonce(10),
        balance: Amount::from_u128(88_888),
        code_hash: Hash::ZERO,
        storage_root: Hash::ZERO,
        flags: 0,
    };

    let mut tree = PersistentJmt::new(&mut db, 1, Hash::ZERO);
    let root = tree.insert(addr, state.clone()).unwrap();

    let proof = tree.generate_proof(&addr).unwrap();
    assert!(
        verify_jmt_proof(root, &addr, &state, &proof),
        "Untampered proof must verify against root"
    );

    // 1. Tampered state balance fails
    let tampered_state = AccountState {
        balance: Amount::from_u128(99_999),
        ..state.clone()
    };
    assert!(
        !verify_jmt_proof(root, &addr, &tampered_state, &proof),
        "Tampered state must fail proof verification"
    );

    // 2. Tampered address fails
    let wrong_addr = Address([0x88; 20]);
    assert!(
        !verify_jmt_proof(root, &wrong_addr, &state, &proof),
        "Tampered address must fail proof verification"
    );

    // 3. Tampered sibling level fails
    let mut tampered_proof = proof.clone();
    if let Some(first_level) = tampered_proof.sibling_levels.first_mut() {
        first_level[0] = Some(Hash([0xFF; 32]));
        assert!(
            !verify_jmt_proof(root, &addr, &state, &tampered_proof),
            "Tampered proof sibling level must fail verification"
        );
    }
}
