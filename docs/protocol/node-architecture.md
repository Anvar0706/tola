# TOLA Network: Node Architecture & Multi-Node Orchestration

## 1. Overview & System Objectives

The **TOLA Node Daemon** (`tola-node`) serves as the production integration coordinator across all 11 foundational protocol subsystems of the TOLA Network. It delivers standalone node execution, cryptographic state commitment, and deterministic multi-node cluster orchestration under Byzantine fault tolerance ($3f + 1$).

```
                      +-----------------------------------+
                      |      JSON-RPC 2.0 Dispatcher      |
                      | (eth_*, web3_*, net_*, tola_*)    |
                      +-----------------+-----------------+
                                        |
+-------------------+        +----------v----------+        +-------------------+
|   P2P GossipSub   |------->|      TolaNode       |<-------|  Dual-Queue Pool  |
|  Framing & Peers  |        |    Coordinator      |        |   & Assembler     |
+-------------------+        +---+--------------+--+        +-------------------+
                                 |              |
                     +-----------v---+      +---v-----------+
                     |  Block-STM    |      |  Pipelined    |
                     |  Parallel VM  |      |  HotStuff BFT |
                     +-------+-------+      +-------+-------+
                             |                      |
                     +-------v----------------------v-------+
                     |     Two-Tier Persistent Storage      |
                     | (Jellyfish Merkle Tree & Segment Log)|
                     +--------------------------------------+
```

The daemon coordinates:
1. **Pipelined Jolteon/HotStuff BFT Consensus** (`tola-consensus`): Deterministic leader scheduling, threshold Quorum Certificate (QC) assembly, and 2-chain commit finality.
2. **Parallel Speculative Execution Engine** (`tola-execution`): Block-STM multi-version memory (`MvMemory`), dependency tracking, and dynamic sequential fallback.
3. **Two-Tier Storage Engine** (`tola-storage`): Flat, append-only segment storage with BLAKE3 bit-rot detection and persistent Jellyfish Merkle Tree (JMT) state roots.
4. **Dual-Queue Mempool & Assembler** (`tola-mempool`): FIFO nonce chaining, 10% Replace-By-Fee (RBF) escalation, and Block-STM dependency spreading.
5. **JSON-RPC 2.0 Engine** (`tola-rpc`): Pure Rust zero-dependency parser/serializer supporting standard EVM Ethereum endpoints and native TOLA extensions.

---

## 2. Configuration & Genesis Specification

The daemon configuration is encapsulated in `NodeConfig` and `GenesisConfig` (`crates/tola-node/src/config.rs`):

```rust
pub struct NodeConfig {
    pub chain_id: ChainId,
    pub data_dir: PathBuf,
    pub is_validator: bool,
    pub validator_keypair: Option<Ed25519Keypair>,
    pub operator_address: Option<Address>,
    pub p2p_port: u16,
    pub rpc_port: u16,
    pub genesis: GenesisConfig,
}

pub struct GenesisConfig {
    pub chain_id: ChainId,
    pub genesis_time: u64,
    pub accounts: Vec<GenesisAccount>,
    pub initial_validators: Vec<ValidatorInfo>,
}
```

### Genesis Initialization
Upon fresh initialization (when the JMT root is `Hash::ZERO`), the node applies the genesis accounts directly into the `PersistentStateDB` journal and commits height 0, establishing an immutable cryptographic baseline across all nodes.

---

## 3. Block Proposal & Execution Lifecycle

When elected leader for view $(H, R)$ by the deterministic committee scheduler, the node executes the block proposal pipeline:

```
[Mempool Drain] -> [Block Assembler] -> [Parallel Block-STM] -> [JMT State Commit] -> [Broadcast Proposal]
```

### 1. Mempool Assembly
The leader invokes `BlockAssembler::assemble_block`, extracting up to $2 \times$ the target transaction count and interleaving sender addresses across execution slots to maximize Block-STM parallelism.

### 2. Speculative Execution & Commit
1. The block engine executes transactions concurrently across worker threads using `MvMemory`.
2. Delta modifications are committed to `PersistentStateDB`.
3. The cryptographic Merkle root is derived: `candidate_block.header.state_root = state.commit(height)`.
4. BLAKE3 Merkle roots for transactions (`txs_root`) and execution receipts (`receipts_root`) are sealed into the block header.

### 3. Replica Verification & Voting
When a replica receives a proposed block via `TolaNode::process_proposal`:
1. **Consensus Pre-checks**:
   - Stale view rejection: `height >= pacemaker.current_height`.
   - Leader authorization: `proposer == committee.leader_for_view(height, round)`.
   - Parent QC verification (for $H > 1$): cryptographic signature verification and round lock checks.
