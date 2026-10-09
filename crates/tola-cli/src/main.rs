//! TOLA Network operational command-line interface (`tola-cli`).

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use tola_codec::CanonicalSerialize;
use tola_crypto::{Ed25519Keypair, Secp256k1Keypair};
use tola_node::config::{GenesisAccount, GenesisConfig};
use tola_node::genesis::{compute_genesis_state_root, parse_genesis_json, serialize_genesis_json};
use tola_rpc::json::JsonValue;
use tola_types::*;

pub mod ceremony;
pub mod faucet;
pub mod security_council;
pub mod wallet;

fn print_help() {
    println!(
        "TOLA Network Command-Line Interface (tola-cli)\n\n\
USAGE:\n\
    tola-cli <COMMAND> [OPTIONS]\n\n\
COMMANDS:\n\
    status                         Query status and health of the running local node\n\
    node [health|status]           Inspect node health, block height, and active peers\n\
    genesis generate [OPTIONS]     Generate a deterministic testnet genesis file\n\
    genesis show [OPTIONS]         Inspect an existing genesis configuration and state root\n\
    ceremony [keygen|sign-pop|verify-pop|drand-bind|assemble]\n\
                                   Execute Mainnet Genesis Key Ceremony & entropy operations\n\
    security-council [setup|sign-pause|verify-pause]\n\
                                   Manage 5-of-7 Community Security Council emergency pause\n\
    wallet [generate|recover|address|public-key|sign]\n\
                                   Manage native BIP-39 mnemonic wallets and clear signing\n\
    account new                    Generate a fresh Secp256k1 account keypair and address\n\
    account balance <ADDR>         Query the balance of an account via RPC\n\
    tx transfer [OPTIONS]          Construct, sign, and broadcast a native transfer transaction\n\
    query block <HEIGHT|HASH>      Query block details by height or block hash\n\
    faucet [serve|request]         Run testnet token faucet server or request testnet tokens\n\
    connect [verify-prompt]        Validate TOLA Connect clear-signing prompt\n    external-asset [verify-attestation|show|canonical-id]\n                                   Verify attestations, inspect assets, or compute canonical IDs\n\nOPTIONS:
    --rpc-url <URL>                Target JSON-RPC URL (default: 127.0.0.1:8545)
    -h, --help                     Print help information\n"
    );
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        print_help();
        return;
    }

    let default_rpc = "127.0.0.1:8545";
    let rpc_url = get_arg_value(&args, "--rpc-url").unwrap_or_else(|| default_rpc.to_string());

    match args[1].as_str() {
        "-h" | "--help" => print_help(),
        "status" => cmd_status(&rpc_url),
        "node" => {
            let sub = args.get(2).map(|s| s.as_str()).unwrap_or("status");
            match sub {
                "health" => cmd_node_health(&rpc_url),
                "status" => cmd_status(&rpc_url),
                _ => {
                    eprintln!("Unknown node subcommand: {}. Use 'health' or 'status'", sub);
                }
            }
        }
        "genesis" => {
            let sub = args.get(2).map(|s| s.as_str()).unwrap_or("show");
            match sub {
                "generate" => cmd_genesis_generate(&args[3..]),
                "show" => cmd_genesis_show(&args[3..]),
                _ => {
                    eprintln!(
                        "Unknown genesis subcommand: {}. Use 'generate' or 'show'",
                        sub
                    );
                }
            }
        }
        "wallet" => {
            wallet::cmd_wallet(&args[2..], &rpc_url);
        }
        "account" => {
            let sub = args.get(2).map(|s| s.as_str()).unwrap_or("new");
            match sub {
                "new" => cmd_account_new(),
                "balance" => {
                    if let Some(addr_str) = args.get(3) {
                        cmd_account_balance(&rpc_url, addr_str);
                    } else {
                        eprintln!("Usage: tola-cli account balance <0xAddress>");
                    }
                }
                _ => {
                    eprintln!(
                        "Unknown account subcommand: {}. Use 'new' or 'balance'",
                        sub
                    );
                }
            }
        }
        "tx" => {
            let sub = args.get(2).map(|s| s.as_str()).unwrap_or("transfer");
            match sub {
                "transfer" => cmd_tx_transfer(&rpc_url, &args[3..]),
                _ => {
                    eprintln!("Unknown tx subcommand: {}. Use 'transfer'", sub);
                }
            }
        }
        "query" => {
            let sub = args.get(2).map(|s| s.as_str()).unwrap_or("block");
            match sub {
                "block" => {
                    if let Some(target) = args.get(3) {
                        cmd_query_block(&rpc_url, target);
                    } else {
                        eprintln!("Usage: tola-cli query block <height_or_hash>");
                    }
                }
                "tx" => {
                    if let Some(target) = args.get(3) {
                        cmd_query_tx(&rpc_url, target);
                    } else {
                        eprintln!("Usage: tola-cli query tx <tx_hash>");
                    }
                }
                _ => {
                    eprintln!("Unknown query subcommand: {}. Use 'block' or 'tx'", sub);
                }
            }
        }
        "faucet" => {
            let sub = args.get(2).map(|s| s.as_str()).unwrap_or("request");
            match sub {
                "serve" => cmd_faucet_serve(&args[3..], &rpc_url),
                "request" => cmd_faucet_request(&args[3..]),
                _ => {
                    eprintln!(
                        "Unknown faucet subcommand: {}. Use 'serve' or 'request'",
                        sub
                    );
                }
            }
        }
        "ceremony" => {
            cmd_ceremony(&args[2..]);
        }
        "security-council" => {
            cmd_security_council(&args[2..]);
        }
        "connect" => {
            cmd_connect(&args[2..]);
        }
        "external-asset" => {
            cmd_external_asset(&args[2..], &rpc_url);
        }
        other => {
            eprintln!(
                "Unknown command: '{}'. Run 'tola-cli --help' for available commands.",
                other
            );
        }
    }
}

fn cmd_faucet_serve(args: &[String], rpc_url: &str) {
    let port = get_arg_value_slice(args, "--port")
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(8080);
    let key_hex = match get_arg_value_slice(args, "--faucet-key") {
        Some(k) => k,
        None => {
            eprintln!(
                "Usage: tola-cli faucet serve --faucet-key <HEX> [--port 8080] [--amount 10]"
            );
            return;
        }
    };
    let payout_tola = get_arg_value_slice(args, "--amount")
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(10);
    let rpc = get_arg_value_slice(args, "--rpc-url").unwrap_or_else(|| rpc_url.to_string());

    let clean = key_hex.strip_prefix("0x").unwrap_or(&key_hex);
    if clean.len() != 64 {
        eprintln!("Faucet key must be 32 bytes (64 hex characters)");
        return;
    }
    let mut bytes = [0u8; 32];
    for i in 0..32 {
        bytes[i] = u8::from_str_radix(&clean[i * 2..i * 2 + 2], 16).unwrap_or(0);
    }
    let kp = match Secp256k1Keypair::from_private_key_bytes(&bytes) {
        Ok(k) => k,
        Err(e) => {
            eprintln!("Invalid key: {:?}", e);
            return;
        }
    };

    let server = faucet::FaucetServer::new(port, kp, rpc, payout_tola);
    if let Err(e) = server.start() {
        eprintln!("[ERROR] Faucet failed: {}", e);
    }
}

