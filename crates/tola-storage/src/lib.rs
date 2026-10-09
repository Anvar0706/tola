#![allow(clippy::all)]
//! Storage engine implementations and table schemas for TOLA Network.
//!
//! Complies with ADR-0008 (MDBX for state, append-only flat files for history)
//! and ADR-0010 (Jellyfish Merkle Tree with BLAKE3):
//! - Multi-table isolation and zero-copy lookups
//! - Atomic batch updates with ACID consistency
//! - Append-only flat segment files with BLAKE3 checksums
//! - Persistent 16-ary JMT with compact inclusion/non-inclusion proofs
//! - Persistent StateDB implementation for node execution

pub mod jmt;
pub mod kv;
pub mod persistent_state;
pub mod schema;
pub mod segment;

// Re-export primary types for ergonomic usage
pub use jmt::{
    verify_jmt_proof, JmtNode, JmtProof, NibblePath, PersistentJmt, PersistentJmtReader,
};
pub use kv::{BatchOp, DiskKvStore, KeyValueStore, StorageBatch, StorageError};
pub use persistent_state::PersistentStateDB;
pub use schema::{
    decode_storage_history_key, encode_account_key, encode_hash_key, encode_height_key,
    encode_storage_history_key, encode_storage_key, Table,
};
pub use segment::{
    SegmentLocation, SegmentStorage, RECORD_BLOCK_BODY, RECORD_BLOCK_HEADER, RECORD_RECEIPT,
};
