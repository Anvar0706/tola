# ADR-0014: Cross-Shard State Proofs, Dynamic Sharding & Inter-Rollup Bridge Architecture

## Title
Asynchronous Two-Phase Cross-Shard Receipt Routing, Coordination Layer Anchors, and Trust-Minimized State Proof Bridges

## Status
ACCEPTED

## Context
TOLA Network's single-shard execution pipeline achieves tens of thousands of TPS via Block-STM parallel scheduling and MDBX/JMT storage. However, long-term horizontal scalability requires partitioning the global state into $K$ independent shardchains. 

Single-chain monolithic systems hit bandwidth and disk I/O walls. Naive sharding approaches (such as synchronous cross-shard execution) introduce distributed lock contention, cascading aborts, and catastrophic latency degradation. Furthermore, multi-rollup ecosystems and inter-chain bridges require standardized cryptographic inclusion proofs to transfer assets and state without trusting centralized multisig custodians.

## Problem
1. **Cross-Shard Atomicity & Double-Spend Prevention**: How to transfer assets from Shard $A$ to Shard $B$ asynchronously without risk of loss, duplication, or race conditions during network partitions.
2. **Validator Committee Security (1% Takeover Attack)**: How to prevent an adversary with a minority share of global stake from capturing a specific shard committee.
3. **Global State Anchor**: How light clients and external rollups can verify shard state transitions with $O(\log N)$ cryptographic proofs.
4. **Exact Supply Conservation**: How to guarantee mathematically that the total circulating token supply remains invariant while receipts are in flight.

## Decision
1. **Deterministic Address-to-Shard Partitioning**:
   - Accounts are partitioned into $K$ shards ($K \in \{4, 8, 16, 32\}$) using canonical high-order address byte modulo:
     $$\text{ShardID} = (\text{Address}[0] \ll 8 \mid \text{Address}[1]) \pmod K$$
   - Guarantees deterministic, instant local shard routing across all nodes and SDKs without RPC lookups.

2. **Asynchronous Two-Phase Commit Transfer Protocol (2PC-Receipts)**:
   - **Phase 1 (Debit on Source Shard $A$)**:
     * Source account is debited $V + \text{Fee}$.
     * Shard $A$ emits a canonical `CrossShardReceipt` containing:
       - `source_shard`, `target_shard`, `receipt_nonce`
       - `sender`, `recipient`, `value`, `payload`
       - `timeout_height` (source block height after which refund can be claimed)
     * Receipt hash is incorporated into Shard $A$'s `receipts_root`.
   - **Phase 2 (Credit on Target Shard $B$)**:
     * Receipt is relayed to Shard $B$ alongside a cryptographic Merkle inclusion proof against Shard $A$'s header committed to the Coordination Layer.
     * Shard $B$ computes unique cryptographic nullifier:
       $$\text{Nullifier} = \text{BLAKE3}(\text{b"TOLA-RECEIPT-NULLIFIER-v1-" } \parallel \text{source} \parallel \text{target} \parallel \text{nonce} \parallel \text{sender})$$
     * Shard $B$ verifies nullifier is unconsumed in its persistent `NullifierSet`, marks it consumed, and credits `recipient`.

3. **Deterministic Timeout & Bounce State Machine**:
   - If the receipt cannot be delivered, recipient rejects, or block height exceeds `timeout_height`:
   - A `RefundReceipt` is emitted back to Shard $A$.
   - Shard $A$ verifies the bounce proof and restores the sender's balance.
   - Eliminates stranded funds under any partition scenario.

4. **Coordination Masterchain (Beacon Anchor)**:
   - Coordination Layer validator set comprises 100% of bonded network stake.
   - Coordination blocks anchor the finalized state roots and receipt roots of all active shards:
     $$\text{CoordinationBlock} = \{ \text{height}, \text{epoch}, \text{vrf\_seed}, [\text{ShardRootEntry}_0, \dots, \text{ShardRootEntry}_{K-1}] \}$$
   - Rotates shard validator committees pseudo-randomly each epoch using Verifiable Random Functions (VRF), neutralizing 1% takeover attacks.

5. **Global Supply Conservation Invariant**:
   $$\sum_{i=0}^{K-1} \text{StateSupply}(i) + \sum \text{InFlightReceipts} = \text{TotalSupply}$$
   - Every attotola in flight is tracked and cryptographically bonded.

6. **Inter-Rollup / Light Client State Proof Bridge**:
   - Standardized JMT / Merkle inclusion proofs allow external EVM contracts, Solana programs, and L2 rollups to verify TOLA cross-shard state roots in $O(\log N)$ time.

## Consequences
- **Positive**:
  - Horizontal scaling: throughput scales linearly with shard count.
  - Zero synchronous distributed locks: shards operate independently at wire speed.
  - Provable double-spend immunity via cryptographic nullifiers.
  - Clean migration path from single-shard L1 to multi-shard cluster.
- **Negative / Trade-offs**:
  - Cross-shard transactions experience 2-block to 4-block asynchronous latency (~4–6s) compared to intra-shard single-block finality (~2s).
  - Target shards must persist nullifiers indefinitely (mitigated by epoch-bounded pruning of expired receipts).

## Security Analysis
- **Replay Attack**: Prevented by persistent nullifier set. Duplicate receipt submission fails immediately.
- **Forged Receipt Attack**: Ingestion requires Merkle inclusion proof verified against the finalized Coordination Layer state root signed by $\ge 2/3 + 1$ validator stake.
- **Supply Inflation Bug**: Balanced debit/credit state transitions with formal supply tracking make inflation mathematically impossible.

## Migration Path
- Phase 19 establishes the core cryptographic types, receipts, routing engine, nullifier verification, coordination anchors, and bridge state proof verifiers in `tola-types` and `tola-consensus`.