fn cmd_faucet_request(args: &[String]) {
    let to_str = match get_arg_value_slice(args, "--to") {
        Some(t) => t,
        None => {
            eprintln!("Usage: tola-cli faucet request --to <0xAddress|ttola1...>");
            return;
        }
    };
    let faucet_url =
        get_arg_value_slice(args, "--faucet-url").unwrap_or_else(|| "127.0.0.1:8080".to_string());

    let clean = faucet_url.strip_prefix("http://").unwrap_or(&faucet_url);
    let mut stream = match TcpStream::connect(clean) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[ERROR] Could not connect to faucet at {}: {}", clean, e);
            return;
        }
    };

    let body = format!("{{\"address\":\"{}\"}}", to_str);
    let req = format!(
        "POST /request HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        clean,
        body.len(),
        body
    );

    if let Err(e) = stream.write_all(req.as_bytes()) {
        eprintln!("[ERROR] Send request failed: {}", e);
        return;
    }

    let mut resp = String::new();
    let _ = stream.read_to_string(&mut resp);
    println!("Faucet Response: {}", resp.lines().last().unwrap_or(""));
}

// ============================================================================
// Command Handlers
// ============================================================================

fn cmd_status(rpc_url: &str) {
    println!("Connecting to TOLA Node at {}...", rpc_url);

    let chain_id_resp = rpc_call(rpc_url, "eth_chainId", vec![]);
    let block_num_resp = rpc_call(rpc_url, "eth_blockNumber", vec![]);
    let peers_resp = rpc_call(rpc_url, "net_peerCount", vec![]);
    let listening_resp = rpc_call(rpc_url, "net_listening", vec![]);

    match (chain_id_resp, block_num_resp, peers_resp, listening_resp) {
        (Ok(cid), Ok(h), Ok(peers), Ok(listen)) => {
            println!("=======================================================");
            println!("               TOLA NETWORK NODE STATUS                ");
            println!("=======================================================");
            println!(
                "Node Status:        {}",
                if listen.as_bool().unwrap_or(false) {
                    "ONLINE"
                } else {
                    "OFFLINE"
                }
            );
            println!("Chain ID:           {}", cid.as_str().unwrap_or("unknown"));
            println!(
                "Current Height:     {}",
                parse_hex_u64(h.as_str().unwrap_or("0x0"))
            );
            println!(
                "Connected Peers:    {}",
                parse_hex_u64(peers.as_str().unwrap_or("0x0"))
            );
            println!("=======================================================");
        }
        _ => {
            eprintln!(
                "[ERROR] Node is not responding at {}. Is tola-node running?",
                rpc_url
            );
        }
    }
}

fn cmd_node_health(rpc_url: &str) {
    match http_get(rpc_url, "/health") {
        Ok(body) => {
            println!("Health Response: {}", body.trim());
        }
        Err(e) => {
            eprintln!("[ERROR] Failed to query /health: {}", e);
        }
    }
}

fn cmd_genesis_generate(args: &[String]) {
    let chain_id = get_arg_value_slice(args, "--chain-id")
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(102);
    let num_validators = get_arg_value_slice(args, "--validators")
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(4);
    let output_path =
        get_arg_value_slice(args, "--output").unwrap_or_else(|| "genesis.json".to_string());

    println!(
        "Generating deterministic genesis for Chain ID {} with {} validators...",
        chain_id, num_validators
    );

    let mut validators = Vec::new();
    for i in 1..=num_validators {
        let mut seed = [0u8; 32];
        seed[0] = i as u8;
        let kp = Ed25519Keypair::from_seed(&seed);
        let pubkey_bytes = kp.public_key_bytes();
        let mut addr = [0u8; 20];
        addr.copy_from_slice(&pubkey_bytes[..20]);

        validators.push(ValidatorInfo {
            validator_id: ValidatorId(Address(addr)),
            consensus_pubkey: kp.public_key(),
            voting_power: 100_000,
            commission_bp: 500,
        });
    }

    // Default funded account for testing (Alice)
    let alice_addr = Address([0xAA; 20]);
    let accounts = vec![GenesisAccount {
        address: alice_addr,
        balance: Amount(1_000_000_000_000_000_000_000), // 1000 TOLA
    }];

    let genesis = GenesisConfig {
        chain_id: ChainId(chain_id),
        genesis_time: 1770000000,
        accounts,
        initial_validators: validators,
    };

    let root = compute_genesis_state_root(&genesis);
    let json = serialize_genesis_json(&genesis);

    if let Err(e) = std::fs::write(&output_path, &json) {
        eprintln!("[ERROR] Failed to write genesis file: {}", e);
        return;
    }

    println!(
        "[SUCCESS] Generated genesis specification written to: {}",
        output_path
    );
    println!("Genesis State Root:  {:?}", root);
    println!("Initial Validators:  {}", num_validators);
    println!("Initial Accounts:    1 (Alice: {})", alice_addr.to_hex());
}

fn cmd_genesis_show(args: &[String]) {
    let file_path =
        get_arg_value_slice(args, "--file").unwrap_or_else(|| "genesis.json".to_string());

    let content = match std::fs::read_to_string(&file_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[ERROR] Failed to read file {}: {}", file_path, e);
            return;
        }
    };

    let config = match parse_genesis_json(&content) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[ERROR] Failed to parse genesis JSON: {}", e);
            return;
        }
    };

    let root = compute_genesis_state_root(&config);

    println!("=======================================================");
    println!("               GENESIS SPECIFICATION                   ");
    println!("=======================================================");
    println!("Chain ID:            {}", config.chain_id.0);
    println!("Genesis Timestamp:   {}", config.genesis_time);
    println!("Genesis State Root:  {:?}", root);
    println!("Accounts Count:      {}", config.accounts.len());
    for (i, a) in config.accounts.iter().enumerate() {
        println!(
            "  [{}] Address: {} | Balance: {} base units",
            i,
            a.address.to_hex(),
            a.balance.0
        );
    }
    println!("Validators Count:    {}", config.initial_validators.len());
    for (i, v) in config.initial_validators.iter().enumerate() {
        println!(
            "  [{}] ID: {} | Voting Power: {}",
            i,
            v.validator_id.0.to_hex(),
            v.voting_power
        );
    }
    println!("=======================================================");
}

