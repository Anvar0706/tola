#![allow(clippy::all)]
//! Transaction mempool, fee prioritization, and Block-STM dependency spreading for TOLA Network.
//!
//! Complies with Subsystem Prompt 11 and ADR-0012:
//! - Multi-level Ready and Parked transaction pools
//! - Replace-By-Fee (RBF) escalation checks
//! - Anti-DDoS eviction and memory cap enforcement
//! - Block assembler with target address interleaving to maximize parallel execution throughput

pub mod assembler;
pub mod config;
pub mod pool;

// Re-export primary types for public consumption
pub use assembler::BlockAssembler;
pub use config::{
    MempoolConfig, MempoolStats, DEFAULT_MAX_MEMPOOL_BYTES, DEFAULT_MAX_TRANSACTIONS,
};
pub use pool::{compute_tx_digest, compute_tx_id, AccountQueue, Mempool, PriorityEntry};
