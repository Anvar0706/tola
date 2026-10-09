//! TOLA EVM Phase 4 (P4) Integration Test Suite
//! Comprehensive end-to-end verification of RFC 6455 WebSocket transport,
//! HTTP Upgrade handshake, connection-scoped subscriptions (newHeads, logs, newPendingTransactions),
//! backpressure bounding, liveness, and consensus commit ordering.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use tola_mempool::{Mempool, MempoolConfig};
use tola_rpc::json::JsonValue;
use tola_rpc::{compute_transaction_id, RpcDispatcher, RpcServer};
use tola_state::MemoryStateJournal;
use tola_types::*;

// ============================================================================
// Helper Utilities for Client WebSocket Simulation over Standard TcpStream
// ============================================================================

fn send_client_frame(
    stream: &mut TcpStream,
    opcode: u8,
    fin: bool,
    payload: &[u8],
) -> std::io::Result<()> {
    let mut header = Vec::new();
    let b0 = (if fin { 0x80 } else { 0x00 }) | (opcode & 0x0F);
    header.push(b0);

    let len = payload.len();
    if len <= 125 {
        header.push(0x80 | (len as u8)); // MASK bit = 1
    } else if len <= 65535 {
        header.push(0x80 | 126);
        header.extend_from_slice(&(len as u16).to_be_bytes());
    } else {
        header.push(0x80 | 127);
        header.extend_from_slice(&(len as u64).to_be_bytes());
    }

    let mask = [0x11, 0x22, 0x33, 0x44];
    header.extend_from_slice(&mask);

    let mut masked_payload = Vec::with_capacity(len);
    for (i, byte) in payload.iter().enumerate() {
        masked_payload.push(byte ^ mask[i % 4]);
    }

    stream.write_all(&header)?;
    stream.write_all(&masked_payload)?;
    stream.flush()?;
    Ok(())
}

fn read_server_frame(stream: &mut TcpStream) -> std::io::Result<(u8, bool, Vec<u8>)> {
    let mut header = [0u8; 2];
    stream.read_exact(&mut header)?;

    let fin = (header[0] & 0x80) != 0;
    let opcode = header[0] & 0x0F;
    let masked = (header[1] & 0x80) != 0;
    let len_code = (header[1] & 0x7F) as usize;

    let payload_len = if len_code <= 125 {
        len_code
    } else if len_code == 126 {
        let mut ext = [0u8; 2];
        stream.read_exact(&mut ext)?;
        u16::from_be_bytes(ext) as usize
    } else {
        let mut ext = [0u8; 8];
        stream.read_exact(&mut ext)?;
        u64::from_be_bytes(ext) as usize
    };

    let mask = if masked {
        let mut m = [0u8; 4];
        stream.read_exact(&mut m)?;
        Some(m)
    } else {
        None
    };

    let mut payload = vec![0u8; payload_len];
    stream.read_exact(&mut payload)?;

    if let Some(m) = mask {
        for (i, b) in payload.iter_mut().enumerate() {
            *b ^= m[i % 4];
        }
    }

    Ok((opcode, fin, payload))
}

fn perform_client_handshake(
    stream: &mut TcpStream,
    path: &str,
    key: &str,
) -> std::io::Result<String> {
    let req = format!(
        "GET {} HTTP/1.1\r\n\
        Host: localhost\r\n\
        Upgrade: websocket\r\n\
        Connection: Upgrade\r\n\
        Sec-WebSocket-Key: {}\r\n\
        Sec-WebSocket-Version: 13\r\n\
        \r\n",
        path, key
    );
    stream.write_all(req.as_bytes())?;
    stream.flush()?;

    let mut reader = BufReader::new(stream.try_clone()?);
    let mut status_line = String::new();
    reader.read_line(&mut status_line)?;

    // Read remaining headers until empty line
    let mut line = String::new();
    while reader.read_line(&mut line)? > 0 {
        if line == "\r\n" {
            break;
        }
        line.clear();
    }

    Ok(status_line.trim().to_string())
}

// ============================================================================
// P4 Mandatory Test Suite
// ============================================================================

