//! Asynchronous, decoupled chain indexing engine for TOLA JSON-RPC and secondary query caches.
//!
//! Complies with Decision D13 (Decoupled Indexer Architecture).
//! Decouples core state execution and persistent storage from secondary RPC indexes,
//! tracking indexing height, last indexed hash, and indexer lag.
//! Provides restart recovery from StateDB, deterministic re-indexing, and idempotency guarantees.

use crate::dispatcher::{compute_transaction_id, RpcDispatcher};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use tola_core::StateDB;
use tola_types::{Block, BlockHeight, Hash, Receipt};

/// Errors encountered during checked indexing
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum IndexerError {
    NonSequentialHeight {
        expected: u64,
        got: u64,
    },
    HashMismatch {
        height: u64,
        expected: Hash,
        got: Hash,
    },
    StateDBError(String),
}

impl core::fmt::Display for IndexerError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NonSequentialHeight { expected, got } => {
                write!(
                    f,
                    "Non-sequential indexing: expected height {expected}, got {got}"
                )
            }
            Self::HashMismatch {
                height,
                expected,
                got,
            } => {
                write!(
                    f,
                    "Equivocated block at height {height}: existing {:?}, got {:?}",
                    expected, got
                )
            }
            Self::StateDBError(s) => write!(f, "StateDB error during recovery: {s}"),
        }
    }
}

impl std::error::Error for IndexerError {}

/// Observable status of the decoupled chain indexer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexerStatus {
    pub indexed_height: u64,
    pub tip_height: u64,
    pub lag: u64,
    pub indexed_hash: Option<Hash>,
    pub is_synced: bool,
}

/// Decoupled chain indexer tracking canonical blocks into JSON-RPC query caches.
pub struct ChainIndexer<S: StateDB> {
    dispatcher: Arc<RpcDispatcher<S>>,
    tip_height: Arc<AtomicU64>,
    indexed_height: Arc<AtomicU64>,
    indexed_hash: Arc<RwLock<Option<Hash>>>,
}

impl<S: StateDB> ChainIndexer<S> {
    /// Create a new `ChainIndexer` bound to the given RPC dispatcher.
    pub fn new(dispatcher: Arc<RpcDispatcher<S>>) -> Self {
        let init_h = dispatcher.get_current_height();
        Self {
            dispatcher,
            tip_height: Arc::new(AtomicU64::new(init_h)),
            indexed_height: Arc::new(AtomicU64::new(init_h)),
            indexed_hash: Arc::new(RwLock::new(None)),
        }
    }

    /// Return the current tip height known by the node.
    pub fn tip_height(&self) -> u64 {
        self.tip_height.load(Ordering::SeqCst)
    }

    /// Update the known chain tip height (e.g. from consensus commit).
    pub fn set_tip_height(&self, height: u64) {
        self.tip_height.store(height, Ordering::SeqCst);
    }

    /// Return the highest height indexed into RPC caches.
    pub fn indexed_height(&self) -> u64 {
        self.indexed_height.load(Ordering::SeqCst)
    }

    /// Return the block hash at the highest indexed height.
    pub fn indexed_hash(&self) -> Option<Hash> {
        *self.indexed_hash.read().unwrap()
    }

    /// Compute indexer lag (tip_height - indexed_height).
    pub fn lag(&self) -> u64 {
        let tip = self.tip_height();
        let indexed = self.indexed_height();
        tip.saturating_sub(indexed)
    }

    /// Check whether the indexer is fully caught up with the tip.
    pub fn is_synced(&self) -> bool {
        self.lag() == 0
    }

    /// Index a committed block and its execution receipts with strict validation and equivocation detection.
    pub fn index_block_checked(
        &self,
        block: Block,
        receipts: Vec<Receipt>,
    ) -> Result<(), IndexerError> {
        let h = block.header.height.0;
        let block_hash = tola_consensus::PipelinedBftEngine::compute_block_hash(&block);
        let cur_indexed = self.indexed_height.load(Ordering::SeqCst);

        if h <= cur_indexed {
            // Already indexed height: check if hash matches (idempotency check)
            if let Some(existing_hash) = *self.indexed_hash.read().unwrap() {
                if h == cur_indexed && existing_hash != block_hash {
                    return Err(IndexerError::HashMismatch {
                        height: h,
                        expected: existing_hash,
                        got: block_hash,
                    });
                }
            }
            // Idempotent re-index
            self.dispatcher.record_block(block, receipts);
            return Ok(());
        }

        self.index_block(block, receipts);
        Ok(())
    }

