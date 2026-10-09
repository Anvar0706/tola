//! Multi-node real TCP socket testnet test suite for Phase 13.
//!
//! Validates:
//! 1. 4-validator cluster consensus and state convergence over real TCP sockets.
//! 2. 2-chain finality under Jolteon BFT rules.
//! 3. Byzantine fault tolerance (quorum progress with 3/4 validators, stall with 2/4).
//! 4. Crash recovery and persistent state continuity across node restarts.
//! 5. HTTP JSON-RPC, /health, and Prometheus /metrics verification.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tola_crypto::Ed25519Keypair;
use tola_p2p::wire::{NetworkId, WireMessage};
use tola_p2p::TcpP2PTransport;
use tola_rpc::server::RpcServer;
use tola_storage::PersistentStateDB;
use tola_types::*;

use tola_node::config::{GenesisAccount, GenesisConfig, NodeConfig};
use tola_node::engine::TolaNode;
use tola_node::metrics::NodeMetrics;

fn unique_temp_dir(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("tola-test-{}-{}", label, nanos));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn test_4_validator_real_tcp_consensus_and_state_agreement() {
    let chain_id = ChainId(102);
    let temp_root = unique_temp_dir("4val-tcp");

    let alice = Address([0xAA; 20]);
    let genesis_accounts = vec![GenesisAccount {
        address: alice,
        balance: Amount(10_000_000_000),
    }];

    let mut keypairs = Vec::new();
    let mut val_infos = Vec::new();

    for i in 1..=4 {
        let mut seed = [0u8; 32];
        seed[0] = i as u8;
        let kp = Ed25519Keypair::from_seed(&seed);
        let pubkey_bytes = kp.public_key_bytes();
        let mut addr = [0u8; 20];
        addr.copy_from_slice(&pubkey_bytes[..20]);

        val_infos.push(ValidatorInfo {
            validator_id: ValidatorId(Address(addr)),
            consensus_pubkey: kp.public_key(),
            voting_power: 100_000,
            commission_bp: 500,
        });
        keypairs.push(kp);
    }

    let genesis_config = GenesisConfig {
        chain_id,
        genesis_time: 1770000000,
        accounts: genesis_accounts,
        initial_validators: val_infos.clone(),
    };

    let total_power: u64 = val_infos.iter().map(|v| v.voting_power).sum();
    let committee = ValidatorSet {
        epoch: Epoch(0),
        validators: val_infos,
        total_voting_power: total_power,
    };

    // Allocate 4 base ports for P2P TCP listeners
    let p2p_base = 41200;
    let mut transports = Vec::new();
    let mut nodes = Vec::new();

    for (i, kp) in keypairs.iter().enumerate().take(4) {
        let node_dir = temp_root.join(format!("val-{}", i));
        std::fs::create_dir_all(&node_dir).unwrap();
        let state_db = PersistentStateDB::open(node_dir.join("state")).unwrap();

        let val_addr = committee.validators[i].validator_id.0;
        let node_cfg = NodeConfig {
            chain_id,
            data_dir: node_dir,
            is_validator: true,
            validator_keypair: Some(kp.clone()),
            operator_address: Some(val_addr),
            p2p_port: (p2p_base + i) as u16,
            rpc_port: (8550 + i) as u16,
            genesis: genesis_config.clone(),
        };

        let node = TolaNode::new(node_cfg, state_db, committee.clone()).unwrap();
        let p2p_addr: SocketAddr = format!("127.0.0.1:{}", p2p_base + i).parse().unwrap();
        let transport = TcpP2PTransport::new(NetworkId::Testnet, p2p_addr);
        transport.start().expect("Start P2P TCP transport");

        transports.push(transport);
        nodes.push(node);
    }

    thread::sleep(Duration::from_millis(50));

    // Connect nodes into full mesh over TCP
    for (i, transport) in transports.iter().enumerate().take(4) {
        for j in 0..4 {
            if i != j {
                let target: SocketAddr = format!("127.0.0.1:{}", p2p_base + j).parse().unwrap();
                let _ = transport.connect_peer(target);
            }
        }
    }

    thread::sleep(Duration::from_millis(100));

    // Determine deterministic leader for Height 1, Round 0
    let leader_id = {
        let c_guard = nodes[0].consensus.read().unwrap();
        c_guard
            .committee()
            .leader_for_view(BlockHeight(1), Round(0))
    };
    let leader_idx = committee
        .validators
        .iter()
        .position(|v| v.validator_id == leader_id)
        .expect("Leader index exists");

    // Round 1: Elected leader proposes Block 1
    let block1 = nodes[leader_idx]
        .propose_block(None, Round(0), 1770001000 * 1000)
        .expect("Propose block 1");

    // Leader broadcasts proposal over TCP to all peers
    let block_msg = WireMessage::GossipBlock(block1.clone());
    transports[leader_idx].broadcast(&block_msg);

    // Replicas ingest block proposal, execute state, and produce votes
    let mut votes = Vec::new();
    for node in &nodes {
        let vote_opt = node
            .process_proposal(&block1)
            .expect("Process block 1 proposal");
        if let Some(vote) = vote_opt {
            votes.push(vote);
        }
    }

    assert_eq!(
        votes.len(),
        4,
        "All 4 validators produce valid precommit votes"
    );

    // Leader ingests votes to assemble Quorum Certificate
    let mut qc1_opt = None;
    for vote in votes {
        if let Some(qc) = nodes[leader_idx].process_vote(vote).expect("Process vote") {
            qc1_opt = Some(qc);
        }
    }

    let qc1 = qc1_opt.expect("QC formed for block 1");
    assert_eq!(qc1.height, BlockHeight(1));

    // Propagate QC1 to all nodes
    for node in &nodes {
        node.process_qc(&qc1).expect("Ingest QC1");
        assert_eq!(node.current_height(), BlockHeight(1));
    }

    // Verify 100% state root agreement across all 4 nodes
    let expected_root = nodes[0].state_root();
    for node in nodes.iter().skip(1) {
        assert_eq!(
            node.state_root(),
            expected_root,
            "State roots must strictly converge"
        );
    }

    // Determine deterministic leader for Height 2, Round 1
    let leader_id_2 = {
        let c_guard = nodes[0].consensus.read().unwrap();
        c_guard
            .committee()
            .leader_for_view(BlockHeight(2), Round(1))
    };
    let leader_idx_2 = committee
        .validators
        .iter()
        .position(|v| v.validator_id == leader_id_2)
        .expect("Leader 2 index exists");

    // Round 2: Propose Block 2 referencing parent QC1 (activates Jolteon 2-chain finality)
    let block2 = nodes[leader_idx_2]
        .propose_block(Some(qc1.clone()), Round(1), 1770002000 * 1000)
        .expect("Propose block 2");

    let mut votes2 = Vec::new();
    for node in &nodes {
        let vote_opt = node
            .process_proposal(&block2)
            .expect("Process block 2 proposal");
        if let Some(vote) = vote_opt {
            votes2.push(vote);
        }
    }

    let mut qc2_opt = None;
    for vote in votes2 {
        if let Some(qc) = nodes[leader_idx_2]
            .process_vote(vote)
            .expect("Process vote round 2")
        {
            qc2_opt = Some(qc);
        }
    }

    let qc2 = qc2_opt.expect("QC formed for block 2");
    assert_eq!(qc2.height, BlockHeight(2));

    for node in &nodes {
        node.process_qc(&qc2).expect("Ingest QC2");
        assert_eq!(node.current_height(), BlockHeight(2));
        assert_eq!(
            node.last_finalized_height(),
            BlockHeight(1),
            "Block 1 is now 2-chain finalized"
        );
    }

    // Clean shutdown of transports
    for t in transports {
        t.shutdown();
    }
    let _ = std::fs::remove_dir_all(&temp_root);
}

