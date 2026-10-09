//! Production TCP transport for P2P wire framing, socket management, and message broadcast.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use tola_core::NetworkError;

use crate::wire::{
    decode_frame, encode_frame, NetworkId, WireMessage, FRAME_HEADER_SIZE, MAX_PAYLOAD_SIZE,
    PROTOCOL_MAGIC,
};

#[derive(Clone, Debug)]
pub struct InboundMessage {
    pub peer_addr: SocketAddr,
    pub message: WireMessage,
}

pub struct TcpP2PTransport {
    network_id: NetworkId,
    listen_addr: SocketAddr,
    running: Arc<AtomicBool>,
    inbound_tx: Sender<InboundMessage>,
    inbound_rx: Mutex<Receiver<InboundMessage>>,
    peers: Arc<RwLock<HashMap<SocketAddr, Arc<Mutex<TcpStream>>>>>,
    threads: Mutex<Vec<JoinHandle<()>>>,
}

impl TcpP2PTransport {
    pub fn new(network_id: NetworkId, listen_addr: SocketAddr) -> Self {
        let (inbound_tx, inbound_rx) = channel();
        Self {
            network_id,
            listen_addr,
            running: Arc::new(AtomicBool::new(false)),
            inbound_tx,
            inbound_rx: Mutex::new(inbound_rx),
            peers: Arc::new(RwLock::new(HashMap::new())),
            threads: Mutex::new(Vec::new()),
        }
    }

