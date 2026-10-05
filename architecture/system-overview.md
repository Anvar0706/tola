# TOLA Network: System Architecture Overview

## 1. High-Level Architectural Layers

TOLA Network is architected as a modular, high-performance Layer-1 distributed ledger. It is structured into five distinct abstraction layers:

```
┌────────────────────────────────────────────────────────────────────────┐
│                        LAYER 4: ECOSYSTEM & CLIENTS                    │
│   TOLA Messenger (Decoupled) │ TOLA Web Wallet │ Mini Apps │ Explorer  │
└───────────────────────────────────▲────────────────────────────────────┘
                                    │ TOLA Connect / WebSockets / E2EE
┌───────────────────────────────────▼────────────────────────────────────┐
│                        LAYER 3: INTERFACES & TOOLING                   │
│       JSON-RPC 2.0 Engine │ WebSocket Event Stream │ Developer SDKs    │
└───────────────────────────────────▲────────────────────────────────────┘
                                    │ Node Internal Bus
┌───────────────────────────────────▼────────────────────────────────────┐
│                        LAYER 2: STATE & EXECUTION                      │
│   Execution Engine (Modular VM) │ Mempool │ State DB (RocksDB/SMT)     │
└───────────────────────────────────▲────────────────────────────────────┘
                                    │ Block Proposal & Validation
┌───────────────────────────────────▼────────────────────────────────────┐
│                        LAYER 1: CONSENSUS & FINALITY                   │
│   Proof-of-Stake (PoS) Engine │ Fast BFT Finality │ Slashing Engine    │
└───────────────────────────────────▲────────────────────────────────────┘
                                    │ GossipSub / Encrypted Transport
┌───────────────────────────────────▼────────────────────────────────────┐
│                        LAYER 0: NETWORKING & P2P                       │
│    libp2p Transport │ Peer Discovery (Kademlia DHT) │ Peer Scoring     │
└────────────────────────────────────────────────────────────────────────┘
```

---

## 2. Core Subsystem Responsibilities

### Layer 0: Networking & P2P
- **Transport**: Encrypted streams using Noise Protocol Framework or TLS 1.3 over TCP/QUIC.
- **Discovery**: Kademlia DHT for robust peer discovery with Sybil-resistant bootstrap protocols.
- **Propagation**: libp2p GossipSub (v1.1) with message scoring, strict duplicate suppression, and aggressive rate limiting.

### Layer 1: Consensus & Finality
- **Proof-of-Stake**: Validator active set selected based on bonded stake with minimum thresholds and validator rotation epochs.
- **BFT Finality**: Pipelined BFT consensus providing deterministic 1-to-2 block finality. Quorum requires $\ge 2/3 + 1$ validator voting power.
- **Slashing Engine**: Automated cryptographic proof verification for double-signing (equivocation) and liveness fault penalties.

### Layer 2: State & Execution Engine
- **Mempool**: Priority queue ordered by effective tip/fee with strict per-account nonce ordering, eviction policies, and anti-spam size limits.
- **Virtual Machine**: Pluggable VM execution interface. Initial baseline provides full EVM compatibility (Revive/revm engine), enabling immediate ecosystem toolchain support (Solidity, Hardhat, Foundry).
- **State Storage**: High-performance LSM-tree (RocksDB/Pebble) maintaining a Sparse Merkle Tree (SMT) for cryptographic state root commitments.

### Layer 3: Interfaces & Developer Tooling
- **RPC Engine**: High-throughput JSON-RPC 2.0 with both standard Ethereum API compatibility (`eth_*`) and native high-performance TOLA methods (`tola_*`).
- **SDKs**: TypeScript/JavaScript and Rust client libraries providing cryptographic signing, transaction construction, and contract interactions.

### Layer 4: Ecosystem & Decoupled Clients
- **TOLA Connect**: End-to-end encrypted session bridge linking web/mobile applications and Mini Apps to non-custodial user wallets.
- **TOLA Messenger**: Decentralized social communication platform integrating wallet features as an independent client.