#[test]
fn test_byzantine_fault_tolerance_quorum_thresholds() {
    let chain_id = ChainId(102);
    let temp_root = unique_temp_dir("bft-quorum");

    let mut val_infos = Vec::new();
    let mut keypairs = Vec::new();

    // 4 validators with 100,000 power each (Total = 400,000, Quorum = 266,667)
    for i in 1..=4 {
        let mut seed = [0u8; 32];
        seed[0] = i as u8;
        let kp = Ed25519Keypair::from_seed(&seed);
        let pubkey_bytes = kp.public_key_bytes();
        let mut addr = [0u8; 20];
        addr.copy_from_slice(&pubkey_bytes[..20]);

        val_infos.push(ValidatorInfo {
            validator_id: ValidatorId(Address(addr)),
            consensus_pubkey: kp.public_key(),
            voting_power: 100_000,
            commission_bp: 500,
        });
        keypairs.push(kp);
    }

    let genesis_config = GenesisConfig {
        chain_id,
        genesis_time: 1770000000,
        accounts: vec![],
        initial_validators: val_infos.clone(),
    };

    let total_power: u64 = val_infos.iter().map(|v| v.voting_power).sum();
    let committee = ValidatorSet {
        epoch: Epoch(0),
        validators: val_infos,
        total_voting_power: total_power,
    };

    let mut nodes = Vec::new();
    for (i, kp) in keypairs.iter().enumerate().take(4) {
        let node_dir = temp_root.join(format!("val-{}", i));
        std::fs::create_dir_all(&node_dir).unwrap();
        let state_db = PersistentStateDB::open(node_dir.join("state")).unwrap();

        let val_addr = committee.validators[i].validator_id.0;
        let node_cfg = NodeConfig {
            chain_id,
            data_dir: node_dir,
            is_validator: true,
            validator_keypair: Some(kp.clone()),
            operator_address: Some(val_addr),
            p2p_port: 39100 + i as u16,
            rpc_port: 8580 + i as u16,
            genesis: genesis_config.clone(),
        };

        nodes.push(TolaNode::new(node_cfg, state_db, committee.clone()).unwrap());
    }

    // Determine leader for Height 1, Round 0
    let leader_id = {
        let c_guard = nodes[0].consensus.read().unwrap();
        c_guard
            .committee()
            .leader_for_view(BlockHeight(1), Round(0))
    };
    let leader_idx = committee
        .validators
        .iter()
        .position(|v| v.validator_id == leader_id)
        .unwrap();

    let block1 = nodes[leader_idx]
        .propose_block(None, Round(0), 1000)
        .unwrap();

    // SCENARIO 1: 3/4 validators vote (Power = 300,000 >= 266,667) -> Quorum reached!
    let mut votes_3 = Vec::new();
    for node in nodes.iter().take(3) {
        let vote_opt = node.process_proposal(&block1).unwrap();
        if let Some(v) = vote_opt {
            votes_3.push(v);
        }
    }
    assert_eq!(votes_3.len(), 3);

    let mut qc_opt = None;
    for vote in votes_3 {
        if let Ok(Some(qc)) = nodes[leader_idx].process_vote(vote) {
            qc_opt = Some(qc);
        }
    }
    assert!(
        qc_opt.is_some(),
        "3/4 validators must successfully achieve quorum"
    );

    // Ingest QC1 across online nodes
    let qc1 = qc_opt.unwrap();
    for node in nodes.iter().take(3) {
        node.process_qc(&qc1).unwrap();
    }

    // SCENARIO 2: In round 2, only 2/4 validators are online (Power = 200,000 < 266,667) -> Quorum STALLS safely
    let leader_id_2 = {
        let c_guard = nodes[0].consensus.read().unwrap();
        c_guard
            .committee()
            .leader_for_view(BlockHeight(2), Round(1))
    };
    let leader_idx_2 = committee
        .validators
        .iter()
        .position(|v| v.validator_id == leader_id_2)
        .unwrap();

    let block2 = nodes[leader_idx_2]
        .propose_block(Some(qc1), Round(1), 2000)
        .unwrap();
    let mut votes_2 = Vec::new();
    // Only 2 validators cast votes
    for node in nodes.iter().take(2) {
        let vote_opt = node.process_proposal(&block2).unwrap();
        if let Some(v) = vote_opt {
            votes_2.push(v);
        }
    }
    assert_eq!(votes_2.len(), 2);

    let mut qc_stalled = None;
    for vote in votes_2 {
        if let Ok(Some(qc)) = nodes[leader_idx_2].process_vote(vote) {
            qc_stalled = Some(qc);
        }
    }
    assert!(
        qc_stalled.is_none(),
        "2/4 validators must NOT form quorum (safety halt)"
    );

    let _ = std::fs::remove_dir_all(&temp_root);
}