    /// Starts the TCP listener on the configured bind address.
    pub fn start(&self) -> Result<(), String> {
        let listener = TcpListener::bind(self.listen_addr)
            .map_err(|e| format!("Failed to bind TCP listener to {}: {}", self.listen_addr, e))?;

        // Non-blocking so accept loop can check self.running flag
        listener
            .set_nonblocking(true)
            .map_err(|e| format!("Failed to set non-blocking on listener: {}", e))?;

        self.running.store(true, Ordering::SeqCst);

        let running = self.running.clone();
        let peers = self.peers.clone();
        let inbound_tx = self.inbound_tx.clone();
        let network_id = self.network_id;

        let handle = thread::Builder::new()
            .name(format!("p2p-listener-{}", self.listen_addr.port()))
            .spawn(move || {
                while running.load(Ordering::SeqCst) {
                    match listener.accept() {
                        Ok((stream, peer_addr)) => {
                            let _ = stream.set_nodelay(true);
                            let stream_arc = Arc::new(Mutex::new(stream));
                            peers.write().unwrap().insert(peer_addr, stream_arc.clone());

                            // Spawn dedicated reader thread for inbound peer
                            let r_running = running.clone();
                            let r_tx = inbound_tx.clone();
                            let r_peers = peers.clone();

                            let _ = thread::Builder::new()
                                .name(format!("p2p-read-{}", peer_addr))
                                .spawn(move || {
                                    handle_peer_inbound(
                                        peer_addr, stream_arc, network_id, r_running, r_tx, r_peers,
                                    );
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
            .map_err(|e| format!("Failed to spawn accept thread: {}", e))?;

        self.threads.lock().unwrap().push(handle);
        Ok(())
    }

    /// Connects to a remote peer address.
    pub fn connect_peer(&self, peer_addr: SocketAddr) -> Result<(), String> {
        // Check if already connected
        if self.peers.read().unwrap().contains_key(&peer_addr) {
            return Ok(());
        }

        let stream = TcpStream::connect_timeout(&peer_addr, Duration::from_secs(3))
            .map_err(|e| format!("Failed to connect to peer {}: {}", peer_addr, e))?;

        let _ = stream.set_nodelay(true);
        let stream_arc = Arc::new(Mutex::new(stream));
        self.peers
            .write()
            .unwrap()
            .insert(peer_addr, stream_arc.clone());

        // Spawn dedicated reader thread
        let running = self.running.clone();
        let inbound_tx = self.inbound_tx.clone();
        let peers = self.peers.clone();
        let network_id = self.network_id;

        let handle = thread::Builder::new()
            .name(format!("p2p-out-read-{}", peer_addr))
            .spawn(move || {
                handle_peer_inbound(
                    peer_addr, stream_arc, network_id, running, inbound_tx, peers,
                );
            })
            .map_err(|e| format!("Failed to spawn peer reader: {}", e))?;

        self.threads.lock().unwrap().push(handle);
        Ok(())
    }

    /// Broadcasts an encoded wire message to all connected peers.
    pub fn broadcast(&self, message: &WireMessage) -> usize {
        let frame = encode_frame(self.network_id, message);
        let peers = self.peers.read().unwrap().clone();
        let mut failed = Vec::new();
        let mut success = 0;

        for (addr, stream_mutex) in peers {
            let mut stream = stream_mutex.lock().unwrap();
            match stream.write_all(&frame) {
                Ok(_) => {
                    let _ = stream.flush();
                    success += 1;
                }
                Err(_) => {
                    failed.push(addr);
                }
            }
        }

        if !failed.is_empty() {
            let mut p_write = self.peers.write().unwrap();
            for addr in failed {
                p_write.remove(&addr);
            }
        }

        success
    }

    /// Sends a wire message to a specific peer.
    pub fn send_to(&self, peer_addr: &SocketAddr, message: &WireMessage) -> Result<(), String> {
        let frame = encode_frame(self.network_id, message);
        let stream_opt = self.peers.read().unwrap().get(peer_addr).cloned();

        match stream_opt {
            Some(stream_mutex) => {
                let mut stream = stream_mutex.lock().unwrap();
                stream
                    .write_all(&frame)
                    .map_err(|e| format!("Write error to {}: {}", peer_addr, e))?;
                let _ = stream.flush();
                Ok(())
            }
            None => Err(format!("Peer {} not connected", peer_addr)),
        }
    }

    /// Drains all incoming messages received from connected peers.
    pub fn drain_inbound(&self) -> Vec<InboundMessage> {
        let rx = self.inbound_rx.lock().unwrap();
        let mut msgs = Vec::new();
        while let Ok(msg) = rx.try_recv() {
            msgs.push(msg);
        }
        msgs
    }

    /// Waits for an inbound message with timeout.
    pub fn recv_inbound_timeout(&self, timeout: Duration) -> Option<InboundMessage> {
        let rx = self.inbound_rx.lock().unwrap();
        rx.recv_timeout(timeout).ok()
    }

    /// Returns the number of currently active connected peers.
    pub fn peer_count(&self) -> usize {
        self.peers.read().unwrap().len()
    }

    /// Returns the list of connected peer socket addresses.
    pub fn peer_addrs(&self) -> Vec<SocketAddr> {
        self.peers.read().unwrap().keys().cloned().collect()
    }

    /// Shuts down listener and drops all peer streams.
    pub fn shutdown(&self) {
        self.running.store(false, Ordering::SeqCst);
        self.peers.write().unwrap().clear();
    }
}

impl Drop for TcpP2PTransport {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Dedicated per-peer worker reading framed binary wire messages.
fn handle_peer_inbound(
    peer_addr: SocketAddr,
    stream_mutex: Arc<Mutex<TcpStream>>,
    network_id: NetworkId,
    running: Arc<AtomicBool>,
    inbound_tx: Sender<InboundMessage>,
    peers: Arc<RwLock<HashMap<SocketAddr, Arc<Mutex<TcpStream>>>>>,
) {
    // Clone stream to own reading without holding the mutex continuously
    let mut reader_stream = match stream_mutex.lock().unwrap().try_clone() {
        Ok(s) => s,
        Err(_) => {
            peers.write().unwrap().remove(&peer_addr);
            return;
        }
    };

    let _ = reader_stream.set_read_timeout(Some(Duration::from_millis(200)));

    while running.load(Ordering::SeqCst) {
        match read_wire_frame(&mut reader_stream, network_id) {
            Ok(message) => {
                let _ = inbound_tx.send(InboundMessage { peer_addr, message });
            }
            Err(WireReadError::Timeout) => {
                // Read timeout, continue loop to check running flag
            }
            Err(WireReadError::Closed) => {
                // Peer disconnected or connection closed
                break;
            }
            Err(WireReadError::Protocol(_)) => {
                // Malformed message or protocol violation
                break;
            }
        }
    }

    // Clean up peer on disconnect
    peers.write().unwrap().remove(&peer_addr);
}

enum WireReadError {
    Timeout,
    Closed,
    #[allow(dead_code)]
    Protocol(NetworkError),
}

/// Reads a single canonical framed wire message from a stream.
fn read_wire_frame<R: Read>(
    reader: &mut R,
    network_id: NetworkId,
) -> Result<WireMessage, WireReadError> {
    let mut header = [0u8; FRAME_HEADER_SIZE];
    match reader.read_exact(&mut header) {
        Ok(_) => {}
        Err(ref e)
            if e.kind() == std::io::ErrorKind::TimedOut
                || e.kind() == std::io::ErrorKind::WouldBlock =>
        {
            return Err(WireReadError::Timeout);
        }
        Err(_) => return Err(WireReadError::Closed),
    }

    if header[0..4] != PROTOCOL_MAGIC {
        return Err(WireReadError::Protocol(NetworkError::InvalidMagic));
    }

    let payload_len = u32::from_le_bytes([header[8], header[9], header[10], header[11]]) as usize;
    if payload_len > MAX_PAYLOAD_SIZE {
        return Err(WireReadError::Protocol(NetworkError::MessageTooLarge));
    }

    let mut frame_buf = vec![0u8; FRAME_HEADER_SIZE + payload_len];
    frame_buf[0..FRAME_HEADER_SIZE].copy_from_slice(&header);

    match reader.read_exact(&mut frame_buf[FRAME_HEADER_SIZE..]) {
        Ok(_) => {}
        Err(_) => return Err(WireReadError::Closed),
    }

    decode_frame(network_id, &frame_buf).map_err(WireReadError::Protocol)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tcp_transport_real_socket_roundtrip() {
        let net_id = NetworkId::Testnet;
        let addr1: SocketAddr = "127.0.0.1:40101".parse().unwrap();
        let addr2: SocketAddr = "127.0.0.1:40102".parse().unwrap();

        let t1 = TcpP2PTransport::new(net_id, addr1);
        let t2 = TcpP2PTransport::new(net_id, addr2);

        t1.start().expect("Start t1");
        t2.start().expect("Start t2");

        thread::sleep(Duration::from_millis(50));

        // t2 connects to t1
        t2.connect_peer(addr1).expect("t2 connect to t1");

        thread::sleep(Duration::from_millis(100));

        // Send ping from t2 to t1
        let ping = WireMessage::Ping(12345);
        assert_eq!(t2.broadcast(&ping), 1);

        // Receive on t1
        let msg = t1
            .recv_inbound_timeout(Duration::from_secs(2))
            .expect("t1 receives ping");

        match msg.message {
            WireMessage::Ping(nonce) => assert_eq!(nonce, 12345),
            _ => panic!("Expected Ping message"),
        }

        // Send pong back from t1
        let pong = WireMessage::Pong(12345);
        assert_eq!(t1.broadcast(&pong), 1);

        // Receive on t2
        let msg2 = t2
            .recv_inbound_timeout(Duration::from_secs(2))
            .expect("t2 receives pong");

        match msg2.message {
            WireMessage::Pong(nonce) => assert_eq!(nonce, 12345),
            _ => panic!("Expected Pong message"),
        }

        t1.shutdown();
        t2.shutdown();
    }
}