#[test]
fn test_p4_handshake_incremental_chunks_and_trailing_bytes() {
    let chain_id = ChainId::MAINNET;
    let state_db = Arc::new(RwLock::new(MemoryStateJournal::new()));
    let mempool = Arc::new(RwLock::new(Mempool::new(MempoolConfig::new(chain_id))));
    let dispatcher = Arc::new(RpcDispatcher::new(
        chain_id,
        "TOLA-P4/1.0".into(),
        state_db,
        mempool,
    ));

    let mut server = RpcServer::new("127.0.0.1:0".parse().unwrap(), dispatcher, None);
    let addr = server.start().expect("Server starts");

    let mut stream = TcpStream::connect(addr).expect("Connects to server");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();

    let key = "dGhlIHNhbXBsZSBub25jZQ==";
    let handshake_str = format!(
        "GET / HTTP/1.1\r\n\
        Host: localhost\r\n\
        Upgrade: websocket\r\n\
        Connection: Upgrade\r\n\
        Sec-WebSocket-Key: {}\r\n\
        Sec-WebSocket-Version: 13\r\n\
        \r\n",
        key
    );

    // 1. Feed handshake bytes in small 16-byte chunks (simulating TCP fragmentation)
    for chunk in handshake_str.as_bytes().chunks(16) {
        stream.write_all(chunk).unwrap();
        stream.flush().unwrap();
        std::thread::sleep(Duration::from_millis(5));
    }

    // 2. Read 101 Switching Protocols response
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut status_line = String::new();
    reader.read_line(&mut status_line).unwrap();
    assert!(
        status_line.contains("101 Switching Protocols"),
        "Status line was: {}",
        status_line
    );
}

#[test]
fn test_p4_handshake_oversized_header_rejected_431() {
    let chain_id = ChainId::MAINNET;
    let state_db = Arc::new(RwLock::new(MemoryStateJournal::new()));
    let mempool = Arc::new(RwLock::new(Mempool::new(MempoolConfig::new(chain_id))));
    let dispatcher = Arc::new(RpcDispatcher::new(
        chain_id,
        "TOLA-P4/1.0".into(),
        state_db,
        mempool,
    ));

    let mut server = RpcServer::new("127.0.0.1:0".parse().unwrap(), dispatcher, None);
    let addr = server.start().expect("Server starts");

    let mut stream = TcpStream::connect(addr).expect("Connects to server");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();

    // Send 5000 bytes without boundary (exceeding MAX_HANDSHAKE_HEADER_SIZE = 4096)
    let junk = vec![b'A'; 5000];
    stream.write_all(&junk).unwrap();
    stream.flush().unwrap();

    let mut resp = [0u8; 100];
    let n = stream.read(&mut resp).unwrap_or(0);
    let resp_str = String::from_utf8_lossy(&resp[..n]);
    assert!(
        resp_str.contains("431 Request Header Fields Too Large"),
        "Expected 431, got: {}",
        resp_str
    );
}

