# ADR-0009: Pipelined BFT Consensus (HotStuff/Jolteon) and Realistic Block Timing

## Title
Adoption of Pipelined BFT Consensus with Decoupled Execution and 2.0-Second Target Block Cadence

## Status
ACCEPTED (Supersedes ADR-0003 timing and message model)

## Context
ADR-0003 proposed a generic BFT-PoS mechanism targeting "sub-2-second block times". Research paper `research/06-consensus-validation.md` proved that all-to-all voting gossip incurs $O(N^2)$ message complexity, and cross-continental latency physics makes sub-2-second block times unstable for large validator sets.

## Problem
Establishing an optimal consensus protocol that scales to hundreds of validators with linear message complexity and deterministic, reorg-free finality.

## Options
1. *Classical 2-Phase BFT (Tendermint)*: All-to-all gossip, $O(N^2)$ messages, high latency at $N > 100$.
2. *Nakamoto PoS*: Long probabilistic finality (minutes), vulnerable to reorgs.
3. *Pipelined BFT (HotStuff / Jolteon)*: Leader-based threshold voting with $O(N)$ message complexity in both normal and view-change paths, pipelined commitments, and decoupled execution.

## Decision
Adopt **Pipelined BFT (Jolteon/HotStuff paradigm)** with:
- Linear $O(N)$ message complexity using BLS/Ed25519 signature aggregation.
- Decoupled execution (validating block $H$ while executing block $H-1$).
- Target block time: **$2.0 - 2.5\text{ seconds}$**.
- Deterministic 2-block finality: **$4.0 - 5.0\text{ seconds}$**.

## Why
Provides mathematically proven safety under partial synchrony, eliminates gossip storms, and guarantees stable block production across global validator distributions.

## Consequences
Requires active validator committee rotation (150 active validators) and leader pacemaker timeouts.

## Security Impact
Guarantees zero chain reorganizations once a block achieves a 2-chain Quorum Certificate.

## Scalability Impact
Sustainable operation up to 500+ validators without consensus message blowup.

## Migration Impact
Genesis decision.
