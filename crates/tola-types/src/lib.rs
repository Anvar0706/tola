//! Canonical data types and foundation structures for TOLA Network.
//!
//! Provides zero-allocation, type-safe primitives for addresses, transactions,
//! blocks, accounts, and consensus primitives.

pub mod account;
pub mod address;
pub mod block;
pub mod consensus;
pub mod ecosystem;
pub mod primitives;
pub mod sharding;
pub mod transaction;

// Re-export all primary types at crate root for ergonomic access
pub use account::{
    AccountError, AccountState, FLAG_CONTRACT, FLAG_FROZEN, FLAG_NONE, FLAG_VALIDATOR,
};
pub use address::{Address, AddressError, Network};
pub use block::{Block, BlockHeader, Log, Receipt};
pub use consensus::{
    QuorumCertificate, SlashReceipt, SlashingEvidence, ValidatorId, ValidatorInfo, ValidatorSet,
    ValidatorStatus, Vote, VoteType,
};
pub use ecosystem::{
    ClearSigningError, ClearSigningPrompt, ConnectMessageType, DeviceRecord, ExternalAsset,
    ExternalAssetAttestation, ExternalAssetAuthorization, ExternalAssetType,
    EXTERNAL_ASSET_CONTRACT_ADDRESS,
};
pub use primitives::{
    Amount, BlockHeight, ChainId, Epoch, Gas, Hash, Nonce, PublicKey, Round, Signature, Timestamp,
};
pub use sharding::{
    compute_shard_id, CoordinationBlock, CrossShardReceipt, ReceiptNullifier, ReceiptStatus,
    ShardHeader, ShardId, ShardRootEntry, DEFAULT_NUM_SHARDS, DOMAIN_COORD_BLOCK,
    DOMAIN_RECEIPT_HASH, DOMAIN_RECEIPT_NULLIFIER,
};
pub use transaction::{
    AccessListItem, Transaction, TransactionError, TransactionId, MAX_ACCESS_LIST_ITEMS,
    MAX_MEMO_BYTES, MAX_TRANSACTION_PAYLOAD_BYTES, MIN_TRANSACTION_GAS_LIMIT,
};
