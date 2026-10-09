//! Automated testnet token faucet server and client request operations.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tola_codec::CanonicalSerialize;
use tola_crypto::Secp256k1Keypair;
use tola_rpc::json::JsonValue;
use tola_types::*;

pub struct FaucetServer {
    listen_port: u16,
    faucet_key: Secp256k1Keypair,
    rpc_url: String,
    payout_amount: Amount,
    rate_limit_secs: u64,
    history: Arc<Mutex<HashMap<Address, u64>>>,
    ip_history: Arc<Mutex<HashMap<String, u64>>>,
    running: Arc<AtomicBool>,
}

impl FaucetServer {
    pub fn new(
        listen_port: u16,
        faucet_key: Secp256k1Keypair,
        rpc_url: String,
        payout_tola: u64,
    ) -> Self {
        Self {
            listen_port,
            faucet_key,
            rpc_url,
            payout_amount: Amount(payout_tola as u128 * Amount::ONE_TOLA),
            rate_limit_secs: 86400, // 24 hours
            history: Arc::new(Mutex::new(HashMap::new())),
            ip_history: Arc::new(Mutex::new(HashMap::new())),
            running: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn start(&self) -> Result<(), String> {
        let bind_addr = format!("0.0.0.0:{}", self.listen_port);
        let listener = TcpListener::bind(&bind_addr)
            .map_err(|e| format!("Failed to bind faucet to {}: {}", bind_addr, e))?;

        listener
            .set_nonblocking(true)
            .map_err(|e| format!("Set non-blocking failed: {}", e))?;

        self.running.store(true, Ordering::SeqCst);

        let faucet_addr = self.faucet_key.address();
        println!("=======================================================");
        println!("               TOLA TESTNET FAUCET SERVICE             ");
        println!("=======================================================");
        println!("Listening on:        http://0.0.0.0:{}", self.listen_port);
        println!("Faucet Address:      {}", faucet_addr.to_hex());
        println!("Target RPC:          {}", self.rpc_url);
        println!(
            "Payout Per Request:  {} TOLA",
            self.payout_amount.0 / Amount::ONE_TOLA
        );
        println!("Rate Limit Window:   24 hours");
        println!("=======================================================");

        let running = self.running.clone();
        let faucet_key = self.faucet_key.clone();
        let rpc_url = self.rpc_url.clone();
        let payout = self.payout_amount;
        let rate_limit = self.rate_limit_secs;
        let history = self.history.clone();
        let ip_history = self.ip_history.clone();

        while running.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((mut stream, client_addr)) => {
                    let f_key = faucet_key.clone();
                    let r_url = rpc_url.clone();
                    let hist = history.clone();
                    let ip_hist = ip_history.clone();

                    let _ = thread::spawn(move || {
                        handle_faucet_connection(
                            &mut stream,
                            client_addr,
                            &f_key,
                            &r_url,
                            payout,
                            rate_limit,
                            &hist,
                            &ip_hist,
                        );
                    });
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(20));
                }
                Err(_) => {
                    thread::sleep(Duration::from_millis(50));
                }
            }
        }