    /// Index a committed block and its execution receipts into RPC dispatcher caches (idempotent).
    pub fn index_block(&self, block: Block, receipts: Vec<Receipt>) {
        let h = block.header.height.0;
        let block_hash = tola_consensus::PipelinedBftEngine::compute_block_hash(&block);

        // Update dispatcher RPC caches
        self.dispatcher.record_block(block, receipts);

        // Record indexing progress
        self.indexed_height.store(h, Ordering::SeqCst);
        *self.indexed_hash.write().unwrap() = Some(block_hash);

        // Ensure tip_height tracks at least the indexed height
        let cur_tip = self.tip_height.load(Ordering::SeqCst);
        if h > cur_tip {
            self.tip_height.store(h, Ordering::SeqCst);
        }
    }

    /// Recovers and deterministically re-indexes blocks from `from_height` to `to_height` from StateDB.
    /// Safely handles node restart recovery, cold starts, and re-syncing without state corruption.
    pub fn recover_from_statedb(&self, from_height: u64, to_height: u64) -> Result<u64, String> {
        let blocks_with_receipts: Vec<(Block, Vec<Receipt>)> = {
            let state = self
                .dispatcher
                .state_db
                .read()
                .map_err(|e| format!("Lock error on StateDB: {:?}", e))?;
            let cur_tip = state.current_height().0;
            let end_h = to_height.min(cur_tip);
            let mut items = Vec::new();

            for h in from_height..=end_h {
                if let Ok(Some(block)) = state.get_block_by_height(BlockHeight(h)) {
                    let mut receipts = Vec::new();
                    for tx in &block.transactions {
                        let tx_id = compute_transaction_id(tx);
                        if let Ok(Some(r)) = state.get_receipt(&tx_id) {
                            receipts.push(r);
                        }
                    }
                    items.push((block, receipts));
                }
            }
            items
        };

        let count = blocks_with_receipts.len() as u64;
        for (block, receipts) in blocks_with_receipts {
            self.index_block(block, receipts);
        }
        Ok(count)
    }

    /// Re-indexes all blocks from height 1 up to the latest canonical tip in StateDB.
    pub fn reindex_all(&self) -> Result<u64, String> {
        let tip = {
            let state = self
                .dispatcher
                .state_db
                .read()
                .map_err(|e| format!("Lock error on StateDB: {:?}", e))?;
            state.current_height().0
        };
        self.recover_from_statedb(1, tip)
    }