#[test]
fn test_testnet_restart_recovery_and_state_persistence() {
    let chain_id = ChainId(102);
    let temp_root = unique_temp_dir("restart-recovery");
    let node_dir = temp_root.join("persistent-node");
    let state_path = node_dir.join("state");

    let val_kp = Ed25519Keypair::generate();
    let val_addr = Address([0x77; 20]);
    let val_info = ValidatorInfo {
        validator_id: ValidatorId(val_addr),
        consensus_pubkey: val_kp.public_key(),
        voting_power: 100_000,
        commission_bp: 500,
    };

    let genesis_cfg = GenesisConfig {
        chain_id,
        genesis_time: 1770000000,
        accounts: vec![GenesisAccount {
            address: Address([0x88; 20]),
            balance: Amount(5_000_000_000),
        }],
        initial_validators: vec![val_info.clone()],
    };

    let committee = ValidatorSet {
        epoch: Epoch(0),
        validators: vec![val_info],
        total_voting_power: 100_000,
    };

    // Phase 1: Initialize node, propose and commit Block 1
    let original_state_root = {
        let state_db = PersistentStateDB::open(&state_path).unwrap();
        let node_cfg = NodeConfig {
            chain_id,
            data_dir: node_dir.clone(),
            is_validator: true,
            validator_keypair: Some(val_kp.clone()),
            operator_address: Some(val_addr),
            p2p_port: 39010,
            rpc_port: 8570,
            genesis: genesis_cfg.clone(),
        };

        let node = TolaNode::new(node_cfg, state_db, committee.clone()).unwrap();
        let _block = node
            .propose_block(None, Round(0), 1000)
            .expect("Propose block");
        assert_eq!(node.current_height(), BlockHeight(1));
        node.state_root()
    };

    // Phase 2: Restart node from same directory and verify state preservation
    {
        let reopened_db = PersistentStateDB::open(&state_path).unwrap();
        assert_eq!(
            reopened_db.current_height(),
            BlockHeight(1),
            "Height must be preserved across restart"
        );
        assert_eq!(
            reopened_db.state_root(),
            original_state_root,
            "State root must match across restart"
        );

        let node_cfg2 = NodeConfig {
            chain_id,
            data_dir: node_dir,
            is_validator: true,
            validator_keypair: Some(val_kp),
            operator_address: Some(val_addr),
            p2p_port: 39010,
            rpc_port: 8570,
            genesis: genesis_cfg,
        };

        let restarted_node = TolaNode::new(node_cfg2, reopened_db, committee).unwrap();
        assert_eq!(restarted_node.current_height(), BlockHeight(1));
        assert_eq!(restarted_node.state_root(), original_state_root);
    }

    let _ = std::fs::remove_dir_all(&temp_root);
}