fn cmd_account_new() {
    let kp = Secp256k1Keypair::generate();
    let addr = kp.address();
    let bech32_testnet = addr.to_bech32m(&Network::Testnet);
    let bech32_mainnet = addr.to_bech32m(&Network::Mainnet);
    let pk_bytes = kp.public_key_bytes();

    println!("=======================================================");
    println!("              NEW TOLA ACCOUNT GENERATED               ");
    println!("=======================================================");
    println!("Address (Hex):      {}", addr.to_hex());
    println!("Address (Testnet):  {}", bech32_testnet);
    println!("Address (Mainnet):  {}", bech32_mainnet);
    println!("Public Key (Hex):   0x{}", hex_encode(pk_bytes));
    println!("=======================================================");
    println!("IMPORTANT: Store your private key securely!");
}

fn cmd_account_balance(rpc_url: &str, addr_str: &str) {
    let params = vec![
        JsonValue::String(addr_str.to_string()),
        JsonValue::String("latest".to_string()),
    ];

    match rpc_call(rpc_url, "eth_getBalance", params) {
        Ok(res) => {
            let hex_str = res.as_str().unwrap_or("0x0");
            let wei = parse_hex_u128(hex_str);
            let tola_whole = wei / Amount::ONE_TOLA;
            let tola_remainder = wei % Amount::ONE_TOLA;

            println!("Account: {}", addr_str);
            println!(
                "Balance: {} base units (~{}.{:04} TOLA)",
                wei,
                tola_whole,
                tola_remainder / 100_000_000_000_000
            );
        }
        Err(e) => eprintln!("[ERROR] Query failed: {}", e),
    }
}

fn cmd_tx_transfer(rpc_url: &str, args: &[String]) {
    let to_str = match get_arg_value_slice(args, "--to") {
        Some(s) => s,
        None => {
            eprintln!(
                "Usage: tola-cli tx transfer --to <ADDR> --amount <TOLA> --private-key <HEX> [--memo <TEXT>]"
            );
            return;
        }
    };

    let amount_tola =
        match get_arg_value_slice(args, "--amount").and_then(|s| s.parse::<f64>().ok()) {
            Some(a) if a > 0.0 => a,
            _ => {
                eprintln!("Missing or invalid --amount (must be positive number of TOLA)");
                return;
            }
        };

    let pk_hex = match get_arg_value_slice(args, "--private-key") {
        Some(s) => s,
        None => {
            eprintln!("Missing --private-key <HEX>");
            return;
        }
    };

    let memo_opt = get_arg_value_slice(args, "--memo").and_then(|s| {
        let trimmed = s.trim().to_string();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    });

    if let Some(ref m) = memo_opt {
        if m.len() > tola_types::MAX_MEMO_BYTES {
            eprintln!(
                "[ERROR] Memo exceeds maximum length of 256 UTF-8 bytes (got {} bytes)",
                m.len()
            );
            return;
        }
    }

    let to_addr = if to_str.starts_with("tola1")
        || to_str.starts_with("ttola1")
        || to_str.starts_with("dtola1")
    {
        match Address::from_bech32m(&to_str) {
            Ok((_, a)) => a,
            Err(e) => {
                eprintln!("Invalid recipient bech32m address: {:?}", e);
                return;
            }
        }
    } else {
        match Address::from_hex(&to_str) {
            Ok(a) => a,
            Err(e) => {
                eprintln!("Invalid recipient address: {:?}", e);
                return;
            }
        }
    };

    let clean_pk = pk_hex.strip_prefix("0x").unwrap_or(&pk_hex);
    if clean_pk.len() != 64 {
        eprintln!("Private key must be 32 bytes (64 hex characters)");
        return;
    }
    let mut pk_bytes = [0u8; 32];
    for i in 0..32 {
        pk_bytes[i] = u8::from_str_radix(&clean_pk[i * 2..i * 2 + 2], 16).unwrap_or(0);
    }
    let kp = match Secp256k1Keypair::from_private_key_bytes(&pk_bytes) {
        Ok(k) => k,
        Err(e) => {
            eprintln!("Invalid private key: {:?}", e);
            return;
        }
    };

    let from_addr = kp.address();
    println!("Sender Address:    {}", from_addr.to_hex());
    println!("Recipient Address: {}", to_addr.to_hex());
    if let Some(ref m) = memo_opt {
        println!("Memo:              \"{}\"", m);
    }

    // 1. Query nonce
    let nonce_resp = rpc_call(
        rpc_url,
        "eth_getTransactionCount",
        vec![
            JsonValue::String(from_addr.to_hex()),
            JsonValue::String("latest".into()),
        ],
    );
    let nonce_val = match nonce_resp {
        Ok(v) => parse_hex_u64(v.as_str().unwrap_or("0x0")),
        Err(e) => {
            eprintln!("[ERROR] Failed to query sender nonce: {}", e);
            return;
        }
    };

    // 2. Query chain ID
    let chain_id = match rpc_call(rpc_url, "eth_chainId", vec![]) {
        Ok(v) => parse_hex_u64(v.as_str().unwrap_or("0x66")),
        Err(_) => 102,
    };

    // 3. Assemble and sign transaction
    let amount_base = (amount_tola * (Amount::ONE_TOLA as f64)) as u128;
    let mut tx = Transaction {
        chain_id: ChainId(chain_id),
        nonce: Nonce(nonce_val),
        from: from_addr,
        to: Some(to_addr),
        value: Amount(amount_base),
        gas_limit: Gas(21_000),
        max_fee_per_gas: Amount(1_000_000_000),
        max_priority_fee_per_gas: Amount(1_000_000_000),
        valid_until_block: BlockHeight(1_000_000),
        data: vec![],
        memo: memo_opt,
        signature: Signature::Secp256k1 {
            r: [0u8; 32],
            s: [0u8; 32],
            v: 0,
        },
        tx_type: 0,
        access_list: vec![],
    };

    let digest = compute_tx_digest(&tx);
    let sig_65 = kp
        .sign_recoverable(digest.as_bytes())
        .expect("Sign transaction");
    let mut r = [0u8; 32];
    let mut s = [0u8; 32];
    r.copy_from_slice(&sig_65[0..32]);
    s.copy_from_slice(&sig_65[32..64]);
    let v = sig_65[64];
    tx.signature = Signature::Secp256k1 { r, s, v };

    let wire_bytes = tx.to_canonical_bytes();
    let raw_hex = format!("0x{}", hex_encode(&wire_bytes));

    // 4. Broadcast via eth_sendRawTransaction
    match rpc_call(
        rpc_url,
        "eth_sendRawTransaction",
        vec![JsonValue::String(raw_hex)],
    ) {
        Ok(res) => {
            println!("[SUCCESS] Transaction Broadcast!");
            println!("Transaction Hash: {}", res.as_str().unwrap_or(""));
        }
        Err(e) => {
            eprintln!("[ERROR] Broadcast failed: {}", e);
        }
    }
}

