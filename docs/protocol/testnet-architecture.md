# TOLA Network — Testnet Infrastructure & Deployment Architecture (Phase 13)

## 1. Executive Summary

Phase 13 delivers production-grade packaging, containerization, operational tooling, and multi-node testnet orchestration for the TOLA Network Layer-1 protocol. Building upon the 100% complete core protocol implementation (Phases 0–12), Phase 13 converts the modular protocol crates into deployable system binaries, container orchestrations, and comprehensive integration testing harnesses communicating over real TCP sockets.

---

## 2. Infrastructure Architecture & Components

```
+-----------------------------------------------------------------------------------+
|                                  OPERATOR LAYER                                   |
|   +-----------------------+                 +---------------------------------+   |
|   |   tola-cli (CLI Tool) |                 |   External RPC Clients / Wallets|   |
|   +-----------+-----------+                 +----------------+----------------+   |
|               | (JSON-RPC / HTTP)                            | (JSON-RPC)         |
+---------------|----------------------------------------------|--------------------+
                |                                              |
                v                                              v
+-----------------------------------------------------------------------------------+
|                        TOLA NODE DAEMON (tola-node)                               |
|                                                                                   |
|   +---------------------------------------------------------------------------+   |
|   |                       HTTP Server (tola-rpc)                              |   |
|   |       POST / (JSON-RPC 2.0)   |   GET /health   |   GET /metrics          |   |
|   +---------------------------------------------------------------------------+   |
|                                         |                                         |
|   +---------------------------------------------------------------------------+   |
|   |                     Runtime Coordinator & Engine                          |   |
|   |   +--------------------+  +--------------------+  +-------------------+   |   |
|   |   |  Mempool (tola-mp) |  | Execution (stm-vm) |  | Consensus (BFT)   |   |   |
|   |   +--------------------+  +--------------------+  +-------------------+   |   |
|   |   +--------------------+  +--------------------+  +-------------------+   |   |
|   |   | StateDB (tola-st)  |  | Storage (jmt/kv)   |  | Metrics (Prom)    |   |   |
|   |   +--------------------+  +--------------------+  +-------------------+   |   |
|   +---------------------------------------------------------------------------+   |
|                                         |                                         |
|   +---------------------------------------------------------------------------+   |
|   |                     P2P Transport (tola-p2p)                              |   |
|   |   - Binary wire framing (44-byte canonical header with BLAKE3 checksum)   |   |
|   |   - Real non-blocking TCP socket listener & peer connection manager       |   |
|   +---------------------------------------------------------------------------+   |
+-----------------------------------------------------------------------------------+
```

---

## 3. Node Daemon Lifecycle & Deterministic Initialization Order

The `tola-node` binary initializes components in a strict, deterministic sequence:

1. **Genesis Configuration & Validation:**
   - Reads genesis specification file (`--genesis <path>`) or applies default testnet parameters.
   - Computes canonical 32-byte Merkle state root (`compute_genesis_state_root`), guaranteeing account order invariance.
2. **Persistent Storage Layer:**
   - Opens `PersistentStateDB` on `--data-dir/state`.
   - Reconstructs state trees or commits initial genesis account allocations if initializing from an empty database.
3. **Validator Identity & Committee Assembly:**
   - In validator mode (`--validator`), loads 32-byte Ed25519 seed (`--validator-key`) or generates a keypair.
   - Builds active `ValidatorSet` committee from genesis validators and verifies total voting power $W$.
4. **Mempool & Execution Engine:**
   - Instantiates parallel Block-STM execution engine (`ParallelBlockEngine`) with 30,000,000 block gas capacity.
   - Initializes transaction validation queue (`Mempool`) with EIP-1559 base fee accounting.
5. **Consensus Engine (Pipelined HotStuff / Jolteon):**
   - Configures view pacemaker, locked/highest Quorum Certificate tracking, and leader rotation.
6. **P2P Transport Layer:**
   - Binds `TcpP2PTransport` to `--p2p-addr`.
   - Initiates TCP connections to all configured `--bootstrap-peer` endpoints.
7. **HTTP Server (JSON-RPC, Health & Metrics):**
   - Binds `RpcServer` to `--rpc-addr`.
   - Exposes JSON-RPC 2.0 endpoints, `/health` JSON monitor, and `/metrics` Prometheus exposition.
8. **Runtime Coordinator Loop:**
   - Spawns background worker thread to process inbound wire messages (`GossipBlock`, `GossipVote`, `GossipTransaction`).
   - Runs view pacing, leader proposals, vote aggregation, and Jolteon 2-chain commit finalization.

---

## 4. Operational CLI Tooling (`tola-cli`)

The `tola-cli` binary provides an operational suite for nodes, networks, and accounts:

