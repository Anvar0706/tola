# TOLA Network: Coordination Layer (Masterchain Protocol)

## 1. Role & Purpose
The **Coordination Layer (Masterchain)** serves as the decentralized root of trust for TOLA Network. It anchors shard state commitments, performs epoch transitions, drives Verifiable Random Function (VRF) committee shuffling, and guarantees cross-shard settlement finality.

Conforming to **ADR-0014**, the Coordination Layer executes zero user contracts, preserving 100% of its bandwidth for high-assurance protocol coordination.

```
Shard 0 State Root ──┐
Shard 1 State Root ──┼──► Coordination Block Header ──► Global BFT Finality
Shard K State Root ──┘    (Anchor for Light Clients)
```

---

## 2. CoordinationBlock Schema
```rust
pub struct CoordinationBlock {
    pub height: BlockHeight,              // Monotonic coordination height
    pub parent_hash: Hash,                // Previous coordination block hash
    pub epoch: Epoch,                     // Current network epoch
    pub vrf_seed: Hash,                   // Verifiable Random Function entropy seed
    pub shard_roots: Vec<ShardRootEntry>, // Finalized state & receipt roots of all shards
}

pub struct ShardRootEntry {
    pub shard_id: ShardId,                // Shard identifier (0..K-1)
    pub shard_height: BlockHeight,        // Shardchain height
    pub state_root: Hash,                 // Jellyfish Merkle Tree state root
    pub receipts_root: Hash,              // Cross-shard outbound receipt root
}
```

---

## 3. Light Client & Inter-Rollup State Proofs
External consumers (EVM contracts, Solana programs, IBC bridges, and mobile light clients) verify that a user balance or cross-shard receipt was finalized without downloading the full shard history:

1. Retrieve the `CoordinationBlock` header verified by $\ge 2/3 + 1$ validator stake.
2. Verify the `ShardRootEntry` for the target shard in the coordination header.
3. Verify the binary Merkle proof of the receipt or account leaf against the `receipts_root` or `state_root`.

$$\text{Proof Complexity} = O(\log N) \text{ cryptographic hashes}$$
Verification cost on EVM bridges is $< 60,000$ gas.

---

## 4. VRF Committee Shuffling Protocol
1. **Entropy Beacon**: Every epoch transition ingests threshold BLS entropy from the League of Entropy (Drand) or internal BFT random beacon:
   $$\text{Seed}_{e+1} = \text{BLAKE3}(\text{Seed}_e \parallel \text{BeaconEntropy})$$
2. **Deterministic Shuffling**:
   $$\text{Rank}(v) = \text{BLAKE3}(\text{Seed}_{e+1} \parallel \text{ShardID} \parallel \text{Epoch} \parallel \text{ValidatorAddress})$$
3. **Sybil & Takeover Immunity**: Validator stake-ranking combined with unbiased VRF entropy ensures that corrupting a single shard requires compromising $> 2/3$ of the *global* network stake.
