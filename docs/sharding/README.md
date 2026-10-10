# TOLA Network: Sharding & Cross-Shard Protocols

## Scope & Purpose
Documents the multi-shard horizontal scaling architecture, coordination masterchain, VRF validator assignment, asynchronous cross-shard receipt routing, and inter-rollup state proofs.

## Core Specifications
1. **Dynamic State Sharding**: [sharding-architecture.md](./sharding-architecture.md)
   - High-order byte modulo address partitioning ($O(1)$ local routing).
   - Shard header schema and BFT committee consensus.
   - Dynamic VRF committee rotation neutralizing 1% takeover attacks.
2. **Cross-Shard Messaging & Receipts**: [cross-shard-messaging.md](./cross-shard-messaging.md)
   - Asynchronous Two-Phase Commit transfer lifecycle (2PC-Receipts).
   - Replay protection via persistent BLAKE3 receipt nullifiers.
   - Deterministic timeout and bounce refund state machine.
   - Mathematical token supply conservation invariant.
3. **Coordination Layer (Masterchain)**: [coordination-layer.md](./coordination-layer.md)
   - Global anchor for shard state roots and receipt roots.
   - Epoch transitions, VRF entropy beacon, and committee assignment.
   - Trust-minimized $O(\log N)$ light client and inter-rollup state proof bridges.

## Client SDK Integration
- **Rust**: `tola-types::sharding`, `tola-crypto`, `tola-consensus::sharding`
- **TypeScript**: `@tola/sdk` (`computeShardId`, `computeReceiptNullifier`, `verifyMerkleInclusionProof`)
- **Python**: `tola.sharding` (`compute_shard_id`, `compute_receipt_nullifier`, `verify_merkle_inclusion_proof`)