| Command | Description | Example Usage |
| :--- | :--- | :--- |
| `tola-cli status` | Displays node online status, chain ID, height, and peers | `tola-cli status --rpc-url 127.0.0.1:8545` |
| `tola-cli node health` | Checks node `/health` endpoint for readiness probes | `tola-cli node health` |
| `tola-cli genesis generate` | Generates deterministic genesis configuration file | `tola-cli genesis generate --chain-id 102 --validators 4 --output genesis.json` |
| `tola-cli genesis show` | Inspects genesis accounts, validator set, and state root | `tola-cli genesis show --file genesis.json` |
| `tola-cli account new` | Generates fresh Secp256k1 keypair, hex address, and Bech32m | `tola-cli account new` |
| `tola-cli account balance` | Queries account balance via `eth_getBalance` | `tola-cli account balance 0x...` |
| `tola-cli tx transfer` | Assembles, signs, and broadcasts a native coin transfer | `tola-cli tx transfer --to 0x... --amount 5 --private-key 0x...` |
| `tola-cli query block` | Fetches block header, state root, and txs by height or hash | `tola-cli query block 1` |
| `tola-cli query tx` | Fetches transaction receipt, gas used, and status by hash | `tola-cli query tx 0x...` |

---

## 5. Docker Packaging & Testnet Orchestration

### Multi-Stage Container (`Dockerfile`)
- **Stage 1 (Builder):** Debian Bullseye with Rust toolchain compiling optimized release binaries for `tola-node` and `tola-cli`.
- **Stage 2 (Runtime):** Hardened `debian:bookworm-slim` runtime image running as non-root unprivileged user `tola` (UID 1000).
- Exposes port `30303` (P2P TCP) and `8545` (HTTP RPC/Metrics).
- Volume mounted at `/data` for state persistence.
- Embedded healthcheck querying `http://127.0.0.1:8545/health`.

### 4-Validator Testnet Cluster (`docker-compose.testnet.yml`)
- Brings up a 4-node Byzantine Fault Tolerant testnet cluster (`validator-1`, `validator-2`, `validator-3`, `validator-4`).
- P2P Ports: `30301`, `30302`, `30303`, `30304` on isolated bridge network `tola-testnet`.
- RPC Ports: `8541`, `8542`, `8543`, `8544`.
- Shared immutable genesis specification (`genesis.json`).
- Deterministic validator private keys configured via environment/arguments without credentials baked into image layers.

---

## 6. Observability & Prometheus Metrics

Node daemons expose runtime metrics via HTTP GET `/metrics` in Prometheus text exposition format (version 0.0.4):

- `tola_block_height` (gauge): Current state block height.
- `tola_finalized_height` (gauge): Current 2-chain finalized block height under Jolteon rules.
- `tola_consensus_round` (gauge): Current pacemaker view round.
- `tola_connected_peers` (gauge): Active connected P2P socket peers.
- `tola_mempool_size` (gauge): Pending transactions in mempool queue.
- `tola_transactions_processed_total` (counter): Cumulative transactions executed.
- `tola_blocks_proposed_total` (counter): Blocks proposed by this validator.
- `tola_blocks_committed_total` (counter): Blocks executed and committed to storage.
- `tola_votes_received_total` (counter): Precommit votes ingested.
- `tola_qc_formed_total` (counter): Quorum Certificates assembled.

---

## 7. Multi-Node Integration Test Suite

The integration test suite (`crates/tola-node/tests/multi_process_testnet.rs`) verifies real multi-node network behavior:

1. **4-Validator Real TCP Consensus:**
   - 4 independent `TolaNode` instances bound to real loopback TCP sockets.
   - Leader proposals broadcasted over TCP wire framing.
   - Replicas process proposals, execute Block-STM state transitions, emit precommit votes over TCP.
   - Leader collects votes, forms QC, broadcasts QC, and achieves 100% state root convergence across all 4 nodes.
   - Advances to Round 2 with parent QC, achieving Jolteon 2-chain finality on Block 1.
2. **Byzantine Fault Tolerance & Quorum Thresholds:**
   - Total voting power $W = 400,000$, Quorum threshold $Q = 266,667$.
   - 3/4 validators online ($300,000 \ge 266,667$): consensus succeeds and forms QCs.
   - 2/4 validators online ($200,000 < 266,667$): consensus safely stalls, preventing split-brain or fork divergence.
3. **Restart Recovery & State Persistence:**
   - Validator proposes and commits state at height 1.
   - Process shuts down and restarts from the persistent database.
   - Verifies identical state root and block height persistence across reboots.
4. **End-to-End RPC Integration:**
   - Live HTTP requests verifying `/health`, `/metrics`, `eth_chainId`, `eth_getBalance`, and transaction submission.
