//! Production HTTP server hosting JSON-RPC 2.0, `/health`, and Prometheus `/metrics` endpoints.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use tola_core::StateDB;

use crate::dispatcher::{FilterCriteria, RpcDispatcher};
use crate::json::JsonValue;
use crate::subscription::ConnectionSender;
use crate::types::{JsonRpcError, JsonRpcRequest, JsonRpcResponse};
use crate::ws::{
    compute_websocket_accept, WsFrame, WsFrameDecoder, WsMessageAssembler, WsOpcode,
    MAX_HANDSHAKE_HEADER_SIZE, MAX_MESSAGE_SIZE,
};

pub const MAX_HTTP_BODY_SIZE: usize = 5 * 1024 * 1024; // 5 MiB

pub type MetricsCallback = Arc<dyn Fn() -> String + Send + Sync>;

pub struct RpcServer<S: StateDB + 'static> {
    listen_addr: SocketAddr,
    bound_addr: Option<SocketAddr>,
    dispatcher: Arc<RpcDispatcher<S>>,
    metrics_provider: Option<MetricsCallback>,
    running: Arc<AtomicBool>,
    server_thread: Option<JoinHandle<()>>,
}

impl<S: StateDB + 'static> RpcServer<S> {
    pub fn new(
        listen_addr: SocketAddr,
        dispatcher: Arc<RpcDispatcher<S>>,
        metrics_provider: Option<MetricsCallback>,
    ) -> Self {
        Self {
            listen_addr,
            bound_addr: None,
            dispatcher,
            metrics_provider,
            running: Arc::new(AtomicBool::new(false)),
            server_thread: None,
        }
    }

    /// Starts the HTTP server listener loop in a background thread.
    pub fn start(&mut self) -> Result<SocketAddr, String> {
        let listener = TcpListener::bind(self.listen_addr).map_err(|e| {
            format!(
                "Failed to bind HTTP RPC server to {}: {}",
                self.listen_addr, e
            )
        })?;

        let actual_addr = listener
            .local_addr()
            .map_err(|e| format!("Failed to get local address: {}", e))?;
        self.bound_addr = Some(actual_addr);

        listener
            .set_nonblocking(true)
            .map_err(|e| format!("Failed to set non-blocking on listener: {}", e))?;

        self.running.store(true, Ordering::SeqCst);

        let running = self.running.clone();
        let dispatcher = self.dispatcher.clone();
        let metrics_provider = self.metrics_provider.clone();

        let handle = thread::Builder::new()
            .name(format!("rpc-server-{}", actual_addr.port()))
            .spawn(move || {
                while running.load(Ordering::SeqCst) {
                    match listener.accept() {
                        Ok((mut stream, _client_addr)) => {
                            let d = dispatcher.clone();
                            let m = metrics_provider.clone();
                            let _ = thread::spawn(move || {
                                handle_http_connection(&mut stream, &d, m.as_ref());
                            });
                        }
                        Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(10));
                        }
                        Err(_) => {
                            thread::sleep(Duration::from_millis(20));
                        }
                    }
                }
            })
            .map_err(|e| format!("Failed to spawn RPC server listener thread: {}", e))?;

        self.server_thread = Some(handle);
        Ok(actual_addr)
    }

    /// Returns the bound local socket address if running.
    pub fn local_addr(&self) -> Option<SocketAddr> {
        self.bound_addr
    }

    /// Shuts down the HTTP server.
    pub fn shutdown(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        if let Some(handle) = self.server_thread.take() {
            let _ = handle.join();
        }
    }
}

