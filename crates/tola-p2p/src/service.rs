//! P2P network service coordinating wire codecs, GossipSub topics, peer scoring, and broadcasts.
//!
//! Implements `tola_core::P2PService` with anti-eclipse defenses and duplicate message suppression.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::RwLock;

use tola_codec::CanonicalSerialize;
use tola_core::{NetworkError, P2PService};
use tola_types::{Block, BlockHeight, Hash, Transaction, Vote};

use crate::gossip::{GossipTopic, MessageId, MessageSeenCache};
use crate::peer::{PeerId, PeerScoreManager, PeerScoringParams, SubnetLimiter};
use crate::wire::{decode_frame, encode_frame, HandshakePayload, NetworkId, WireMessage};

#[derive(Clone, Debug)]
pub struct PeerConnectionInfo {
    pub peer_id: PeerId,
    pub ip: IpAddr,
    pub head_height: BlockHeight,
    pub head_hash: Hash,
    pub client_version: String,
    pub connected_at: u64,
}

pub struct P2PNodeConfig {
    pub network_id: NetworkId,
    pub genesis_hash: Hash,
    pub local_peer_id: PeerId,
    pub max_peers_per_subnet: usize,
    pub max_seen_cache_size: usize,
    pub scoring_params: PeerScoringParams,
}

impl Default for P2PNodeConfig {
    fn default() -> Self {
        Self {
            network_id: NetworkId::Mainnet,
            genesis_hash: Hash([0x00; 32]),
            local_peer_id: PeerId([0x01; 32]),
            max_peers_per_subnet: 2,
            max_seen_cache_size: 50_000,
            scoring_params: PeerScoringParams::default(),
        }
    }
}

pub struct P2PNetwork {
    config: P2PNodeConfig,
    peers: HashMap<PeerId, PeerConnectionInfo>,
    subnet_limiter: SubnetLimiter,
    score_manager: PeerScoreManager,
    seen_cache: MessageSeenCache,
    current_height: BlockHeight,
    current_head_hash: Hash,
}

impl P2PNetwork {
    pub fn new(
        config: P2PNodeConfig,
        initial_height: BlockHeight,
        initial_head_hash: Hash,
    ) -> Self {
        let max_per_subnet = config.max_peers_per_subnet;
        let seen_capacity = config.max_seen_cache_size;
        let scoring_params = config.scoring_params.clone();

        Self {
            config,
            peers: HashMap::new(),
            subnet_limiter: SubnetLimiter::new(max_per_subnet),
            score_manager: PeerScoreManager::new(scoring_params),
            seen_cache: MessageSeenCache::new(seen_capacity),
            current_height: initial_height,
            current_head_hash: initial_head_hash,
        }
    }

    pub fn local_peer_id(&self) -> PeerId {
        self.config.local_peer_id
    }

    pub fn network_id(&self) -> NetworkId {
        self.config.network_id
    }

    pub fn peer_count(&self) -> usize {
        self.peers.len()
    }

    pub fn is_peer_connected(&self, peer_id: &PeerId) -> bool {
        self.peers.contains_key(peer_id)
    }

    pub fn get_peer_score(&self, peer_id: &PeerId) -> f64 {
        self.score_manager.get_score(peer_id)
    }

    pub fn is_banned(&self, peer_id: &PeerId, current_time: u64) -> bool {
        self.score_manager.is_banned(peer_id, current_time)
    }

    pub fn disconnect_peer(&mut self, peer_id: &PeerId) {
        self.peers.remove(peer_id);
        self.subnet_limiter.unregister_peer(peer_id);
    }

