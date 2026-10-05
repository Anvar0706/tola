# ADR-0010: State Commitment Scheme: Jellyfish Merkle Tree (JMT)

## Title
Adoption of Jellyfish Merkle Tree (JMT) with BLAKE3 Hashing for Global State Commitments

## Status
ACCEPTED (Supersedes ADR-0007 state commitment scheme)

## Context
ADR-0007 specified a generic Sparse Merkle Tree (SMT). Research paper `research/09-state-commitment-validation.md` demonstrated that a naive 256-bit SMT requires up to 256 hash operations per leaf update, creating a massive CPU bottleneck during high-throughput block commits.

## Problem
Selecting an authenticated data structure that minimizes hashing overhead while generating compact Merkle proofs for light clients.

## Options
1. *Ethereum Hexary MPT*: High disk amplification, complex proofs, slow updates.
2. *Naive Binary SMT*: 256 hashes per update, CPU exhaustion under high TPS.
3. *Verkle Trees*: Very small proofs, but update costs are $100\times$ slower and require trusted curve setups.
4. *Jellyfish Merkle Tree (JMT)*: 16-ary path compression, $8 - 14$ hashes per update, compact proofs ($< 1.5\text{ KB}$), battle-tested in Aptos and Penumbra.

## Decision
Adopt the **Jellyfish Merkle Tree (JMT)** with **BLAKE3 hashing** as TOLA Network's state commitment scheme.

## Why
Reduces block state root computation latency to $< 35\text{ ms}$ for 5,000 updates, provides native non-inclusion proofs, and maps cleanly onto MDBX key-value tables.

## Consequences
Requires implementing JMT path compression and nibble traversal in Rust.

## Security Impact
Tamper-proof cryptographic state commitments with collision-resistant BLAKE3 hashing.

## Scalability Impact
Enables rapid block commits without stalling consensus.

## Migration Impact
Genesis decision.