        Ok(())
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[allow(clippy::too_many_arguments)]
fn handle_faucet_connection(
    stream: &mut TcpStream,
    client_addr: SocketAddr,
    faucet_key: &Secp256k1Keypair,
    rpc_url: &str,
    payout: Amount,
    rate_limit: u64,
    history: &Arc<Mutex<HashMap<Address, u64>>>,
    ip_history: &Arc<Mutex<HashMap<String, u64>>>,
) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));

    let mut buf = [0u8; 4096];
    let n = match stream.read(&mut buf) {
        Ok(n) if n > 0 => n,
        _ => return,
    };

    let req_str = match std::str::from_utf8(&buf[..n]) {
        Ok(s) => s,
        Err(_) => {
            let _ = send_response(
                stream,
                400,
                "Bad Request",
                "application/json",
                "{\"error\":\"Invalid UTF-8\"}",
            );
            return;
        }
    };

    let req_line = req_str.lines().next().unwrap_or("");
    let parts: Vec<&str> = req_line.split_whitespace().collect();
    if parts.len() < 2 {
        let _ = send_response(
            stream,
            400,
            "Bad Request",
            "application/json",
            "{\"error\":\"Malformed request\"}",
        );
        return;
    }

    let method = parts[0];
    let path = parts[1];

    if method == "OPTIONS" {
        let headers = "HTTP/1.1 204 No Content\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: GET, POST, OPTIONS\r\nAccess-Control-Allow-Headers: Content-Type\r\nConnection: close\r\n\r\n";
        let _ = stream.write_all(headers.as_bytes());
        return;
    }

    if method == "GET" && (path == "/" || path == "/status" || path == "/health") {
        let body = format!(
            "{{\"status\":\"ok\",\"faucet_address\":\"{}\",\"payout_tola\":{}}}\n",
            faucet_key.address().to_hex(),
            payout.0 / Amount::ONE_TOLA
        );
        let _ = send_response(stream, 200, "OK", "application/json", &body);
        return;
    }

    if method == "POST" && (path == "/request" || path == "/faucet") {
        let body_str = extract_body(req_str);
        let parsed = match JsonValue::parse(body_str) {
            Ok(v) => v,
            Err(e) => {
                let err_resp = format!("{{\"error\":\"Invalid JSON: {}\"}}", e);
                let _ = send_response(stream, 400, "Bad Request", "application/json", &err_resp);
                return;
            }
        };

        let target_addr_str = match parsed.get("address").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => {
                let _ = send_response(
                    stream,
                    400,
                    "Bad Request",
                    "application/json",
                    "{\"error\":\"Missing 'address' in body\"}",
                );
                return;
            }
        };

        let target_addr = if target_addr_str.starts_with("0x") || target_addr_str.starts_with("0X")
        {
            match Address::from_hex(target_addr_str) {
                Ok(a) => a,
                Err(e) => {
                    let err_resp = format!("{{\"error\":\"Invalid hex address: {:?}\"}}", e);
                    let _ =
                        send_response(stream, 400, "Bad Request", "application/json", &err_resp);
                    return;
                }
            }
        } else {
            match Address::from_bech32m(target_addr_str) {
                Ok((_, a)) => a,
                Err(e) => {
                    let err_resp = format!("{{\"error\":\"Invalid Bech32m address: {:?}\"}}", e);
                    let _ =
                        send_response(stream, 400, "Bad Request", "application/json", &err_resp);
                    return;
                }
            }
        };

        let now = now_secs();
        let ip_key = client_addr.ip().to_string();

        // 1. Check IP rate limit
        {
            let mut ip_lock = ip_history.lock().unwrap();
            if let Some(&last) = ip_lock.get(&ip_key) {
                if now < last + rate_limit {
                    let wait = (last + rate_limit) - now;
                    let err_resp = format!(
                        "{{\"error\":\"IP rate limit exceeded. Try again in {} seconds\"}}",
                        wait
                    );
                    let _ = send_response(
                        stream,
                        429,
                        "Too Many Requests",
                        "application/json",
                        &err_resp,
                    );
                    return;
                }
            }
            ip_lock.insert(ip_key, now);
        }

        // 2. Check Address rate limit
        {
            let mut addr_lock = history.lock().unwrap();
            if let Some(&last) = addr_lock.get(&target_addr) {
                if now < last + rate_limit {
                    let wait = (last + rate_limit) - now;
                    let err_resp = format!(
                        "{{\"error\":\"Address rate limit exceeded. Try again in {} seconds\"}}",
                        wait
                    );
                    let _ = send_response(
                        stream,
                        429,
                        "Too Many Requests",
                        "application/json",
                        &err_resp,
                    );
                    return;
                }
            }
            addr_lock.insert(target_addr, now);
        }

        // 3. Dispatch transaction via RPC
        match dispatch_faucet_transfer(rpc_url, faucet_key, target_addr, payout) {
            Ok(tx_hash) => {
                let success_resp = format!(
                    "{{\"status\":\"ok\",\"tx_hash\":\"{}\",\"recipient\":\"{}\",\"amount_tola\":{}}}\n",
                    tx_hash,
                    target_addr.to_hex(),
                    payout.0 / Amount::ONE_TOLA
                );
                let _ = send_response(stream, 200, "OK", "application/json", &success_resp);
            }
            Err(e) => {
                let err_resp = format!("{{\"error\":\"Transfer failed: {}\"}}", e);
                let _ = send_response(
                    stream,
                    500,
                    "Internal Server Error",
                    "application/json",
                    &err_resp,
                );
            }
        }
        return;
    }

    let _ = send_response(
        stream,
        404,
        "Not Found",
        "application/json",
        "{\"error\":\"Endpoint not found\"}",
    );
}