    /// Capture an immutable snapshot of current indexer health and lag.
    pub fn status(&self) -> IndexerStatus {
        let tip = self.tip_height();
        let indexed = self.indexed_height();
        let lag = tip.saturating_sub(indexed);
        IndexerStatus {
            indexed_height: indexed,
            tip_height: tip,
            lag,
            indexed_hash: self.indexed_hash(),
            is_synced: lag == 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tola_mempool::{Mempool, MempoolConfig};
    use tola_state::MemoryStateJournal;
    use tola_types::{Address, BlockHeader, ChainId, Epoch, Gas, Round, Timestamp};

    fn make_test_block(h: u64, proposer: Address) -> Block {
        Block {
            header: BlockHeader {
                parent_hash: Hash::ZERO,
                height: BlockHeight(h),
                epoch: Epoch(h / 21600),
                round: Round(h as u32),
                timestamp: Timestamp(1000 + h),
                proposer,
                state_root: Hash::ZERO,
                txs_root: Hash::ZERO,
                receipts_root: Hash::ZERO,
                gas_used: Gas::ZERO,
                gas_limit: Gas(30_000_000),
                base_fee_per_gas: tola_types::Amount::ZERO,
                extra_data: Vec::new(),
            },
            transactions: Vec::new(),
            last_commit_qc: None,
        }
    }

    #[test]
    fn test_indexer_status_and_lag() {
        let state = Arc::new(RwLock::new(MemoryStateJournal::new()));
        let mempool = Arc::new(RwLock::new(Mempool::new(MempoolConfig::new(ChainId(1)))));
        let dispatcher = Arc::new(RpcDispatcher::new(
            ChainId(1),
            "tola/0.1.0".to_string(),
            state,
            mempool,
        ));

        let indexer = ChainIndexer::new(dispatcher);
        assert_eq!(indexer.indexed_height(), 0);
        assert_eq!(indexer.tip_height(), 0);
        assert_eq!(indexer.lag(), 0);
        assert!(indexer.is_synced());

        // Simulate tip advancing ahead of indexer
        indexer.set_tip_height(5);
        assert_eq!(indexer.lag(), 5);
        assert!(!indexer.is_synced());

        let status = indexer.status();
        assert_eq!(status.tip_height, 5);
        assert_eq!(status.indexed_height, 0);
        assert_eq!(status.lag, 5);
        assert!(!status.is_synced);

        // Index block 1
        let block1 = make_test_block(1, Address([1u8; 20]));
        indexer.index_block(block1, Vec::new());

        assert_eq!(indexer.indexed_height(), 1);
        assert_eq!(indexer.lag(), 4);
        assert!(!indexer.is_synced());
        assert!(indexer.indexed_hash().is_some());

        // Catch up to tip
        indexer.set_tip_height(1);
        assert_eq!(indexer.lag(), 0);
        assert!(indexer.is_synced());
    }

    #[test]
    fn test_indexer_idempotency_and_equivocation_protection() {
        let state = Arc::new(RwLock::new(MemoryStateJournal::new()));
        let mempool = Arc::new(RwLock::new(Mempool::new(MempoolConfig::new(ChainId(1)))));
        let dispatcher = Arc::new(RpcDispatcher::new(
            ChainId(1),
            "tola/0.1.0".to_string(),
            state,
            mempool,
        ));

        let indexer = ChainIndexer::new(dispatcher);

        let block1 = make_test_block(1, Address([1u8; 20]));

        // First index
        assert!(indexer.index_block_checked(block1.clone(), vec![]).is_ok());
        assert_eq!(indexer.indexed_height(), 1);

        // Idempotent re-index of the exact same block
        assert!(indexer.index_block_checked(block1.clone(), vec![]).is_ok());
        assert_eq!(indexer.indexed_height(), 1);

        // Equivocated block at the same height (different proposer -> different hash)
        let equivocated_block1 = make_test_block(1, Address([2u8; 20]));
        let err = indexer
            .index_block_checked(equivocated_block1, vec![])
            .unwrap_err();
        match err {
            IndexerError::HashMismatch { height, .. } => assert_eq!(height, 1),
            _ => panic!("Expected HashMismatch error"),
        }
    }

    #[test]
    fn test_indexer_restart_recovery_from_statedb() {
        let mut journal = MemoryStateJournal::new();

        // Commit block 1 and block 2 into state
        let block1 = make_test_block(1, Address([1u8; 20]));
        let hash1 = tola_consensus::PipelinedBftEngine::compute_block_hash(&block1);
        journal.record_block(&block1, hash1);
        journal.commit(BlockHeight(1)).unwrap();

        let block2 = make_test_block(2, Address([2u8; 20]));
        let hash2 = tola_consensus::PipelinedBftEngine::compute_block_hash(&block2);
        journal.record_block(&block2, hash2);
        journal.commit(BlockHeight(2)).unwrap();

        let state_arc = Arc::new(RwLock::new(journal));
        let mempool_arc = Arc::new(RwLock::new(Mempool::new(MempoolConfig::new(ChainId(1)))));

        // Create fresh RPC dispatcher and indexer simulating cold restart
        let dispatcher = Arc::new(RpcDispatcher::new(
            ChainId(1),
            "tola/0.1.0".to_string(),
            state_arc,
            mempool_arc,
        ));
        let indexer = ChainIndexer::new(dispatcher.clone());

        // Tip is 2 in state, but indexer indexed_height starts at current_height (or 0)
        indexer.set_tip_height(2);

        // Recover all blocks from StateDB
        let reindexed = indexer
            .recover_from_statedb(1, 2)
            .expect("Recovery succeeds");
        assert_eq!(reindexed, 2);
        assert_eq!(indexer.indexed_height(), 2);
        assert_eq!(indexer.lag(), 0);
        assert!(indexer.is_synced());
        assert_eq!(indexer.indexed_hash(), Some(hash2));

        // Blocks are now accessible in RPC dispatcher
        assert!(dispatcher.blocks_by_height.read().unwrap().contains_key(&1));
        assert!(dispatcher.blocks_by_height.read().unwrap().contains_key(&2));
    }
}
