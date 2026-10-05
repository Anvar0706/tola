//! P2P networking types, GossipSub topics, and transport abstraction.

pub const TOPIC_TRANSACTIONS: &str = "/tola/tx/1.0.0";
pub const TOPIC_BLOCKS: &str = "/tola/block/1.0.0";
pub const TOPIC_CONSENSUS_VOTES: &str = "/tola/vote/1.0.0";

#[derive(Debug, PartialEq, Eq)]
pub enum TransportType {
    TcpNoise,
    QuicTls13,
}

pub struct PeerScore {
    pub score: i32,
    pub is_banned: bool,
}
