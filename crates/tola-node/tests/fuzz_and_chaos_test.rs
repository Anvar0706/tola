//! Fuzz testing, adversarial payload mutation, and chaos failure injection test suite.
//!
//! Validates:
//! 1. Wire frame fuzzing: bit-flips, truncated headers, corrupted checksums, and random payloads (0 panics).
//! 2. BIP-350 Bech32m address fuzzing: mixed-case, invalid checksums, mutated payload characters (0 panics).
//! 3. Canonical transaction codec boundary fuzzing.
//! 4. Chaos testing: validator dropout during multi-round consensus and subsequent catch-up state convergence.

use std::time::{SystemTime, UNIX_EPOCH};

use tola_codec::{ByteReader, CanonicalDeserialize, CanonicalSerialize};
use tola_core::NetworkError;
use tola_p2p::wire::{decode_frame, encode_frame, NetworkId, WireMessage};
use tola_types::*;

use tola_node::config::GenesisAccount;
use tola_node::network::LocalMultiNodeNetwork;

#[test]
fn test_fuzz_wire_frame_corruption_and_boundary_checks() {
    let net = NetworkId::Testnet;
    let valid_msg = WireMessage::Ping(0xDEAD_BEEF_CAFE_BABE);
    let valid_frame = encode_frame(net, &valid_msg);

    // Baseline: valid frame decodes cleanly
    let decoded = decode_frame(net, &valid_frame).expect("Valid frame must decode");
    assert_eq!(decoded, valid_msg);

    // 1. Bit-flip fuzzing across all header bytes (44 bytes)
    for i in 0..44 {
        let mut corrupted = valid_frame.clone();
        corrupted[i] ^= 0x01; // flip 1 bit
        let res = decode_frame(net, &corrupted);
        assert!(
            res.is_err(),
            "Corrupted header byte at index {} must fail decode",
            i
        );
    }

    // 2. Bit-flip fuzzing across payload bytes
    for i in 44..valid_frame.len() {
        let mut corrupted = valid_frame.clone();
        corrupted[i] ^= 0x01; // flip 1 bit
        let res = decode_frame(net, &corrupted);
        // Payload corruption must trigger ChecksumMismatch
        assert_eq!(
            res,
            Err(NetworkError::ChecksumMismatch),
            "Payload bit-flip at index {} must be rejected as ChecksumMismatch",
            i
        );
    }

    // 3. Truncation fuzzing: all lengths from 0 to 43 (sub-header sizes)
    for len in 0..44 {
        let truncated = &valid_frame[..len];
        let res = decode_frame(net, truncated);
        assert!(
            res.is_err(),
            "Truncated frame of length {} must fail decode",
            len
        );
    }

    // 4. Randomized fuzzing: 500 pseudo-random byte buffers
    let mut lcg: u64 = 0x1234_5678_9ABC_DEF0;
    for iter in 0..500 {
        // Linear congruential generator for deterministic pseudo-random fuzzing
        lcg = lcg.wrapping_mul(6364136223846793005).wrapping_add(1);
        let len = (lcg % 512) as usize + 1;
        let mut rand_bytes = vec![0u8; len];
        for b in rand_bytes.iter_mut() {
            lcg = lcg.wrapping_mul(6364136223846793005).wrapping_add(1);
            *b = (lcg >> 24) as u8;
        }

        // Must reject without panic
        let res = decode_frame(net, &rand_bytes);
        assert!(
            res.is_err() || rand_bytes.starts_with(b"TOLA"),
            "Random bytes iteration {} caused unexpected behavior",
            iter
        );
    }
}

#[test]
fn test_fuzz_bech32m_address_mutation_and_rejection() {
    let valid_addr_str = "ttola1wp9narejdk2jxra3leew42wt6sdvnvq3e7pxlu";

    // Baseline: valid address decodes cleanly
    let (net, addr) = Address::from_bech32m(valid_addr_str).expect("Valid Bech32m must decode");
    assert_eq!(net, Network::Testnet);
    assert_ne!(addr, Address::ZERO);

    // 1. Mixed-case rejection (BIP-350 strict rule)
    assert!(
        Address::from_bech32m("Ttola1wp9narejdk2jxra3leew42wt6sdvnvq3e7pxlu").is_err(),
        "Leading uppercase must be rejected"
    );
    assert!(
        Address::from_bech32m("ttola1WP9narejdk2jxra3leew42wt6sdvnvq3e7pxlu").is_err(),
        "Mixed-case payload must be rejected"
    );

    // 2. Corrupted checksum rejection: mutate characters in the 6-char checksum
    for i in 0..6 {
        let idx = valid_addr_str.len() - 1 - i;
        let mut chars: Vec<char> = valid_addr_str.chars().collect();
        // Flip character to another valid bech32 char
        chars[idx] = if chars[idx] == 'u' { 'q' } else { 'u' };
        let corrupted: String = chars.into_iter().collect();
        assert!(
            Address::from_bech32m(&corrupted).is_err(),
            "Corrupted checksum at offset {} must fail decode",
            i
        );
    }

    // 3. Invalid characters outside Bech32 charset ('b', 'i', 'o', '1' in data)
    let invalid_chars = ['b', 'i', 'o', '1'];
    for &ch in &invalid_chars {
        let mut chars: Vec<char> = valid_addr_str.chars().collect();
        chars[10] = ch; // inject invalid character
        let corrupted: String = chars.into_iter().collect();
        assert!(
            Address::from_bech32m(&corrupted).is_err(),
            "Invalid character '{}' must fail decode",
            ch
        );
    }

    // 4. Random mutations across 300 iterations
    let mut lcg: u64 = 0xABCD_EF01_2345_6789;
    for iter in 0..300 {
        lcg = lcg.wrapping_mul(6364136223846793005).wrapping_add(1);
        let mutation_idx = (lcg as usize) % valid_addr_str.len();
        let mut chars: Vec<char> = valid_addr_str.chars().collect();
        chars[mutation_idx] = ((lcg >> 16) as u8 % 26 + b'a') as char;
        let mutated: String = chars.into_iter().collect();

        if mutated != valid_addr_str {
            let res = Address::from_bech32m(&mutated);
            // Must not panic, and overwhelmingly must reject
            if let Ok((_, decoded_addr)) = res {
                // If it coincidentally formed a valid checksum, it must decode to a different address
                assert_ne!(decoded_addr, addr, "Iteration {} hash collision", iter);
            }
        }
    }
}