    /// Processes an incoming raw binary wire frame from a network peer.
    pub fn handle_inbound_frame(
        &mut self,
        sender_peer: PeerId,
        sender_ip: IpAddr,
        frame_bytes: &[u8],
        current_time: u64,
    ) -> Result<Option<WireMessage>, NetworkError> {
        // 1. Check if peer is banned
        if self.score_manager.is_banned(&sender_peer, current_time) {
            return Err(NetworkError::PeerBanned);
        }

        // 2. Decode frame with network ID and BLAKE3 checksum verification
        let message = match decode_frame(self.config.network_id, frame_bytes) {
            Ok(msg) => msg,
            Err(e) => {
                // Penalize peer for malformed frame
                let _ = self
                    .score_manager
                    .record_invalid_message(&sender_peer, current_time);
                return Err(e);
            }
        };

        // 3. Process wire message
        match &message {
            WireMessage::Handshake(payload) => {
                // Check network ID
                if payload.network_id != self.config.network_id {
                    let _ = self
                        .score_manager
                        .record_invalid_message(&sender_peer, current_time);
                    return Err(NetworkError::NetworkMismatch);
                }

                // Check genesis hash
                if payload.genesis_hash != self.config.genesis_hash {
                    let _ = self
                        .score_manager
                        .record_invalid_message(&sender_peer, current_time);
                    return Err(NetworkError::GenesisMismatch);
                }

                // Enforce subnet limit
                self.subnet_limiter.register_peer(sender_peer, sender_ip)?;

                // Register peer
                self.peers.insert(
                    sender_peer,
                    PeerConnectionInfo {
                        peer_id: sender_peer,
                        ip: sender_ip,
                        head_height: payload.head_height,
                        head_hash: payload.head_hash,
                        client_version: payload.client_version.clone(),
                        connected_at: current_time,
                    },
                );

                Ok(Some(WireMessage::HandshakeAck {
                    success: true,
                    message: "Welcome to TOLA Network".to_string(),
                }))
            }

            WireMessage::HandshakeAck {
                success,
                message: msg,
            } => {
                if !*success {
                    self.disconnect_peer(&sender_peer);
                    return Err(NetworkError::GossipRejected(msg.clone()));
                }
                Ok(None)
            }

            WireMessage::Ping(nonce) => Ok(Some(WireMessage::Pong(*nonce))),

            WireMessage::Pong(_) => {
                // Keep-alive received
                Ok(None)
            }

            WireMessage::GossipTransaction(tx) => {
                let payload = tx.to_canonical_bytes();
                let msg_id = MessageId::compute(GossipTopic::Transactions, &payload);

                if !self.seen_cache.mark_seen(msg_id) {
                    // Already processed
                    return Ok(None);
                }

                self.score_manager.record_first_delivery(&sender_peer);
                Ok(Some(message))
            }

            WireMessage::GossipBlock(block) => {
                let payload = block.to_canonical_bytes();
                let msg_id = MessageId::compute(GossipTopic::Blocks, &payload);

                if !self.seen_cache.mark_seen(msg_id) {
                    // Already processed
                    return Ok(None);
                }

                self.score_manager.record_first_delivery(&sender_peer);
                Ok(Some(message))
            }

            WireMessage::GossipVote(vote) => {
                let payload = vote.to_canonical_bytes();
                let msg_id = MessageId::compute(GossipTopic::Votes, &payload);

                if !self.seen_cache.mark_seen(msg_id) {
                    // Already processed
                    return Ok(None);
                }

                self.score_manager.record_first_delivery(&sender_peer);
                Ok(Some(message))
            }

            WireMessage::GetBlocks {
                start_height: _,
                max_count: _,
            } => {
                // Handled by higher-level sync engine
                Ok(Some(message))
            }

            WireMessage::Blocks(_) => Ok(Some(message)),

            WireMessage::Disconnect {
                reason: _,
                message: _,
            } => {
                self.disconnect_peer(&sender_peer);
                Ok(None)
            }
        }
    }

    /// Constructs an outbound handshake frame to initiate connection with a remote peer.
    pub fn build_handshake_frame(&self) -> Vec<u8> {
        let handshake = HandshakePayload {
            network_id: self.config.network_id,
            genesis_hash: self.config.genesis_hash,
            head_height: self.current_height,
            head_hash: self.current_head_hash,
            client_version: "tola-p2p/0.1.0".to_string(),
            listen_port: 9000,
        };

        encode_frame(self.config.network_id, &WireMessage::Handshake(handshake))
    }

