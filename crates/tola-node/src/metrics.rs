//! Prometheus metrics instrumentation for node runtime observability.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

#[derive(Default)]
pub struct NodeMetrics {
    pub block_height: AtomicU64,
    pub finalized_height: AtomicU64,
    pub consensus_round: AtomicU64,
    pub connected_peers: AtomicUsize,
    pub mempool_size: AtomicUsize,
    pub transactions_processed_total: AtomicU64,
    pub blocks_proposed_total: AtomicU64,
    pub blocks_committed_total: AtomicU64,
    pub votes_received_total: AtomicU64,
    pub qc_formed_total: AtomicU64,
}

impl NodeMetrics {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn set_height(&self, height: u64) {
        self.block_height.store(height, Ordering::SeqCst);
    }

    pub fn set_finalized_height(&self, height: u64) {
        self.finalized_height.store(height, Ordering::SeqCst);
    }

    pub fn set_round(&self, round: u64) {
        self.consensus_round.store(round, Ordering::SeqCst);
    }

    pub fn set_peers(&self, peers: usize) {
        self.connected_peers.store(peers, Ordering::SeqCst);
    }

    pub fn set_mempool_size(&self, size: usize) {
        self.mempool_size.store(size, Ordering::SeqCst);
    }

    pub fn inc_txs(&self, count: u64) {
        self.transactions_processed_total
            .fetch_add(count, Ordering::SeqCst);
    }

    pub fn inc_blocks_proposed(&self) {
        self.blocks_proposed_total.fetch_add(1, Ordering::SeqCst);
    }

    pub fn inc_blocks_committed(&self) {
        self.blocks_committed_total.fetch_add(1, Ordering::SeqCst);
    }

    pub fn inc_votes(&self) {
        self.votes_received_total.fetch_add(1, Ordering::SeqCst);
    }

    pub fn inc_qc(&self) {
        self.qc_formed_total.fetch_add(1, Ordering::SeqCst);
    }

    /// Renders all metrics in Prometheus text exposition format (version 0.0.4).
    pub fn render_prometheus(&self) -> String {
        format!(
            "# HELP tola_block_height Current state block height\n\
# TYPE tola_block_height gauge\n\
tola_block_height {}\n\
# HELP tola_finalized_height Current 2-chain finalized block height\n\
# TYPE tola_finalized_height gauge\n\
tola_finalized_height {}\n\
# HELP tola_consensus_round Current pacemaker view round\n\
# TYPE tola_consensus_round gauge\n\
tola_consensus_round {}\n\
# HELP tola_connected_peers Count of active connected P2P peers\n\
# TYPE tola_connected_peers gauge\n\
tola_connected_peers {}\n\
# HELP tola_mempool_size Pending transactions in mempool\n\
# TYPE tola_mempool_size gauge\n\
tola_mempool_size {}\n\
# HELP tola_transactions_processed_total Cumulative transactions committed\n\
# TYPE tola_transactions_processed_total counter\n\
tola_transactions_processed_total {}\n\
# HELP tola_blocks_proposed_total Blocks proposed by this node\n\
# TYPE tola_blocks_proposed_total counter\n\
tola_blocks_proposed_total {}\n\
# HELP tola_blocks_committed_total Blocks executed and committed\n\
# TYPE tola_blocks_committed_total counter\n\
tola_blocks_committed_total {}\n\
# HELP tola_votes_received_total Quorum votes ingested\n\
# TYPE tola_votes_received_total counter\n\
tola_votes_received_total {}\n\
# HELP tola_qc_formed_total Quorum certificates assembled\n\
# TYPE tola_qc_formed_total counter\n\
tola_qc_formed_total {}\n",
            self.block_height.load(Ordering::SeqCst),
            self.finalized_height.load(Ordering::SeqCst),
            self.consensus_round.load(Ordering::SeqCst),
            self.connected_peers.load(Ordering::SeqCst),
            self.mempool_size.load(Ordering::SeqCst),
            self.transactions_processed_total.load(Ordering::SeqCst),
            self.blocks_proposed_total.load(Ordering::SeqCst),
            self.blocks_committed_total.load(Ordering::SeqCst),
            self.votes_received_total.load(Ordering::SeqCst),
            self.qc_formed_total.load(Ordering::SeqCst)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_node_metrics_rendering() {
        let metrics = NodeMetrics::new();
        metrics.set_height(42);
        metrics.set_finalized_height(40);
        metrics.set_round(5);
        metrics.set_peers(3);
        metrics.set_mempool_size(15);
        metrics.inc_txs(100);
        metrics.inc_blocks_proposed();
        metrics.inc_blocks_committed();
        metrics.inc_votes();
        metrics.inc_qc();

        let rendered = metrics.render_prometheus();
        assert!(rendered.contains("tola_block_height 42"));
        assert!(rendered.contains("tola_finalized_height 40"));
        assert!(rendered.contains("tola_consensus_round 5"));
        assert!(rendered.contains("tola_connected_peers 3"));
        assert!(rendered.contains("tola_mempool_size 15"));
        assert!(rendered.contains("tola_transactions_processed_total 100"));
        assert!(rendered.contains("tola_blocks_proposed_total 1"));
        assert!(rendered.contains("tola_blocks_committed_total 1"));
        assert!(rendered.contains("tola_votes_received_total 1"));
        assert!(rendered.contains("tola_qc_formed_total 1"));
    }
}