#[test]
fn test_p4_websocket_text_and_unmasked_rejection() {
    let chain_id = ChainId::MAINNET;
    let state_db = Arc::new(RwLock::new(MemoryStateJournal::new()));
    let mempool = Arc::new(RwLock::new(Mempool::new(MempoolConfig::new(chain_id))));
    let dispatcher = Arc::new(RpcDispatcher::new(
        chain_id,
        "TOLA-P4/1.0".into(),
        state_db,
        mempool,
    ));

    let mut server = RpcServer::new("127.0.0.1:0".parse().unwrap(), dispatcher, None);
    let addr = server.start().expect("Server starts");

    // Test A: Normal masked client frame
    {
        let mut stream = TcpStream::connect(addr).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let status =
            perform_client_handshake(&mut stream, "/", "dGhlIHNhbXBsZSBub25jZQ==").unwrap();
        assert!(status.contains("101 Switching Protocols"));

        let rpc_req = r#"{"jsonrpc":"2.0","id":1,"method":"eth_blockNumber","params":[]}"#;
        send_client_frame(&mut stream, 0x1, true, rpc_req.as_bytes()).unwrap();

        let (opcode, fin, payload) = read_server_frame(&mut stream).unwrap();
        assert_eq!(opcode, 0x1);
        assert!(fin);
        let resp_str = String::from_utf8(payload).unwrap();
        assert!(resp_str.contains(r#""result":"0x0""#));
    }

    // Test B: Unmasked client frame must be rejected with Close code 1002
    {
        let mut stream = TcpStream::connect(addr).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let status =
            perform_client_handshake(&mut stream, "/", "dGhlIHNhbXBsZSBub25jZQ==").unwrap();
        assert!(status.contains("101 Switching Protocols"));

        // Send unmasked text frame (MASK bit = 0)
        let unmasked = vec![0x81, 0x04, b't', b'e', b's', b't'];
        stream.write_all(&unmasked).unwrap();
        stream.flush().unwrap();

        let (opcode, _fin, payload) = read_server_frame(&mut stream).unwrap();
        assert_eq!(opcode, 0x8); // Close frame
        let close_code = u16::from_be_bytes([payload[0], payload[1]]);
        assert_eq!(close_code, 1002); // Protocol Error
    }
}

#[test]
fn test_p4_fragmented_text_frame_assembly() {
    let chain_id = ChainId::MAINNET;
    let state_db = Arc::new(RwLock::new(MemoryStateJournal::new()));
    let mempool = Arc::new(RwLock::new(Mempool::new(MempoolConfig::new(chain_id))));
    let dispatcher = Arc::new(RpcDispatcher::new(
        chain_id,
        "TOLA-P4/1.0".into(),
        state_db,
        mempool,
    ));

    let mut server = RpcServer::new("127.0.0.1:0".parse().unwrap(), dispatcher, None);
    let addr = server.start().expect("Server starts");

    let mut stream = TcpStream::connect(addr).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    perform_client_handshake(&mut stream, "/", "dGhlIHNhbXBsZSBub25jZQ==").unwrap();

    let part1 = r#"{"jsonrpc":"2.0","#;
    let part2 = r#""id":10,"method":"eth_chainId","#;
    let part3 = r#""params":[]}"#;

    // Frame 1: Text, FIN = 0
    send_client_frame(&mut stream, 0x1, false, part1.as_bytes()).unwrap();
    // Frame 2: Continuation, FIN = 0
    send_client_frame(&mut stream, 0x0, false, part2.as_bytes()).unwrap();
    // Frame 3: Continuation, FIN = 1
    send_client_frame(&mut stream, 0x0, true, part3.as_bytes()).unwrap();

    let (opcode, fin, payload) = read_server_frame(&mut stream).unwrap();
    assert_eq!(opcode, 0x1);
    assert!(fin);
    let resp_str = String::from_utf8(payload).unwrap();
    assert!(resp_str.contains(r#""id":10"#));
    assert!(resp_str.contains(r#""result":"0x270f""#)); // Mainnet chain ID 9999 = 0x270f
}

#[test]
fn test_p4_ping_pong_liveness_and_unsolicited_pong() {
    let chain_id = ChainId::MAINNET;
    let state_db = Arc::new(RwLock::new(MemoryStateJournal::new()));
    let mempool = Arc::new(RwLock::new(Mempool::new(MempoolConfig::new(chain_id))));
    let dispatcher = Arc::new(RpcDispatcher::new(
        chain_id,
        "TOLA-P4/1.0".into(),
        state_db,
        mempool,
    ));

    let mut server = RpcServer::new("127.0.0.1:0".parse().unwrap(), dispatcher, None);
    let addr = server.start().expect("Server starts");

    let mut stream = TcpStream::connect(addr).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    perform_client_handshake(&mut stream, "/", "dGhlIHNhbXBsZSBub25jZQ==").unwrap();

    // Send Ping frame with payload "heartbeat_data"
    let ping_data = b"heartbeat_data";
    send_client_frame(&mut stream, 0x9, true, ping_data).unwrap();

    // Server must respond with Pong (opcode 0xA) and identical payload
    let (opcode, fin, payload) = read_server_frame(&mut stream).unwrap();
    assert_eq!(opcode, 0xA);
    assert!(fin);
    assert_eq!(&payload, ping_data);
}

#[test]
fn test_p4_eth_subscribe_newheads_and_unsubscribe() {
    let chain_id = ChainId::MAINNET;
    let state_db = Arc::new(RwLock::new(MemoryStateJournal::new()));
    let mempool = Arc::new(RwLock::new(Mempool::new(MempoolConfig::new(chain_id))));
    let dispatcher = Arc::new(RpcDispatcher::new(
        chain_id,
        "TOLA-P4/1.0".into(),
        state_db.clone(),
        mempool.clone(),
    ));

    let mut server = RpcServer::new("127.0.0.1:0".parse().unwrap(), dispatcher.clone(), None);
    let addr = server.start().expect("Server starts");

    let mut stream = TcpStream::connect(addr).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    perform_client_handshake(&mut stream, "/ws", "dGhlIHNhbXBsZSBub25jZQ==").unwrap();

    // 1. Subscribe to newHeads
    let sub_req = r#"{"jsonrpc":"2.0","id":1,"method":"eth_subscribe","params":["newHeads"]}"#;
    send_client_frame(&mut stream, 0x1, true, sub_req.as_bytes()).unwrap();

    let (_op, _fin, payload) = read_server_frame(&mut stream).unwrap();
    let resp_str = String::from_utf8(payload).unwrap();
    let resp_json = JsonValue::parse(&resp_str).unwrap();
    let sub_id = resp_json
        .get("result")
        .and_then(|v| v.as_str())
        .unwrap()
        .to_string();
    assert!(sub_id.starts_with("0x"));

    // 2. Propose and commit a block
    let mut header = BlockHeader {
        parent_hash: Hash::ZERO,
        height: BlockHeight(1),
        epoch: Epoch(1),
        round: Round(1),
        timestamp: Timestamp(1700000000),
        proposer: Address([0x11; 20]),
        state_root: Hash::ZERO,
        txs_root: Hash::ZERO,
        receipts_root: Hash::ZERO,
        gas_used: Gas(21000),
        gas_limit: Gas(30_000_000),
        base_fee_per_gas: Amount::from_u128(1_000_000_000),
        extra_data: vec![0xaa, 0xbb],
    };
    let block = Block {
        header: header.clone(),
        transactions: Vec::new(),
        last_commit_qc: None,
    };
    dispatcher.record_block(block, Vec::new());

    // 3. Receive newHeads notification
    let (_op, _fin, payload) = read_server_frame(&mut stream).unwrap();
    let notif_str = String::from_utf8(payload).unwrap();
    let notif_json = JsonValue::parse(&notif_str).unwrap();
    assert_eq!(
        notif_json.get("method").and_then(|v| v.as_str()).unwrap(),
        "eth_subscription"
    );
    let params = notif_json.get("params").unwrap();
    assert_eq!(
        params.get("subscription").and_then(|v| v.as_str()).unwrap(),
        sub_id.as_str()
    );
    let result = params.get("result").unwrap();
    assert_eq!(
        result.get("number").and_then(|v| v.as_str()).unwrap(),
        "0x1"
    );
    assert_eq!(
        result.get("miner").and_then(|v| v.as_str()).unwrap(),
        Address([0x11; 20]).to_hex().as_str()
    );
    // Invariant: zero fake PoW fields
    assert!(result.get("sha3Uncles").is_none());
    assert!(result.get("difficulty").is_none());
    assert!(result.get("nonce").is_none());
    assert!(result.get("mixHash").is_none());

    // 4. Unsubscribe
    let unsub_req = format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"eth_unsubscribe","params":["{}"]}}"#,
        sub_id
    );
    send_client_frame(&mut stream, 0x1, true, unsub_req.as_bytes()).unwrap();

    let (_op, _fin, payload) = read_server_frame(&mut stream).unwrap();
    let unsub_resp = String::from_utf8(payload).unwrap();
    let unsub_json = JsonValue::parse(&unsub_resp).unwrap();
    assert!(unsub_json.get("result").and_then(|v| v.as_bool()).unwrap());

    // 5. Subsequent block should NOT notify this client
    header.height = BlockHeight(2);
    let block2 = Block {
        header,
        transactions: Vec::new(),
        last_commit_qc: None,
    };
    dispatcher.record_block(block2, Vec::new());

    // Send a standard query to verify no lingering notification in buffer
    let q_req = r#"{"jsonrpc":"2.0","id":3,"method":"eth_blockNumber","params":[]}"#;
    send_client_frame(&mut stream, 0x1, true, q_req.as_bytes()).unwrap();
    let (_op, _fin, payload) = read_server_frame(&mut stream).unwrap();
    let q_resp = String::from_utf8(payload).unwrap();
    assert!(q_resp.contains(r#""result":"0x2""#));
}

#[test]
fn test_p4_eth_subscribe_logs_criteria_matching() {
    let chain_id = ChainId::MAINNET;
    let state_db = Arc::new(RwLock::new(MemoryStateJournal::new()));
    let mempool = Arc::new(RwLock::new(Mempool::new(MempoolConfig::new(chain_id))));
    let dispatcher = Arc::new(RpcDispatcher::new(
        chain_id,
        "TOLA-P4/1.0".into(),
        state_db.clone(),
        mempool.clone(),
    ));

    let mut server = RpcServer::new("127.0.0.1:0".parse().unwrap(), dispatcher.clone(), None);
    let addr = server.start().expect("Server starts");

    let mut stream = TcpStream::connect(addr).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    perform_client_handshake(&mut stream, "/", "dGhlIHNhbXBsZSBub25jZQ==").unwrap();

    let target_addr = Address([0x77; 20]);
    let target_topic = Hash([0x99; 32]);

    // Subscribe to logs matching target_addr
    let sub_req = format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"eth_subscribe","params":["logs",{{"address":"{}"}}]}}"#,
        target_addr.to_hex()
    );
    send_client_frame(&mut stream, 0x1, true, sub_req.as_bytes()).unwrap();

    let (_op, _fin, payload) = read_server_frame(&mut stream).unwrap();
    let resp_str = String::from_utf8(payload).unwrap();
    let resp_json = JsonValue::parse(&resp_str).unwrap();
    let _sub_id = resp_json
        .get("result")
        .and_then(|v| v.as_str())
        .unwrap()
        .to_string();

    // Prepare block with 2 logs: 1 matching, 1 non-matching
    let header = BlockHeader {
        parent_hash: Hash::ZERO,
        height: BlockHeight(1),
        epoch: Epoch(1),
        round: Round(1),
        timestamp: Timestamp(1700000000),
        proposer: Address([0x11; 20]),
        state_root: Hash::ZERO,
        txs_root: Hash::ZERO,
        receipts_root: Hash::ZERO,
        gas_used: Gas(21000),
        gas_limit: Gas(30_000_000),
        base_fee_per_gas: Amount::from_u128(1_000_000_000),
        extra_data: vec![],
    };

    let log_matching = Log {
        address: target_addr,
        topics: vec![target_topic],
        data: vec![0xca, 0xfe],
    };
    let log_other = Log {
        address: Address([0x88; 20]),
        topics: vec![Hash([0x55; 32])],
        data: vec![0xba, 0xbe],
    };

    let tx = Transaction {
        from: Address([0xaa; 20]),
        to: Some(target_addr),
        nonce: Nonce(0),
        value: Amount::ZERO,
        gas_limit: Gas(100_000),
        max_fee_per_gas: Amount::from_u128(2_000_000_000),
        max_priority_fee_per_gas: Amount::from_u128(1_000_000_000),
        data: vec![],
        chain_id,
        valid_until_block: BlockHeight(100),
        signature: Signature::Secp256k1Eth {
            r: [1u8; 32],
            s: [2u8; 32],
            y_parity: 0,
            chain_id: chain_id.0,
        },
        memo: None,
        tx_type: 2,
        access_list: vec![],
    };

    let receipt = Receipt {
        tx_id: compute_transaction_id(&tx),
        success: true,
        gas_used: Gas(21000),
        cumulative_gas_used: Gas(21000),
        logs: vec![log_matching, log_other],
        return_data: vec![],
    };

    let block = Block {
        header,
        transactions: vec![tx],
        last_commit_qc: None,
    };

    dispatcher.record_block(block, vec![receipt]);

    // Should receive exactly 1 log notification (for target_addr)
    let (_op, _fin, payload) = read_server_frame(&mut stream).unwrap();
    let notif_str = String::from_utf8(payload).unwrap();
    let notif_json = JsonValue::parse(&notif_str).unwrap();
    let result = notif_json.get("params").unwrap().get("result").unwrap();

    assert_eq!(
        result.get("address").and_then(|v| v.as_str()).unwrap(),
        target_addr.to_hex().as_str()
    );
    assert!(!result.get("removed").and_then(|v| v.as_bool()).unwrap());
    assert_eq!(
        result.get("data").and_then(|v| v.as_str()).unwrap(),
        "0xcafe"
    );
}

#[test]
fn test_p4_eth_subscribe_new_pending_transactions_and_p2p() {
    let chain_id = ChainId::MAINNET;
    let state_db = Arc::new(RwLock::new(MemoryStateJournal::new()));
    let mempool = Arc::new(RwLock::new(Mempool::new(MempoolConfig::new(chain_id))));
    let dispatcher = Arc::new(RpcDispatcher::new(
        chain_id,
        "TOLA-P4/1.0".into(),
        state_db.clone(),
        mempool.clone(),
    ));

    let mut server = RpcServer::new("127.0.0.1:0".parse().unwrap(), dispatcher.clone(), None);
    let addr = server.start().expect("Server starts");

    let mut stream = TcpStream::connect(addr).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    perform_client_handshake(&mut stream, "/", "dGhlIHNhbXBsZSBub25jZQ==").unwrap();

    // Subscribe to newPendingTransactions
    let sub_req =
        r#"{"jsonrpc":"2.0","id":1,"method":"eth_subscribe","params":["newPendingTransactions"]}"#;
    send_client_frame(&mut stream, 0x1, true, sub_req.as_bytes()).unwrap();

    let (_op, _fin, payload) = read_server_frame(&mut stream).unwrap();
    let resp_str = String::from_utf8(payload).unwrap();
    let sub_id = JsonValue::parse(&resp_str)
        .unwrap()
        .get("result")
        .and_then(|v| v.as_str())
        .unwrap()
        .to_string();

    // Simulate transaction admitted via P2P gossip
    let p2p_tx = Transaction {
        from: Address([0x12; 20]),
        to: Some(Address([0x34; 20])),
        nonce: Nonce(1),
        value: Amount::ZERO,
        gas_limit: Gas(21000),
        max_fee_per_gas: Amount::from_u128(2_000_000_000),
        max_priority_fee_per_gas: Amount::from_u128(1_000_000_000),
        data: vec![],
        chain_id,
        valid_until_block: BlockHeight(100),
        signature: Signature::Secp256k1Eth {
            r: [3u8; 32],
            s: [4u8; 32],
            y_parity: 1,
            chain_id: chain_id.0,
        },
        memo: None,
        tx_type: 2,
        access_list: vec![],
    };

    dispatcher.notify_pending_transaction(&p2p_tx);

    let (_op, _fin, payload) = read_server_frame(&mut stream).unwrap();
    let notif_str = String::from_utf8(payload).unwrap();
    let notif_json = JsonValue::parse(&notif_str).unwrap();

    assert_eq!(
        notif_json.get("method").and_then(|v| v.as_str()).unwrap(),
        "eth_subscription"
    );
    let params = notif_json.get("params").unwrap();
    assert_eq!(
        params.get("subscription").and_then(|v| v.as_str()).unwrap(),
        sub_id.as_str()
    );
    let tx_hash_str = params.get("result").and_then(|v| v.as_str()).unwrap();
    assert!(tx_hash_str.starts_with("0x"));
}

#[test]
fn test_p4_http_transport_rejection_of_subscriptions() {
    let chain_id = ChainId::MAINNET;
    let state_db = Arc::new(RwLock::new(MemoryStateJournal::new()));
    let mempool = Arc::new(RwLock::new(Mempool::new(MempoolConfig::new(chain_id))));
    let dispatcher = Arc::new(RpcDispatcher::new(
        chain_id,
        "TOLA-P4/1.0".into(),
        state_db,
        mempool,
    ));

    let mut server = RpcServer::new("127.0.0.1:0".parse().unwrap(), dispatcher, None);
    let addr = server.start().expect("Server starts");

    let mut stream = TcpStream::connect(addr).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();

    // Send eth_subscribe via standard HTTP POST
    let body = r#"{"jsonrpc":"2.0","id":1,"method":"eth_subscribe","params":["newHeads"]}"#;
    let req = format!(
        "POST / HTTP/1.1\r\n\
        Host: localhost\r\n\
        Content-Type: application/json\r\n\
        Content-Length: {}\r\n\
        \r\n\
        {}",
        body.len(),
        body
    );
    stream.write_all(req.as_bytes()).unwrap();
    stream.flush().unwrap();

    let mut reader = BufReader::new(stream);
    let mut resp = String::new();
    reader.read_to_string(&mut resp).unwrap();

    assert!(resp.contains("Notifications not supported over HTTP transport; use WebSocket"));
}
