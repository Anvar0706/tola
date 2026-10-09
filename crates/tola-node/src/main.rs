#![allow(clippy::all)]
//! TOLA Network node daemon executable entrypoint.

use std::collections::HashSet;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tola_consensus::PipelinedBftEngine;
use tola_crypto::Ed25519Keypair;
use tola_p2p::wire::{NetworkId, WireMessage};
use tola_p2p::TcpP2PTransport;
use tola_rpc::{compute_transaction_id, server::RpcServer};
use tola_storage::PersistentStateDB;
use tola_types::*;

use tola_node::config::{GenesisConfig, NodeConfig};
use tola_node::engine::TolaNode;
use tola_node::genesis::{compute_genesis_state_root, parse_genesis_json};
use tola_node::metrics::NodeMetrics;

fn print_help() {
    println!(
        "TOLA Network Node Daemon (tola-node)\n\n\
USAGE:\n\
    tola-node [OPTIONS]\n\n\
OPTIONS:\n\
    --chain-id <ID>              Network chain ID (default: 102)\n\
    --data-dir <DIR>             Node database and state directory (default: ./data)\n\
    --p2p-addr <ADDR>            P2P listen address (default: 127.0.0.1:30303)\n\
    --rpc-addr <ADDR>            JSON-RPC / HTTP listen address (default: 127.0.0.1:8545)\n\
    --validator                  Run in active BFT validator consensus mode\n\
    --validator-key <HEX>        32-byte Ed25519 consensus private key seed in hex\n\
    --operator-address <HEX>     20-byte validator operator address in hex\n\
    --genesis <FILE>             Path to genesis.json specification file\n\
    --bootstrap-peer <ADDR>      Remote peer address to connect to (can specify multiple)\n\
    --log-level <LEVEL>          Logging verbosity: error, warn, info, debug, trace (default: info)\n\
    -h, --help                   Print help information\n"
    );
}

struct ParsedArgs {
    chain_id: u64,
    data_dir: PathBuf,
    p2p_addr: SocketAddr,
    rpc_addr: SocketAddr,
    is_validator: bool,
    validator_key: Option<String>,
    operator_address: Option<String>,
    genesis_file: Option<PathBuf>,
    bootstrap_peers: Vec<SocketAddr>,
    log_level: String,
}