fn cmd_query_block(rpc_url: &str, target: &str) {
    let method = if target.starts_with("0x") && target.len() == 66 {
        "eth_getBlockByHash"
    } else {
        "eth_getBlockByNumber"
    };

    let param = if method == "eth_getBlockByNumber" {
        if let Ok(num) = target.parse::<u64>() {
            format!("0x{:x}", num)
        } else {
            target.to_string()
        }
    } else {
        target.to_string()
    };

    let params = vec![JsonValue::String(param), JsonValue::Bool(true)];
    match rpc_call(rpc_url, method, params) {
        Ok(block_val) => {
            println!("Block Details: {}", block_val.serialize());
        }
        Err(e) => {
            eprintln!("[ERROR] Query block failed: {}", e);
        }
    }
}

fn cmd_query_tx(rpc_url: &str, tx_hash: &str) {
    let params = vec![JsonValue::String(tx_hash.to_string())];
    match rpc_call(rpc_url, "eth_getTransactionByHash", params.clone()) {
        Ok(tx_val) => {
            println!("Transaction Payload: {}", tx_val.serialize());
            if let Ok(rec_val) = rpc_call(rpc_url, "eth_getTransactionReceipt", params) {
                println!("Transaction Receipt: {}", rec_val.serialize());
            }
        }
        Err(e) => {
            eprintln!("[ERROR] Query tx failed: {}", e);
        }
    }
}

// ============================================================================
// RPC & Networking Utilities
// ============================================================================

fn rpc_call(rpc_url: &str, method: &str, params: Vec<JsonValue>) -> Result<JsonValue, String> {
    let clean = rpc_url.strip_prefix("http://").unwrap_or(rpc_url);
    let mut stream = TcpStream::connect(clean)
        .map_err(|e| format!("Failed to connect to RPC node at {}: {}", clean, e))?;

    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));

    let req_obj = JsonValue::Object(vec![
        ("jsonrpc".into(), JsonValue::String("2.0".into())),
        ("method".into(), JsonValue::String(method.into())),
        ("params".into(), JsonValue::Array(params)),
        ("id".into(), JsonValue::Integer(1)),
    ]);
    let req_body = req_obj.serialize();

    let http_req = format!(
        "POST / HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        clean,
        req_body.len(),
        req_body
    );

    stream
        .write_all(http_req.as_bytes())
        .map_err(|e| format!("Failed to write to RPC stream: {}", e))?;
    stream.flush().map_err(|e| format!("Flush error: {}", e))?;

    let mut response_bytes = Vec::new();
    stream
        .read_to_end(&mut response_bytes)
        .map_err(|e| format!("Failed to read RPC response: {}", e))?;

    let resp_str = String::from_utf8_lossy(&response_bytes);
    let body = extract_http_body(&resp_str)
        .ok_or_else(|| "Failed to extract body from HTTP response".to_string())?;

    let parsed = JsonValue::parse(body).map_err(|e| format!("JSON parse error: {}", e))?;

    if let Some(err) = parsed.get("error") {
        return Err(format!("RPC Error: {}", err.serialize()));
    }

    parsed
        .get("result")
        .cloned()
        .ok_or_else(|| "Missing 'result' field in RPC response".to_string())
}

fn http_get(rpc_url: &str, path: &str) -> Result<String, String> {
    let clean = rpc_url.strip_prefix("http://").unwrap_or(rpc_url);
    let mut stream =
        TcpStream::connect(clean).map_err(|e| format!("Failed to connect to {}: {}", clean, e))?;

    let _ = stream.set_read_timeout(Some(Duration::from_secs(3)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(3)));

    let req = format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
        path, clean
    );
    stream
        .write_all(req.as_bytes())
        .map_err(|e| e.to_string())?;
    stream.flush().map_err(|e| e.to_string())?;

    let mut response_bytes = Vec::new();
    stream
        .read_to_end(&mut response_bytes)
        .map_err(|e| e.to_string())?;

    let resp_str = String::from_utf8_lossy(&response_bytes);
    extract_http_body(&resp_str)
        .map(|s| s.to_string())
        .ok_or_else(|| "Missing HTTP body".to_string())
}

fn extract_http_body(http_response: &str) -> Option<&str> {
    if let Some(pos) = http_response.find("\r\n\r\n") {
        Some(&http_response[pos + 4..])
    } else if let Some(pos) = http_response.find("\n\n") {
        Some(&http_response[pos + 2..])
    } else {
        None
    }
}

fn parse_hex_u64(s: &str) -> u64 {
    let clean = s
        .strip_prefix("0x")
        .or_else(|| s.strip_prefix("0X"))
        .unwrap_or(s);
    u64::from_str_radix(clean, 16).unwrap_or(0)
}

fn parse_hex_u128(s: &str) -> u128 {
    let clean = s
        .strip_prefix("0x")
        .or_else(|| s.strip_prefix("0X"))
        .unwrap_or(s);
    u128::from_str_radix(clean, 16).unwrap_or(0)
}

