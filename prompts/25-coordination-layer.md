# SUBSYSTEM: Coordination Layer (Masterchain Protocol)

## Objective
Design the central coordination layer that manages global consensus, validator registration, epoch transitions, shard header commitments, and cross-shard message verification.

## Why It Exists
A multi-shard system requires a root of trust to anchor shard commitments, rotate validator committees, enforce global governance, and guarantee cross-shard finality.

## Dependencies
- `prompts/13-consensus.md`
- `prompts/14-validator-system.md`
- `prompts/23-sharding.md`

## Non-Goals
- Executing user smart contracts or end-user transactions (coordination layer is reserved strictly for consensus and metadata).

## Requirements
- Global Anchor: Every block committed by the coordination layer contains the finalized state roots of all active shardchains.
- Validator Staking Engine: All validator staking, delegation, and slashing occurs on the coordination layer.
- Committee Randomness: Houses the Verifiable Random Function (VRF) beacon to rotate shard committees every epoch.
- Minimal State Footprint: Does not store user balances, preserving maximum bandwidth and memory for coordination.

## Architecture
```
Shard 0 State Root ──┐
Shard 1 State Root ──┼──► Coordination Block Header ──► Global BFT Finality
Shard N State Root ──┘    (Anchor for Light Clients)
```

## Interfaces
```rust
pub trait CoordinationEngine {
    fn register_shard_block(&mut self, header: ShardHeader) -> Result<(), CoordError>;
    fn get_shard_state_root(&self, shard_id: u32, height: u64) -> Option<Hash256>;
    fn advance_epoch(&mut self) -> EpochInfo;
}
```

## Data Structures
```rust
pub struct CoordinationBlock {
    pub height: u64,
    pub parent_hash: [u8; 32],
    pub epoch: u64,
    pub vrf_seed: [u8; 32],
    pub shard_roots: Vec<ShardRootEntry>,
    pub slashing_events: Vec<SlashingEvent>,
}

pub struct ShardRootEntry {
    pub shard_id: u32,
    pub shard_height: u64,
    pub state_root: [u8; 32],
}
```

## Security Requirements
- Maximum Byzantine security: The coordination layer validator set consists of the entire bonded stake of the network.
- Zero non-critical state: Immunity from user contract gas exhaustion or storage bloat.

## Failure Modes
- Coordination layer liveness stall halting global cross-shard finality (individual shards continue local execution).

## Testing Requirements
- Multi-shard anchor verification tests.
- Epoch rotation and VRF seed permutation tests.

## Performance Requirements
- Coordination block commit time $< 1.0\text{ second}$.

## Migration Requirements
- Dynamic registration of newly added shards via governance without hard forks.

## Acceptance Criteria
- Coordination layer specification documented in `docs/sharding/coordination-layer.md`.
- State anchor verification algorithms formally specified.

## Open Questions
- What is the maximum number of shards a single coordination block can aggregate without header size bloat?

## Implementation Tasks
1. Author coordination block header schema.
2. Implement shard root aggregation logic.
3. Specify epoch transition and VRF beacon updates.

## Completion Criteria
Coordination layer specification approved and aligned with consensus architecture.
