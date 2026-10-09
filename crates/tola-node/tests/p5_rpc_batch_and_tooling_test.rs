//! TOLA EVM Phase 5 (P5) Integration Test Suite
//! Comprehensive end-to-end verification of:
//! 1. JSON-RPC 2.0 batch requests (valid, mixed, empty, single, oversized, websocket).
//! 2. HTTP Content-Length & body-size protection (MAX_HTTP_BODY_SIZE = 5 MiB, HTTP 413, no chunked).
//! 3. Simulation block-context fidelity (block.timestamp, block.basefee, block.coinbase from canonical header).
//! 4. Tooling RPCs: eth_coinbase, eth_mining, eth_hashrate.
//! 5. Transaction type preservation: tx_type 0 vs 2, receipt parity, fee equality preservation.
//! 6. Type-2 "accessList": [].
//! 7. Canonical block-tag parsing: "safe", "finalized", "latest", decimal strings, hex.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::atomic::Ordering;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use tola_codec::{CanonicalDeserialize, CanonicalSerialize};
use tola_crypto::{blake3_hash, Secp256k1Keypair};
use tola_mempool::{Mempool, MempoolConfig};
use tola_rpc::dispatcher::RpcDispatcher;
use tola_rpc::json::JsonValue;
use tola_rpc::{compute_transaction_id, RpcServer};
use tola_state::MemoryStateJournal;
use tola_storage::PersistentStateDB;
use tola_types::*;

const TEST_CHAIN_ID: ChainId = ChainId(9999);

fn setup_test_dispatcher() -> Arc<RpcDispatcher<MemoryStateJournal>> {
    let state_db = Arc::new(RwLock::new(MemoryStateJournal::new()));
    let mempool = Arc::new(RwLock::new(Mempool::new(MempoolConfig::new(TEST_CHAIN_ID))));
    Arc::new(RpcDispatcher::new(
        TEST_CHAIN_ID,
        "TOLA-P5/1.0".into(),
        state_db,
        mempool,
    ))
}

fn hex_decode(s: &str) -> Vec<u8> {
    let clean = s.strip_prefix("0x").unwrap_or(s);
    let mut bytes = Vec::with_capacity(clean.len() / 2);
    let mut chars = clean.chars();
    while let (Some(h), Some(l)) = (chars.next(), chars.next()) {
        let hi = h.to_digit(16).expect("valid hex digit") as u8;
        let lo = l.to_digit(16).expect("valid hex digit") as u8;
        bytes.push((hi << 4) | lo);
    }
    bytes
}

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
        header.push(0x80 | (len as u8));
    } else if len <= 65535 {
        header.push(0x80 | 126);
        header.extend_from_slice(&(len as u16).to_be_bytes());
    } else {
        header.push(0x80 | 127);
        header.extend_from_slice(&(len as u64).to_be_bytes());
    }

    let mask = [0x55, 0x66, 0x77, 0x88];
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
         Host: 127.0.0.1\r\n\
         Upgrade: websocket\r\n\
         Connection: Upgrade\r\n\
         Sec-WebSocket-Key: {}\r\n\
         Sec-WebSocket-Version: 13\r\n\r\n",
        path, key
    );
    stream.write_all(req.as_bytes())?;
    stream.flush()?;

    let mut buf = [0u8; 2048];
    let n = stream.read(&mut buf)?;
    Ok(String::from_utf8_lossy(&buf[..n]).to_string())
}

// ============================================================================
// 1. JSON-RPC Batch Requests
// ============================================================================

#[test]
fn test_p5_batch_requests_comprehensive() {
    let disp = setup_test_dispatcher();

    // 1. Normal batch (3 valid calls: eth_blockNumber, eth_chainId, net_version)
    let req_normal = r#"[
        {"jsonrpc":"2.0","id":1,"method":"eth_blockNumber","params":[]},
        {"jsonrpc":"2.0","id":2,"method":"eth_chainId","params":[]},
        {"jsonrpc":"2.0","id":3,"method":"net_version","params":[]}
    ]"#;
    let res_normal = disp.dispatch_json(req_normal);
    let val_normal = JsonValue::parse(&res_normal).expect("Parse batch normal response");
    let arr_normal = val_normal.as_array().expect("Batch must return array");
    assert_eq!(arr_normal.len(), 3);
    assert_eq!(arr_normal[0].get("id").unwrap().as_i64(), Some(1));
    assert_eq!(arr_normal[0].get("result").unwrap().as_str(), Some("0x0"));
    assert_eq!(arr_normal[1].get("id").unwrap().as_i64(), Some(2));
    assert_eq!(
        arr_normal[1].get("result").unwrap().as_str(),
        Some("0x270f")
    );
    assert_eq!(arr_normal[2].get("id").unwrap().as_i64(), Some(3));
    assert_eq!(arr_normal[2].get("result").unwrap().as_str(), Some("9999"));

    // 2. Mixed batch (valid call + invalid method + invalid params)
    let req_mixed = r#"[
        {"jsonrpc":"2.0","id":10,"method":"eth_chainId","params":[]},
        {"jsonrpc":"2.0","id":11,"method":"unknown_bogus_method","params":[]},
        {"jsonrpc":"2.0","id":12,"method":"eth_getBlockByNumber","params":["not_a_valid_block_tag"]}
    ]"#;
    let res_mixed = disp.dispatch_json(req_mixed);
    let val_mixed = JsonValue::parse(&res_mixed).expect("Parse batch mixed response");
    let arr_mixed = val_mixed.as_array().expect("Mixed batch returns array");
    assert_eq!(arr_mixed.len(), 3);
    assert!(arr_mixed[0].get("result").is_some());
    assert_eq!(
        arr_mixed[1]
            .get("error")
            .unwrap()
            .get("code")
            .unwrap()
            .as_i64(),
        Some(-32601)
    );
    assert!(arr_mixed[2].get("error").is_some());

    // 3. Empty batch [] -> single error object with code -32600
    let res_empty = disp.dispatch_json("[]");
    let val_empty = JsonValue::parse(&res_empty).expect("Parse empty batch response");
    assert!(val_empty.get("error").is_some());
    assert_eq!(
        val_empty
            .get("error")
            .unwrap()
            .get("code")
            .unwrap()
            .as_i64(),
        Some(-32600)
    );

    // 4. Single-element batch -> returns array of 1
    let req_single = r#"[{"jsonrpc":"2.0","id":42,"method":"eth_chainId","params":[]}]"#;
    let res_single = disp.dispatch_json(req_single);
    let val_single = JsonValue::parse(&res_single).expect("Parse single batch response");
    let arr_single = val_single.as_array().expect("Single batch returns array");
    assert_eq!(arr_single.len(), 1);
    assert_eq!(arr_single[0].get("id").unwrap().as_i64(), Some(42));
    assert_eq!(
        arr_single[0].get("result").unwrap().as_str(),
        Some("0x270f")
    );

    // 5. Oversized batch (>100 calls) -> single error object with code -32600
    let mut oversized_vec = Vec::new();
    for i in 1..=101 {
        oversized_vec.push(format!(
            r#"{{"jsonrpc":"2.0","id":{},"method":"eth_chainId","params":[]}}"#,
            i
        ));
    }
    let req_oversized = format!("[{}]", oversized_vec.join(","));
    let res_oversized = disp.dispatch_json(&req_oversized);
    let val_oversized = JsonValue::parse(&res_oversized).expect("Parse oversized batch response");
    assert!(val_oversized.get("error").is_some());
    assert_eq!(
        val_oversized
            .get("error")
            .unwrap()
            .get("code")
            .unwrap()
            .as_i64(),
        Some(-32600)
    );

    // 6. Batch containing notification (no id) alongside keyed request
    let req_notif = r#"[
        {"jsonrpc":"2.0","method":"eth_blockNumber","params":[]},
        {"jsonrpc":"2.0","id":55,"method":"eth_chainId","params":[]}
    ]"#;
    let res_notif = disp.dispatch_json(req_notif);
    let val_notif = JsonValue::parse(&res_notif).expect("Parse notification batch response");
    let arr_notif = val_notif.as_array().expect("Returns array");
    assert_eq!(arr_notif.len(), 1);
    assert_eq!(arr_notif[0].get("id").unwrap().as_i64(), Some(55));

    // 7. Batch containing invalid JSON element (e.g. non-object)
    let req_invalid_elem = r#"[
        123,
        {"jsonrpc":"2.0","id":77,"method":"eth_chainId","params":[]}
    ]"#;
    let res_invalid_elem = disp.dispatch_json(req_invalid_elem);
    let val_invalid_elem =
        JsonValue::parse(&res_invalid_elem).expect("Parse invalid elem batch response");
    let arr_invalid_elem = val_invalid_elem.as_array().expect("Returns array");
    assert_eq!(arr_invalid_elem.len(), 2);
    assert_eq!(
        arr_invalid_elem[0]
            .get("error")
            .unwrap()
            .get("code")
            .unwrap()
            .as_i64(),
        Some(-32600)
    );
    assert_eq!(arr_invalid_elem[1].get("id").unwrap().as_i64(), Some(77));
}

