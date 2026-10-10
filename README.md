# TOLA Network

[![CI](https://github.com/Anvar0706/tola/actions/workflows/ci.yml/badge.svg)](https://github.com/Anvar0706/tola/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-1.80%2B-orange.svg)](https://www.rust-lang.org/)
[![Node.js](https://img.shields.io/badge/Node.js-20.x-green.svg)](https://nodejs.org/)
[![Python](https://img.shields.io/badge/Python-3.10%2B-blue.svg)](https://www.python.org/)

**TOLA Network** is an independent, sovereign Layer-1 blockchain engineered for high-throughput, deterministic finality, and native EVM Shanghai execution compatibility.

---

## 1. System Architecture

TOLA is architected as a modular, high-assurance distributed state machine:

- **Consensus Engine (`crates/tola-consensus`):** Pipelined HotStuff / Jolteon BFT consensus engine implementing the 2-chain commit finality rule, deterministic view-change pacemaker, BLS12-381/Ed25519 quorum certificates, and Byzantine fault tolerance ($3f + 1$).
- **EVM Execution Core (`crates/tola-execution`):** Embedded Ethereum Shanghai execution environment powered by REVM. Implements full EIP-1559 dynamic fee calculation, EIP-2930 optional access lists, strict 1,024-frame call-depth stack limits, and Block-STM parallel execution scheduling.
- **State Commitment (`crates/tola-state`, `crates/tola-storage`):** Jellyfish Merkle Tree (JMT) using BLAKE3 hashing for state commitments, coupled with transactional key-value storage and append-only segment storage.
- **P2P Networking (`crates/tola-p2p`):** Asynchronous GossipSub message propagation, authenticated peer wire protocol, and peer scoring with anti-Sybil subnet controls.
- **JSON-RPC Engine (`crates/tola-rpc`):** Dual JSON-RPC 2.0 interface exposing standard Ethereum endpoints (`eth_*`, `net_*`, `web3_*`) alongside native TOLA extensions (`tola_*`).
- **Node Daemon & CLI (`crates/tola-node`, `crates/tola-cli`):** Node coordinating all subsystems, accompanied by an operational CLI for key management, offline transaction signing, and network operations.

---

## 2. Multi-Language SDK Suite

TOLA provides production-tested, platform-neutral client SDKs across three languages:

### TypeScript SDK (`sdk/typescript/`)
```bash
cd sdk/typescript
npm install
npm test
```
- EIP-1193 compatible Web3 provider (`TolaProvider`, `JsonRpcProvider`).
- Platform-neutral application bridge (`AppWalletBridge`) and session management (`TolaConnectSession`).
- BIP-39 mnemonic generation, BIP-44 key derivation, and transaction serialization.
- Fully typed TypeScript definitions (`dist/*.d.ts`).

### Python SDK (`sdk/python/`)
```bash
cd sdk/python
pip install -e .
python -m unittest discover -s tests
```
- Strongly-typed Python client library (`tola`).
- Determinstic wallet generation and BIP-44 key derivation (`tola.crypto`, `tola.wallet`).
- Transaction builder and JSON-RPC node client (`tola.client`).

### Rust Client SDK (`crates/tola-sdk/`)
```toml
[dependencies]
tola-sdk = { path = "crates/tola-sdk", version = "0.1.0" }
```
- Strongly-typed Rust bindings for high-performance validator tooling and backend services.
- `TolaClient` with typed JSON-RPC dispatch and `TransactionBuilder`.
- Secret isolation preventing private key leakage in `Debug` implementations.

---

## 3. Repository Structure

```text
├── Cargo.toml                  # Cargo workspace manifest (13 crates)
├── Cargo.lock                  # Deterministic dependency lockfile
├── Dockerfile                  # Multi-stage production container build
├── docker-compose.testnet.yml  # Local multi-node testnet cluster setup
├── crates/
│   ├── tola-cli/               # Operational CLI & ceremony tooling
│   ├── tola-codec/             # Borsh & RLP wire serialization
│   ├── tola-consensus/         # Pipelined HotStuff / Jolteon BFT engine
│   ├── tola-core/              # Core trait definitions and interfaces
│   ├── tola-crypto/            # BLAKE3, Keccak, Ed25519, Secp256k1, BIP-39
│   ├── tola-execution/         # EVM Shanghai & Block-STM runtime
│   ├── tola-mempool/           # Priority-fee mempool with anti-DDoS eviction
│   ├── tola-node/              # Daemon coordinator, genesis & integration tests
│   ├── tola-p2p/               # P2P GossipSub transport & peer wire protocol
│   ├── tola-rpc/               # JSON-RPC 2.0 & WebSocket subscription engine
│   ├── tola-sdk/               # Official Rust client SDK
│   ├── tola-state/             # State journal, delta buffers & JMT trees
│   ├── tola-storage/           # MDBX table schemas & append-only storage
│   └── tola-types/             # Canonical blockchain primitives & types
├── sdk/
│   ├── typescript/             # Official TypeScript SDK (@tola/sdk)
│   └── python/                 # Official Python SDK (tola)
├── contracts/                  # Reference Solidity contracts & test fixtures
├── architecture/               # v0.1 formal architectural specifications
├── research/                   # Protocol empirical validation studies
└── docs/                       # Technical documentation & reports
```

---

## 4. Building and Testing

### Prerequisites
- **Rust:** `1.80.0` or later (`rustup default stable`)
- **Node.js:** `20.x` or later
- **Python:** `3.10` or later
- **Docker:** (Optional) for containerized deployments

### Run the Rust Workspace Test Suite
```bash
cargo test --workspace --all-targets --all-features
```

### Run Code Formatting & Linter Checks
```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

### Run Client SDK Test Suites
```bash
# TypeScript SDK (49 tests)
cd sdk/typescript && node --test test/sdk.test.mjs

# Python SDK (46 tests)
cd ../../sdk/python && python -m unittest discover -s tests

# Rust SDK (10 tests)
cd ../.. && cargo test -p tola-sdk
```

### Build with Docker
```bash
docker build -t tola-node:latest .
```

---

## 5. Network Parameters

| Parameter | Value | Reference |
| :--- | :--- | :--- |
| **Mainnet Chain ID** | `9999` | [docs/protocol/address-encoding.md](docs/protocol/address-encoding.md) |
| **Testnet Chain ID** | `102` | [docs/protocol/address-encoding.md](docs/protocol/address-encoding.md) |
| **Devnet Chain ID** | `1337` | [docs/protocol/address-encoding.md](docs/protocol/address-encoding.md) |
| **Block Time** | `2.0 seconds` | [docs/protocol/consensus-engine.md](docs/protocol/consensus-engine.md) |
| **Finality Rule** | `2-Chain Commit (Jolteon)` | [docs/protocol/consensus-engine.md](docs/protocol/consensus-engine.md) |
| **Base Currency** | `TOLA` ($10^{18}$ base units) | [docs/protocol/native-asset.md](docs/protocol/native-asset.md) |
| **EVM Version** | `Shanghai` (EIP-3860, EIP-3651, EIP-3855, EIP-2930) | [docs/smart-contracts/evm-core-specification.md](docs/smart-contracts/evm-core-specification.md) |
| **BIP-44 Derivation** | `m/44'/9999'/0'/0/0` | [docs/wallet/tola-wallet-derivation-standard-v1.md](docs/wallet/tola-wallet-derivation-standard-v1.md) |

---

## 6. License

This repository is licensed under the [MIT License](LICENSE).
