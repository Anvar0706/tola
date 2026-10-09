//! Table schema definitions and key encoding standards for MDBX and TOLA persistent storage.
//!
//! Complies with ADR-0008 and ADR-0010.

use tola_types::{Address, BlockHeight, Hash};

/// Registered table namespaces in the persistent database.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Table {
    /// Active accounts: Address (20B) -> AccountState (92B)
    Accounts,
    /// Smart contract storage slots: (Address (20B) || Slot (32B)) [52B] -> Value (32B)
    Storage,
    /// Jellyfish Merkle Tree persistent nodes: (Version (8B) || NodeHash (32B)) -> Serialized Node bytes
    TreeNodes,
    /// Block headers: BlockHash (32B) -> Canonical BlockHeader bytes
    BlockHeaders,
    /// Block bodies: BlockHash (32B) -> Canonical Block bytes
    BlockBodies,
    /// Height to block hash index: BlockHeight (8B BE) -> BlockHash (32B)
    BlockHeightIndex,
    /// Block hash to height index: BlockHash (32B) -> BlockHeight (8B BE)
    BlockHashIndex,
    /// Transaction lookup: TransactionId (32B) -> Segment Location / Block Hash
    Transactions,
    /// Execution receipts: TransactionId (32B) -> Canonical Receipt bytes
    Receipts,
    /// Database metadata and system checkpoints (LatestHeight, FinalizedHash, ChainId)
    Metadata,
    /// Contract runtime bytecode: CodeHash (32B) -> Bytecode bytes
    Code,
    /// Ethereum transaction lookup: EthTxHash (32B) -> TransactionId (32B)
    EthTxLookup,
    /// Historical contract storage mutations: (Address (20B) || Slot (32B) || BlockHeight (8B BE)) [60B] -> Value (32B)
    StorageHistory,
}

impl Table {
    pub const ALL: &'static [Table] = &[
        Table::Accounts,
        Table::Storage,
        Table::TreeNodes,
        Table::BlockHeaders,
        Table::BlockBodies,
        Table::BlockHeightIndex,
        Table::BlockHashIndex,
        Table::Transactions,
        Table::Receipts,
        Table::Metadata,
        Table::Code,
        Table::EthTxLookup,
        Table::StorageHistory,
    ];

    pub fn name(&self) -> &'static str {
        match self {
            Table::Accounts => "accounts",
            Table::Storage => "storage",
            Table::TreeNodes => "tree_nodes",
            Table::BlockHeaders => "block_headers",
            Table::BlockBodies => "block_bodies",
            Table::BlockHeightIndex => "block_height_index",
            Table::BlockHashIndex => "block_hash_index",
            Table::Transactions => "transactions",
            Table::Receipts => "receipts",
            Table::Metadata => "metadata",
            Table::Code => "code",
            Table::EthTxLookup => "eth_tx_lookup",
            Table::StorageHistory => "storage_history",
        }
    }
}

// ----------------------------------------------------------------------------
// Key Encoding Helpers
// ----------------------------------------------------------------------------

/// Encode 20-byte address into canonical table key.
pub fn encode_account_key(address: &Address) -> [u8; 20] {
    *address.as_bytes()
}

/// Encode 52-byte composite contract storage key: Address (20B) || Slot (32B).
pub fn encode_storage_key(address: &Address, slot: &Hash) -> [u8; 52] {
    let mut key = [0u8; 52];
    key[0..20].copy_from_slice(address.as_bytes());
    key[20..52].copy_from_slice(slot.as_bytes());
    key
}

/// Decode 52-byte composite storage key into Address and Slot.
pub fn decode_storage_key(key: &[u8; 52]) -> (Address, Hash) {
    let mut addr = [0u8; 20];
    addr.copy_from_slice(&key[0..20]);
    let mut slot = [0u8; 32];
    slot.copy_from_slice(&key[20..52]);
    (Address(addr), Hash(slot))
}

/// Encode block height as 8-byte Big-Endian integer (ensures lexicographical order).
pub fn encode_height_key(height: BlockHeight) -> [u8; 8] {
    height.0.to_be_bytes()
}

/// Decode 8-byte Big-Endian block height key.
pub fn decode_height_key(bytes: &[u8; 8]) -> BlockHeight {
    BlockHeight(u64::from_be_bytes(*bytes))
}

/// Encode 32-byte hash as key.
pub fn encode_hash_key(hash: &Hash) -> [u8; 32] {
    *hash.as_bytes()
}

/// Encode JMT node key: NodeHash (32B).
pub fn encode_jmt_node_key(node_hash: &Hash) -> [u8; 32] {
    *node_hash.as_bytes()
}

/// Encode 60-byte historical storage key: Address (20B) || Slot (32B) || BlockHeight (8B BE).
pub fn encode_storage_history_key(address: &Address, slot: &Hash, height: BlockHeight) -> [u8; 60] {
    let mut key = [0u8; 60];
    key[0..20].copy_from_slice(address.as_bytes());
    key[20..52].copy_from_slice(slot.as_bytes());
    key[52..60].copy_from_slice(&height.0.to_be_bytes());
    key
}

/// Decode 60-byte historical storage key into Address, Slot, and BlockHeight.
pub fn decode_storage_history_key(key: &[u8; 60]) -> (Address, Hash, BlockHeight) {
    let mut addr = [0u8; 20];
    addr.copy_from_slice(&key[0..20]);
    let mut slot = [0u8; 32];
    slot.copy_from_slice(&key[20..52]);
    let height = u64::from_be_bytes(key[52..60].try_into().unwrap());
    (Address(addr), Hash(slot), BlockHeight(height))
}