#[test]
fn test_p5_websocket_batch_request() {
    let disp = setup_test_dispatcher();
    let mut server = RpcServer::new("127.0.0.1:0".parse().unwrap(), disp, None);
    let addr = server.start().expect("Start RpcServer");

    let mut stream = TcpStream::connect(addr).expect("Connect TCP");
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .expect("set timeout");

    let hs_resp =
        perform_client_handshake(&mut stream, "/", "dGhlIHNhbXBsZSBub25jZQ==").expect("Handshake");
    assert!(hs_resp.contains("101 Switching Protocols"));

    // Send batch array over WebSocket text frame
    let batch_req = r#"[
        {"jsonrpc":"2.0","id":101,"method":"eth_chainId","params":[]},
        {"jsonrpc":"2.0","id":102,"method":"net_version","params":[]}
    ]"#;
    send_client_frame(&mut stream, 0x01, true, batch_req.as_bytes()).expect("Send batch frame");

    let (opcode, fin, payload) = read_server_frame(&mut stream).expect("Read server frame");
    assert_eq!(opcode, 0x01);
    assert!(fin);

    let resp_str = String::from_utf8(payload).expect("Valid utf8 response");
    let val = JsonValue::parse(&resp_str).expect("Valid json response");
    let arr = val
        .as_array()
        .expect("WebSocket batch response must be array");
    assert_eq!(arr.len(), 2);
    assert_eq!(arr[0].get("id").unwrap().as_i64(), Some(101));
    assert_eq!(arr[0].get("result").unwrap().as_str(), Some("0x270f"));
    assert_eq!(arr[1].get("id").unwrap().as_i64(), Some(102));
    assert_eq!(arr[1].get("result").unwrap().as_str(), Some("9999"));

    server.shutdown();
}

// ============================================================================
// 2. HTTP Content-Length & Body Size Protection
// ============================================================================