fn hex_encode<T: AsRef<[u8]>>(data: T) -> String {
    let mut s = String::new();
    for b in data.as_ref() {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

fn compute_tx_digest(tx: &Transaction) -> Hash {
    let mut unsigned_bytes = Vec::new();
    tola_codec::serialize_unsigned_transaction(tx, &mut unsigned_bytes);
    let payload_hash = tola_crypto::blake3_hash(&unsigned_bytes);
    tola_crypto::domain_separated_hash(
        tola_crypto::DOMAIN_TRANSACTION_PAYLOAD,
        tx.chain_id.0,
        0,
        &payload_hash,
    )
}

fn get_arg_value(args: &[String], key: &str) -> Option<String> {
    for i in 0..args.len() {
        if args[i] == key && i + 1 < args.len() {
            return Some(args[i + 1].clone());
        }
    }
    None
}

fn get_arg_value_slice(args: &[String], key: &str) -> Option<String> {
    for i in 0..args.len() {
        if args[i] == key && i + 1 < args.len() {
            return Some(args[i + 1].clone());
        }
    }
    None
}

fn cmd_ceremony(args: &[String]) {
    if args.is_empty() {
        println!("Usage: tola-cli ceremony <keygen|sign-pop|verify-pop|drand-bind|assemble>");
        return;
    }
    match args[0].as_str() {
        "keygen" => {
            println!("=======================================================");
            println!("       TOLA AIR-GAPPED VALIDATOR KEY GENERATION        ");
            println!("=======================================================");
            println!("WARNING: This tool generates high-security consensus keys.");
            println!("         Ensure this machine is completely air-gapped.");
            println!("         NEVER share or store private keys on any network.");
            println!("-------------------------------------------------------");
            let keypair = Ed25519Keypair::generate();
            let pubkey_bytes = keypair.public_key_bytes();
            let mut addr = [0u8; 20];
            addr.copy_from_slice(&pubkey_bytes[..20]);
            let operator = Address(addr);

            println!("Operator Address (Hex): 0x{}", hex::encode(operator.0));
            println!("Consensus Public Key:   0x{}", hex::encode(pubkey_bytes));
            println!(
                "Public Key Fingerprint: {}",
                hex::encode(tola_crypto::blake3_hash(&pubkey_bytes).0)
            );
            println!("-------------------------------------------------------");
            println!("[NOTICE] Secure your 32-byte secret seed in an institutional HSM.");
            println!("=======================================================");
        }
        "sign-pop" => {
            let key_hex = match get_arg_value_slice(&args[1..], "--key") {
                Some(k) => k,
                None => {
                    eprintln!("Error: --key <HEX> is required");
                    return;
                }
            };
            let op_hex = match get_arg_value_slice(&args[1..], "--operator") {
                Some(o) => o,
                None => {
                    eprintln!("Error: --operator <HEX> is required");
                    return;
                }
            };
            let chain_id = get_arg_value_slice(&args[1..], "--chain-id")
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(9999);

            let clean_key = key_hex.strip_prefix("0x").unwrap_or(&key_hex);
            let key_bytes = match hex::decode(clean_key) {
                Ok(b) if b.len() == 32 => {
                    let mut s = [0u8; 32];
                    s.copy_from_slice(&b);
                    s
                }
                _ => {
                    eprintln!("Error: --key must be 32 bytes (64 hex characters)");
                    return;
                }
            };
            let operator = match Address::from_hex(&op_hex) {
                Ok(a) => a,
                Err(e) => {
                    eprintln!("Error: Invalid operator address: {:?}", e);
                    return;
                }
            };

            let kp = Ed25519Keypair::from_seed(&key_bytes);
            let sig = ceremony::sign_proof_of_possession(&kp, &operator, chain_id);
            println!("=======================================================");
            println!("        PROOF OF POSSESSION (PoP) GENERATED            ");
            println!("=======================================================");
            println!("Chain ID:             {}", chain_id);
            println!("Operator Address:     {}", operator.to_hex());
            println!(
                "Consensus Pubkey:     0x{}",
                hex::encode(kp.public_key_bytes())
            );
            println!("PoP Signature:        0x{}", hex::encode(sig));
            println!("=======================================================");
        }
        "verify-pop" => {
            let pub_hex = match get_arg_value_slice(&args[1..], "--pubkey") {
                Some(p) => p,
                None => {
                    eprintln!("Error: --pubkey <HEX> is required");
                    return;
                }
            };
            let op_hex = match get_arg_value_slice(&args[1..], "--operator") {
                Some(o) => o,
                None => {
                    eprintln!("Error: --operator <HEX> is required");
                    return;
                }
            };
            let sig_hex = match get_arg_value_slice(&args[1..], "--signature") {
                Some(s) => s,
                None => {
                    eprintln!("Error: --signature <HEX> is required");
                    return;
                }
            };
            let chain_id = get_arg_value_slice(&args[1..], "--chain-id")
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(9999);

            let clean_pub = pub_hex.strip_prefix("0x").unwrap_or(&pub_hex);
            let pub_bytes = match hex::decode(clean_pub) {
                Ok(b) if b.len() == 32 => {
                    let mut p = [0u8; 32];
                    p.copy_from_slice(&b);
                    p
                }
                _ => {
                    eprintln!("Error: --pubkey must be 32 bytes");
                    return;
                }
            };
            let operator = match Address::from_hex(&op_hex) {
                Ok(a) => a,
                Err(e) => {
                    eprintln!("Error: Invalid operator address: {:?}", e);
                    return;
                }
            };
            let clean_sig = sig_hex.strip_prefix("0x").unwrap_or(&sig_hex);
            let sig_bytes = match hex::decode(clean_sig) {
                Ok(b) if b.len() == 64 => {
                    let mut s = [0u8; 64];
                    s.copy_from_slice(&b);
                    s
                }
                _ => {
                    eprintln!("Error: --signature must be 64 bytes");
                    return;
                }
            };

            match ceremony::verify_proof_of_possession(&pub_bytes, &operator, chain_id, &sig_bytes)
            {
                Ok(()) => {
                    println!("[SUCCESS] Proof of Possession is CRYPTOGRAPHICALLY VALID!");
                }
                Err(e) => {
                    eprintln!("[FAILURE] Proof of Possession verification FAILED: {}", e);
                }
            }
        }
        "drand-bind" => {
            let round = match get_arg_value_slice(&args[1..], "--round")
                .and_then(|s| s.parse::<u64>().ok())
            {
                Some(r) => r,
                None => {
                    eprintln!("Error: --round <N> is required");
                    return;
                }
            };
            let sig_hex = match get_arg_value_slice(&args[1..], "--signature") {
                Some(s) => s,
                None => {
                    eprintln!("Error: --signature <HEX> is required");
                    return;
                }
            };
            let genesis_path = match get_arg_value_slice(&args[1..], "--genesis") {
                Some(g) => g,
                None => {
                    eprintln!("Error: --genesis <FILE> is required");
                    return;
                }
            };
            let output_path = get_arg_value_slice(&args[1..], "--output")
                .unwrap_or_else(|| "genesis-mainnet-drand.json".to_string());

            let clean_sig = sig_hex.strip_prefix("0x").unwrap_or(&sig_hex);
            let sig_bytes: Vec<u8> = match hex::decode(clean_sig) {
                Ok(b) => b,
                Err(e) => {
                    eprintln!("Error: Invalid signature hex: {:?}", e);
                    return;
                }
            };

            let content = match std::fs::read_to_string(&genesis_path) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Error reading genesis file: {}", e);
                    return;
                }
            };
            let mut genesis = match parse_genesis_json(&content) {
                Ok(g) => g,
                Err(e) => {
                    eprintln!("Error parsing genesis JSON: {}", e);
                    return;
                }
            };

            match ceremony::mix_drand_entropy(&mut genesis, round, &sig_bytes) {
                Ok(entropy_hash) => {
                    let out_json = serialize_genesis_json(&genesis);
                    if let Err(e) = std::fs::write(&output_path, out_json) {
                        eprintln!("Error writing output genesis: {}", e);
                        return;
                    }
                    let state_root = compute_genesis_state_root(&genesis);
                    println!("=======================================================");
                    println!("       DRAND ENTROPY BEACON BOUND TO GENESIS           ");
                    println!("=======================================================");
                    println!("Drand Round:          {}", round);
                    println!("Entropy Hash:         0x{}", hex::encode(entropy_hash.0));
                    println!("Genesis State Root:   0x{}", hex::encode(state_root.0));
                    println!("Output File:          {}", output_path);
                    println!("=======================================================");
                }
                Err(e) => {
                    eprintln!("Failed to mix Drand entropy: {}", e);
                }
            }
        }
        "assemble" => {
            let records_path = match get_arg_value_slice(&args[1..], "--records") {
                Some(r) => r,
                None => {
                    eprintln!("Error: --records <FILE> is required");
                    return;
                }
            };
            let template_path = match get_arg_value_slice(&args[1..], "--template") {
                Some(t) => t,
                None => {
                    eprintln!("Error: --template <FILE> is required");
                    return;
                }
            };
            let output_path = get_arg_value_slice(&args[1..], "--output")
                .unwrap_or_else(|| "genesis-mainnet-final.json".to_string());

            let template_content = match std::fs::read_to_string(&template_path) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Error reading template: {}", e);
                    return;
                }
            };
            let template = match parse_genesis_json(&template_content) {
                Ok(t) => t,
                Err(e) => {
                    eprintln!("Error parsing template JSON: {}", e);
                    return;
                }
            };

            let records_content = match std::fs::read_to_string(&records_path) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Error reading records: {}", e);
                    return;
                }
            };
            let records_val = match JsonValue::parse(&records_content) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("Error parsing records JSON: {:?}", e);
                    return;
                }
            };
            let records_arr = match records_val {
                JsonValue::Array(a) => a,
                _ => {
                    eprintln!("Records file must contain a JSON array");
                    return;
                }
            };

            let mut records = Vec::new();
            for item in records_arr {
                let rec = match ceremony::CeremonyRecord::from_json(&item.serialize()) {
                    Ok(r) => r,
                    Err(e) => {
                        eprintln!("Error parsing record item: {}", e);
                        return;
                    }
                };
                records.push(rec);
            }

            match ceremony::assemble_mainnet_genesis(&template, &records) {
                Ok(final_genesis) => {
                    let out_json = serialize_genesis_json(&final_genesis);
                    if let Err(e) = std::fs::write(&output_path, out_json) {
                        eprintln!("Error writing output: {}", e);
                        return;
                    }
                    let state_root = compute_genesis_state_root(&final_genesis);
                    println!("=======================================================");
                    println!("    CANONICAL MAINNET GENESIS ASSEMBLED SUCCESSFULLY    ");
                    println!("=======================================================");
                    println!("Chain ID:             {}", final_genesis.chain_id.0);
                    println!(
                        "Validator Count:      {}",
                        final_genesis.initial_validators.len()
                    );
                    println!("Genesis State Root:   0x{}", hex::encode(state_root.0));
                    println!("Output File:          {}", output_path);
                    println!("=======================================================");
                }
                Err(e) => {
                    eprintln!("[ERROR] Genesis assembly failed: {}", e);
                }
            }
        }
        other => {
            eprintln!("Unknown ceremony subcommand: {}. Use keygen, sign-pop, verify-pop, drand-bind, or assemble", other);
        }
    }
}

