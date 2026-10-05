# ADR-0002: Core Node Implementation Language Strategy

## Title
Rust as Core Node Implementation Language

## Status
ACCEPTED

## Context
A Layer-1 blockchain client requires strict deterministic execution, zero-cost memory safety, high concurrency, fine-grained control over low-level networking, and bare-metal performance without non-deterministic garbage collection pauses.

## Problem
Selecting the primary language and toolchain for the reference TOLA Network validator and node daemon.

## Options
1. *Go*: Fast compilation, simple concurrency model, but runtime garbage collection can induce micro-latency spikes during BFT voting rounds.
2. *C++*: Peak raw performance, but high memory vulnerability risk (use-after-free, buffer overflows).
3. *Rust*: Memory safety without garbage collection, rich type system, outstanding async ecosystem (`tokio`), high-performance cryptographic libraries, industry standard for modern L1s (Solana, Polkadot, Near, Aptos, Sui).

## Decision
Select **Rust** for the core node daemon and consensus protocol. Developer SDKs and tooling will be built primarily in TypeScript/JavaScript, with auxiliary SDKs in Go and Python.

## Why
Guarantees memory safety, eliminates GC jitter during consensus timeouts, and provides the safest foundation for cryptographic state transitions.

## Consequences
Higher initial development rigor and borrow-checker discipline, offset by dramatically reduced runtime crash risks and security exploits.

## Security Impact
Eliminates memory safety vulnerabilities (CVEs) at compile time.

## Scalability Impact
Optimal CPU and memory utilization on validator hardware.

## Migration Impact
None (Genesis).