#[test]
fn test_p5_http_content_length_and_body_size_protection() {
    let disp = setup_test_dispatcher();
    let mut server = RpcServer::new("127.0.0.1:0".parse().unwrap(), disp, None);
    let addr = server.start().expect("Start RpcServer");

    // 1. Content-Length = 5 MiB + 1 (5242881) -> rejected with 413
    {
        let mut stream = TcpStream::connect(addr).expect("Connect TCP");
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let req = "POST / HTTP/1.1\r\n\
                   Host: 127.0.0.1\r\n\
                   Content-Type: application/json\r\n\
                   Content-Length: 5242881\r\n\r\n";
        stream.write_all(req.as_bytes()).unwrap();
        stream.flush().unwrap();

        let mut buf = [0u8; 1024];
        let n = stream.read(&mut buf).unwrap();
        let resp = String::from_utf8_lossy(&buf[..n]);
        assert!(
            resp.contains("413 Payload Too Large"),
            "Expected 413, got: {resp}"
        );
    }

    // 2. Content-Length = 100 MB -> immediately rejected with 413 without allocation
    {
        let mut stream = TcpStream::connect(addr).expect("Connect TCP");
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let req = "POST / HTTP/1.1\r\n\
                   Host: 127.0.0.1\r\n\
                   Content-Type: application/json\r\n\
                   Content-Length: 104857600\r\n\r\n";
        stream.write_all(req.as_bytes()).unwrap();
        stream.flush().unwrap();

        let mut buf = [0u8; 1024];
        let n = stream.read(&mut buf).unwrap();
        let resp = String::from_utf8_lossy(&buf[..n]);
        assert!(
            resp.contains("413 Payload Too Large"),
            "Expected 413, got: {resp}"
        );
    }

    // 3. Missing Content-Length -> 411 Length Required
    {
        let mut stream = TcpStream::connect(addr).expect("Connect TCP");
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let req = "POST / HTTP/1.1\r\n\
                   Host: 127.0.0.1\r\n\
                   Content-Type: application/json\r\n\r\n";
        stream.write_all(req.as_bytes()).unwrap();
        stream.flush().unwrap();

        let mut buf = [0u8; 1024];
        let n = stream.read(&mut buf).unwrap();
        let resp = String::from_utf8_lossy(&buf[..n]);
        assert!(
            resp.contains("411 Length Required"),
            "Expected 411, got: {resp}"
        );
    }

    // 4. Invalid Content-Length ("abc") -> 400 Bad Request
    {
        let mut stream = TcpStream::connect(addr).expect("Connect TCP");
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let req = "POST / HTTP/1.1\r\n\
                   Host: 127.0.0.1\r\n\
                   Content-Type: application/json\r\n\
                   Content-Length: abc\r\n\r\n";
        stream.write_all(req.as_bytes()).unwrap();
        stream.flush().unwrap();

        let mut buf = [0u8; 1024];
        let n = stream.read(&mut buf).unwrap();
        let resp = String::from_utf8_lossy(&buf[..n]);
        assert!(
            resp.contains("400 Bad Request"),
            "Expected 400, got: {resp}"
        );
    }

    // 5. Valid body matching Content-Length exactly -> 200 OK
    {
        let mut stream = TcpStream::connect(addr).expect("Connect TCP");
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let body = r#"{"jsonrpc":"2.0","id":1,"method":"eth_chainId","params":[]}"#;
        let req = format!(
            "POST / HTTP/1.1\r\n\
             Host: 127.0.0.1\r\n\
             Content-Type: application/json\r\n\
             Content-Length: {}\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(req.as_bytes()).unwrap();
        stream.flush().unwrap();

        let mut buf = [0u8; 1024];
        let n = stream.read(&mut buf).unwrap();
        let resp = String::from_utf8_lossy(&buf[..n]);
        assert!(resp.contains("200 OK"), "Expected 200 OK, got: {resp}");
        assert!(resp.contains(r#""result":"0x270f""#));
    }

    server.shutdown();
}

// ============================================================================
// 3. Simulation Block-Context Fidelity
// ============================================================================

#[test]
fn test_p5_simulation_block_context_fidelity() {
    let disp = setup_test_dispatcher();

    // EVM bytecode that reads and returns (block.timestamp, block.basefee, block.coinbase):
    // Opcode 0x42: TIMESTAMP -> MSTORE(0, timestamp)
    // Opcode 0x48: BASEFEE   -> MSTORE(0x20, basefee)
    // Opcode 0x41: COINBASE  -> MSTORE(0x40, coinbase)
    // RETURN(0, 0x60) -> returns 96 bytes
    let bytecode: Vec<u8> = vec![
        0x42, 0x60, 0x00, 0x52, // TIMESTAMP; PUSH1 0; MSTORE
        0x48, 0x60, 0x20, 0x52, // BASEFEE;   PUSH1 0x20; MSTORE
        0x41, 0x60, 0x40, 0x52, // COINBASE;  PUSH1 0x40; MSTORE
        0x60, 0x60, 0x60, 0x00, 0xf3, // PUSH1 0x60; PUSH1 0; RETURN
    ];

    let contract_addr = Address([0x77; 20]);
    let code_hash = tola_crypto::keccak256_hash(&bytecode);

    {
        let mut state = disp.state_db.write().unwrap();
        let mut acct = AccountState::new_with_balance(Amount::ZERO);
        acct.code_hash = code_hash;
        acct.flags = FLAG_CONTRACT;
        state.set_account(contract_addr, acct);
        state.set_code(code_hash, bytecode);
    }

    let expected_timestamp = 1_710_000_000u64;
    let expected_base_fee = 2_500_000_000u128; // 2.5 Gwei
    let expected_proposer = Address([0x99; 20]);

    // Commit a block with explicit header context
    let block1 = Block {
        header: BlockHeader {
            parent_hash: Hash::ZERO,
            height: BlockHeight(1),
            epoch: Epoch(0),
            round: Round(1),
            timestamp: Timestamp(expected_timestamp),
            proposer: expected_proposer,
            state_root: Hash::ZERO,
            txs_root: Hash::ZERO,
            receipts_root: Hash::ZERO,
            gas_used: Gas(0),
            gas_limit: Gas(30_000_000),
            base_fee_per_gas: Amount::from_u128(expected_base_fee),
            extra_data: vec![],
        },
        transactions: vec![],
        last_commit_qc: None,
    };
    disp.record_block(block1, vec![]);

    // 1. eth_call simulates against contract
    let req_call = format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"eth_call","params":[{{ "to": "{}" }}, "latest"]}}"#,
        contract_addr.to_hex()
    );
    let res_call = disp.dispatch_json(&req_call);
    let val_call = JsonValue::parse(&res_call).expect("Parse eth_call response");
    assert!(
        val_call.get("result").is_some(),
        "eth_call failed: {res_call}"
    );
    let hex_res = val_call.get("result").unwrap().as_str().unwrap();
    let ret_bytes = hex_decode(hex_res);
    assert_eq!(ret_bytes.len(), 96);

    // Verify block.timestamp (offset 24..32)
    let mut ts_bytes = [0u8; 8];
    ts_bytes.copy_from_slice(&ret_bytes[24..32]);
    let returned_timestamp = u64::from_be_bytes(ts_bytes);
    assert_eq!(
        returned_timestamp, expected_timestamp,
        "Timestamp must match canonical block header timestamp, not wall-clock"
    );

    // Verify block.basefee (offset 48..64)
    let mut fee_bytes = [0u8; 16];
    fee_bytes.copy_from_slice(&ret_bytes[48..64]);
    let returned_base_fee = u128::from_be_bytes(fee_bytes);
    assert_eq!(
        returned_base_fee, expected_base_fee,
        "Base fee must match canonical block header base fee"
    );

    // Verify block.coinbase (lower 20 bytes of third 32-byte word)
    let mut proposer_bytes = [0u8; 20];
    proposer_bytes.copy_from_slice(&ret_bytes[76..96]);
    assert_eq!(
        Address(proposer_bytes),
        expected_proposer,
        "Coinbase must match canonical block proposer"
    );

    // 2. eth_estimateGas executes without failure
    let req_est = format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"eth_estimateGas","params":[{{ "to": "{}" }}]}}"#,
        contract_addr.to_hex()
    );
    let res_est = disp.dispatch_json(&req_est);
    let val_est = JsonValue::parse(&res_est).expect("Parse eth_estimateGas response");
    assert!(val_est.get("result").is_some());
    let est_str = val_est.get("result").unwrap().as_str().unwrap();
    assert!(est_str.starts_with("0x"));

    // 3. Simulation height 0 fallback
    let fresh_disp = setup_test_dispatcher();
    let (h0, t0, b0, p0, g0) = fresh_disp.latest_committed_block_context();
    assert_eq!(h0, BlockHeight(0));
    assert_eq!(t0, Timestamp(0));
    assert_eq!(b0, Amount::from_u128(1_000_000_000));
    assert_eq!(p0, Address::ZERO);
    assert_eq!(g0, Gas(30_000_000));
}

#[test]
fn test_p5_simulation_revert_data_preservation() {
    let disp = setup_test_dispatcher();

    // Reverting contract bytecode with custom payload (0x08c379a0)
    let rev_bytecode = vec![
        0x63, 0x08, 0xc3, 0x79, 0xa0, 0x60, 0x00, 0x52, 0x60, 0x04, 0x60, 0x1c, 0xfd,
    ];
    let rev_hash = tola_crypto::keccak256_hash(&rev_bytecode);
    let rev_addr = Address([0x55; 20]);
    {
        let mut state = disp.state_db.write().unwrap();
        let mut acct = AccountState::new_with_balance(Amount::ZERO);
        acct.code_hash = rev_hash;
        acct.flags = FLAG_CONTRACT;
        state.set_account(rev_addr, acct);
        state.set_code(rev_hash, rev_bytecode);
    }

    let req = format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"eth_call","params":[{{ "to": "{}" }}, "latest"]}}"#,
        rev_addr.to_hex()
    );
    let res = disp.dispatch_json(&req);
    let val = JsonValue::parse(&res).expect("Parse eth_call revert response");
    assert!(val.get("error").is_some());
    let err = val.get("error").unwrap();
    assert_eq!(err.get("code").unwrap().as_i64(), Some(3));
    let data = err.get("data").unwrap().as_str().unwrap();
    assert!(data.starts_with("0x08c379a0"));
}

// ============================================================================
// 4. Tooling RPCs: eth_coinbase, eth_mining, eth_hashrate
// ============================================================================