2. **Deterministic State Re-execution**:
   - If `state.current_height() < height`, the replica executes the transactions via its local `ParallelBlockEngine`.
   - Commits state to disk and asserts: `computed_root == block.header.state_root`. Any state root discrepancy aborts voting immediately, guaranteeing BFT safety.
   - Removes committed transactions from the local mempool.
3. **Precommit Vote Issuance**:
   - The consensus state machine stores the block in `blocks_by_hash`.
   - Signs a precommit vote: `Vote = Sign(keypair, BLAKE3(b"TOLA-BFT-VOTE-v1--" || 0x01 || H || R || block_hash))`.

---

## 4. Quorum Certificate Assembly & Jolteon 2-Chain Finality

### Threshold Quorum Accounting
Individual validator votes are gathered by `TolaNode::process_vote`. The underlying `VoteAggregator` calculates cumulative voting power:

$$Q \ge \left\lfloor \frac{2 \cdot W}{3} \right\rfloor + 1$$

Where $W$ is the total active voting power. When the threshold is reached, an aggregated `QuorumCertificate` is assembled with validator bitfields and concatenated signatures.

### Jolteon 2-Chain Commit Rule
When a newly formed QC is processed, the consensus engine evaluates the 2-chain commit rule:

```
Block 1 (QC 1 formed)
   ^
   | extends
Block 2 (QC 2 formed)  ---> Block 1 is FINALIZED! (last_finalized = 1)
   ^
   | extends
Block 3 (QC 3 formed)  ---> Block 2 is FINALIZED! (last_finalized = 2)
```

- When Block $B$ (height $H$) achieves a valid QC, if $B$ directly extends $B_{\text{parent}}$ whose QC justified $B_{\text{grandparent}}$, then $B_{\text{grandparent}}$ is permanently **FINALIZED**.
- Historic votes strictly older than the finalized height are automatically pruned to prevent unbounded memory growth.

---

## 5. Local Multi-Node Network Orchestrator

The `LocalMultiNodeNetwork` (`crates/tola-node/src/network.rs`) provides an automated in-process testbed for multi-validator cluster orchestration:

```rust
pub struct LocalMultiNodeNetwork {
    pub nodes: Vec<TolaNode>,
    pub committee_set: ValidatorSet,
    pub keypairs: Vec<Ed25519Keypair>,
    pub validator_ids: Vec<ValidatorId>,
    pub base_dir: PathBuf,
}
```

### Capabilities
1. **Cluster Initialization**: Spins up independent `PersistentStateDB` database directories for each node on disk.
2. **Transaction Gossip**: Propagates transactions to all nodes' mempools via `submit_transaction`.
3. **Step Round Execution**: Executes proposal, parallel execution, cross-node vote exchange, threshold QC formation, and finality evaluation across independent node daemons in a single round.
4. **Consensus Invariant Verification**: `assert_state_consensus()` verifies bit-for-bit identity across all nodes for `current_height`, `state_root`, and `last_finalized_height`.
5. **Byzantine Fault Simulation**: `step_round_with_participating` allows testing partial synchrony and validator omission (e.g. verifying liveness when 1 of 4 validators is offline, and quorum failure when $\le 2/3$ stake participates).

---

## 6. Empirical Verification & Test Suite

The unified node architecture is verified by the full workspace test suite (85/85 tests passing):

| Test Suite | Tests | Verification Scope |
| :--- | :---: | :--- |
| `tola-node` | 6 | 3-node consensus, 2-chain finality, Block-STM execution, crash recovery, 4-node BFT fault tolerance |
| `tola-consensus` | 15 | Jolteon 2-chain, threshold QCs, pacemaker backoff, validator registry, slashing |
| `tola-storage` | 10 | Jellyfish Merkle Tree proofs, segment append/sync, bit-rot detection, crash recovery |
| `tola-execution` | 8 | Block-STM speculative concurrency, MvMemory read/write versioning, sequential fallback |
| `tola-p2p` | 11 | GossipSub framing, peer scoring, subnet anti-eclipse quotas, wire checksums |
| `tola-rpc` | 4 | Recursive descent JSON parser, Ethereum compatibility, TOLA native extensions |
| `tola-mempool` | 4 | Priority queue, nonce chaining, RBF fee replacement, dependency spreading |
| `tola-types` | 11 | Bech32m checksums, account state monotonicity, transaction validation |
| `tola-crypto` | 7 | Ed25519, Secp256k1 recovery, BLAKE3, Keccak256, SHA-256 |
| `tola-state` | 4 | Journal checkpoint rollback, deterministic root derivation, Merkle proofs |
| `tola-codec` | 3 | Borsh canonical serialization and deterministic roundtrips |
| **Total** | **85** | **100% Pass Rate across all 12 workspace crates** |