fn dispatch_faucet_transfer(
    rpc_url: &str,
    faucet_key: &Secp256k1Keypair,
    to: Address,
    value: Amount,
) -> Result<String, String> {
    let from = faucet_key.address();

    // Query nonce
    let nonce_val = query_nonce(rpc_url, &from)?;
    let chain_id_val = query_chain_id(rpc_url)?;

    let mut tx = Transaction {
        chain_id: ChainId(chain_id_val),
        nonce: Nonce(nonce_val),
        from,
        to: Some(to),
        value,
        gas_limit: Gas(21_000),
        max_fee_per_gas: Amount::from_u128(1_000_000_000),
        max_priority_fee_per_gas: Amount::from_u128(1_000_000_000),
        valid_until_block: BlockHeight(1_000_000),
        data: vec![],
        memo: None,
        signature: Signature::Secp256k1 {
            r: [0u8; 32],
            s: [0u8; 32],
            v: 0,
        },
        tx_type: 0,
        access_list: vec![],
    };

    let mut unsigned_bytes = Vec::new();
    tola_codec::serialize_unsigned_transaction(&tx, &mut unsigned_bytes);
    let payload_hash = tola_crypto::blake3_hash(&unsigned_bytes);
    let digest = tola_crypto::domain_separated_hash(
        tola_crypto::DOMAIN_TRANSACTION_PAYLOAD,
        tx.chain_id.0,
        0,
        &payload_hash,
    );

    let sig_65 = faucet_key
        .sign_recoverable(digest.as_bytes())
        .map_err(|e| format!("Sign error: {:?}", e))?;

    let mut r = [0u8; 32];
    let mut s = [0u8; 32];
    r.copy_from_slice(&sig_65[0..32]);
    s.copy_from_slice(&sig_65[32..64]);
    let v = sig_65[64];
    tx.signature = Signature::Secp256k1 { r, s, v };

    let signed_bytes = tx.to_canonical_bytes();
    let raw_hex = format!("0x{}", hex_encode(&signed_bytes));

    broadcast_raw_tx(rpc_url, &raw_hex)
}

fn query_nonce(rpc_url: &str, from: &Address) -> Result<u64, String> {
    let params = vec![
        JsonValue::String(from.to_hex()),
        JsonValue::String("latest".into()),
    ];
    let res = rpc_http_call(rpc_url, "eth_getTransactionCount", params)?;
    let hex_str = res.as_str().unwrap_or("0x0");
    let clean = hex_str
        .strip_prefix("0x")
        .or_else(|| hex_str.strip_prefix("0X"))
        .unwrap_or(hex_str);
    u64::from_str_radix(clean, 16).map_err(|e| e.to_string())
}

fn query_chain_id(rpc_url: &str) -> Result<u64, String> {
    let res = rpc_http_call(rpc_url, "eth_chainId", vec![])?;
    let hex_str = res.as_str().unwrap_or("0x66");
    let clean = hex_str
        .strip_prefix("0x")
        .or_else(|| hex_str.strip_prefix("0X"))
        .unwrap_or(hex_str);
    u64::from_str_radix(clean, 16).map_err(|e| e.to_string())
}

fn broadcast_raw_tx(rpc_url: &str, raw_hex: &str) -> Result<String, String> {
    let params = vec![JsonValue::String(raw_hex.to_string())];
    let res = rpc_http_call(rpc_url, "eth_sendRawTransaction", params)?;
    res.as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| "Missing hash".to_string())
}

fn rpc_http_call(rpc_url: &str, method: &str, params: Vec<JsonValue>) -> Result<JsonValue, String> {
    let clean = rpc_url.strip_prefix("http://").unwrap_or(rpc_url);
    let mut stream =
        TcpStream::connect(clean).map_err(|e| format!("Connect error {}: {}", clean, e))?;

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
        .map_err(|e| e.to_string())?;
    stream.flush().map_err(|e| e.to_string())?;

    let mut response_bytes = Vec::new();
    stream
        .read_to_end(&mut response_bytes)
        .map_err(|e| e.to_string())?;

    let resp_str = String::from_utf8_lossy(&response_bytes);
    let body = extract_body(&resp_str);
    let parsed = JsonValue::parse(body).map_err(|e| e.to_string())?;

    if let Some(err) = parsed.get("error") {
        return Err(format!("RPC Error: {}", err.serialize()));
    }

    parsed
        .get("result")
        .cloned()
        .ok_or_else(|| "Missing result".to_string())
}

fn extract_body(resp: &str) -> &str {
    if let Some(pos) = resp.find("\r\n\r\n") {
        &resp[pos + 4..]
    } else if let Some(pos) = resp.find("\n\n") {
        &resp[pos + 2..]
    } else {
        ""
    }
}

fn send_response(
    stream: &mut TcpStream,
    code: u16,
    reason: &str,
    content_type: &str,
    body: &str,
) -> std::io::Result<()> {
    let resp = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n{}",
        code,
        reason,
        content_type,
        body.len(),
        body
    );
    stream.write_all(resp.as_bytes())?;
    stream.flush()
}

fn hex_encode(data: &[u8]) -> String {
    let mut s = String::new();
    for b in data {
        s.push_str(&format!("{b:02x}"));
    }
    s
}
