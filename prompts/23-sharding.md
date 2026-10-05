# SUBSYSTEM: Dynamic State Sharding Architecture

## Objective
Research and design the multi-shard architecture for TOLA Network, enabling parallel transaction execution and partitioned state storage across independent shardchains coordinated by a central layer.

## Why It Exists
Single-chain throughput is fundamentally bounded by individual validator CPU, memory bandwidth, and disk I/O. Sharding provides horizontal scalability to handle tens of thousands of transactions per second.

## Dependencies
- `prompts/10-block-engine.md`
- `prompts/13-consensus.md`
- `prompts/24-cross-shard.md`
- `prompts/25-coordination-layer.md`

## Non-Goals
- Immediate Phase 1 implementation (Sharding is a later phase; design must be established now without premature code).

## Requirements
- Shard Partitioning: Account space partitioned into $K$ shards using address prefix hashing: $\text{ShardID} = \text{Address}[0..N] \pmod K$.
- Shard Committees: Validators pseudo-randomly assigned to shard committees via Verifiable Random Functions (VRF) and reshuffled each epoch.
- Intra-Shard Consensus: Each shard processes internal transactions synchronously and independently, producing shard blocks.
- Shard State Isolation: Nodes assigned to Shard $i$ are only required to store and execute state for Shard $i$, keeping hardware costs low.

## Architecture
```
                  ┌───────────────────────────────┐
                  │      Coordination Layer       │
                  │ (Global State Roots & VRF)    │
                  └───┬───────────────┬───────┬───┘
                      │               │       │
              ┌───────▼───────┐ ┌─────▼─────┐ ┌──▼────────────┐
              │    Shard 0    │ │  Shard 1  │ │    Shard N    │
              │ (Accounts 0x0)│ │(Accts 0x1)│ │(Accounts 0xN) │
              └───────────────┘ └───────────┘ └───────────────┘
```

## Interfaces
```rust
pub trait ShardManager {
    fn get_shard_id(&self, addr: &Address) -> u32;
    fn get_committee(&self, shard_id: u32, epoch: u64) -> Vec<ValidatorInfo>;
    fn submit_shard_header(&mut self, header: ShardHeader) -> Result<(), ShardError>;
}
```

## Data Structures
```rust
pub struct ShardHeader {
    pub shard_id: u32,
    pub height: u64,
    pub parent_hash: [u8; 32],
    pub state_root: [u8; 32],
    pub cross_shard_receipts_root: [u8; 32],
    pub committee_signatures: Vec<VoteSignature>,
}
```

## Security Requirements
- 1% Shard Takeover Attack Prevention: Dynamic, unpredictable validator committee shuffling via VRF prevents attackers with minority stake from capturing a single shard.
- Data Availability: Coordination layer must verify erasure-coded shard data availability before finalizing shard headers.

## Failure Modes
- Single shard stalling due to committee unavailability (fallback rotation protocol).
- Invalid state transition within a shard (slashing and fraud proof challenge).

## Testing Requirements
- Multi-shard network simulation with 4 shards running in Docker.
- Cross-shard reorg resilience testing.

## Performance Requirements
- Linear TPS scaling: $K$ shards achieve $\sim K \times$ single-chain throughput.

## Migration Requirements
- Seamless migration from single-chain to sharded architecture when network load threshold is reached.

## Acceptance Criteria
- Sharding specification documented in `docs/sharding/sharding-architecture.md`.
- Research trade-offs validated against TON, Near, and Ethereum Danksharding.

## Open Questions
- What is the optimal number of initial shards ($K=4, 8, 16$)?

## Implementation Tasks
1. Model address partitioning mathematics.
2. Design VRF committee selection algorithm.
3. Author data availability verification protocol.

## Completion Criteria
Sharding design document completed, reviewed against security threat models, and filed in `docs/sharding/`.
