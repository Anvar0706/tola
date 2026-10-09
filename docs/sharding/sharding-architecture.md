# TOLA Network: Dynamic State Sharding Architecture

## 1. Executive Summary & Design Principles
TOLA Network enforces a deterministic, horizontally scalable **Dynamic State Sharding Architecture** conforming to **ADR-0014**.

The global account space is partitioned across $K$ independent shardchains coordinated by a central masterchain anchor (**Coordination Layer**). Transactions within a shard execute with parallel optimistic concurrency (Block-STM) at sub-second local latency, while cross-shard interactions utilize asynchronous two-phase receipts with strict cryptographic double-spend immunity.

```
                  ┌───────────────────────────────────────────────┐
                  │       Coordination Layer (Masterchain)        │
                  │   Global State Roots, VRF Seeds & Finality    │
                  └───┬───────────────────────────────┬───────┬───┘
                      │                               │       │
              ┌───────▼───────┐               ┌───────▼─────┐ ┌──▼────────────┐
              │    Shard 0    │◄──Receipts───►│   Shard 1   │ │    Shard K    │
              │ (Accounts 0x0)│               │(Accts 0x1..)│ │(Accounts 0xK) │
              └───────────────┘               └─────────────┘ └───────────────┘
```

---

## 2. Deterministic Shard Partitioning
Accounts are partitioned into $K$ shards ($K \in \{4, 8, 16, 32\}$) using high-order address byte modulo:

$$\text{ShardID} = ((\text{Address}[0] \ll 8) \mid \text{Address}[1]) \pmod K$$

### Mathematical Invariants:
1. **Zero RPC Overhead**: Every node, wallet, and SDK calculates the target shard instantly and locally in $O(1)$ operations without querying full nodes.
2. **Uniform Distribution**: The Keccak-256 / RIPEMD-160 address derivation guarantees uniform distribution across all active shard partitions.
3. **Partition Stability**: Shard assignment is immutable for the lifetime of an account.

---

## 3. Shard Header Specification
Each shard validator committee independently executes local transactions and produces a `ShardHeader`:

```rust
pub struct ShardHeader {
    pub shard_id: ShardId,                     // Shard identifier (0..K-1)
    pub height: BlockHeight,                   // Local shard block height
    pub parent_hash: Hash,                     // Previous shard block hash
    pub state_root: Hash,                      // Jellyfish Merkle Tree state root
    pub receipts_root: Hash,                   // Binary Merkle root of outbound receipts
    pub committee_signatures: Vec<Signature>,  // BFT quorum signatures (>= 2/3 + 1)
}
```

---

## 4. VRF Committee Shuffling & Anti-Takeover Security
A fundamental vulnerability of static sharding is the **1% Shard Takeover Attack**: an attacker with a minority fraction of global stake could concentrate their power to corrupt a single shard.

TOLA Network neutralizes this threat through **Dynamic VRF Committee Rotation**:
- Every epoch, the Coordination Layer rotates its Verifiable Random Function (VRF) seed:
  $$\text{Seed}_{\text{epoch}} = \text{BLAKE3}(\text{Seed}_{\text{epoch}-1} \parallel \text{Entropy}_{\text{beacon}})$$
- For each shard and validator $v$, a pseudo-random election weight is computed:
  $$\text{Score}(v, \text{shard}, \text{epoch}) = \text{BLAKE3}(\text{Seed}_{\text{epoch}} \parallel \text{ShardID} \parallel \text{Epoch} \parallel \text{ValidatorAddress})$$
- Validators are ranked descending by score, selecting the top $M$ validators for the shard committee.
- Any attacker cannot predict or manipulate which shard their validators will be assigned to in advance.

---

## 5. Failure Modes & Graceful Degradation
| Failure Scenario | Impact | Mitigation Mechanism |
|---|---|---|
| **Shard Committee Stalling** | Local shard stops producing blocks | Fallback pacemaker triggers committee rotation and slash non-responsive members |
| **Network Partition** | Receipts delayed between shards | Deterministic timeout and bounce state machine refunds senders |
| **State Tampering Attempt** | Invalid shard block proposed | Merkle proof verification fails against Coordination Layer anchor; attacker slashed 10% |