#[test]
fn test_p5_tooling_rpcs_coinbase_mining_hashrate() {
    let disp = setup_test_dispatcher();

    // 1. eth_coinbase on empty chain returns zero address
    let res_cb0 =
        disp.dispatch_json(r#"{"jsonrpc":"2.0","id":1,"method":"eth_coinbase","params":[]}"#);
    let val_cb0 = JsonValue::parse(&res_cb0).unwrap();
    assert_eq!(
        val_cb0.get("result").unwrap().as_str(),
        Some("0x0000000000000000000000000000000000000000")
    );

    // 2. Commit block with proposer Address([0x88; 20])
    let proposer = Address([0x88; 20]);
    let block = Block {
        header: BlockHeader {
            parent_hash: Hash::ZERO,
            height: BlockHeight(1),
            epoch: Epoch(0),
            round: Round(1),
            timestamp: Timestamp(12345),
            proposer,
            state_root: Hash::ZERO,
            txs_root: Hash::ZERO,
            receipts_root: Hash::ZERO,
            gas_used: Gas(0),
            gas_limit: Gas(30_000_000),
            base_fee_per_gas: Amount::from_u128(1_000_000_000),
            extra_data: vec![],
        },
        transactions: vec![],
        last_commit_qc: None,
    };
    disp.record_block(block, vec![]);

    // eth_coinbase returns latest proposer
    let res_cb1 =
        disp.dispatch_json(r#"{"jsonrpc":"2.0","id":2,"method":"eth_coinbase","params":[]}"#);
    let val_cb1 = JsonValue::parse(&res_cb1).unwrap();
    assert_eq!(
        val_cb1.get("result").unwrap().as_str(),
        Some(proposer.to_hex().as_str())
    );

    // 3. eth_mining returns false
    let res_mining =
        disp.dispatch_json(r#"{"jsonrpc":"2.0","id":3,"method":"eth_mining","params":[]}"#);
    let val_mining = JsonValue::parse(&res_mining).unwrap();
    assert_eq!(val_mining.get("result").unwrap().as_bool(), Some(false));

    // 4. eth_hashrate returns "0x0"
    let res_hashrate =
        disp.dispatch_json(r#"{"jsonrpc":"2.0","id":4,"method":"eth_hashrate","params":[]}"#);
    let val_hashrate = JsonValue::parse(&res_hashrate).unwrap();
    assert_eq!(val_hashrate.get("result").unwrap().as_str(), Some("0x0"));
}

// ============================================================================
// 5. Transaction Type Preservation & Access List
// ============================================================================

#[test]
fn test_p5_transaction_type_preservation_legacy_and_type2() {
    let disp = setup_test_dispatcher();
    let kp = Secp256k1Keypair::generate();
    let sender = kp.address();
    let recipient = Address([0x33; 20]);

    // 1. Ingest a Legacy transaction (Type 0)
    let raw_legacy_rlp = {
        let mut tx = Transaction {
            chain_id: TEST_CHAIN_ID,
            nonce: Nonce(0),
            from: sender,
            to: Some(recipient),
            value: Amount::from_u128(100),
            gas_limit: Gas(21_000),
            max_fee_per_gas: Amount::from_u128(20_000_000_000),
            max_priority_fee_per_gas: Amount::from_u128(20_000_000_000),
            valid_until_block: BlockHeight(100),
            data: vec![],
            memo: None,
            signature: Signature::Ed25519([0u8; 64]),
            tx_type: 0,
            access_list: vec![],
        };
        let hash = tola_codec::rlp::compute_legacy_signing_hash(&tx);
        let sig = kp.sign_recoverable(hash.as_bytes()).unwrap();
        let mut r = [0u8; 32];
        let mut s = [0u8; 32];
        r.copy_from_slice(&sig[0..32]);
        s.copy_from_slice(&sig[32..64]);
        tx.signature = Signature::Secp256k1Eth {
            r,
            s,
            y_parity: sig[64],
            chain_id: TEST_CHAIN_ID.0,
        };
        tola_codec::rlp::encode_raw_eth_transaction(&tx).expect("encode legacy")
    };

    let (tx_legacy, legacy_eth_hash) =
        tola_codec::rlp::decode_raw_eth_transaction(&raw_legacy_rlp).expect("Decode legacy");
    assert_eq!(
        tx_legacy.tx_type, 0,
        "Decoded legacy tx must have tx_type == 0"
    );

    // 2. Ingest a Type-2 transaction (Type 2) where max_fee == max_priority_fee
    let raw_type2_rlp = {
        let mut tx = Transaction {
            chain_id: TEST_CHAIN_ID,
            nonce: Nonce(1),
            from: sender,
            to: Some(recipient),
            value: Amount::from_u128(200),
            gas_limit: Gas(21_000),
            max_fee_per_gas: Amount::from_u128(20_000_000_000),
            max_priority_fee_per_gas: Amount::from_u128(20_000_000_000), // Equal fees!
            valid_until_block: BlockHeight(100),
            data: vec![],
            memo: None,
            signature: Signature::Ed25519([0u8; 64]),
            tx_type: 2,
            access_list: vec![],
        };
        let hash = tola_codec::rlp::compute_type2_signing_hash(&tx);
        let sig = kp.sign_recoverable(hash.as_bytes()).unwrap();
        let mut r = [0u8; 32];
        let mut s = [0u8; 32];
        r.copy_from_slice(&sig[0..32]);
        s.copy_from_slice(&sig[32..64]);
        tx.signature = Signature::Secp256k1Eth {
            r,
            s,
            y_parity: sig[64],
            chain_id: TEST_CHAIN_ID.0,
        };
        tola_codec::rlp::encode_raw_eth_transaction(&tx).expect("encode type2")
    };

    let (tx_type2, type2_eth_hash) =
        tola_codec::rlp::decode_raw_eth_transaction(&raw_type2_rlp).expect("Decode type2");
    assert_eq!(
        tx_type2.tx_type, 2,
        "Decoded type2 tx must have tx_type == 2"
    );

    // Commit both transactions in block 1
    let r0 = Receipt {
        tx_id: compute_transaction_id(&tx_legacy),
        success: true,
        gas_used: Gas(21_000),
        cumulative_gas_used: Gas(21_000),
        logs: vec![],
        return_data: vec![],
    };
    let r1 = Receipt {
        tx_id: compute_transaction_id(&tx_type2),
        success: true,
        gas_used: Gas(21_000),
        cumulative_gas_used: Gas(42_000),
        logs: vec![],
        return_data: vec![],
    };

    let block1 = Block {
        header: BlockHeader {
            parent_hash: Hash::ZERO,
            height: BlockHeight(1),
            epoch: Epoch(0),
            round: Round(1),
            timestamp: Timestamp(1000),
            proposer: Address([0x11; 20]),
            state_root: Hash::ZERO,
            txs_root: Hash::ZERO,
            receipts_root: Hash::ZERO,
            gas_used: Gas(42_000),
            gas_limit: Gas(30_000_000),
            base_fee_per_gas: Amount::from_u128(1_000_000_000),
            extra_data: vec![],
        },
        transactions: vec![tx_legacy.clone(), tx_type2.clone()],
        last_commit_qc: None,
    };
    disp.record_block(block1, vec![r0, r1]);

    // Verify Legacy transaction JSON formatting
    let req_tx_legacy = format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"eth_getTransactionByHash","params":["{}"]}}"#,
        legacy_eth_hash.to_hex()
    );
    let res_tx_legacy = disp.dispatch_json(&req_tx_legacy);
    let val_tx_legacy = JsonValue::parse(&res_tx_legacy).unwrap();
    let obj_legacy = val_tx_legacy.get("result").unwrap();
    assert_eq!(
        obj_legacy.get("type").unwrap().as_str(),
        Some("0x0"),
        "Legacy transaction must have type 0x0"
    );

    // Verify Legacy receipt formatting
    let req_rc_legacy = format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"eth_getTransactionReceipt","params":["{}"]}}"#,
        legacy_eth_hash.to_hex()
    );
    let res_rc_legacy = disp.dispatch_json(&req_rc_legacy);
    let val_rc_legacy = JsonValue::parse(&res_rc_legacy).unwrap();
    let obj_rc_legacy = val_rc_legacy.get("result").unwrap();
    assert_eq!(
        obj_rc_legacy.get("type").unwrap().as_str(),
        Some("0x0"),
        "Legacy receipt must have type 0x0"
    );

    // Verify Type-2 transaction JSON formatting
    let req_tx_type2 = format!(
        r#"{{"jsonrpc":"2.0","id":3,"method":"eth_getTransactionByHash","params":["{}"]}}"#,
        type2_eth_hash.to_hex()
    );
    let res_tx_type2 = disp.dispatch_json(&req_tx_type2);
    let val_tx_type2 = JsonValue::parse(&res_tx_type2).unwrap();
    let obj_type2 = val_tx_type2.get("result").unwrap();
    assert_eq!(
        obj_type2.get("type").unwrap().as_str(),
        Some("0x2"),
        "Type-2 transaction must preserve type 0x2 even when max_fee == max_priority_fee"
    );
    assert_eq!(
        obj_type2
            .get("accessList")
            .unwrap()
            .as_array()
            .map(|a| a.len()),
        Some(0),
        "Type-2 transaction must include accessList: []"
    );

    // Verify Type-2 receipt formatting
    let req_rc_type2 = format!(
        r#"{{"jsonrpc":"2.0","id":4,"method":"eth_getTransactionReceipt","params":["{}"]}}"#,
        type2_eth_hash.to_hex()
    );
    let res_rc_type2 = disp.dispatch_json(&req_rc_type2);
    let val_rc_type2 = JsonValue::parse(&res_rc_type2).unwrap();
    let obj_rc_type2 = val_rc_type2.get("result").unwrap();
    assert_eq!(
        obj_rc_type2.get("type").unwrap().as_str(),
        Some("0x2"),
        "Type-2 receipt must have type 0x2"
    );
}

