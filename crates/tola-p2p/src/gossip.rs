//! GossipSub v1.1 pubsub topic routing, duplicate suppression, and payload filters.
//!
//! Enforces strict topic payload bounds and bounded LRU seen caches to prevent
//! duplicate re-propagation loops and message amplification attacks.

use crate::peer::PeerId;
use std::collections::{HashSet, VecDeque};
use tola_core::NetworkError;
use tola_crypto::blake3_hash;

pub const TOPIC_TRANSACTIONS: &str = "/tola/tx/1.0.0";
pub const TOPIC_BLOCKS: &str = "/tola/block/1.0.0";
pub const TOPIC_CONSENSUS_VOTES: &str = "/tola/vote/1.0.0";

pub const MAX_TX_GOSSIP_SIZE: usize = 128 * 1024; // 128 KB
pub const MAX_BLOCK_GOSSIP_SIZE: usize = 4 * 1024 * 1024; // 4 MB
pub const MAX_VOTE_GOSSIP_SIZE: usize = 64 * 1024; // 64 KB

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum GossipTopic {
    Transactions,
    Blocks,
    Votes,
}

impl GossipTopic {
    pub fn as_str(&self) -> &'static str {
        match self {
            GossipTopic::Transactions => TOPIC_TRANSACTIONS,
            GossipTopic::Blocks => TOPIC_BLOCKS,
            GossipTopic::Votes => TOPIC_CONSENSUS_VOTES,
        }
    }

    pub fn max_size(&self) -> usize {
        match self {
            GossipTopic::Transactions => MAX_TX_GOSSIP_SIZE,
            GossipTopic::Blocks => MAX_BLOCK_GOSSIP_SIZE,
            GossipTopic::Votes => MAX_VOTE_GOSSIP_SIZE,
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            TOPIC_TRANSACTIONS => Some(GossipTopic::Transactions),
            TOPIC_BLOCKS => Some(GossipTopic::Blocks),
            TOPIC_CONSENSUS_VOTES => Some(GossipTopic::Votes),
            _ => None,
        }
    }
}

/// 32-byte cryptographic identifier for gossip messages.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct MessageId(pub [u8; 32]);

impl MessageId {
    pub fn compute(topic: GossipTopic, payload: &[u8]) -> Self {
        let mut data = Vec::with_capacity(topic.as_str().len() + payload.len());
        data.extend_from_slice(topic.as_str().as_bytes());
        data.extend_from_slice(payload);
        let hash = blake3_hash(&data);
        MessageId(hash.0)
    }
}

/// Bounded FIFO/LRU seen cache preventing duplicate message relaying.
pub struct MessageSeenCache {
    capacity: usize,
    queue: VecDeque<MessageId>,
    set: HashSet<MessageId>,
}

impl MessageSeenCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            queue: VecDeque::with_capacity(capacity),
            set: HashSet::with_capacity(capacity),
        }
    }

    /// Checks if message was already seen; if not, inserts it and evicts oldest if full.
    /// Returns true if message was newly inserted (NOT previously seen).
    pub fn mark_seen(&mut self, msg_id: MessageId) -> bool {
        if self.set.contains(&msg_id) {
            return false;
        }

        if self.queue.len() >= self.capacity {
            if let Some(oldest) = self.queue.pop_front() {
                self.set.remove(&oldest);
            }
        }

        self.queue.push_back(msg_id);
        self.set.insert(msg_id);
        true
    }

    pub fn is_seen(&self, msg_id: &MessageId) -> bool {
        self.set.contains(msg_id)
    }

    pub fn len(&self) -> usize {
        self.set.len()
    }

    pub fn is_empty(&self) -> bool {
        self.set.is_empty()
    }
}

/// Complete gossip message container.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct GossipEnvelope {
    pub topic: GossipTopic,
    pub source: PeerId,
    pub payload: Vec<u8>,
    pub message_id: MessageId,
}

impl GossipEnvelope {
    pub fn new(topic: GossipTopic, source: PeerId, payload: Vec<u8>) -> Self {
        let message_id = MessageId::compute(topic, &payload);
        Self {
            topic,
            source,
            payload,
            message_id,
        }
    }

    /// Validates basic gossip constraints: payload size limits.
    pub fn validate_bounds(&self) -> Result<(), NetworkError> {
        if self.payload.len() > self.topic.max_size() {
            return Err(NetworkError::MessageTooLarge);
        }
        if self.payload.is_empty() {
            return Err(NetworkError::MalformedMessage(
                "Empty gossip payload".into(),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_seen_cache_duplicate_suppression_and_bounded_eviction() {
        let mut cache = MessageSeenCache::new(3);

        let id1 = MessageId([1; 32]);
        let id2 = MessageId([2; 32]);
        let id3 = MessageId([3; 32]);
        let id4 = MessageId([4; 32]);

        // First insertions succeed
        assert!(cache.mark_seen(id1));
        assert!(cache.mark_seen(id2));
        assert!(cache.mark_seen(id3));

        // Duplicate insertions rejected
        assert!(!cache.mark_seen(id1));
        assert!(!cache.mark_seen(id2));

        // Inserting 4th evicts oldest (id1)
        assert!(cache.mark_seen(id4));
        assert!(!cache.is_seen(&id1)); // Evicted!
        assert!(cache.is_seen(&id2));
        assert!(cache.is_seen(&id3));
        assert!(cache.is_seen(&id4));
    }

    #[test]
    fn test_gossip_envelope_bounds_validation() {
        let peer = PeerId::random_for_test(1);

        // Valid tx payload
        let valid_env = GossipEnvelope::new(GossipTopic::Transactions, peer, vec![0x11; 500]);
        assert!(valid_env.validate_bounds().is_ok());

        // Empty payload rejected
        let empty_env = GossipEnvelope::new(GossipTopic::Transactions, peer, vec![]);
        assert_eq!(
            empty_env.validate_bounds().unwrap_err(),
            NetworkError::MalformedMessage("Empty gossip payload".into())
        );

        // Oversized tx payload rejected (> 128 KB)
        let oversized_env = GossipEnvelope::new(
            GossipTopic::Transactions,
            peer,
            vec![0x22; MAX_TX_GOSSIP_SIZE + 1],
        );
        assert_eq!(
            oversized_env.validate_bounds().unwrap_err(),
            NetworkError::MessageTooLarge
        );
    }
}
