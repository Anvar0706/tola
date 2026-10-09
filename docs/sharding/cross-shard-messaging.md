# TOLA Network: Cross-Shard Routing & Asynchronous Messaging

## 1. Overview & Two-Phase Receipt Lifecycle
When an account on Shard $A$ sends native assets or contract calls to an account on Shard $B$, the transaction cannot execute synchronously without distributed locking. TOLA Network implements an asynchronous **Two-Phase Commit Transfer Protocol (2PC-Receipts)**.

```
Shard A (Sender)                  Coordination Layer               Shard B (Recipient)
       │                                  │                                 │
       ├── 1. Debit & Emit Receipt ──────►│                                 │
       │                                  ├── 2. Anchor Receipt Root ──────►│
       │                                  │                                 ├── 3. Verify Merkle Proof
       │                                  │                                 ├── 4. Check & Consume Nullifier
       │                                  │                                 ├── 5. Credit Recipient
       │◄── 6. (If Timeout) Refund ───────┴─────────────────────────────────┤
```

---

## 2. CrossShardReceipt Data Model
```rust
pub struct CrossShardReceipt {
    pub source_shard: ShardId,        // Emitting shard
    pub target_shard: ShardId,        // Destination shard
    pub receipt_nonce: u64,           // Monotonic counter per sender/shard
    pub sender: Address,              // Debited sender address
    pub recipient: Address,           // Destination recipient address
    pub value: Amount,                // Transferred token amount
    pub data: Vec<u8>,                // Contract invocation or memo payload
    pub timeout_height: BlockHeight,  // Height after which refund is claimable
}
```

---

## 3. Cryptographic Double-Spend Immunity (Nullifier Tree)
To prevent receipts from being submitted more than once or replayed across epochs, target shards maintain a persistent **Nullifier Set**.

### Nullifier Formulation:
$$\text{Nullifier} = \text{BLAKE3}(\text{b"TOLA-RECEIPT-NULLIFIER-v1-" } \parallel \text{source\_shard} \parallel \text{target\_shard} \parallel \text{nonce} \parallel \text{sender})$$

### Execution Rules:
1. Target shard verifies `nullifier \notin ConsumedNullifiers`.
2. Target shard verifies Merkle inclusion proof against Shard $A$'s receipt root committed to the Coordination Layer.
3. If valid, `nullifier` is permanently inserted into `ConsumedNullifiers` and `recipient` balance is credited.
4. Any duplicate submission immediately aborts with `ShardError::NullifierAlreadyConsumed`.

---

## 4. Deterministic Timeout & Bounce State Machine
If a receipt is not ingested by Shard $B$ before `timeout_height`:
1. Source shard executes `bounce_expired_receipt(receipt, current_height)`.
2. Source shard checks that `current_height >= timeout_height`.
3. Bounce nullifier is marked consumed to prevent duplicate refunds.
4. Sender's balance on Shard $A$ is credited back in full.

---

## 5. Formal Token Supply Conservation Invariant
At every point in time across the multi-shard cluster:

$$\sum_{s=0}^{K-1} \text{StateSupply}(s) + \sum \text{InFlightValue} = \text{TotalSupply}$$

Every attotola in flight between shards is strictly tracked and mathematically conserved.