fn cmd_security_council(args: &[String]) {
    if args.is_empty() {
        println!("Usage: tola-cli security-council <setup|sign-pause|verify-pause>");
        return;
    }
    match args[0].as_str() {
        "setup" => {
            let policy = security_council::SecurityCouncilPolicy::default_mainnet_council();
            let json = policy.to_json();
            if let Some(out) = get_arg_value_slice(&args[1..], "--output") {
                if let Err(e) = std::fs::write(&out, &json) {
                    eprintln!("Error writing policy: {}", e);
                    return;
                }
                println!("[SUCCESS] Security Council policy written to: {}", out);
            } else {
                println!("{}", json);
            }
        }
        "sign-pause" => {
            let key_hex = match get_arg_value_slice(&args[1..], "--key") {
                Some(k) => k,
                None => {
                    eprintln!("Error: --key <HEX> is required");
                    return;
                }
            };
            let reason = match get_arg_value_slice(&args[1..], "--reason") {
                Some(r) => r,
                None => {
                    eprintln!("Error: --reason <TEXT> is required");
                    return;
                }
            };
            let chain_id = get_arg_value_slice(&args[1..], "--chain-id")
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(9999);
            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();

            let clean_key = key_hex.strip_prefix("0x").unwrap_or(&key_hex);
            let key_bytes = match hex::decode(clean_key) {
                Ok(b) if b.len() == 32 => {
                    let mut s = [0u8; 32];
                    s.copy_from_slice(&b);
                    s
                }
                _ => {
                    eprintln!("Error: --key must be 32 bytes (64 hex characters)");
                    return;
                }
            };

            let kp = Ed25519Keypair::from_seed(&key_bytes);
            let (pubkey, sig) =
                security_council::EmergencyPauseSignal::sign(&kp, &reason, chain_id, timestamp);
            println!("=======================================================");
            println!("     SECURITY COUNCIL EMERGENCY PAUSE SIGNATURE        ");
            println!("=======================================================");
            println!("Chain ID:             {}", chain_id);
            println!("Timestamp:            {}", timestamp);
            println!("Reason:               {}", reason);
            println!("Signer Pubkey:        0x{}", hex::encode(pubkey));
            println!("Signature:            0x{}", hex::encode(sig));
            println!("=======================================================");
        }
        "verify-pause" => {
            let signal_path = match get_arg_value_slice(&args[1..], "--signal") {
                Some(s) => s,
                None => {
                    eprintln!("Error: --signal <FILE> is required");
                    return;
                }
            };
            let policy_path = match get_arg_value_slice(&args[1..], "--policy") {
                Some(p) => p,
                None => {
                    eprintln!("Error: --policy <FILE> is required");
                    return;
                }
            };

            let sig_content = match std::fs::read_to_string(&signal_path) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Error reading signal file: {}", e);
                    return;
                }
            };
            let pol_content = match std::fs::read_to_string(&policy_path) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Error reading policy file: {}", e);
                    return;
                }
            };

            let signal = match security_council::EmergencyPauseSignal::from_json(&sig_content) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("Error parsing signal JSON: {}", e);
                    return;
                }
            };
            let policy = match security_council::SecurityCouncilPolicy::from_json(&pol_content) {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("Error parsing policy JSON: {}", e);
                    return;
                }
            };

            match signal.verify(&policy) {
                Ok(count) => {
                    println!("=======================================================");
                    println!(" [SUCCESS] EMERGENCY PAUSE SIGNAL VERIFIED VALID!      ");
                    println!("=======================================================");
                    println!(
                        "Signatures Verified:  {} / {} (Threshold: {})",
                        count, policy.total_signers, policy.threshold
                    );
                    println!("Reason:               {}", signal.reason);
                    println!("Chain ID:             {}", signal.chain_id);
                    println!("=======================================================");
                }
                Err(e) => {
                    eprintln!("=======================================================");
                    eprintln!(" [REJECTED] PAUSE SIGNAL VALIDATION FAILED:            ");
                    eprintln!(" {}", e);
                    eprintln!("=======================================================");
                }
            }
        }
        other => {
            eprintln!(
                "Unknown security-council subcommand: {}. Use setup, sign-pause, or verify-pause",
                other
            );
        }
    }
}