impl<S: StateDB + 'static> Drop for RpcServer<S> {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Handles a single incoming HTTP or WebSocket request on a client stream.
fn handle_http_connection<S: StateDB>(
    stream: &mut TcpStream,
    dispatcher: &RpcDispatcher<S>,
    metrics_provider: Option<&MetricsCallback>,
) {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));

    // Incremental handshake header read loop bounded by MAX_HANDSHAKE_HEADER_SIZE (4 KB)
    let mut header_buf = Vec::with_capacity(1024);
    let mut temp_buf = [0u8; 1024];
    let mut header_end_offset: Option<usize> = None;

    while header_buf.len() < MAX_HANDSHAKE_HEADER_SIZE {
        match stream.read(&mut temp_buf) {
            Ok(n) if n > 0 => {
                header_buf.extend_from_slice(&temp_buf[..n]);
                if let Some(pos) = header_buf.windows(4).position(|w| w == b"\r\n\r\n") {
                    header_end_offset = Some(pos + 4);
                    break;
                }
            }
            _ => break,
        }
    }

    let header_end = match header_end_offset {
        Some(offset) => offset,
        None => {
            if header_buf.len() >= MAX_HANDSHAKE_HEADER_SIZE {
                let _ = send_response(
                    stream,
                    431,
                    "Request Header Fields Too Large",
                    "text/plain",
                    "Header exceeds maximum 4KB\n",
                );
            } else {
                let _ = send_response(
                    stream,
                    400,
                    "Bad Request",
                    "text/plain",
                    "Incomplete HTTP request header\n",
                );
            }
            return;
        }
    };

    let req_str = match std::str::from_utf8(&header_buf[..header_end]) {
        Ok(s) => s,
        Err(_) => {
            let _ = send_response(stream, 400, "Bad Request", "text/plain", "Invalid UTF-8");
            return;
        }
    };

    let trailing_bytes = &header_buf[header_end..];

    // Parse request line: METHOD PATH HTTP/VERSION
    let mut lines = req_str.split("\r\n");
    let req_line = match lines.next() {
        Some(l) if !l.is_empty() => l,
        _ => return,
    };

    let parts: Vec<&str> = req_line.split_whitespace().collect();
    if parts.len() < 2 {
        let _ = send_response(
            stream,
            400,
            "Bad Request",
            "text/plain",
            "Malformed request line",
        );
        return;
    }

    let method = parts[0];
    let path = parts[1];

    // Check for WebSocket Upgrade headers
    let mut is_upgrade = false;
    let mut ws_key: Option<String> = None;
    let mut ws_version: Option<String> = None;

    for line in req_str.lines() {
        let lower = line.to_lowercase();
        if lower.starts_with("upgrade:") && lower.contains("websocket") {
            is_upgrade = true;
        }
        if lower.starts_with("sec-websocket-key:") {
            if let Some(val) = line.split(':').nth(1) {
                ws_key = Some(val.trim().to_string());
            }
        }
        if lower.starts_with("sec-websocket-version:") {
            if let Some(val) = line.split(':').nth(1) {
                ws_version = Some(val.trim().to_string());
            }
        }
    }

    if is_upgrade {
        let key = match ws_key {
            Some(k) if !k.is_empty() => k,
            _ => {
                let _ = send_response(
                    stream,
                    400,
                    "Bad Request",
                    "text/plain",
                    "Missing Sec-WebSocket-Key",
                );
                return;
            }
        };

        if let Some(ref ver) = ws_version {
            if ver != "13" {
                let _ = send_response(
                    stream,
                    400,
                    "Bad Request",
                    "text/plain",
                    "Unsupported WebSocket version, must be 13",
                );
                return;
            }
        }

        let accept = compute_websocket_accept(&key);
        let handshake_resp = format!(
            "HTTP/1.1 101 Switching Protocols\r\n\
Upgrade: websocket\r\n\
Connection: Upgrade\r\n\
Sec-WebSocket-Accept: {}\r\n\
\r\n",
            accept
        );

        if let Err(_) = stream.write_all(handshake_resp.as_bytes()) {
            return;
        }
        let _ = stream.flush();

        // Transition into persistent RFC 6455 WebSocket session loop
        handle_websocket_session(stream, trailing_bytes, dispatcher);
        return;
    }

    if method == "OPTIONS" {
        let _ = send_cors_preflight(stream);
        return;
    }

    if method == "GET" || method == "HEAD" {
        if path == "/health" {
            let height = dispatcher.current_height.load(Ordering::SeqCst);
            let peers = dispatcher.peer_count.load(Ordering::SeqCst);
            let chain_id = dispatcher.chain_id.0;
            let body = format!(
                "{{\"status\":\"ok\",\"chain_id\":{},\"height\":{},\"peers\":{}}}\n",
                chain_id, height, peers
            );
            let _ = send_response(stream, 200, "OK", "application/json", &body);
            return;
        }

        if path == "/metrics" {
            let body = if let Some(provider) = metrics_provider {
                provider()
            } else {
                let height = dispatcher.current_height.load(Ordering::SeqCst);
                let peers = dispatcher.peer_count.load(Ordering::SeqCst);
                format!(
                    "# HELP tola_block_height Current block height\n# TYPE tola_block_height gauge\ntola_block_height {}\n# HELP tola_peers Connected P2P peers\n# TYPE tola_peers gauge\ntola_peers {}\n",
                    height, peers
                )
            };
            let _ = send_response(stream, 200, "OK", "text/plain; version=0.0.4", &body);
            return;
        }

        if path == "/" || path == "/explorer" {
            let html = crate::explorer::render_explorer_html(dispatcher.chain_id.0);
            let _ = send_response(stream, 200, "OK", "text/html; charset=utf-8", &html);
            return;
        }

        let _ = send_response(
            stream,
            404,
            "Not Found",
            "text/plain",
            "Endpoint not found\n",
        );
        return;
    }

    if method == "POST" {
        let mut cl_header_found = false;
        let mut cl_value: Option<Result<usize, ()>> = None;

        for line in req_str.lines() {
            if line.to_lowercase().starts_with("content-length:") {
                cl_header_found = true;
                if let Some(val_str) = line.split(':').nth(1) {
                    cl_value = Some(val_str.trim().parse::<usize>().map_err(|_| ()));
                }
                break;
            }
        }

        if !cl_header_found {
            let _ = send_response(
                stream,
                411,
                "Length Required",
                "text/plain",
                "Content-Length header required\n",
            );
            return;
        }

        let clen = match cl_value {
            Some(Ok(len)) => len,
            _ => {
                let _ = send_response(
                    stream,
                    400,
                    "Bad Request",
                    "text/plain",
                    "Invalid Content-Length header\n",
                );
                return;
            }
        };

        // Security requirement: Validate Content-Length BEFORE allocating memory
        if clen > MAX_HTTP_BODY_SIZE {
            let _ = send_response(
                stream,
                413,
                "Payload Too Large",
                "text/plain",
                "Request body exceeds maximum size (5 MB)\n",
            );
            return;
        }

        let mut body_bytes = Vec::with_capacity(clen);
        body_bytes.extend_from_slice(trailing_bytes);

        while body_bytes.len() < clen {
            let mut more = [0u8; 4096];
            match stream.read(&mut more) {
                Ok(read_len) if read_len > 0 => {
                    body_bytes.extend_from_slice(&more[..read_len]);
                }
                _ => break,
            }
        }

        let body_str = String::from_utf8_lossy(&body_bytes);
        let response_json = dispatcher.dispatch_json(&body_str);
        let _ = send_response(stream, 200, "OK", "application/json", &response_json);
        return;
    }

    let _ = send_response(
        stream,
        405,
        "Method Not Allowed",
        "text/plain",
        "Method not allowed\n",
    );
}

