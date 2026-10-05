# SUBSYSTEM: Transaction Mempool & Admission Control

## Objective
Design and implement the high-throughput, memory-bounded transaction pool, fee prioritization engine, eviction policies, and anti-DDoS admission filters.

## Why It Exists
The mempool buffers valid incoming transactions from RPC clients and P2P gossip before they are ordered into blocks. Without rigorous admission control, adversaries can exhaust validator memory or flood the network with worthless spam.

## Dependencies
- `prompts/06-transaction-model.md`
- `prompts/07-state-model.md`

## Non-Goals
- Distributed consensus ordering (mempool is local to each node).

## Requirements
- Capacity Bounds: Enforce strict byte limit (e.g. 128 MB) and maximum transaction count (e.g. 50,000 txs).
- Prioritization: Priority queue ordered by effective fee per gas unit.
- Nonce Sequence Tracking: Maintain consecutive transactions per account; buffer future nonces up to a configurable depth ($K=16$).
- Eviction Policy: When capacity is reached, evict transactions with the lowest gas price; reject new transactions that cannot beat the eviction threshold.
- Replacement Rule: Replace-by-Fee (RBF) requires incoming transaction to pay at least $10\%$ higher gas price.

## Architecture
```
Incoming Tx -> Stateless Validation -> Signature Verification
            -> Account Nonce & Balance Check -> Mempool Priority Queue
            -> Broadcast to P2P Gossip -> Block Assembler Pull
```

## Interfaces
```rust
pub trait MempoolService: Send + Sync {
    fn submit_transaction(&self, tx: Transaction) -> Result<TxHash, MempoolError>;
    fn get_ready_transactions(&self, max_count: usize, max_gas: u64) -> Vec<Transaction>;
    fn remove_committed(&self, txs: &[Transaction]);
    fn size(&self) -> MempoolStats;
}
```

## Data Structures
```rust
pub struct MempoolConfig {
    pub max_bytes: usize,
    pub max_transactions: usize,
    pub min_gas_price: u128,
    pub replacement_rate_percent: u8,
}

pub struct MempoolStats {
    pub count: usize,
    pub total_bytes: usize,
    pub lowest_gas_price: u128,
}
```

## Security Requirements
- Spam resistance: Enforce minimum non-zero base gas price to impose real economic cost on spam.
- Memory leak prevention: Strict arena or memory cap; aggressive timeout for uncommitted pending transactions (e.g. 20 minutes).

## Failure Modes
- Mempool saturation causing OOM crashes on small validator nodes.
- Transaction starvation for low-fee transactions during sustained network congestion.

## Testing Requirements
- Load tests simulating 100,000 transactions submitted simultaneously.
- RBF eviction verification tests.

## Performance Requirements
- Insertion latency $< 100\mu s$ per transaction.
- Block packaging pull time $< 10\text{ ms}$ for 5,000 transactions.

## Migration Requirements
- Configurable dynamic pricing curves without binary recompilation.

## Acceptance Criteria
- Mempool specification documented in `docs/architecture/mempool.md`.
- Memory bounds and eviction invariants mathematically verified.

## Open Questions
- Should private transaction lanes (MEV protection) be integrated natively into mempool routing?

## Implementation Tasks
1. Author mempool prioritization data structures.
2. Define per-account nonce queue and eviction policies.
3. Author unit tests for RBF logic and memory limits.

## Completion Criteria
Mempool specification approved, test coverage complete, and integration traits defined.