fn cmd_connect(args: &[String]) {
    if args.is_empty() {
        eprintln!("Usage: tola-cli connect verify-prompt [OPTIONS]");
        return;
    }
    match args[0].as_str() {
        "verify-prompt" => {
            let json_str = if let Some(path) = get_arg_value(args, "--file") {
                match std::fs::read_to_string(&path) {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("Error reading file {}: {}", path, e);
                        return;
                    }
                }
            } else if let Some(s) = get_arg_value(args, "--prompt") {
                s
            } else {
                eprintln!("Missing --file <PATH> or --prompt <JSON>");
                return;
            };
            let val = match JsonValue::parse(&json_str) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("JSON parse error: {:?}", e);
                    return;
                }
            };
            let domain = val.get("domain").and_then(|v| v.as_str()).unwrap_or("");
            let action_name = val.get("actionName").and_then(|v| v.as_str()).unwrap_or("");
            let target_str = val
                .get("targetAddress")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let amount = val
                .get("amount")
                .and_then(|v| {
                    if let Some(s) = v.as_str() {
                        s.parse::<u128>().ok()
                    } else {
                        v.as_u64().map(|n| n as u128)
                    }
                })
                .unwrap_or(0);

            let asset_symbol = val
                .get("assetSymbol")
                .and_then(|v| v.as_str())
                .unwrap_or("TOLA");
            let max_fee = val.get("maxFee").and_then(|v| v.as_u64()).unwrap_or(21000);
            let chain_id = val.get("chainId").and_then(|v| v.as_u64()).unwrap_or(9999);
            let nonce = val.get("nonce").and_then(|v| v.as_u64()).unwrap_or(0);

            let memo = val
                .get("memo")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            let target_addr = match Address::from_hex(target_str) {
                Ok(a) => a,
                Err(_) => match Address::from_bech32m(target_str) {
                    Ok((_, a)) => a,
                    Err(e) => {
                        eprintln!("Invalid target address: {:?}", e);
                        return;
                    }
                },
            };

            let prompt = ClearSigningPrompt {
                domain: domain.to_string(),
                action_name: action_name.to_string(),
                target_address: target_addr,
                amount: Amount::from_u128(amount),
                asset_symbol: asset_symbol.to_string(),
                max_fee: Gas(max_fee),
                nonce: Nonce::new(nonce),
                chain_id,
                memo: memo.clone(),
            };

            match prompt.validate() {
                Ok(()) => {
                    println!("=======================================================");
                    println!(" [VALID] CLEAR-SIGNING PROMPT VERIFIED                 ");
                    println!("=======================================================");
                    println!("Domain:       {}", prompt.domain);
                    println!("Action:       {}", prompt.action_name);
                    println!(
                        "Target:       {}",
                        prompt.target_address.to_bech32m(&Network::Mainnet)
                    );
                    println!(
                        "Amount:       {} {}",
                        prompt.amount.as_u128() / Amount::ONE_TOLA,
                        prompt.asset_symbol
                    );
                    println!("Max Fee:      {} gas", prompt.max_fee);
                    println!("Chain ID:     {}", prompt.chain_id);
                    if let Some(ref m) = prompt.memo {
                        println!("Memo:         {}", m);
                    } else {
                        println!("Memo:         (None)");
                    }
                    println!("=======================================================");
                }
                Err(e) => {
                    eprintln!("=======================================================");
                    eprintln!(
                        " [REJECTED] Clear-signing prompt validation failed: {:?}",
                        e
                    );
                    eprintln!("=======================================================");
                }
            }
        }
        other => eprintln!("Unknown connect subcommand: {}. Use verify-prompt", other),
    }
}

fn parse_asset_type_cli(s: &str) -> Option<ExternalAssetType> {
    match s.to_ascii_lowercase().as_str() {
        "messenger_username" | "1" => Some(ExternalAssetType::MessengerUsername),
        "messenger_number" | "2" => Some(ExternalAssetType::MessengerNumber),
        "messenger_gift" | "3" => Some(ExternalAssetType::MessengerGift),
        "messenger_badge" | "4" => Some(ExternalAssetType::MessengerBadge),
        "social_username" | "5" => Some(ExternalAssetType::SocialUsername),
        "social_number" | "6" => Some(ExternalAssetType::SocialNumber),
        "digital_collectible" | "7" => Some(ExternalAssetType::DigitalCollectible),
        "other" | "other_external_asset" | "8" => Some(ExternalAssetType::OtherExternalAsset),
        _ => None,
    }
}