// ============================================================================
// 6. Canonical Block-Tag Parsing
// ============================================================================

#[test]
fn test_p5_canonical_block_tag_parsing() {
    let disp = setup_test_dispatcher();

    let block1 = Block {
        header: BlockHeader {
            parent_hash: Hash::ZERO,
            height: BlockHeight(1),
            epoch: Epoch(0),
            round: Round(1),
            timestamp: Timestamp(1000),
            proposer: Address([0x11; 20]),
            state_root: Hash::ZERO,
            txs_root: Hash::ZERO,
            receipts_root: Hash::ZERO,
            gas_used: Gas(0),
            gas_limit: Gas(30_000_000),
            base_fee_per_gas: Amount::from_u128(1_000_000_000),
            extra_data: vec![],
        },
        transactions: vec![],
        last_commit_qc: None,
    };
    let block2 = Block {
        header: BlockHeader {
            parent_hash: Hash::ZERO,
            height: BlockHeight(2),
            epoch: Epoch(0),
            round: Round(2),
            timestamp: Timestamp(2000),
            proposer: Address([0x22; 20]),
            state_root: Hash::ZERO,
            txs_root: Hash::ZERO,
            receipts_root: Hash::ZERO,
            gas_used: Gas(0),
            gas_limit: Gas(30_000_000),
            base_fee_per_gas: Amount::from_u128(1_000_000_000),
            extra_data: vec![],
        },
        transactions: vec![],
        last_commit_qc: None,
    };

    disp.record_block(block1, vec![]);
    disp.record_block(block2, vec![]);

    // 1. "latest" tag returns block 2
    let res_latest = disp.dispatch_json(
        r#"{"jsonrpc":"2.0","id":1,"method":"eth_getBlockByNumber","params":["latest", false]}"#,
    );
    let val_latest = JsonValue::parse(&res_latest).unwrap();
    assert_eq!(
        val_latest
            .get("result")
            .unwrap()
            .get("number")
            .unwrap()
            .as_str(),
        Some("0x2")
    );

    // 2. "safe" tag returns latest block
    let res_safe = disp.dispatch_json(
        r#"{"jsonrpc":"2.0","id":2,"method":"eth_getBlockByNumber","params":["safe", false]}"#,
    );
    let val_safe = JsonValue::parse(&res_safe).unwrap();
    assert_eq!(
        val_safe
            .get("result")
            .unwrap()
            .get("number")
            .unwrap()
            .as_str(),
        Some("0x2")
    );

    // 3. "finalized" tag returns latest block
    let res_finalized = disp.dispatch_json(
        r#"{"jsonrpc":"2.0","id":3,"method":"eth_getBlockByNumber","params":["finalized", false]}"#,
    );
    let val_finalized = JsonValue::parse(&res_finalized).unwrap();
    assert_eq!(
        val_finalized
            .get("result")
            .unwrap()
            .get("number")
            .unwrap()
            .as_str(),
        Some("0x2")
    );

    // 4. Decimal string "1" returns block 1
    let res_dec1 = disp.dispatch_json(
        r#"{"jsonrpc":"2.0","id":4,"method":"eth_getBlockByNumber","params":["1", false]}"#,
    );
    let val_dec1 = JsonValue::parse(&res_dec1).unwrap();
    assert_eq!(
        val_dec1
            .get("result")
            .unwrap()
            .get("number")
            .unwrap()
            .as_str(),
        Some("0x1")
    );

    // 5. Hex string "0x1" returns block 1
    let res_hex1 = disp.dispatch_json(
        r#"{"jsonrpc":"2.0","id":5,"method":"eth_getBlockByNumber","params":["0x1", false]}"#,
    );
    let val_hex1 = JsonValue::parse(&res_hex1).unwrap();
    assert_eq!(
        val_hex1
            .get("result")
            .unwrap()
            .get("number")
            .unwrap()
            .as_str(),
        Some("0x1")
    );

    // 6. eth_feeHistory with "safe" and "finalized"
    let res_fee_safe = disp.dispatch_json(
        r#"{"jsonrpc":"2.0","id":6,"method":"eth_feeHistory","params":["0x1", "safe", []]}"#,
    );
    let val_fee_safe = JsonValue::parse(&res_fee_safe).unwrap();
    assert!(val_fee_safe.get("result").is_some());

    let res_fee_fin = disp.dispatch_json(
        r#"{"jsonrpc":"2.0","id":7,"method":"eth_feeHistory","params":["0x1", "finalized", []]}"#,
    );
    let val_fee_fin = JsonValue::parse(&res_fee_fin).unwrap();
    assert!(val_fee_fin.get("result").is_some());
}

// ============================================================================
// 7. P5-BLK-01 Remediation Verification
// ============================================================================

