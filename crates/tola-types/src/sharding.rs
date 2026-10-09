//! Dynamic State Sharding, Cross-Shard Receipt Routing & Coordination Layer Types.
//!
//! Conforms strictly to ADR-0014 and core protocol specifications.

use crate::address::Address;
use crate::primitives::{Amount, BlockHeight, Epoch, Hash, Signature};

/// Canonical domain tag for cross-shard receipt hashing.
pub const DOMAIN_RECEIPT_HASH: &[u8] = b"TOLA-RECEIPT-HASH-v1.0.0--";

/// Canonical domain tag for double-spend prevention receipt nullifiers.
pub const DOMAIN_RECEIPT_NULLIFIER: &[u8] = b"TOLA-RECEIPT-NULLIFIER-v1-";

/// Canonical domain tag for coordination masterchain block hashing.
pub const DOMAIN_COORD_BLOCK: &[u8] = b"TOLA-COORD-BLOCK-v1.0.0----";

/// Default number of shards when multi-shard clustering is active.
pub const DEFAULT_NUM_SHARDS: u32 = 4;

/// Shard identifier wrapper (0..K-1).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub struct ShardId(pub u32);

impl ShardId {
    pub const MASTER: Self = Self(0);

    pub fn new(id: u32) -> Self {
        Self(id)
    }

    pub fn as_u32(&self) -> u32 {
        self.0
    }
}

impl std::fmt::Display for ShardId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "shard-{}", self.0)
    }
}

/// Computes the deterministic shard assignment for an address.
///
/// Uses high-order 2 bytes: `((addr.0[0] << 8) | addr.0[1]) % num_shards`.
pub fn compute_shard_id(addr: &Address, num_shards: u32) -> ShardId {
    if num_shards == 0 {
        return ShardId(0);
    }
    let high = (addr.0[0] as u32) << 8 | (addr.0[1] as u32);
    ShardId(high % num_shards)
}

/// Header for a block produced within a specific shardchain.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ShardHeader {
    pub shard_id: ShardId,
    pub height: BlockHeight,
    pub parent_hash: Hash,
    pub state_root: Hash,
    pub receipts_root: Hash,
    pub committee_signatures: Vec<Signature>,
}

/// Status of an asynchronous cross-shard receipt.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReceiptStatus {
    Pending,
    Executed,
    Refunded,
    Expired,
}

/// Asynchronous cross-shard transaction receipt emitted upon debit on the source shard
/// and ingested by the target shard.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CrossShardReceipt {
    pub source_shard: ShardId,
    pub target_shard: ShardId,
    pub receipt_nonce: u64,
    pub sender: Address,
    pub recipient: Address,
    pub value: Amount,
    pub data: Vec<u8>,
    pub timeout_height: BlockHeight,
}

impl CrossShardReceipt {
    /// Returns true if the receipt has passed its expiration height.
    pub fn is_expired(&self, current_source_height: BlockHeight) -> bool {
        current_source_height >= self.timeout_height
    }
}

/// Strongly-typed 32-byte nullifier preventing double-spends of cross-shard receipts.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub struct ReceiptNullifier(pub Hash);

impl ReceiptNullifier {
    pub fn new(hash: Hash) -> Self {
        Self(hash)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0 .0
    }
}

/// Shard state root entry anchored in the Coordination Layer masterchain block.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ShardRootEntry {
    pub shard_id: ShardId,
    pub shard_height: BlockHeight,
    pub state_root: Hash,
    pub receipts_root: Hash,
}

/// Coordination Layer (Masterchain) block anchoring shard states, VRF seeds, and global finality.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CoordinationBlock {
    pub height: BlockHeight,
    pub parent_hash: Hash,
    pub epoch: Epoch,
    pub vrf_seed: Hash,
    pub shard_roots: Vec<ShardRootEntry>,
}

impl CoordinationBlock {
    /// Looks up the state root for a given shard in this coordination block.
    pub fn get_shard_state_root(&self, shard_id: ShardId) -> Option<Hash> {
        self.shard_roots
            .iter()
            .find(|r| r.shard_id == shard_id)
            .map(|r| r.state_root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deterministic_shard_partitioning() {
        // Address starting with 0x00, 0x04 -> high = 4 -> 4 % 4 = 0
        let addr0 = Address([
            0x00, 0x04, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ]);
        assert_eq!(compute_shard_id(&addr0, 4), ShardId(0));

        // Address starting with 0x01, 0x01 -> high = 257 -> 257 % 4 = 1
        let addr1 = Address([
            0x01, 0x01, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ]);
        assert_eq!(compute_shard_id(&addr1, 4), ShardId(1));

        // Same address always maps to identical shard
        assert_eq!(compute_shard_id(&addr1, 4), compute_shard_id(&addr1, 4));
    }

    #[test]
    fn test_receipt_expiration() {
        let receipt = CrossShardReceipt {
            source_shard: ShardId(0),
            target_shard: ShardId(1),
            receipt_nonce: 1,
            sender: Address([0x11; 20]),
            recipient: Address([0x22; 20]),
            value: Amount::from_u128(100),
            data: vec![],
            timeout_height: BlockHeight(500),
        };

        assert!(!receipt.is_expired(BlockHeight(499)));
        assert!(receipt.is_expired(BlockHeight(500)));
        assert!(receipt.is_expired(BlockHeight(501)));
    }

    #[test]
    fn test_coordination_block_state_lookup() {
        let root0 = Hash([0xaa; 32]);
        let root1 = Hash([0xbb; 32]);

        let coord = CoordinationBlock {
            height: BlockHeight(10),
            parent_hash: Hash([0; 32]),
            epoch: Epoch(1),
            vrf_seed: Hash([0x42; 32]),
            shard_roots: vec![
                ShardRootEntry {
                    shard_id: ShardId(0),
                    shard_height: BlockHeight(100),
                    state_root: root0,
                    receipts_root: Hash([0x11; 32]),
                },
                ShardRootEntry {
                    shard_id: ShardId(1),
                    shard_height: BlockHeight(105),
                    state_root: root1,
                    receipts_root: Hash([0x22; 32]),
                },
            ],
        };

        assert_eq!(coord.get_shard_state_root(ShardId(0)), Some(root0));
        assert_eq!(coord.get_shard_state_root(ShardId(1)), Some(root1));
        assert_eq!(coord.get_shard_state_root(ShardId(2)), None);
    }
}
