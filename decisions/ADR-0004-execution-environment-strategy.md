# ADR-0004: Execution Environment Strategy

## Title
Pluggable VM Architecture with Initial EVM Compatibility

## Status
ACCEPTED

## Context
Developer ecosystem adoption is the primary barrier to entry for any new Layer-1 blockchain. Writing an entirely proprietary virtual machine and custom programming language creates high friction and delays ecosystem growth.

## Problem
How to deliver an execution environment that maximizes developer adoption while maintaining a path toward high-performance parallel execution.

## Options
1. *Proprietary Custom VM*: Highly branded, but zero existing tooling, compilers, or developer familiarity.
2. *Pure WASM VM*: Fast execution, but limited smart contract tooling compared to Ethereum.
3. *EVM Compatibility via Modular Interface*: Immediate support for Solidity, Vyper, Hardhat, Foundry, and MetaMask/EVM toolchains, while abstracting the VM behind a clean internal interface that permits future secondary runtimes (e.g., WASM).

## Decision
Select Option 3: **Modular VM with Initial EVM Compatibility** (utilizing high-performance Rust EVM engines such as `revm`).

## Why
Enables day-one deployment of audited DeFi protocols, token standards (ERC-20/721 equivalents), and battle-tested smart contract libraries.

## Consequences
Must ensure rigorous deterministic gas metering and exact state transition behavior.

## Security Impact
EVM attack vectors are well understood, documented, and protected by mature security tools.

## Scalability Impact
EVM single-thread bottleneck is mitigated by modular architecture and planned parallel state execution.

## Migration Impact
None (Genesis).