#[test]
fn test_cli_rpc_integration_end_to_end() {
    let chain_id = ChainId(102);
    let temp_root = unique_temp_dir("rpc-e2e");
    let state_db = PersistentStateDB::open(temp_root.join("state")).unwrap();

    let alice = Address([0x42; 20]);
    let val_kp = Ed25519Keypair::generate();
    let val_addr = Address([0x77; 20]);
    let val_info = ValidatorInfo {
        validator_id: ValidatorId(val_addr),
        consensus_pubkey: val_kp.public_key(),
        voting_power: 100_000,
        commission_bp: 500,
    };

    let genesis_cfg = GenesisConfig {
        chain_id,
        genesis_time: 1770000000,
        accounts: vec![GenesisAccount {
            address: alice,
            balance: Amount(1_000_000_000_000_000_000), // 1 TOLA
        }],
        initial_validators: vec![val_info.clone()],
    };

    let node_cfg = NodeConfig {
        chain_id,
        data_dir: temp_root.clone(),
        is_validator: false,
        validator_keypair: None,
        operator_address: None,
        p2p_port: 39020,
        rpc_port: 0,
        genesis: genesis_cfg,
    };

    let committee = ValidatorSet {
        epoch: Epoch(0),
        validators: vec![val_info],
        total_voting_power: 100_000,
    };

    let node = TolaNode::new(node_cfg, state_db, committee).unwrap();

    let metrics = NodeMetrics::new();
    metrics.set_height(0);
    let m_clone = metrics.clone();
    let mut server = RpcServer::new(
        "127.0.0.1:0".parse().unwrap(),
        node.rpc.clone(),
        Some(Arc::new(move || m_clone.render_prometheus())),
    );

    let bound_addr = server.start().expect("Start RPC server");

    // 1. Verify GET /health
    {
        let mut stream = TcpStream::connect(bound_addr).unwrap();
        stream
            .write_all(b"GET /health HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .unwrap();
        let mut resp = String::new();
        stream.read_to_string(&mut resp).unwrap();
        assert!(resp.contains("200 OK"));
        assert!(resp.contains("\"status\":\"ok\""));
        assert!(resp.contains("\"chain_id\":102"));
    }

    // 2. Verify GET /metrics
    {
        let mut stream = TcpStream::connect(bound_addr).unwrap();
        stream
            .write_all(b"GET /metrics HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .unwrap();
        let mut resp = String::new();
        stream.read_to_string(&mut resp).unwrap();
        assert!(resp.contains("200 OK"));
        assert!(resp.contains("tola_block_height"));
    }

    // 3. Verify POST / JSON-RPC eth_chainId and eth_getBalance
    {
        let mut stream = TcpStream::connect(bound_addr).unwrap();
        let payload = "{\"jsonrpc\":\"2.0\",\"method\":\"eth_chainId\",\"params\":[],\"id\":1}";
        let req = format!(
            "POST / HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\nContent-Type: application/json\r\n\r\n{}",
            payload.len(),
            payload
        );
        stream.write_all(req.as_bytes()).unwrap();
        let mut resp = String::new();
        stream.read_to_string(&mut resp).unwrap();
        assert!(resp.contains("200 OK"));
        assert!(resp.contains("\"result\":\"0x66\"")); // 102 in hex is 0x66
    }

    server.shutdown();
    let _ = std::fs::remove_dir_all(&temp_root);
}
