# SUBSYSTEM: Cross-Shard Routing & Asynchronous Messaging

## Objective
Design the asynchronous cross-shard communication protocol, message routing, two-phase commits, and double-spending prevention mechanisms for TOLA Network.

## Why It Exists
In a sharded architecture, transactions regularly span accounts located on different shards (e.g. Alice on Shard 0 sends tokens to Bob on Shard 3). A safe cross-shard messaging layer is required to preserve atomicity and balance invariants.

## Dependencies
- `prompts/23-sharding.md`
- `prompts/25-coordination-layer.md`

## Non-Goals
- Synchronous atomic multi-shard smart contract calls (cross-shard execution is strictly asynchronous).

## Requirements
- Two-Phase Transfer Protocol:
  1. **Phase 1 (Debit on Source Shard)**: Account on Shard $A$ is debited; Shard $A$ emits an **Outbound Cross-Shard Receipt** containing recipient, value, and nonce.
  2. **Phase 2 (Credit on Target Shard)**: Receipt is routed to Shard $B$, verified via Merkle proof against Shard $A$'s header committed to the Coordination Layer, and recipient is credited.
- Replay Protection: Target shard tracks consumed receipt nullifiers to guarantee every receipt is executed exactly once.
- Timeout / Bounce: If target shard contract rejects the message, a refund receipt is emitted back to the source shard.

## Architecture
```
Shard A (Sender)                  Coordination Layer               Shard B (Recipient)
       │                                  │                                 │
       ├── 1. Debit & Emit Receipt ──────►│                                 │
       │                                  ├── 2. Attest Shard A Root ──────►│
       │                                  │                                 ├── 3. Verify Receipt Proof
       │                                  │                                 ├── 4. Credit Recipient
       │◄── 5. (If Failed) Refund ────────┴─────────────────────────────────┤
```

## Interfaces
```rust
pub trait CrossShardRouter {
    fn emit_receipt(&mut self, receipt: CrossShardReceipt);
    fn ingest_receipt(&mut self, receipt: CrossShardReceipt, proof: MerkleProof) -> Result<(), RouteError>;
    fn is_receipt_consumed(&self, receipt_hash: &Hash256) -> bool;
}
```

## Data Structures
```rust
pub struct CrossShardReceipt {
    pub source_shard: u32,
    pub target_shard: u32,
    pub receipt_nonce: u64,
    pub sender: Address,
    pub recipient: Address,
    pub value: u128,
    pub payload: Vec<u8>,
}
```

## Security Requirements
- Cryptographic double-spend prevention: A receipt can NEVER be redeemed more than once across any shard.
- Nullifier tree persistence: Nullifier set must persist across reboots and snapshot prunings.

## Failure Modes
- Stranded cross-shard receipts during network partition (resolved via deterministic timeout bounces).
- Forged receipt injection (prevented by strict Merkle proof verification against coordination layer root).

## Testing Requirements
- Chaos testing: Simulating network delays between shards while executing 10,000 cross-shard transactions.
- Invariant testing: Global token supply remains constant throughout cross-shard flight.

## Performance Requirements
- Cross-shard finality latency $< 4.0\text{ seconds}$ (2 blocks on source + 1 coordination + 1 target).

## Migration Requirements
- Extensible receipt format supporting arbitrary smart contract cross-shard payloads.

## Acceptance Criteria
- Cross-shard specification documented in `docs/sharding/cross-shard-messaging.md`.
- Safety and liveness proofs formulated and verified.

## Open Questions
- Should cross-shard routing use relayer incentives or validator gossip?

## Implementation Tasks
1. Specify receipt data schema and serialization.
2. Define Merkle proof verification algorithm.
3. Author timeout and refund state machine.

## Completion Criteria
Cross-shard protocol specification approved and reviewed against distributed race conditions.