fn parse_cli_args() -> Result<Option<ParsedArgs>, String> {
    let args: Vec<String> = std::env::args().collect();
    let mut i = 1;

    let mut chain_id = 102u64;
    let mut data_dir = PathBuf::from("./data");
    let mut p2p_addr: SocketAddr = "127.0.0.1:30303".parse().unwrap();
    let mut rpc_addr: SocketAddr = "127.0.0.1:8545".parse().unwrap();
    let mut is_validator = false;
    let mut validator_key = None;
    let mut operator_address = None;
    let mut genesis_file = None;
    let mut bootstrap_peers = Vec::new();
    let mut log_level = "info".to_string();

    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                print_help();
                return Ok(None);
            }
            "--chain-id" => {
                i += 1;
                if i >= args.len() {
                    return Err("--chain-id requires an argument".into());
                }
                chain_id = args[i]
                    .parse()
                    .map_err(|e| format!("Invalid chain-id: {}", e))?;
            }
            "--data-dir" => {
                i += 1;
                if i >= args.len() {
                    return Err("--data-dir requires an argument".into());
                }
                data_dir = PathBuf::from(&args[i]);
            }
            "--p2p-addr" => {
                i += 1;
                if i >= args.len() {
                    return Err("--p2p-addr requires an argument".into());
                }
                p2p_addr = args[i]
                    .parse()
                    .map_err(|e| format!("Invalid p2p-addr: {}", e))?;
            }
            "--rpc-addr" => {
                i += 1;
                if i >= args.len() {
                    return Err("--rpc-addr requires an argument".into());
                }
                rpc_addr = args[i]
                    .parse()
                    .map_err(|e| format!("Invalid rpc-addr: {}", e))?;
            }
            "--validator" => {
                is_validator = true;
            }
            "--validator-key" => {
                i += 1;
                if i >= args.len() {
                    return Err("--validator-key requires an argument".into());
                }
                validator_key = Some(args[i].clone());
            }
            "--operator-address" => {
                i += 1;
                if i >= args.len() {
                    return Err("--operator-address requires an argument".into());
                }
                operator_address = Some(args[i].clone());
            }
            "--genesis" => {
                i += 1;
                if i >= args.len() {
                    return Err("--genesis requires an argument".into());
                }
                genesis_file = Some(PathBuf::from(&args[i]));
            }
            "--bootstrap-peer" => {
                i += 1;
                if i >= args.len() {
                    return Err("--bootstrap-peer requires an argument".into());
                }
                let peer: SocketAddr = args[i]
                    .parse()
                    .map_err(|e| format!("Invalid bootstrap-peer address: {}", e))?;
                bootstrap_peers.push(peer);
            }
            "--log-level" => {
                i += 1;
                if i >= args.len() {
                    return Err("--log-level requires an argument".into());
                }
                log_level = args[i].clone();
            }
            other => {
                return Err(format!("Unknown option: {}", other));
            }
        }
        i += 1;
    }

    Ok(Some(ParsedArgs {
        chain_id,
        data_dir,
        p2p_addr,
        rpc_addr,
        is_validator,
        validator_key,
        operator_address,
        genesis_file,
        bootstrap_peers,
        log_level,
    }))
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn main() {
    let args = match parse_cli_args() {
        Ok(Some(a)) => a,
        Ok(None) => return,
        Err(e) => {
            eprintln!("[ERROR] {}", e);
            std::process::exit(1);
        }
    };

    println!("=======================================================");
    println!("               TOLA NETWORK NODE DAEMON                ");
    println!("                 v0.1.0-alpha (Phase 13)               ");
    println!("=======================================================");
    // 1. Genesis Configuration
    let genesis_config = if let Some(ref path) = args.genesis_file {
        println!("[GENESIS] Loading genesis from {}", path.display());
        let content = std::fs::read_to_string(path).unwrap_or_else(|e| {
            eprintln!(
                "[ERROR] Failed to read genesis file {}: {}",
                path.display(),
                e
            );
            std::process::exit(1);
        });
        parse_genesis_json(&content).unwrap_or_else(|e| {
            eprintln!("[ERROR] Failed to parse genesis JSON: {}", e);
            std::process::exit(1);
        })
    } else {
        println!("[GENESIS] Using default testnet genesis configuration");
        let mut g = GenesisConfig::testnet_genesis();
        g.chain_id = ChainId(args.chain_id);
        g
    };

    let effective_chain_id = if args.genesis_file.is_some() {
        if args.chain_id != 102 && args.chain_id != genesis_config.chain_id.0 {
            eprintln!(
                "[ERROR] CLI --chain-id {} conflicts with genesis chain ID {}",
                args.chain_id, genesis_config.chain_id.0
            );
            std::process::exit(1);
        }
        genesis_config.chain_id
    } else {
        ChainId(args.chain_id)
    };

    println!("[INIT] Chain ID:           {}", effective_chain_id.0);
    println!("[INIT] Data Directory:     {}", args.data_dir.display());
    println!("[INIT] P2P Listen:         {}", args.p2p_addr);
    println!("[INIT] RPC Listen:         {}", args.rpc_addr);
    println!("[INIT] Validator Mode:     {}", args.is_validator);
    println!("[INIT] Log Level:          {}", args.log_level);

    let genesis_root = compute_genesis_state_root(&genesis_config);
    println!("[GENESIS] Computed Genesis State Root: {:?}", genesis_root);

    // 2. Storage Setup
    let state_db_path = args.data_dir.join("state");
    std::fs::create_dir_all(&args.data_dir).expect("Failed to create data directory");
    let state_db = PersistentStateDB::open(&state_db_path).unwrap_or_else(|e| {
        eprintln!(
            "[ERROR] Failed to open PersistentStateDB at {}: {:?}",
            state_db_path.display(),
            e
        );
        std::process::exit(1);
    });

    // 3. Validator Keypair & Operator Address
    let keypair = if let Some(ref key_hex) = args.validator_key {
        let clean = key_hex.strip_prefix("0x").unwrap_or(key_hex);
        if clean.len() != 64 {
            eprintln!("[ERROR] Validator key must be 32 bytes (64 hex characters)");
            std::process::exit(1);
        }
        let mut seed = [0u8; 32];
        for i in 0..32 {
            seed[i] = u8::from_str_radix(&clean[i * 2..i * 2 + 2], 16).unwrap_or(0);
        }
        Ed25519Keypair::from_seed(&seed)
    } else if args.is_validator {
        println!("[KEYPAIR] Generating ephemeral validator keypair");
        Ed25519Keypair::generate()
    } else {
        Ed25519Keypair::generate()
    };

    let operator_address = if let Some(ref addr_hex) = args.operator_address {
        Address::from_hex(addr_hex).unwrap_or_else(|e| {
            eprintln!("[ERROR] Invalid operator address hex: {:?}", e);
            std::process::exit(1);
        })
    } else {
        let pubkey_bytes = keypair.public_key_bytes();
        let mut addr = [0u8; 20];
        addr.copy_from_slice(&pubkey_bytes[..20]);
        Address(addr)
    };

    println!(
        "[IDENTITY] Operator Address:  {}",
        operator_address.to_hex()
    );
    println!(
        "[IDENTITY] Consensus Pubkey:  0x{}",
        hex::encode(keypair.public_key_bytes())
    );

    // 4. Committee Set
    let mut committee_validators = genesis_config.initial_validators.clone();
    if committee_validators.is_empty() && args.is_validator {
        committee_validators.push(ValidatorInfo {
            validator_id: ValidatorId(operator_address),
            consensus_pubkey: keypair.public_key(),
            voting_power: 100_000,
            commission_bp: 500,
        });
    }

    let total_voting_power: u64 = committee_validators.iter().map(|v| v.voting_power).sum();
    let committee_set = ValidatorSet {
        epoch: Epoch(0),
        validators: committee_validators,
        total_voting_power,
    };

    // 5. Node Configuration
    let node_config = NodeConfig {
        chain_id: effective_chain_id,
        data_dir: args.data_dir.clone(),
        is_validator: args.is_validator,
        validator_keypair: Some(keypair),
        operator_address: Some(operator_address),
        p2p_port: args.p2p_addr.port(),
        rpc_port: args.rpc_addr.port(),
        genesis: genesis_config.clone(),
    };

    let node = Arc::new(
        TolaNode::new(node_config, state_db, committee_set).unwrap_or_else(|e| {
            eprintln!("[ERROR] Failed to initialize TolaNode: {}", e);
            std::process::exit(1);
        }),
    );

    // 6. P2P Transport
    let network_id = match effective_chain_id {
        ChainId::MAINNET => NetworkId::Mainnet,
        ChainId::TESTNET => NetworkId::Testnet,
        ChainId::DEVNET => NetworkId::Devnet,
        _ => NetworkId::Testnet,
    };
    let p2p = Arc::new(TcpP2PTransport::new(network_id, args.p2p_addr));
    p2p.start().unwrap_or_else(|e| {
        eprintln!("[ERROR] Failed to start P2P transport: {}", e);
        std::process::exit(1);
    });
    println!("[P2P] Transport listening on {}", args.p2p_addr);

    for peer in &args.bootstrap_peers {
        println!("[P2P] Connecting to bootstrap peer {}", peer);
        let _ = p2p.connect_peer(*peer);
    }

    // 7. Node Metrics & RPC Server
    let metrics = NodeMetrics::new();
    let m_clone = metrics.clone();
    let metrics_callback = Arc::new(move || m_clone.render_prometheus());

    let mut rpc_server = RpcServer::new(args.rpc_addr, node.rpc.clone(), Some(metrics_callback));
    rpc_server.start().unwrap_or_else(|e| {
        eprintln!("[ERROR] Failed to start HTTP RPC server: {}", e);
        std::process::exit(1);
    });
    println!(
        "[RPC] HTTP JSON-RPC and /metrics listening on {}",
        args.rpc_addr
    );

    // Initial RPC synchronization
    node.rpc
        .current_height
        .store(node.current_height().0, Ordering::SeqCst);
    node.rpc
        .peer_count
        .store(p2p.peer_count(), Ordering::SeqCst);

    let seen_votes: Arc<Mutex<HashSet<(ValidatorId, u64, u32)>>> =
        Arc::new(Mutex::new(HashSet::new()));
    let seen_blocks: Arc<Mutex<HashSet<Hash>>> = Arc::new(Mutex::new(HashSet::new()));
    let seen_txs: Arc<Mutex<HashSet<TransactionId>>> = Arc::new(Mutex::new(HashSet::new()));

    // Wire RPC raw transaction submission to P2P broadcast
    let p_tx_broadcaster = p2p.clone();
    let s_txs_rpc = seen_txs.clone();
    *node.rpc.tx_broadcaster.write().unwrap() = Some(Arc::new(move |tx: &Transaction| {
        let tid = compute_transaction_id(tx);
        if s_txs_rpc.lock().unwrap().insert(tid) {
            p_tx_broadcaster.broadcast(&WireMessage::GossipTransaction(tx.clone()));
        }
    }));

    // 8. Runtime Coordinator
    let running = Arc::new(AtomicBool::new(true));
    let r_sig = running.clone();
    ctrlc_handler(r_sig);

    println!("[RUNTIME] Node coordinator running. Press Ctrl+C to terminate.");

    let p2p_in = p2p.clone();
    let node_in = node.clone();
    let metrics_in = metrics.clone();
    let running_in = running.clone();
    let seen_votes_in = seen_votes.clone();
    let seen_blocks_in = seen_blocks.clone();
    let seen_txs_in = seen_txs.clone();

    // P2P message ingestion worker thread
    let p2p_worker = thread::spawn(move || {
        while running_in.load(Ordering::SeqCst) {
            let messages = p2p_in.drain_inbound();
            for msg in messages {
                match msg.message {
                    WireMessage::GossipTransaction(tx) => {
                        let tid = compute_transaction_id(&tx);
                        if !seen_txs_in.lock().unwrap().insert(tid) {
                            continue;
                        }
                        let state = node_in.state_db.read().unwrap();
                        let current_h = state.current_height();
                        if let Ok(_) =
                            node_in
                                .mempool
                                .write()
                                .unwrap()
                                .insert(tx.clone(), &*state, current_h)
                        {
                            metrics_in.inc_txs(1);
                            p2p_in.broadcast(&WireMessage::GossipTransaction(tx.clone()));
                            node_in.rpc.notify_pending_transaction(&tx);
                        }
                    }
                    WireMessage::GossipBlock(block) => {
                        let b_hash = PipelinedBftEngine::compute_block_hash(&block);
                        if !seen_blocks_in.lock().unwrap().insert(b_hash) {
                            continue;
                        }
                        let cur_h = node_in.current_height();
                        if block.header.height.0 > cur_h.0 {
                            if let Some(ref qc) = block.last_commit_qc {
                                if let Ok(()) = node_in.process_qc(qc) {
                                    metrics_in.inc_blocks_committed();
                                }
                            }
                            match node_in.process_proposal(&block) {
                                Ok(Some(vote)) => {
                                    let v_key = (vote.validator, vote.height.0, vote.round.0);
                                    seen_votes_in.lock().unwrap().insert(v_key);
                                    println!(
                                        "[BLOCK] Ingested proposal for Block #{}! StateRoot: {:?} (Proposer: {})",
                                        block.header.height.0,
                                        block.header.state_root,
                                        block.header.proposer.to_hex()
                                    );

                                    // 1. Broadcast vote to peers
                                    p2p_in.broadcast(&WireMessage::GossipVote(vote.clone()));

                                    // 2. Local self-ingestion of vote for QC
                                    if let Ok(Some(qc)) = node_in.process_vote(vote) {
                                        metrics_in.inc_qc();
                                        println!(
                                            "[QC] Formed Quorum Certificate for Height #{}!",
                                            qc.height.0
                                        );
                                        if let Ok(()) = node_in.process_qc(&qc) {
                                            metrics_in.inc_blocks_committed();
                                        }
                                    }

                                    // 3. Forward block to peers
                                    p2p_in.broadcast(&WireMessage::GossipBlock(block));
                                }
                                Ok(None) => {}
                                Err(e) => {
                                    eprintln!(
                                        "[BLOCK WARNING] Block #{} proposal error: {:?}",
                                        block.header.height.0, e
                                    );
                                }
                            }
                        }
                    }
                    WireMessage::GossipVote(vote) => {
                        let v_key = (vote.validator, vote.height.0, vote.round.0);
                        if !seen_votes_in.lock().unwrap().insert(v_key) {
                            continue;
                        }
                        metrics_in.inc_votes();
                        match node_in.process_vote(vote.clone()) {
                            Ok(Some(qc)) => {
                                metrics_in.inc_qc();
                                println!(
                                    "[QC] Formed Quorum Certificate for Height #{}!",
                                    qc.height.0
                                );
                                let _ = node_in.process_qc(&qc);
                                p2p_in.broadcast(&WireMessage::GossipVote(vote));
                            }
                            Ok(None) => {
                                p2p_in.broadcast(&WireMessage::GossipVote(vote));
                            }
                            Err(_) => {}
                        }
                    }
                    WireMessage::GetBlocks {
                        start_height,
                        max_count,
                    } => {
                        let b_heights = node_in.rpc.blocks_by_height.read().unwrap();
                        let b_hashes = node_in.rpc.blocks_by_hash.read().unwrap();
                        let mut blocks = Vec::new();
                        for h in start_height.0..(start_height.0 + max_count as u64) {
                            if let Some(hash) = b_heights.get(&h) {
                                if let Some(b) = b_hashes.get(hash) {
                                    blocks.push(b.clone());
                                }
                            } else {
                                break;
                            }
                        }
                        if !blocks.is_empty() {
                            let _ = p2p_in.send_to(&msg.peer_addr, &WireMessage::Blocks(blocks));
                        }
                    }
                    WireMessage::Blocks(blocks) => {
                        for b in blocks {
                            let b_hash = PipelinedBftEngine::compute_block_hash(&b);
                            seen_blocks_in.lock().unwrap().insert(b_hash);
                            if b.header.height.0 > node_in.current_height().0 {
                                if let Some(ref qc) = b.last_commit_qc {
                                    let _ = node_in.process_qc(qc);
                                }
                                match node_in.process_proposal(&b) {
                                    Ok(Some(vote)) => {
                                        let v_key = (vote.validator, vote.height.0, vote.round.0);
                                        seen_votes_in.lock().unwrap().insert(v_key);
                                        println!(
                                            "[SYNC] Ingested and committed historic Block #{}! StateRoot: {:?}",
                                            b.header.height.0,
                                            b.header.state_root
                                        );
                                        metrics_in.inc_blocks_committed();
                                        if let Ok(Some(qc)) = node_in.process_vote(vote) {
                                            metrics_in.inc_qc();
                                            let _ = node_in.process_qc(&qc);
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            thread::sleep(Duration::from_millis(10));
        }
    });

    // Main status ticker loop
    let mut last_status = now_millis();
    let mut last_sync = now_millis();
    let mut last_propose_attempt = now_millis();

    while running.load(Ordering::SeqCst) {
        let current = now_millis();

        let height = node.current_height().0;
        let finalized = node.last_finalized_height().0;
        let peers = p2p.peer_count();
        let mp_len = node.mempool.read().unwrap().stats().total_transactions;

        // Synchronize actual node state into RPC dispatcher every loop
        node.rpc.current_height.store(height, Ordering::SeqCst);
        node.rpc.peer_count.store(peers, Ordering::SeqCst);

        // Catch-up sync request if peers exist (every 2.5 seconds)
        if peers > 0 && current.saturating_sub(last_sync) >= 2500 {
            last_sync = current;
            let sync_req = WireMessage::GetBlocks {
                start_height: BlockHeight(height + 1),
                max_count: 50,
            };
            p2p.broadcast(&sync_req);
        }

        // Pacemaker View-Change / Timeout Check:
        if node.config.is_validator {
            let timed_out = node
                .consensus
                .read()
                .unwrap()
                .pacemaker()
                .is_timed_out(current);
            if timed_out {
                let new_round = node.consensus.write().unwrap().on_timeout(current);
                println!(
                    "[PACEMAKER] View timed out. Advanced to Round #{} for Height #{}",
                    new_round.0,
                    height + 1
                );
            }
        }

        // Leader Block Proposal:
        if node.config.is_validator {
            let next_height = BlockHeight(height + 1);
            let current_round = node.consensus.read().unwrap().pacemaker().current_round();
            let leader_id = node
                .consensus
                .read()
                .unwrap()
                .committee()
                .leader_for_view(next_height, current_round);
            let is_leader = node
                .config
                .operator_address
                .map(|addr| ValidatorId(addr) == leader_id)
                .unwrap_or(false);

            // Propose if this node is elected leader and:
            // (a) there are pending transactions to commit, OR
            // (b) regular block cadence if consensus quorum (peers >= 2) is established
            if is_leader
                && (mp_len > 0
                    || (peers >= 2 && current.saturating_sub(last_propose_attempt) >= 1000))
            {
                last_propose_attempt = current;
                match node.propose_next_block(current_round, current) {
                    Ok(block) => {
                        println!(
                            "[BLOCK PROPOSED] Elected leader proposed Block #{} with {} transaction(s)! (TxRoot: {:?})",
                            block.header.height.0,
                            block.transactions.len(),
                            block.header.txs_root
                        );
                        metrics.inc_blocks_proposed();
                        if node.is_standalone() {
                            metrics.inc_blocks_committed();
                        }

                        let b_hash = PipelinedBftEngine::compute_block_hash(&block);
                        seen_blocks.lock().unwrap().insert(b_hash);

                        // 1. Broadcast proposal to all peers
                        p2p.broadcast(&WireMessage::GossipBlock(block.clone()));

                        // 2. Cast leader's own vote
                        if let Ok(Some(vote)) = node.process_proposal(&block) {
                            let v_key = (vote.validator, vote.height.0, vote.round.0);
                            seen_votes.lock().unwrap().insert(v_key);
                            p2p.broadcast(&WireMessage::GossipVote(vote.clone()));
                            if let Ok(Some(qc)) = node.process_vote(vote) {
                                metrics.inc_qc();
                                println!(
                                    "[QC] Leader formed Quorum Certificate for Height #{}!",
                                    qc.height.0
                                );
                                if let Ok(()) = node.process_qc(&qc) {
                                    if !node.is_standalone() {
                                        metrics.inc_blocks_committed();
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("[BLOCK PROPOSE ERROR] {:?}", e);
                    }
                }
            }
        }

        if current.saturating_sub(last_status) >= 5000 {
            last_status = current;

            metrics.set_height(height);
            metrics.set_finalized_height(finalized);
            metrics.set_peers(peers);
            metrics.set_mempool_size(mp_len);

            println!(
                "[STATUS] Height: {} | Finalized: {} | Peers: {} | Mempool: {}",
                height, finalized, peers, mp_len
            );
        }

        thread::sleep(Duration::from_millis(50));
    }

    println!("[SHUTDOWN] Terminating node services...");
    p2p.shutdown();
    rpc_server.shutdown();
    let _ = p2p_worker.join();
    println!("[SHUTDOWN] Node daemon cleanly terminated.");
}

mod hex {
    pub fn encode<T: AsRef<[u8]>>(data: T) -> String {
        let mut s = String::new();
        for b in data.as_ref() {
            s.push_str(&format!("{b:02x}"));
        }
        s
    }
}

fn ctrlc_handler(_running: Arc<AtomicBool>) {
    // Platform-neutral signal handling stub
}