fn make_signed_legacy_tx(
    kp: &Secp256k1Keypair,
    nonce: u64,
    gas_price: u128,
    to: Option<Address>,
    value: u128,
    data: Vec<u8>,
) -> (Transaction, Hash) {
    let mut tx = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(nonce),
        from: kp.address(),
        to,
        value: Amount::from_u128(value),
        gas_limit: Gas(21_000),
        max_fee_per_gas: Amount::from_u128(gas_price),
        max_priority_fee_per_gas: Amount::from_u128(gas_price),
        valid_until_block: BlockHeight(100),
        data,
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 0,
        access_list: vec![],
    };
    let hash = tola_codec::rlp::compute_legacy_signing_hash(&tx);
    let sig = kp.sign_recoverable(hash.as_bytes()).unwrap();
    let mut r = [0u8; 32];
    let mut s = [0u8; 32];
    r.copy_from_slice(&sig[0..32]);
    s.copy_from_slice(&sig[32..64]);
    tx.signature = Signature::Secp256k1Eth {
        r,
        s,
        y_parity: sig[64],
        chain_id: TEST_CHAIN_ID.0,
    };
    let eth_hash = tola_codec::rlp::compute_eth_tx_hash(&tx).expect("compute eth hash legacy");
    (tx, eth_hash)
}

fn make_signed_type2_tx(
    kp: &Secp256k1Keypair,
    nonce: u64,
    max_fee: u128,
    max_priority: u128,
    to: Option<Address>,
    value: u128,
    data: Vec<u8>,
) -> (Transaction, Hash) {
    let mut tx = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(nonce),
        from: kp.address(),
        to,
        value: Amount::from_u128(value),
        gas_limit: Gas(21_000),
        max_fee_per_gas: Amount::from_u128(max_fee),
        max_priority_fee_per_gas: Amount::from_u128(max_priority),
        valid_until_block: BlockHeight(100),
        data,
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let hash = tola_codec::rlp::compute_type2_signing_hash(&tx);
    let sig = kp.sign_recoverable(hash.as_bytes()).unwrap();
    let mut r = [0u8; 32];
    let mut s = [0u8; 32];
    r.copy_from_slice(&sig[0..32]);
    s.copy_from_slice(&sig[32..64]);
    tx.signature = Signature::Secp256k1Eth {
        r,
        s,
        y_parity: sig[64],
        chain_id: TEST_CHAIN_ID.0,
    };
    let eth_hash = tola_codec::rlp::compute_eth_tx_hash(&tx).expect("compute eth hash type2");
    (tx, eth_hash)
}

fn compute_txs_root_for_test(txs: &[Transaction]) -> Hash {
    if txs.is_empty() {
        return Hash::ZERO;
    }
    let mut concat = Vec::with_capacity(txs.len() * 32);
    for tx in txs {
        let digest = blake3_hash(&tx.to_canonical_bytes());
        concat.extend_from_slice(&digest.0);
    }
    blake3_hash(&concat)
}

#[test]
fn test_p5_blk01_codec_and_block_roundtrip() {
    let kp = Secp256k1Keypair::generate();
    let recipient = Address([0x44; 20]);

    // Test 1: Legacy round-trip
    let (tx0, eth_hash0) = make_signed_legacy_tx(
        &kp,
        0,
        20_000_000_000,
        Some(recipient),
        100,
        vec![0xaa, 0xbb],
    );
    assert_eq!(tx0.tx_type, 0);
    let bytes0 = tx0.to_canonical_bytes();
    let decoded0 = Transaction::deserialize_canonical(&bytes0).expect("decode tx0");
    assert_eq!(
        decoded0.tx_type, 0,
        "Legacy tx MUST preserve tx_type == 0 across canonical round-trip"
    );
    assert_eq!(
        compute_transaction_id(&tx0),
        compute_transaction_id(&decoded0)
    );
    assert_eq!(
        Some(eth_hash0),
        tola_codec::rlp::compute_eth_tx_hash(&decoded0),
        "Ethereum tx hash must match before and after"
    );

    // Test 2: Type-2 diff-fee round-trip
    let (tx1, eth_hash1) = make_signed_type2_tx(
        &kp,
        1,
        30_000_000_000,
        2_000_000_000,
        Some(recipient),
        200,
        vec![0xcc],
    );
    assert_eq!(tx1.tx_type, 2);
    let bytes1 = tx1.to_canonical_bytes();
    let decoded1 = Transaction::deserialize_canonical(&bytes1).expect("decode tx1");
    assert_eq!(
        decoded1.tx_type, 2,
        "Type-2 diff-fee tx MUST preserve tx_type == 2 across canonical round-trip"
    );
    assert_eq!(
        compute_transaction_id(&tx1),
        compute_transaction_id(&decoded1)
    );
    assert_eq!(
        Some(eth_hash1),
        tola_codec::rlp::compute_eth_tx_hash(&decoded1)
    );

    // Test 3: Type-2 equal-fee round-trip
    let (tx2, eth_hash2) = make_signed_type2_tx(
        &kp,
        2,
        20_000_000_000,
        20_000_000_000,
        Some(recipient),
        300,
        vec![0xdd],
    );
    assert_eq!(tx2.tx_type, 2);
    let bytes2 = tx2.to_canonical_bytes();
    let decoded2 = Transaction::deserialize_canonical(&bytes2).expect("decode tx2");
    assert_eq!(
        decoded2.tx_type, 2,
        "Type-2 equal-fee tx MUST preserve tx_type == 2 across canonical round-trip"
    );
    assert_eq!(
        compute_transaction_id(&tx2),
        compute_transaction_id(&decoded2)
    );
    assert_eq!(
        Some(eth_hash2),
        tola_codec::rlp::compute_eth_tx_hash(&decoded2)
    );

    // Test 4: Mixed block [0, 2, 2, 0]
    let (tx3, eth_hash3) =
        make_signed_legacy_tx(&kp, 3, 25_000_000_000, Some(recipient), 400, vec![]);
    let txs = vec![tx0.clone(), tx1.clone(), tx2.clone(), tx3.clone()];
    let txs_root = compute_txs_root_for_test(&txs);

    let original_block = Block {
        header: BlockHeader {
            parent_hash: Hash::ZERO,
            height: BlockHeight(42),
            epoch: Epoch(0),
            round: Round(1),
            timestamp: Timestamp(2000),
            proposer: Address([0x11; 20]),
            state_root: Hash::ZERO,
            txs_root,
            receipts_root: Hash::ZERO,
            gas_used: Gas(84_000),
            gas_limit: Gas(30_000_000),
            base_fee_per_gas: Amount::from_u128(1_000_000_000),
            extra_data: vec![],
        },
        transactions: txs,
        last_commit_qc: None,
    };

    let block_bytes = original_block.to_canonical_bytes();
    let decoded_block = Block::deserialize_canonical(&block_bytes).expect("decode block");

    let decoded_types: Vec<u8> = decoded_block
        .transactions
        .iter()
        .map(|t| t.tx_type)
        .collect();
    assert_eq!(
        decoded_types,
        vec![0, 2, 2, 0],
        "Mixed block transactions MUST preserve exact envelope types [0, 2, 2, 0]"
    );

    for (orig, dec) in original_block
        .transactions
        .iter()
        .zip(decoded_block.transactions.iter())
    {
        assert_eq!(compute_transaction_id(orig), compute_transaction_id(dec));
        assert_eq!(
            tola_codec::rlp::compute_eth_tx_hash(orig),
            tola_codec::rlp::compute_eth_tx_hash(dec)
        );
        let orig_raw = tola_codec::rlp::encode_raw_eth_transaction(orig).unwrap();
        let dec_raw = tola_codec::rlp::encode_raw_eth_transaction(dec).unwrap();
        assert_eq!(
            orig_raw, dec_raw,
            "Raw Ethereum wire bytes must match exactly"
        );
    }
    assert_eq!(
        Some(eth_hash3),
        tola_codec::rlp::compute_eth_tx_hash(&decoded_block.transactions[3])
    );

    assert_eq!(
        original_block.header.txs_root, decoded_block.header.txs_root,
        "Block txs_root must remain identical"
    );
}

