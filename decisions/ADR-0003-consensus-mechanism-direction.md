# ADR-0003: Consensus Mechanism Direction

## Title
Proof-of-Stake with Deterministic BFT Finality

## Status
ACCEPTED

## Context
TOLA Network requires high throughput, sub-second block times, low energy consumption, and deterministic finality to support ecosystem micropayments, digital collectibles, and high-frequency social interactions.

## Problem
Selecting the foundational consensus model.

## Options
1. *Proof-of-Work (PoW)*: Energy intensive, probabilistic finality.
2. *Nakamoto-style PoS*: Long finality latency, reorg risk.
3. *Proof-of-Stake with BFT Finality (Tendermint/HotStuff)*: Deterministic finality in 1-2 blocks, zero reorgs once finalized, 33% Byzantine tolerance.

## Decision
Select **Proof-of-Stake with BFT Finality**. The initial phase will deploy a single-chain validator set with pipelined BFT voting, automated slashing for double-signing, and stake delegation.

## Why
Provides the required speed and economic finality guarantees necessary for real-time mobile and messenger integrations.

## Consequences
Requires active validator set management, staking contracts, unbonding periods, and weak subjectivity checkpoints.

## Security Impact
Guarantees immediate safety against double-spend attacks; network halts rather than forks if Byzantine stake exceeds 33%.

## Scalability Impact
Predictable 1-2 second block generation and instant settlement.

## Migration Impact
None (Genesis).
