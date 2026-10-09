//! Typed HTTP JSON-RPC client for interacting with TOLA Network nodes.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use tola_codec::CanonicalSerialize;
use tola_rpc::json::JsonValue;
use tola_types::*;

pub struct TolaClient {
    rpc_url: String,
    timeout: Duration,
}

impl TolaClient {
    pub fn new(rpc_url: &str) -> Self {
        Self {
            rpc_url: rpc_url.to_string(),
            timeout: Duration::from_secs(5),
        }
    }

    pub fn with_timeout(rpc_url: &str, timeout: Duration) -> Self {
        Self {
            rpc_url: rpc_url.to_string(),
            timeout,
        }
    }

    /// Dispatches an arbitrary JSON-RPC 2.0 call and returns the `result` value.
    pub fn call(&self, method: &str, params: Vec<JsonValue>) -> Result<JsonValue, String> {
        let clean = self
            .rpc_url
            .strip_prefix("http://")
            .unwrap_or(&self.rpc_url);
        let mut stream = TcpStream::connect(clean)
            .map_err(|e| format!("Failed to connect to RPC node at {}: {}", clean, e))?;

        let _ = stream.set_read_timeout(Some(self.timeout));
        let _ = stream.set_write_timeout(Some(self.timeout));

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
            .map_err(|e| format!("Failed to send request: {}", e))?;
        stream.flush().map_err(|e| format!("Flush error: {}", e))?;

        let mut response_bytes = Vec::new();
        stream
            .read_to_end(&mut response_bytes)
            .map_err(|e| format!("Read error: {}", e))?;

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
            .ok_or_else(|| "Missing 'result' field in response".to_string())
    }

    /// Query network chain ID.
    pub fn chain_id(&self) -> Result<u64, String> {
        let res = self.call("eth_chainId", vec![])?;
        parse_hex_u64(res.as_str().unwrap_or("0x0"))
    }

    /// Query current block height.
    pub fn block_number(&self) -> Result<u64, String> {
        let res = self.call("eth_blockNumber", vec![])?;
        parse_hex_u64(res.as_str().unwrap_or("0x0"))
    }

    /// Query account balance.
    pub fn get_balance(&self, address: &Address) -> Result<Amount, String> {
        let params = vec![
            JsonValue::String(address.to_hex()),
            JsonValue::String("latest".into()),
        ];
        let res = self.call("eth_getBalance", params)?;
        let wei = parse_hex_u128(res.as_str().unwrap_or("0x0"))?;
        Ok(Amount(wei))
    }

    /// Query account nonce.
    pub fn get_nonce(&self, address: &Address) -> Result<Nonce, String> {
        let params = vec![
            JsonValue::String(address.to_hex()),
            JsonValue::String("latest".into()),
        ];
        let res = self.call("eth_getTransactionCount", params)?;
        let n = parse_hex_u64(res.as_str().unwrap_or("0x0"))?;
        Ok(Nonce(n))
    }

    /// Broadcast a signed transaction payload.
    pub fn send_transaction(&self, tx: &Transaction) -> Result<Hash, String> {
        let raw_bytes = tx.to_canonical_bytes();
        let hex_payload = format!("0x{}", hex_encode(&raw_bytes));
        let params = vec![JsonValue::String(hex_payload)];
        let res = self.call("eth_sendRawTransaction", params)?;
        let hash_str = res
            .as_str()
            .ok_or_else(|| "Invalid hash returned".to_string())?;
        Hash::from_hex(hash_str)
    }

    /// Query transaction receipt by hash.
    pub fn get_transaction_receipt(&self, tx_hash: &Hash) -> Result<Option<JsonValue>, String> {
        let params = vec![JsonValue::String(tx_hash.to_hex())];
        let res = self.call("eth_getTransactionReceipt", params)?;
        match res {
            JsonValue::Null => Ok(None),
            val => Ok(Some(val)),
        }
    }

    /// Checks node /health endpoint.
    pub fn check_health(&self) -> Result<bool, String> {
        let clean = self
            .rpc_url
            .strip_prefix("http://")
            .unwrap_or(&self.rpc_url);
        let mut stream =
            TcpStream::connect(clean).map_err(|e| format!("Failed to connect: {}", e))?;

        let req = format!(
            "GET /health HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
            clean
        );
        stream
            .write_all(req.as_bytes())
            .map_err(|e| e.to_string())?;
        stream.flush().map_err(|e| e.to_string())?;

        let mut buf = Vec::new();
        stream.read_to_end(&mut buf).map_err(|e| e.to_string())?;

        let resp_str = String::from_utf8_lossy(&buf);
        Ok(resp_str.contains("200 OK") && resp_str.contains("\"status\":\"ok\""))
    }
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

fn parse_hex_u64(s: &str) -> Result<u64, String> {
    let clean = s
        .strip_prefix("0x")
        .or_else(|| s.strip_prefix("0X"))
        .unwrap_or(s);
    u64::from_str_radix(clean, 16).map_err(|e| format!("Hex parse error: {}", e))
}

fn parse_hex_u128(s: &str) -> Result<u128, String> {
    let clean = s
        .strip_prefix("0x")
        .or_else(|| s.strip_prefix("0X"))
        .unwrap_or(s);
    u128::from_str_radix(clean, 16).map_err(|e| format!("Hex parse error: {}", e))
}

fn hex_encode(data: &[u8]) -> String {
    let mut s = String::new();
    for b in data {
        s.push_str(&format!("{b:02x}"));
    }
    s
}