#[test]
fn test_fuzz_transaction_canonical_codec_invariance() {
    let tx = Transaction {
        chain_id: ChainId(102),
        nonce: Nonce(42),
        from: Address([0x12; 20]),
        to: Some(Address([0x34; 20])),
        value: Amount(1_000_000_000_000),
        gas_limit: Gas(21_000),
        max_fee_per_gas: Amount(1_000_000_000),
        max_priority_fee_per_gas: Amount(100_000_000),
        valid_until_block: BlockHeight(500),
        data: vec![0xDE, 0xAD, 0xBE, 0xEF],
        memo: None,
        signature: Signature::Secp256k1 {
            r: [0x01; 32],
            s: [0x02; 32],
            v: 0,
        },
        tx_type: 0,
        access_list: vec![],
    };

    let mut buf = Vec::new();
    tx.serialize_canonical(&mut buf);

    // 1. Bit-flip mutation fuzzing across entire transaction serialization
    for i in 0..buf.len() {
        let mut corrupted = buf.clone();
        corrupted[i] ^= 0xFF;
        let mut reader = ByteReader::new(&corrupted);
        let res = Transaction::read_canonical(&mut reader);
        // Deserializing either fails cleanly or produces a distinct transaction
        if let Ok(deserialized) = res {
            assert_ne!(
                deserialized, tx,
                "Bit-flipped transaction at byte {} must not match original",
                i
            );
        }
    }

    // 2. Truncation boundary checks
    for len in 0..buf.len() - 1 {
        let mut reader = ByteReader::new(&buf[..len]);
        assert!(
            Transaction::read_canonical(&mut reader).is_err(),
            "Truncated transaction of length {} must fail canonical read",
            len
        );
    }
}

#[test]
fn test_chaos_validator_failure_and_catchup_reconciliation() {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let temp_dir = std::env::temp_dir().join(format!("tola-chaos-test-{}", nanos));
    std::fs::create_dir_all(&temp_dir).unwrap();

    let genesis_accounts = vec![GenesisAccount {
        address: Address([0xAA; 20]),
        balance: Amount::from_u128(1_000_000_000),
    }];

    // 1. Initialize 4-validator cluster (Byzantine resilience: 3f+1 with f=1)
    let mut network = LocalMultiNodeNetwork::new(4, &temp_dir, ChainId(102), genesis_accounts)
        .expect("Initialize 4-validator cluster");

    // 2. Normal round: All 4 validators participate, produce and commit Block #1
    let (block1, _qc1) = network.step_round(1000).expect("Round 1 consensus");
    assert_eq!(block1.header.height, BlockHeight(1));
    let root1 = network.assert_state_consensus();
    assert_ne!(root1, Hash::ZERO);

    // 3. Chaos injection: Node 3 is offline; only nodes 0, 1, 2 participate (3/4 = 75% >= 2f+1 threshold)
    let (block2, maybe_qc2) = network
        .step_round_with_participating(&[0, 1, 2], 2000)
        .expect("Round 2 consensus with 1 offline validator");
    let qc2 = maybe_qc2.expect("Quorum certificate must form with 3/4 validators");
    assert_eq!(block2.header.height, BlockHeight(2));

    // 4. Recovery: Validator 3 comes back online and synchronizes Block #2
    let recovered_node = &network.nodes[3];
    assert_eq!(
        recovered_node.current_height(),
        BlockHeight(1),
        "Offline node remained at height 1"
    );

    // Deliver Block #2 and QC to recovered node
    let vote_opt = recovered_node
        .process_proposal(&block2)
        .expect("Process proposal on recovered node");
    assert!(
        vote_opt.is_some(),
        "Recovered node must vote for valid block"
    );
    let _ = recovered_node.process_qc(&qc2);
    assert_eq!(
        recovered_node.current_height(),
        BlockHeight(2),
        "Recovered node caught up to height 2"
    );

    // 5. Verify total state root consensus across all 4 nodes!
    let final_root = network.assert_state_consensus();
    assert_eq!(recovered_node.state_root(), final_root);
    assert_eq!(recovered_node.current_height(), BlockHeight(2));

    let _ = std::fs::remove_dir_all(&temp_dir);
}