fn handle_websocket_session<S: StateDB>(
    stream: &mut TcpStream,
    initial_bytes: &[u8],
    dispatcher: &RpcDispatcher<S>,
) {
    let (conn_id, rx, conn_sender) = match dispatcher.subscription_manager.register_connection() {
        Ok(res) => res,
        Err(_) => {
            let close = WsFrame::close(1008, "Connection limit exceeded").encode();
            let _ = stream.write_all(&close);
            let _ = stream.flush();
            return;
        }
    };

    let _ = stream.set_nonblocking(true);
    let mut decoder = WsFrameDecoder::with_initial_bytes(initial_bytes);
    let mut assembler = WsMessageAssembler::new();
    let mut last_activity = std::time::Instant::now();
    let mut ping_sent: Option<std::time::Instant> = None;

    let mut read_buf = [0u8; 4096];

    'session: loop {
        // 1. Drain outbound channel (responses & notifications) and write to socket
        while let Ok(msg) = rx.try_recv() {
            let frame = WsFrame::text(msg);
            let bytes = frame.encode();
            conn_sender.queued_messages.fetch_sub(1, Ordering::SeqCst);
            conn_sender
                .queued_bytes
                .fetch_sub(frame.payload.len(), Ordering::SeqCst);
            if let Err(_) = stream.write_all(&bytes) {
                break 'session;
            }
            let _ = stream.flush();
        }

        // 2. Read incoming frames from TCP stream
        match stream.read(&mut read_buf) {
            Ok(0) => {
                // Client closed socket (EOF)
                break 'session;
            }
            Ok(n) => {
                last_activity = std::time::Instant::now();
                ping_sent = None;
                decoder.feed(&read_buf[..n]);

                loop {
                    match decoder.decode_next(MAX_MESSAGE_SIZE) {
                        Ok(Some(frame)) => match assembler.process_frame(frame, MAX_MESSAGE_SIZE) {
                            Ok(Some((opcode, payload))) => match opcode {
                                WsOpcode::Text => {
                                    let text = match String::from_utf8(payload) {
                                        Ok(s) => s,
                                        Err(_) => {
                                            let close =
                                                WsFrame::close(1007, "Invalid UTF-8").encode();
                                            let _ = stream.write_all(&close);
                                            let _ = stream.flush();
                                            break 'session;
                                        }
                                    };
                                    handle_ws_jsonrpc(
                                        &text,
                                        conn_id,
                                        &conn_sender,
                                        dispatcher,
                                        stream,
                                    );
                                }
                                WsOpcode::Ping => {
                                    let pong = WsFrame::pong(payload).encode();
                                    if let Err(_) = stream.write_all(&pong) {
                                        break 'session;
                                    }
                                    let _ = stream.flush();
                                }
                                WsOpcode::Pong => {
                                    last_activity = std::time::Instant::now();
                                    ping_sent = None;
                                }
                                WsOpcode::Close => {
                                    let close = WsFrame::close(1000, "Normal closure").encode();
                                    let _ = stream.write_all(&close);
                                    let _ = stream.flush();
                                    break 'session;
                                }
                                _ => {}
                            },
                            Ok(None) => {}
                            Err(err) => {
                                let close =
                                    WsFrame::close(err.close_code(), &err.to_string()).encode();
                                let _ = stream.write_all(&close);
                                let _ = stream.flush();
                                break 'session;
                            }
                        },
                        Ok(None) => break,
                        Err(err) => {
                            let close = WsFrame::close(err.close_code(), &err.to_string()).encode();
                            let _ = stream.write_all(&close);
                            let _ = stream.flush();
                            break 'session;
                        }
                    }
                }
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(5));
            }
            Err(_) => {
                break 'session;
            }
        }

        // 3. Heartbeat management (RFC 6455 Ping/Pong)
        let now = std::time::Instant::now();
        if let Some(sent_at) = ping_sent {
            if now.duration_since(sent_at) > Duration::from_secs(15) {
                break 'session;
            }
        } else if now.duration_since(last_activity) > Duration::from_secs(30) {
            let ping = WsFrame::ping(Vec::new()).encode();
            if let Err(_) = stream.write_all(&ping) {
                break 'session;
            }
            let _ = stream.flush();
            ping_sent = Some(now);
        }
    }

    dispatcher.subscription_manager.cleanup_connection(conn_id);
    let _ = stream.shutdown(std::net::Shutdown::Both);
}

