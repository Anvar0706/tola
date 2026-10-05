# ADR-0012: Parallel Execution Pipeline: Block-STM Optimistic Concurrency

## Title
Adoption of Block-STM Optimistic Concurrency for Parallel EVM Execution

## Status
ACCEPTED (Supersedes ADR-0004 execution model)

## Context
ADR-0004 selected EVM compatibility. Research paper `research/07-execution-validation.md` revealed that single-threaded EVM execution caps throughput at 1,500 TPS due to sequential state access locks.

## Problem
Achieving high multi-core execution throughput ($3,000 - 8,000\text{ TPS}$) while preserving 100% compatibility with Solidity smart contracts and standard EVM tooling.

## Options
1. *Single-Threaded EVM*: Simple, but severe throughput bottleneck.
2. *Static Account Access Lists (Solana-style)*: Requires developers to manually declare all accessed accounts; breaks standard EVM tooling and dynamic contract calls.
3. *Block-STM Optimistic Concurrency*: Multi-threaded speculative execution with dynamic multi-version concurrency control (MVCC) conflict detection and re-execution.

## Decision
Adopt **Block-STM optimistic parallel execution** within the Rust EVM (`revm`) execution pipeline.

## Why
Delivers $4\times - 8\times$ speedup on multi-core validator hardware without requiring developers to modify Solidity code or declare static access lists. Outcome is 100% deterministic and equivalent to sequential execution.

## Consequences
Requires implementing MVCC version tracking and dependency validation in the execution harness.

## Security Impact
Deterministic state guarantees; conflicting transactions are rolled back and re-executed automatically.

## Scalability Impact
Enables the single-chain L1 to achieve up to 8,000 real TPS on 16-core servers.

## Migration Impact
Genesis decision.
