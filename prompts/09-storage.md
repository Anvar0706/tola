# SUBSYSTEM: Persistent Storage Engine & Database Layout

## Objective
Design and implement the high-performance, fault-tolerant persistence layer for blocks, transactions, receipts, indexes, and state data.

## Why It Exists
A node must survive crashes, power cuts, and process restarts without corrupting historical blocks or ledger state. The database engine must support high write IOPS during block commits and rapid reads during transaction execution.

## Dependencies
- `prompts/07-state-model.md`
- `prompts/08-serialization.md`

## Non-Goals
- Distributed databases or cloud-dependent storage clusters (nodes must run locally on standard NVMe drives).

## Requirements
- Embedded Key-Value Storage Engine: **RocksDB** or **Pebble** (or libmdbx for read-heavy state).
- Column Families / Namespaces:
  1. `CF_BLOCK_HEADERS`: Block hash -> Header bytes.
  2. `CF_BLOCK_BODIES`: Block hash -> Serialized transactions.
  3. `CF_BLOCK_HEIGHT`: Height -> Block hash.
  4. `CF_TRANSACTIONS`: Tx hash -> Transaction receipt and block location.
  5. `CF_STATE`: State node hash -> SMT node bytes.
- Atomic commits: A block and all its state updates must commit in a single atomic WriteBatch.
- Configurable pruning: Archive nodes keep all historical state; full nodes prune state older than $K$ epochs while retaining block history.

## Architecture
```
                  ┌───────────────────────────────┐
                  │    TOLA Database Interface    │
                  └───────────────┬───────────────┘
                                  │
         ┌────────────────────────┴────────────────────────┐
         │                                                 │
┌────────▼──────────────┐                         ┌────────▼──────────────┐
│  State DB (LSM / SMT) │                         │ History DB (Blocks/Tx)│
│  - Active Accounts    │                         │ - Append-only blocks  │
│  - Contract Storage   │                         │ - Transaction indexes │
└───────────────────────┘                         └───────────────────────┘
```

## Interfaces
```rust
pub trait StorageEngine: Send + Sync {
    fn put(&self, cf: &str, key: &[u8], value: &[u8]) -> Result<(), StorageError>;
    fn get(&self, cf: &str, key: &[u8]) -> Result<Option<Vec<u8>>, StorageError>;
    fn write_batch(&self, batch: StorageBatch) -> Result<(), StorageError>;
    fn snapshot(&self) -> Box<dyn StorageSnapshot>;
}
```

## Data Structures
```rust
pub struct StorageBatch {
    pub operations: Vec<BatchOp>,
}

pub enum BatchOp {
    Put { cf: String, key: Vec<u8>, value: Vec<u8> },
    Delete { cf: String, key: Vec<u8> },
}
```

## Security Requirements
- Crash consistency: ACID guarantees enforced via Write-Ahead Logging (WAL).
- Checksum validation: Block and state records verified via BLAKE3 checksums upon disk read.

## Failure Modes
- Disk full condition during block commit.
- WAL corruption following sudden kernel panic.

## Testing Requirements
- Power-loss simulation tests (killing process during mid-batch writes).
- Disk read/write benchmark on simulated slow I/O devices.

## Performance Requirements
- Write batch commit $< 15\text{ ms}$ for 5,000 transactions.
- Point read latency $< 50\mu s$ from warm OS cache.

## Migration Requirements
- Schema migration tool to upgrade column families without resyncing from genesis.

## Acceptance Criteria
- Storage specification and column family schema documented in `docs/architecture/storage.md`.
- Database crash recovery procedures verified.

## Open Questions
- Does libmdbx provide superior read concurrency compared to RocksDB for smart contract execution?

## Implementation Tasks
1. Define column family schemas.
2. Author storage traits and RocksDB backend adapter.
3. Implement snapshotting and compaction tuning routines.

## Completion Criteria
Storage specification approved, column family schemas validated, and benchmark suite defined.