#[test]
fn test_p5_blk01_persistent_storage_and_rpc_restart() {
    let temp_dir = std::env::temp_dir().join(format!(
        "tola_test_p5_blk01_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&temp_dir).expect("create temp dir");

    let kp = Secp256k1Keypair::generate();
    let recipient = Address([0x55; 20]);

    // Construct 4 transactions: [0, 2, 2, 0]
    let (tx0, eth_hash0) =
        make_signed_legacy_tx(&kp, 0, 10_000_000_000, Some(recipient), 100, vec![]);
    let (tx1, eth_hash1) = make_signed_type2_tx(
        &kp,
        1,
        30_000_000_000,
        2_000_000_000,
        Some(recipient),
        200,
        vec![],
    );
    let (tx2, eth_hash2) = make_signed_type2_tx(
        &kp,
        2,
        20_000_000_000,
        20_000_000_000,
        Some(recipient),
        300,
        vec![],
    ); // equal fees!
    let (tx3, eth_hash3) =
        make_signed_legacy_tx(&kp, 3, 15_000_000_000, Some(recipient), 400, vec![]);

    let txs = vec![tx0.clone(), tx1.clone(), tx2.clone(), tx3.clone()];
    let txs_root = compute_txs_root_for_test(&txs);

    let receipts = vec![
        Receipt {
            tx_id: compute_transaction_id(&tx0),
            success: true,
            gas_used: Gas(21_000),
            cumulative_gas_used: Gas(21_000),
            logs: vec![],
            return_data: vec![],
        },
        Receipt {
            tx_id: compute_transaction_id(&tx1),
            success: true,
            gas_used: Gas(21_000),
            cumulative_gas_used: Gas(42_000),
            logs: vec![],
            return_data: vec![],
        },
        Receipt {
            tx_id: compute_transaction_id(&tx2),
            success: true,
            gas_used: Gas(21_000),
            cumulative_gas_used: Gas(63_000),
            logs: vec![],
            return_data: vec![],
        },
        Receipt {
            tx_id: compute_transaction_id(&tx3),
            success: true,
            gas_used: Gas(21_000),
            cumulative_gas_used: Gas(84_000),
            logs: vec![],
            return_data: vec![],
        },
    ];

    let block = Block {
        header: BlockHeader {
            parent_hash: Hash::ZERO,
            height: BlockHeight(42),
            epoch: Epoch(0),
            round: Round(1),
            timestamp: Timestamp(5000),
            proposer: Address([0x11; 20]),
            state_root: Hash::ZERO,
            txs_root,
            receipts_root: Hash::ZERO,
            gas_used: Gas(84_000),
            gas_limit: Gas(30_000_000),
            base_fee_per_gas: Amount::from_u128(1_000_000_000),
            extra_data: vec![],
        },
        transactions: txs,
        last_commit_qc: None,
    };
    let block_hash = tola_consensus::PipelinedBftEngine::compute_block_hash(&block);

    // 1. Commit block bundle to PersistentStateDB
    {
        let mut state_db = PersistentStateDB::open(&temp_dir).expect("open persistent state_db");
        state_db
            .commit_block_bundle(&block, block_hash, &receipts)
            .expect("commit_block_bundle");
    }

    // 2. Simulate Node Restart: Re-open PersistentStateDB from disk
    let state_db_restarted =
        PersistentStateDB::open(&temp_dir).expect("reopen persistent state_db after restart");
    assert_eq!(
        state_db_restarted.current_height(),
        BlockHeight(42),
        "Persistent state_db must restore current height"
    );

    // 3. Inspect block loaded directly from disk
    let loaded_block = state_db_restarted
        .get_block_by_height(BlockHeight(42))
        .expect("query block by height")
        .expect("block 42 must exist in storage");

    let loaded_types: Vec<u8> = loaded_block
        .transactions
        .iter()
        .map(|t| t.tx_type)
        .collect();
    assert_eq!(
        loaded_types,
        vec![0, 2, 2, 0],
        "Block loaded from persistent disk storage MUST retain exact tx types [0, 2, 2, 0]"
    );

    for (orig, loaded) in block
        .transactions
        .iter()
        .zip(loaded_block.transactions.iter())
    {
        assert_eq!(
            compute_transaction_id(orig),
            compute_transaction_id(loaded),
            "TransactionId must be identical across disk reload"
        );
        assert_eq!(
            tola_codec::rlp::compute_eth_tx_hash(orig),
            tola_codec::rlp::compute_eth_tx_hash(loaded),
            "Eth tx hash must be identical across disk reload"
        );
        let orig_raw = tola_codec::rlp::encode_raw_eth_transaction(orig).unwrap();
        let loaded_raw = tola_codec::rlp::encode_raw_eth_transaction(loaded).unwrap();
        assert_eq!(
            orig_raw, loaded_raw,
            "Raw Ethereum wire bytes must match across disk reload"
        );
    }
    assert_eq!(
        block.header.txs_root, loaded_block.header.txs_root,
        "Block txs_root must match across disk reload"
    );

    // 4. Verify RPC dispatcher backed by restarted PersistentStateDB (with empty in-memory caches)
    let restarted_state_arc = Arc::new(RwLock::new(state_db_restarted));
    let mempool_arc = Arc::new(RwLock::new(Mempool::new(MempoolConfig::new(TEST_CHAIN_ID))));
    let disp_restarted = Arc::new(RpcDispatcher::new(
        TEST_CHAIN_ID,
        "TOLA-P5-RESTARTED/1.0".into(),
        restarted_state_arc,
        mempool_arc,
    ));
    disp_restarted.current_height.store(42, Ordering::SeqCst);

    // RPC Query A: Legacy transaction (tx0)
    let res_tx0 = disp_restarted.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"eth_getTransactionByHash","params":["{}"]}}"#,
        eth_hash0.to_hex()
    ));
    let val_tx0 = JsonValue::parse(&res_tx0).unwrap();
    let obj_tx0 = val_tx0.get("result").expect("tx0 result");
    assert_eq!(
        obj_tx0.get("type").unwrap().as_str(),
        Some("0x0"),
        "Legacy tx must return type 0x0 from restarted persistent storage"
    );

    // RPC Query B: Type-2 equal-fee transaction (tx2)
    let res_tx2 = disp_restarted.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"eth_getTransactionByHash","params":["{}"]}}"#,
        eth_hash2.to_hex()
    ));
    let val_tx2 = JsonValue::parse(&res_tx2).unwrap();
    let obj_tx2 = val_tx2.get("result").expect("tx2 result");
    assert_eq!(
        obj_tx2.get("type").unwrap().as_str(),
        Some("0x2"),
        "Type-2 equal-fee tx must return type 0x2 from restarted persistent storage"
    );
    assert_eq!(
        obj_tx2
            .get("accessList")
            .unwrap()
            .as_array()
            .map(|a| a.len()),
        Some(0),
        "Type-2 equal-fee tx must include accessList: [] from restarted storage"
    );

    // RPC Query C: Legacy receipt (tx0)
    let res_rc0 = disp_restarted.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":3,"method":"eth_getTransactionReceipt","params":["{}"]}}"#,
        eth_hash0.to_hex()
    ));
    let val_rc0 = JsonValue::parse(&res_rc0).unwrap();
    let obj_rc0 = val_rc0.get("result").expect("rc0 result");
    assert_eq!(
        obj_rc0.get("type").unwrap().as_str(),
        Some("0x0"),
        "Legacy receipt must return type 0x0 from restarted persistent storage"
    );

    // RPC Query D: Type-2 equal-fee receipt (tx2)
    let res_rc2 = disp_restarted.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":4,"method":"eth_getTransactionReceipt","params":["{}"]}}"#,
        eth_hash2.to_hex()
    ));
    let val_rc2 = JsonValue::parse(&res_rc2).unwrap();
    let obj_rc2 = val_rc2.get("result").expect("rc2 result");
    assert_eq!(
        obj_rc2.get("type").unwrap().as_str(),
        Some("0x2"),
        "Type-2 equal-fee receipt must return type 0x2 from restarted persistent storage"
    );

    // RPC Query E: eth_getBlockByNumber with full transactions
    let res_blk = disp_restarted.dispatch_json(
        r#"{"jsonrpc":"2.0","id":5,"method":"eth_getBlockByNumber","params":["0x2a", true]}"#,
    );
    let val_blk = JsonValue::parse(&res_blk).unwrap();
    let obj_blk = val_blk.get("result").expect("block result");
    let tx_arr = obj_blk.get("transactions").unwrap().as_array().unwrap();
    assert_eq!(tx_arr.len(), 4);
    assert_eq!(tx_arr[0].get("type").unwrap().as_str(), Some("0x0"));
    assert_eq!(tx_arr[1].get("type").unwrap().as_str(), Some("0x2"));
    assert_eq!(tx_arr[2].get("type").unwrap().as_str(), Some("0x2"));
    assert_eq!(tx_arr[3].get("type").unwrap().as_str(), Some("0x0"));

    // RPC Query F: Type-2 diff-fee transaction (tx1)
    let res_tx1 = disp_restarted.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":6,"method":"eth_getTransactionByHash","params":["{}"]}}"#,
        eth_hash1.to_hex()
    ));
    let val_tx1 = JsonValue::parse(&res_tx1).unwrap();
    let obj_tx1 = val_tx1.get("result").expect("tx1 result");
    assert_eq!(
        obj_tx1.get("type").unwrap().as_str(),
        Some("0x2"),
        "Type-2 diff-fee tx must return type 0x2 from restarted persistent storage"
    );

    // RPC Query G: Second legacy transaction (tx3)
    let res_tx3 = disp_restarted.dispatch_json(&format!(
        r#"{{"jsonrpc":"2.0","id":7,"method":"eth_getTransactionByHash","params":["{}"]}}"#,
        eth_hash3.to_hex()
    ));
    let val_tx3 = JsonValue::parse(&res_tx3).unwrap();
    let obj_tx3 = val_tx3.get("result").expect("tx3 result");
    assert_eq!(
        obj_tx3.get("type").unwrap().as_str(),
        Some("0x0"),
        "Second legacy tx must return type 0x0 from restarted persistent storage"
    );

    // Cleanup
    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_p5_blk01_mock_and_malformed_signature_fallback() {
    let kp = Secp256k1Keypair::generate();
    let recipient = Address([0x66; 20]);

    // 1. Non-secp256k1 signature (Ed25519) always resolves to tx_type 0
    let tx_ed = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(0),
        from: kp.address(),
        to: Some(recipient),
        value: Amount::from_u128(100),
        gas_limit: Gas(21_000),
        max_fee_per_gas: Amount::from_u128(20_000_000_000),
        max_priority_fee_per_gas: Amount::from_u128(10_000_000_000),
        valid_until_block: BlockHeight(100),
        data: vec![],
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![], // will not be serialized
    };
    let bytes_ed = tx_ed.to_canonical_bytes();
    let decoded_ed = Transaction::deserialize_canonical(&bytes_ed).unwrap();
    assert_eq!(
        decoded_ed.tx_type, 0,
        "Non-secp256k1 signature must resolve to tx_type 0"
    );

    // 2. Corrupted / unrecoverable Secp256k1Eth signature with equal fees falls back safely to 0
    let tx_corrupted_equal = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(1),
        from: kp.address(),
        to: Some(recipient),
        value: Amount::from_u128(100),
        gas_limit: Gas(21_000),
        max_fee_per_gas: Amount::from_u128(20_000_000_000),
        max_priority_fee_per_gas: Amount::from_u128(20_000_000_000),
        valid_until_block: BlockHeight(100),
        data: vec![],
        memo: None,
        signature: Signature::Secp256k1Eth {
            r: [0u8; 32], // invalid ECDSA point
            s: [0u8; 32],
            y_parity: 0,
            chain_id: TEST_CHAIN_ID.0,
        },
        tx_type: 2,
        access_list: vec![],
    };
    let bytes_corrupted = tx_corrupted_equal.to_canonical_bytes();
    let decoded_corrupted = Transaction::deserialize_canonical(&bytes_corrupted).unwrap();
    assert_eq!(
        decoded_corrupted.tx_type, 0,
        "Corrupted signature with equal fees must safely fall back to 0 without panicking"
    );

    // 3. Unequal fees with Secp256k1Eth resolves to 2 via fast path without recovery
    let tx_unequal = Transaction {
        chain_id: TEST_CHAIN_ID,
        nonce: Nonce(2),
        from: kp.address(),
        to: Some(recipient),
        value: Amount::from_u128(100),
        gas_limit: Gas(21_000),
        max_fee_per_gas: Amount::from_u128(30_000_000_000),
        max_priority_fee_per_gas: Amount::from_u128(10_000_000_000),
        valid_until_block: BlockHeight(100),
        data: vec![],
        memo: None,
        signature: Signature::Secp256k1Eth {
            r: [0u8; 32],
            s: [0u8; 32],
            y_parity: 0,
            chain_id: TEST_CHAIN_ID.0,
        },
        tx_type: 0,
        access_list: vec![], // unequal fees -> fast path returns 2
    };
    let bytes_unequal = tx_unequal.to_canonical_bytes();
    let decoded_unequal = Transaction::deserialize_canonical(&bytes_unequal).unwrap();
    assert_eq!(
        decoded_unequal.tx_type, 2,
        "Unequal fees with Secp256k1Eth must resolve to tx_type 2 via fast path"
    );
}
