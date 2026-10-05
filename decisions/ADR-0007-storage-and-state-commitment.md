# ADR-0007: Storage Engine and State Commitment Scheme

## Title
LSM-Tree Key-Value Store with Sparse Merkle Tree State Commitment

## Status
ACCEPTED

## Context
Blockchain nodes must persist historical block data, transaction logs, and current account state while calculating cryptographic state roots for every block header to enable light client verification.

## Problem
Selecting the database engine and cryptographic tree structure for state storage.

## Options
1. *Traditional Relational DB (PostgreSQL/SQLite)*: Poor performance under high random key-value write load; complex state hashing.
2. *LevelDB / Merkle Patricia Trie (Ethereum Legacy)*: Proven, but Patricia trie operations generate high disk I/O amplification.
3. *RocksDB / Pebble + Sparse Merkle Tree (SMT)*: LSM-tree architecture delivers peak sequential and random write throughput. Fixed-depth SMT allows compact non-inclusion proofs and parallel updates.

## Decision
Adopt an **LSM-Tree Key-Value Engine (RocksDB/Pebble)** coupled with a **Sparse Merkle Tree (SMT)** for cryptographic state commitments.

## Why
Maximizes write throughput, minimizes write amplification, and provides clean Merkle proofs for light clients and cross-shard validation.

## Consequences
Requires careful caching and tuning of compaction algorithms to avoid write stalls during high-throughput transaction bursts.

## Security Impact
Guarantees tamper-proof cryptographic verification of all account balances and contract storage slots.

## Scalability Impact
Supports high IOPS workloads on commodity NVMe hardware.

## Migration Impact
None (Genesis).
