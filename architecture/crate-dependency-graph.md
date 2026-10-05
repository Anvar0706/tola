# TOLA Network: Crate Architecture & Dependency Graph

## 1. Architectural Boundaries & Modular Principles
TOLA Network is structured as a strict, multi-crate Rust workspace. The primary design invariants are:
1. **Zero Circular Dependencies:** The dependency graph is a strict Directed Acyclic Graph (DAG). Cycles between consensus, storage, execution, and networking are strictly prohibited at compile time.
2. **Type Purity (`tola-types`):** Canonical protocol data types (hashes, addresses, headers, blocks, transactions, receipts) live in an isolated foundation crate with minimal external dependencies.
3. **Decoupled Engine Interfaces (`tola-core`):** Core traits (`StateDB`, `BlockEngine`, `ConsensusEngine`, `VMRuntime`, `MempoolService`, `P2PService`) define abstract behavior without tying consumers to specific database engines or virtual machine implementations.
4. **Zero Protocol Leakage to Client Layers:** RPC, CLI, and node orchestration crates consume protocol engines, but protocol engines never depend on RPC or external clients.

---

## 2. Definitive Crate Dependency DAG

```
                        ┌──────────────┐
                        │  tola-types  │  (Canonical Data Structures & Primitives)
                        └───────┬──────┘
                                │
        ┌───────────────────────┼───────────────────────┐
        ▼                       ▼                       ▼
┌──────────────┐        ┌──────────────┐        ┌──────────────┐
│  tola-codec  │        │ tola-crypto  │        │  tola-core   │  (Trait Contracts & Interfaces)
└───────┬──────┘        └───────┬──────┘        └───────┬──────┘
        │                       │                       │
        └───────────────────────┼───────────────────────┘
                                │
        ┌───────────────────────┼──────────────────────────────────────────┐
        ▼                       ▼                                          ▼
┌──────────────┐        ┌──────────────┐                           ┌──────────────┐
│  tola-state  │        │ tola-mempool │                           │   tola-p2p   │
└───────┬──────┘        └───────┬──────┘                           └───────┬──────┘
        │                       │                                          │
        ▼                       │                                          │
┌──────────────┐                │                                          │
│ tola-storage │                │                                          │
└───────┬──────┘                │                                          │
        │                       │                                          │
        ▼                       │                                          │
┌──────────────┐                │                                          │
│tola-execution│                │                                          │
└───────┬──────┘                │                                          │
        │                       │                                          │
        ▼                       ▼                                          │
┌──────────────┐────────────────┘                                          │
│tola-consensus│                                                           │
└───────┬──────┘                                                           │
        │                                                                  │
        └───────────────────────┬──────────────────────────────────────────┘
                                │
                        ┌───────▼──────┐
                        │  tola-node   │  (Node Daemon, Coordination & Lifecycle)
                        └───────┬──────┘
                                │
                        ┌───────▼──────┐
                        │   tola-rpc   │  (JSON-RPC 2.0 & WebSocket APIs)
                        └──────────────┘
```

---

## 3. Crate Responsibilities & Boundary Invariants

| Crate Name | Primary Responsibility | Direct Dependencies | Prohibited Dependencies |
| :--- | :--- | :--- | :--- |
| `tola-types` | Canonical data structures, integers, hashes, addresses, blocks, txs | None (Zero-dep / standard types) | Any engine or I/O crate |
| `tola-codec` | Canonical serialization (Borsh, RLP wrappers, varints) | `tola-types` | Consensus, storage, execution |
| `tola-crypto`| Ed25519, Secp256k1, BLS12-381, BLAKE3, Keccak, domain separation | `tola-types` | State, consensus, networking |
| `tola-core` | Trait contracts (`StateDB`, `BlockEngine`, `VMRuntime`, `ConsensusEngine`) | `tola-types`, `tola-crypto` | Concrete DB or VM engines |
| `tola-state` | In-memory state journal, MVCC versioning, Jellyfish Merkle Tree | `tola-types`, `tola-crypto`, `tola-core` | P2P, RPC, Consensus |
| `tola-storage`| MDBX key-value tables and append-only block history files | `tola-types`, `tola-core`, `tola-state` | Consensus, RPC |
| `tola-execution`| Block-STM parallel scheduler and `revm` EVM execution engine | `tola-types`, `tola-core`, `tola-state`, `tola-storage`| Consensus, P2P, RPC |
| `tola-consensus`| Pipelined BFT state machine, leader election, voting, QCs, slashing | `tola-types`, `tola-crypto`, `tola-core` | Storage, RPC |
| `tola-mempool`| Priority fee queue, nonce tracking, eviction policies, anti-DDoS | `tola-types`, `tola-core` | P2P wire protocols, RPC |
| `tola-p2p` | libp2p swarm, GossipSub topics, Noise transport, peer scoring | `tola-types`, `tola-codec`, `tola-core` | Storage, Execution |
| `tola-node` | Top-level daemon wiring, signal handling, config, lifecycle orchestration| All protocol crates | RPC implementations |
| `tola-rpc` | JSON-RPC 2.0 engine, Ethereum API compatibility (`eth_*`), WebSockets | `tola-types`, `tola-node`, `tola-core` | Consensus internals |