fn cmd_external_asset(args: &[String], rpc_url: &str) {
    if args.is_empty() {
        eprintln!(
            "Usage: tola-cli external-asset [verify-attestation|show|canonical-id] [OPTIONS]"
        );
        return;
    }

    match args[0].as_str() {
        "verify-attestation" => {
            let json_str = if let Some(path) = get_arg_value_slice(args, "--file") {
                match std::fs::read_to_string(&path) {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("Error reading file {}: {}", path, e);
                        return;
                    }
                }
            } else if let Some(s) = get_arg_value_slice(args, "--attestation") {
                s
            } else {
                eprintln!("Missing --file <PATH> or --attestation <JSON>");
                return;
            };

            let val = match JsonValue::parse(&json_str) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("JSON parse error: {:?}", e);
                    return;
                }
            };

            let chain_id = val.get("chainId").and_then(|v| v.as_u64()).unwrap_or(0);
            let nonce = val.get("nonce").and_then(|v| v.as_u64()).unwrap_or(0);
            let dest_str = val
                .get("destinationWallet")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let expires_at = val.get("expiresAt").and_then(|v| v.as_u64()).unwrap_or(0);
            let source_platform = val
                .get("sourcePlatform")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let external_asset_id = val
                .get("externalAssetId")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let verifier_str = val
                .get("authorizer")
                .or_else(|| val.get("authorizerAddress"))
                .or_else(|| val.get("verifierAddress"))
                .and_then(|v| v.as_str())
                .unwrap_or("");

            let asset_type = match val.get("assetType") {
                Some(JsonValue::String(s)) => parse_asset_type_cli(s),
                Some(JsonValue::Integer(i)) => parse_asset_type_cli(&i.to_string()),
                _ => None,
            };

            let asset_type = match asset_type {
                Some(t) => t,
                None => {
                    eprintln!("Invalid or missing assetType");
                    return;
                }
            };

            let dest_addr = match Address::from_hex(dest_str) {
                Ok(a) => a,
                Err(_) => match Address::from_bech32m(dest_str) {
                    Ok((_, a)) => a,
                    Err(_) => {
                        eprintln!("Invalid destination wallet address: {}", dest_str);
                        return;
                    }
                },
            };

            let verifier_addr = match Address::from_hex(verifier_str) {
                Ok(a) => a,
                Err(_) => match Address::from_bech32m(verifier_str) {
                    Ok((_, a)) => a,
                    Err(_) => {
                        eprintln!("Invalid verifier address: {}", verifier_str);
                        return;
                    }
                },
            };

            let mut sig_65 = [0u8; 65];
            if let Some(sig_val) = val.get("signature") {
                if let Some(sig_hex) = sig_val.as_str() {
                    let clean = sig_hex.strip_prefix("0x").unwrap_or(sig_hex);
                    if clean.len() != 130 {
                        eprintln!("Signature hex must be 65 bytes (130 hex characters)");
                        return;
                    }
                    for i in 0..65 {
                        sig_65[i] = u8::from_str_radix(&clean[i * 2..i * 2 + 2], 16).unwrap_or(0);
                    }
                } else {
                    let r_str = sig_val.get("r").and_then(|v| v.as_str()).unwrap_or("");
                    let s_str = sig_val.get("s").and_then(|v| v.as_str()).unwrap_or("");
                    let v_num = sig_val.get("v").and_then(|v| v.as_u64()).unwrap_or(0) as u8;
                    if r_str.len() != 64 || s_str.len() != 64 {
                        eprintln!("Invalid signature r/s length");
                        return;
                    }
                    for i in 0..32 {
                        sig_65[i] = u8::from_str_radix(&r_str[i * 2..i * 2 + 2], 16).unwrap_or(0);
                        sig_65[32 + i] =
                            u8::from_str_radix(&s_str[i * 2..i * 2 + 2], 16).unwrap_or(0);
                    }
                    sig_65[64] = v_num;
                }
            } else {
                eprintln!("Missing signature in attestation");
                return;
            }

            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();

            let canonical_id = tola_crypto::compute_external_asset_canonical_id(
                source_platform,
                asset_type,
                external_asset_id,
            );

            match tola_crypto::verify_external_asset_attestation(
                chain_id,
                nonce,
                &dest_addr,
                expires_at,
                source_platform,
                asset_type,
                external_asset_id,
                &verifier_addr,
                &sig_65,
                now,
            ) {
                Ok(true) => {
                    println!("=======================================================");
                    println!(" [VERIFIED] EXTERNAL ASSET ATTESTATION IS VALID!       ");
                    println!("=======================================================");
                    println!("Canonical ID:        0x{}", hex_encode(canonical_id));
                    println!("Source Platform:     {}", source_platform);
                    println!("Asset Type:          {:?}", asset_type);
                    println!("External Asset ID:   {}", external_asset_id);
                    println!("Destination Wallet:  {}", dest_addr.to_hex());
                    println!("Platform Authorizer: {}", verifier_addr.to_hex());
                    println!("Expires At:          {} (current: {})", expires_at, now);
                    println!("=======================================================");
                }
                Ok(false) => {
                    eprintln!("=======================================================");
                    eprintln!(
                        " [REJECTED] External asset attestation signature verification failed"
                    );
                    eprintln!("=======================================================");
                }
                Err(e) => {
                    eprintln!("=======================================================");
                    eprintln!(" [REJECTED] Attestation verification failed: {:?}", e);
                    eprintln!("=======================================================");
                }
            }
        }
        "show" => {
            let token_id = get_arg_value_slice(args, "--token-id");
            let canonical_id = get_arg_value_slice(args, "--canonical-id");

            let param = if let Some(t_id) = token_id {
                if let Ok(num) = t_id.parse::<u64>() {
                    JsonValue::Integer(num as i64)
                } else {
                    JsonValue::String(t_id)
                }
            } else if let Some(c_id) = canonical_id {
                JsonValue::String(c_id)
            } else {
                eprintln!(
                    "Usage: tola-cli external-asset show [--token-id <ID> | --canonical-id <HEX>]"
                );
                return;
            };

            match rpc_call(rpc_url, "tola_getExternalAsset", vec![param]) {
                Ok(val) => {
                    if matches!(val, JsonValue::Null) {
                        println!("External asset not found.");
                        return;
                    }
                    println!("=======================================================");
                    println!("           TOKENIZED EXTERNAL ASSET RECORD             ");
                    println!("=======================================================");
                    if let Some(t_id) = val.get("tokenId").and_then(|v| v.as_u64()) {
                        println!("Token ID:            {}", t_id);
                    }
                    if let Some(c_id) = val.get("canonicalId").and_then(|v| v.as_str()) {
                        println!("Canonical ID:        {}", c_id);
                    }
                    if let Some(p) = val.get("sourcePlatform").and_then(|v| v.as_str()) {
                        println!("Source Platform:     {}", p);
                    }
                    if let Some(t) = val.get("assetType").and_then(|v| v.as_str()) {
                        println!("Asset Type:          {}", t);
                    }
                    if let Some(id) = val.get("externalAssetId").and_then(|v| v.as_str()) {
                        println!("External Asset ID:   {}", id);
                    }
                    if let Some(owner) = val.get("owner").and_then(|v| v.as_str()) {
                        println!("TOLA Owner:          {}", owner);
                    }
                    if let Some(auth) = val
                        .get("authorizer")
                        .or_else(|| val.get("verifierAddress"))
                        .and_then(|v| v.as_str())
                    {
                        println!("Platform Authorizer: {}", auth);
                    }
                    if let Some(uri) = val.get("metadataUri").and_then(|v| v.as_str()) {
                        println!("Metadata URI:        {}", uri);
                    }
                    if let Some(ts) = val.get("mintTimestamp").and_then(|v| v.as_u64()) {
                        println!("Minted At:           {}", ts);
                    }
                    println!("=======================================================");
                }
                Err(e) => {
                    eprintln!("[ERROR] Query external asset failed: {}", e);
                }
            }
        }
        "canonical-id" => {
            let platform = match get_arg_value_slice(args, "--platform") {
                Some(p) => p,
                None => {
                    eprintln!("Usage: tola-cli external-asset canonical-id --platform <STR> --type <TYPE> --id <STR>");
                    return;
                }
            };
            let type_str = match get_arg_value_slice(args, "--type") {
                Some(t) => t,
                None => {
                    eprintln!("Missing --type <TYPE>");
                    return;
                }
            };
            let asset_id = match get_arg_value_slice(args, "--id") {
                Some(id) => id,
                None => {
                    eprintln!("Missing --id <STR>");
                    return;
                }
            };

            let asset_type = match parse_asset_type_cli(&type_str) {
                Some(t) => t,
                None => {
                    eprintln!("Invalid --type: {}", type_str);
                    return;
                }
            };

            let cid =
                tola_crypto::compute_external_asset_canonical_id(&platform, asset_type, &asset_id);
            println!("Canonical ID: 0x{}", hex_encode(cid));
        }
        other => {
            eprintln!(
                "Unknown external-asset subcommand: {}. Use 'verify-attestation', 'show', or 'canonical-id'",
                other
            );
        }
    }
}