fn handle_ws_jsonrpc<S: StateDB>(
    text: &str,
    conn_id: u64,
    conn_sender: &ConnectionSender,
    dispatcher: &RpcDispatcher<S>,
    stream: &mut TcpStream,
) {
    let req_val = match JsonValue::parse(text) {
        Ok(v) => v,
        Err(_) => {
            let err_resp =
                JsonRpcResponse::error(None, JsonRpcError::parse_error("Malformed JSON"))
                    .serialize();
            let frame = WsFrame::text(err_resp).encode();
            let _ = stream.write_all(&frame);
            let _ = stream.flush();
            return;
        }
    };

    if matches!(req_val, JsonValue::Array(_)) {
        let resp_str = dispatcher.dispatch_json(text);
        let frame = WsFrame::text(resp_str).encode();
        let _ = stream.write_all(&frame);
        let _ = stream.flush();
        return;
    }

    let req = match JsonRpcRequest::from_json(&req_val) {
        Ok(r) => r,
        Err(e) => {
            let id = req_val.get("id").cloned();
            let err_resp = JsonRpcResponse::error(id, e).serialize();
            let frame = WsFrame::text(err_resp).encode();
            let _ = stream.write_all(&frame);
            let _ = stream.flush();
            return;
        }
    };

    let resp_str = match req.method.as_str() {
        "eth_subscribe" => {
            let sub_type = req.params.get(0).and_then(|p| p.as_str()).unwrap_or("");
            match sub_type {
                "newHeads" => {
                    match dispatcher
                        .subscription_manager
                        .subscribe_new_heads(conn_id, conn_sender.clone())
                    {
                        Ok(sub_id) => {
                            JsonRpcResponse::success(req.id, JsonValue::String(sub_id)).serialize()
                        }
                        Err(e) => {
                            JsonRpcResponse::error(req.id, JsonRpcError::server_error(-32000, e))
                                .serialize()
                        }
                    }
                }
                "logs" => {
                    let cur_h = dispatcher.current_height.load(Ordering::SeqCst);
                    let criteria = match req.params.get(1) {
                        Some(val) => match RpcDispatcher::<S>::parse_filter_criteria(val, cur_h) {
                            Ok(c) => c,
                            Err(e) => {
                                let err_resp = JsonRpcResponse::error(req.id, e).serialize();
                                let frame = WsFrame::text(err_resp).encode();
                                let _ = stream.write_all(&frame);
                                let _ = stream.flush();
                                return;
                            }
                        },
                        None => FilterCriteria::default_range(cur_h, cur_h),
                    };
                    match dispatcher.subscription_manager.subscribe_logs(
                        conn_id,
                        criteria,
                        conn_sender.clone(),
                    ) {
                        Ok(sub_id) => {
                            JsonRpcResponse::success(req.id, JsonValue::String(sub_id)).serialize()
                        }
                        Err(e) => {
                            JsonRpcResponse::error(req.id, JsonRpcError::server_error(-32000, e))
                                .serialize()
                        }
                    }
                }
                "newPendingTransactions" => {
                    match dispatcher
                        .subscription_manager
                        .subscribe_pending_transactions(conn_id, conn_sender.clone())
                    {
                        Ok(sub_id) => {
                            JsonRpcResponse::success(req.id, JsonValue::String(sub_id)).serialize()
                        }
                        Err(e) => {
                            JsonRpcResponse::error(req.id, JsonRpcError::server_error(-32000, e))
                                .serialize()
                        }
                    }
                }
                other => JsonRpcResponse::error(
                    req.id,
                    JsonRpcError::invalid_params(&format!(
                        "Unsupported subscription type: {}",
                        other
                    )),
                )
                .serialize(),
            }
        }
        "eth_unsubscribe" => {
            let sub_id = req.params.get(0).and_then(|p| p.as_str()).unwrap_or("");
            let removed = dispatcher.subscription_manager.unsubscribe(conn_id, sub_id);
            JsonRpcResponse::success(req.id, JsonValue::Bool(removed)).serialize()
        }
        _ => dispatcher.dispatch_json(text),
    };

    let frame = WsFrame::text(resp_str).encode();
    let _ = stream.write_all(&frame);
    let _ = stream.flush();
}

