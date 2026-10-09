//! Mempool configuration parameters and statistics.
//!
//! Complies with Subsystem Prompt 11 and ADR-0012:
//! - Memory bounds (bytes and transaction counts)
//! - Configurable Replace-By-Fee (RBF) bump rate
//! - Anti-spam minimum gas price thresholds

use tola_types::{Amount, ChainId};

pub const DEFAULT_MAX_TRANSACTIONS: usize = 50_000;
pub const DEFAULT_MAX_MEMPOOL_BYTES: usize = 128 * 1024 * 1024; // 128 MB
pub const DEFAULT_MAX_FUTURE_GAP: u64 = 16;
pub const DEFAULT_RBF_BUMP_PERCENT: u8 = 10; // 10% higher priority fee required

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MempoolConfig {
    pub chain_id: ChainId,
    pub max_transactions: usize,
    pub max_bytes: usize,
    pub max_future_gap: u64,
    pub min_gas_price: Amount,
    pub rbf_bump_percent: u8,
}

impl MempoolConfig {
    pub fn new(chain_id: ChainId) -> Self {
        Self {
            chain_id,
            max_transactions: DEFAULT_MAX_TRANSACTIONS,
            max_bytes: DEFAULT_MAX_MEMPOOL_BYTES,
            max_future_gap: DEFAULT_MAX_FUTURE_GAP,
            min_gas_price: Amount::from_u128(1),
            rbf_bump_percent: DEFAULT_RBF_BUMP_PERCENT,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct MempoolStats {
    pub total_transactions: usize,
    pub ready_transactions: usize,
    pub parked_transactions: usize,
    pub total_bytes: usize,
    pub lowest_priority_fee: Amount,
}