    /// Encodes a transaction gossip frame for broadcast to mesh peers.
    pub fn build_tx_gossip_frame(&mut self, tx: &Transaction) -> Vec<u8> {
        let payload = tx.to_canonical_bytes();
        let msg_id = MessageId::compute(GossipTopic::Transactions, &payload);
        self.seen_cache.mark_seen(msg_id);

        encode_frame(
            self.config.network_id,
            &WireMessage::GossipTransaction(tx.clone()),
        )
    }

    /// Encodes a block gossip frame for broadcast to mesh peers.
    pub fn build_block_gossip_frame(&mut self, block: &Block) -> Vec<u8> {
        let payload = block.to_canonical_bytes();
        let msg_id = MessageId::compute(GossipTopic::Blocks, &payload);
        self.seen_cache.mark_seen(msg_id);

        encode_frame(
            self.config.network_id,
            &WireMessage::GossipBlock(block.clone()),
        )
    }

    /// Encodes a consensus vote gossip frame for broadcast to mesh peers.
    pub fn build_vote_gossip_frame(&mut self, vote: &Vote) -> Vec<u8> {
        let payload = vote.to_canonical_bytes();
        let msg_id = MessageId::compute(GossipTopic::Votes, &payload);
        self.seen_cache.mark_seen(msg_id);

        encode_frame(
            self.config.network_id,
            &WireMessage::GossipVote(vote.clone()),
        )
    }
}

/// Thread-safe adapter implementing `tola_core::P2PService`.
pub struct SharedP2PService {
    network: RwLock<P2PNetwork>,
}

impl SharedP2PService {
    pub fn new(network: P2PNetwork) -> Self {
        Self {
            network: RwLock::new(network),
        }
    }
}

impl P2PService for SharedP2PService {
    fn broadcast_transaction(&self, tx: &Transaction) -> Result<(), NetworkError> {
        let mut net = self
            .network
            .write()
            .map_err(|_| NetworkError::GossipRejected("Lock error".into()))?;
        let _frame = net.build_tx_gossip_frame(tx);
        Ok(())
    }

    fn broadcast_block(&self, block: &Block) -> Result<(), NetworkError> {
        let mut net = self
            .network
            .write()
            .map_err(|_| NetworkError::GossipRejected("Lock error".into()))?;
        let _frame = net.build_block_gossip_frame(block);
        Ok(())
    }

    fn broadcast_vote(&self, vote: &Vote) -> Result<(), NetworkError> {
        let mut net = self
            .network
            .write()
            .map_err(|_| NetworkError::GossipRejected("Lock error".into()))?;
        let _frame = net.build_vote_gossip_frame(vote);
        Ok(())
    }