fn send_cors_preflight(stream: &mut TcpStream) -> std::io::Result<()> {
    let headers = "HTTP/1.1 204 No Content\r\n\
Access-Control-Allow-Origin: *\r\n\
Access-Control-Allow-Methods: GET, POST, OPTIONS\r\n\
Access-Control-Allow-Headers: Content-Type\r\n\
Access-Control-Max-Age: 86400\r\n\
Connection: close\r\n\
\r\n";
    stream.write_all(headers.as_bytes())?;
    let _ = stream.flush();
    let _ = stream.shutdown(std::net::Shutdown::Write);
    Ok(())
}

fn send_response(
    stream: &mut TcpStream,
    status_code: u16,
    status_text: &str,
    content_type: &str,
    body: &str,
) -> std::io::Result<()> {
    let response = format!(
        "HTTP/1.1 {} {}\r\n\
Content-Type: {}\r\n\
Content-Length: {}\r\n\
Access-Control-Allow-Origin: *\r\n\
Access-Control-Allow-Methods: GET, POST, OPTIONS\r\n\
Access-Control-Allow-Headers: Content-Type\r\n\
Connection: close\r\n\
\r\n\
{}",
        status_code,
        status_text,
        content_type,
        body.len(),
        body
    );
    stream.write_all(response.as_bytes())?;
    let _ = stream.flush();
    let _ = stream.shutdown(std::net::Shutdown::Write);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::sync::RwLock;
    use tola_mempool::{Mempool, MempoolConfig};
    use tola_state::MemoryStateJournal;
    use tola_types::ChainId;

    #[test]
    fn test_rpc_server_health_and_jsonrpc_http() {
        let chain_id = ChainId::MAINNET;
        let state_db = Arc::new(RwLock::new(MemoryStateJournal::new()));
        let mempool = Arc::new(RwLock::new(Mempool::new(MempoolConfig::new(chain_id))));
        let dispatcher = Arc::new(RpcDispatcher::new(
            chain_id,
            "TOLA-Test/1.0".into(),
            state_db,
            mempool,
        ));

        let mut server = RpcServer::new("127.0.0.1:0".parse().unwrap(), dispatcher, None);

        let bound = server.start().expect("Start HTTP RPC server");

        // 1. Test GET /health
        {
            let mut stream = TcpStream::connect(bound).expect("Connect to RPC server");
            let req = "GET /health HTTP/1.1\r\nHost: localhost\r\n\r\n";
            stream.write_all(req.as_bytes()).unwrap();

            let mut reader = BufReader::new(stream);
            let mut status_line = String::new();
            reader.read_line(&mut status_line).unwrap();
            assert!(
                status_line.contains("200 OK"),
                "Status line: {}",
                status_line
            );

            let mut body = String::new();
            let mut reading_body = false;
            let mut line = String::new();
            while reader.read_line(&mut line).unwrap() > 0 {
                if reading_body {
                    body.push_str(&line);
                } else if line == "\r\n" {
                    reading_body = true;
                }
                line.clear();
            }
            assert!(body.contains("\"status\":\"ok\""), "Body: {}", body);
            assert!(body.contains("\"chain_id\":9999"), "Body: {}", body);
        }

        // 2. Test POST / JSON-RPC
        {
            let mut stream = TcpStream::connect(bound).expect("Connect to RPC server");
            let json_body =
                "{\"jsonrpc\":\"2.0\",\"method\":\"net_listening\",\"params\":[],\"id\":1}";
            let req = format!(
                "POST / HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                json_body.len(),
                json_body
            );
            stream.write_all(req.as_bytes()).unwrap();

            let mut reader = BufReader::new(stream);
            let mut status_line = String::new();
            reader.read_line(&mut status_line).unwrap();
            assert!(
                status_line.contains("200 OK"),
                "Status line: {}",
                status_line
            );

            let mut body = String::new();
            let mut reading_body = false;
            let mut line = String::new();
            while reader.read_line(&mut line).unwrap() > 0 {
                if reading_body {
                    body.push_str(&line);
                } else if line == "\r\n" {
                    reading_body = true;
                }
                line.clear();
            }
            assert!(body.contains("\"result\":true"), "Body: {}", body);
        }

        // 3. Test GET /metrics
        {
            let mut stream = TcpStream::connect(bound).expect("Connect to RPC server");
            let req = "GET /metrics HTTP/1.1\r\nHost: localhost\r\n\r\n";
            stream.write_all(req.as_bytes()).unwrap();

            let mut reader = BufReader::new(stream);
            let mut status_line = String::new();
            reader.read_line(&mut status_line).unwrap();
            assert!(
                status_line.contains("200 OK"),
                "Status line: {}",
                status_line
            );

            let mut body = String::new();
            let mut reading_body = false;
            let mut line = String::new();
            while reader.read_line(&mut line).unwrap() > 0 {
                if reading_body {
                    body.push_str(&line);
                } else if line == "\r\n" {
                    reading_body = true;
                }
                line.clear();
            }
            assert!(body.contains("tola_block_height"), "Body: {}", body);
        }

        // 4. Test GET /explorer
        {
            let mut stream = TcpStream::connect(bound).expect("Connect to RPC server");
            let req = "GET /explorer HTTP/1.1\r\nHost: localhost\r\n\r\n";
            stream.write_all(req.as_bytes()).unwrap();

            let mut reader = BufReader::new(stream);
            let mut status_line = String::new();
            reader.read_line(&mut status_line).unwrap();
            assert!(
                status_line.contains("200 OK"),
                "Status line: {}",
                status_line
            );

            let mut body = String::new();
            let mut reading_body = false;
            let mut line = String::new();
            while reader.read_line(&mut line).unwrap() > 0 {
                if reading_body {
                    body.push_str(&line);
                } else if line == "\r\n" {
                    reading_body = true;
                }
                line.clear();
            }
            assert!(body.contains("TOLA Network"), "Body: {}", body);
            assert!(body.contains("Explorer"), "Body: {}", body);
        }

        server.shutdown();
    }
}
