# Research: High-Performance Blockchain Storage Engines

## 1. Candidate Storage Engines
- **RocksDB**: Industry standard LSM-tree key-value store (C++ with Rust bindings). Highly tuned for fast concurrent writes and point lookups.
- **Pebble**: Go-based LSM-tree alternative modeled after RocksDB. Excellent performance, used in CockroachDB.
- **MDBX (libmdbx)**: Memory-mapped B+tree key-value database. Extremely fast zero-copy reads, ACID compliance, deterministic transaction ordering. Used by Reth (Ethereum).

## 2. State Commitment Architecture
- **Sparse Merkle Tree (SMT)**: Fixed-depth 256-bit Merkle tree. Provides compact cryptographic non-inclusion proofs, deterministic root calculation, and efficient parallel updates.
- **Hybrid Storage Strategy**: High-speed KV store for raw block and state data, coupled with an optimized Sparse Merkle Tree for state root calculation.