    fn peer_count(&self) -> usize {
        let net = self.network.read().expect("Lock unpoisoned");
        net.peer_count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tola_types::*;

    #[test]
    fn test_handshake_flow_and_peer_registration() {
        let genesis = Hash([0x99; 32]);
        let config = P2PNodeConfig {
            network_id: NetworkId::Mainnet,
            genesis_hash: genesis,
            local_peer_id: PeerId([0x01; 32]),
            max_peers_per_subnet: 2,
            max_seen_cache_size: 1000,
            scoring_params: PeerScoringParams::default(),
        };

        let mut node = P2PNetwork::new(config, BlockHeight(0), Hash([0x00; 32]));

        let remote_peer = PeerId([0x02; 32]);
        let remote_ip = "192.168.1.50".parse::<IpAddr>().unwrap();

        let remote_handshake = HandshakePayload {
            network_id: NetworkId::Mainnet,
            genesis_hash: genesis,
            head_height: BlockHeight(0),
            head_hash: Hash([0x00; 32]),
            client_version: "tola-test/0.1.0".to_string(),
            listen_port: 9001,
        };
        let frame = encode_frame(
            NetworkId::Mainnet,
            &WireMessage::Handshake(remote_handshake),
        );

        let res = node
            .handle_inbound_frame(remote_peer, remote_ip, &frame, 1000)
            .unwrap();
        match res {
            Some(WireMessage::HandshakeAck { success, .. }) => assert!(success),
            other => panic!("Expected HandshakeAck, got {other:?}"),
        }

        assert_eq!(node.peer_count(), 1);
        assert!(node.is_peer_connected(&remote_peer));
    }

    #[test]
    fn test_handshake_rejects_incompatible_genesis() {
        let node_genesis = Hash([0x11; 32]);
        let foreign_genesis = Hash([0x22; 32]);

        let config = P2PNodeConfig {
            network_id: NetworkId::Mainnet,
            genesis_hash: node_genesis,
            local_peer_id: PeerId([0x01; 32]),
            ..Default::default()
        };

        let mut node = P2PNetwork::new(config, BlockHeight(0), Hash([0x00; 32]));
        let remote_peer = PeerId([0x02; 32]);
        let remote_ip = "192.168.1.50".parse::<IpAddr>().unwrap();

        let remote_handshake = HandshakePayload {
            network_id: NetworkId::Mainnet,
            genesis_hash: foreign_genesis,
            head_height: BlockHeight(0),
            head_hash: Hash([0x00; 32]),
            client_version: "tola-test/0.1.0".to_string(),
            listen_port: 9001,
        };
        let frame = encode_frame(
            NetworkId::Mainnet,
            &WireMessage::Handshake(remote_handshake),
        );

        let err = node
            .handle_inbound_frame(remote_peer, remote_ip, &frame, 1000)
            .unwrap_err();
        assert_eq!(err, NetworkError::GenesisMismatch);
        assert_eq!(node.peer_count(), 0);
    }

    #[test]
    fn test_gossip_duplicate_suppression_and_scoring() {
        let genesis = Hash([0x11; 32]);
        let config = P2PNodeConfig {
            network_id: NetworkId::Mainnet,
            genesis_hash: genesis,
            local_peer_id: PeerId([0x01; 32]),
            ..Default::default()
        };

        let mut node = P2PNetwork::new(config, BlockHeight(0), Hash([0x00; 32]));
        let peer = PeerId([0x02; 32]);
        let ip = "192.168.1.50".parse::<IpAddr>().unwrap();

        // Register peer first
        let handshake = HandshakePayload {
            network_id: NetworkId::Mainnet,
            genesis_hash: genesis,
            head_height: BlockHeight(0),
            head_hash: Hash([0x00; 32]),
            client_version: "tola-test/0.1.0".to_string(),
            listen_port: 9001,
        };
        let hs_frame = encode_frame(NetworkId::Mainnet, &WireMessage::Handshake(handshake));
        let _ = node.handle_inbound_frame(peer, ip, &hs_frame, 1000);

        // Receive transaction gossip
        let tx = Transaction {
            chain_id: ChainId::MAINNET,
            nonce: Nonce(1),
            from: Address([0x11; 20]),
            to: Some(Address([0x22; 20])),
            value: Amount::from_u128(100),
            gas_limit: Gas(21000),
            max_fee_per_gas: Amount::from_u128(10),
            max_priority_fee_per_gas: Amount::from_u128(2),
            valid_until_block: BlockHeight(50),
            data: vec![],
            memo: None,
            signature: Signature::Secp256k1 {
                r: [0; 32],
                s: [0; 32],
                v: 0,
            },
            tx_type: 0,
            access_list: vec![],
        };
        let tx_frame = encode_frame(
            NetworkId::Mainnet,
            &WireMessage::GossipTransaction(tx.clone()),
        );

        // First delivery -> Accepted and credited to peer score
        let first_res = node
            .handle_inbound_frame(peer, ip, &tx_frame, 1001)
            .unwrap();
        assert!(first_res.is_some());
        assert_eq!(node.get_peer_score(&peer), 1.0);

        // Duplicate delivery -> Suppressed (returns None)
        let dup_res = node
            .handle_inbound_frame(peer, ip, &tx_frame, 1002)
            .unwrap();
        assert!(dup_res.is_none());
        assert_eq!(node.get_peer_score(&peer), 1.0); // Score not incremented again
    }
}
