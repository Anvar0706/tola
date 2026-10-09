//! P2P overlay networking, wire protocols, GossipSub topics, and peer reputation scoring.
//!
//! Provides libp2p GossipSub v1.1 pubsub abstraction, binary wire framing,
//! anti-eclipse subnet quotas, and anti-Sybil peer scoring.

pub mod gossip;
pub mod peer;
pub mod service;
pub mod transport;
pub mod wire;

pub use gossip::{
    GossipEnvelope, GossipTopic, MessageId, MessageSeenCache, MAX_BLOCK_GOSSIP_SIZE,
    MAX_TX_GOSSIP_SIZE, MAX_VOTE_GOSSIP_SIZE, TOPIC_BLOCKS, TOPIC_CONSENSUS_VOTES,
    TOPIC_TRANSACTIONS,
};
pub use peer::{IpSubnet, PeerId, PeerScore, PeerScoreManager, PeerScoringParams, SubnetLimiter};
pub use service::{P2PNetwork, P2PNodeConfig, PeerConnectionInfo, SharedP2PService};
pub use transport::{InboundMessage, TcpP2PTransport};
pub use wire::{
    decode_frame, encode_frame, DisconnectReason, HandshakePayload, MessageType, NetworkId,
    WireMessage, FRAME_HEADER_SIZE, MAX_PAYLOAD_SIZE, PROTOCOL_MAGIC, PROTOCOL_VERSION,
};

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum TransportType {
    TcpNoise,
    QuicTls13,
}
