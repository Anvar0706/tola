# ADR-0008: Storage Engine Selection: MDBX for State and Append-Only Flat Files for History

## Title
Adoption of MDBX (`libmdbx`) for State Database and Flat Append-Only Files for Historical Block Storage

## Status
ACCEPTED (Supersedes ADR-0007 storage layer)

## Context
ADR-0007 selected RocksDB/Pebble (LSM-tree). Research paper `research/08-storage-validation.md` demonstrated that LSM-trees suffer severe write amplification ($15\times - 30\times$) and unpredictable compaction write stalls, which cause validators to miss block proposal deadlines in high-throughput environments.

## Problem
Selecting a storage architecture that guarantees zero compaction stalls, sub-microsecond state lookups, low write amplification, and robust crash recovery.

## Options
1. *RocksDB (LSM-Tree)*: Proven, but heavy write amplification and compaction latency spikes.
2. *Pebble (Go LSM-Tree)*: Incompatible with native Rust node architecture.
3. *MDBX (`libmdbx` B+Tree)*: Memory-mapped, zero-copy reads, zero compaction stalls, ACID compliance, tested extensively in Reth (Ethereum).

## Decision
Adopt **MDBX (`libmdbx`)** for the active account state, smart contract storage slots, and authenticated tree nodes. Use **flat append-only segment files** for historical block headers, bodies, and receipts.

## Why
Eliminates compaction stalls completely, reduces point read latency to $< 1\mu s$, and protects validator NVMe SSDs from premature wear.

## Consequences
Requires pre-allocation of virtual memory space (standard for modern 64-bit systems); state pruning implemented via copy-on-write page recycling.

## Security Impact
Guarantees robust crash recovery and deterministic atomic commits without complex Write-Ahead Log replay risks.

## Scalability Impact
Supports $> 10,000$ state point lookups/sec per CPU core.

## Migration Impact
Genesis decision; no migration required.
