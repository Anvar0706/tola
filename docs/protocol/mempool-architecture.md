# TOLA Network: Mempool Architecture & Admission Specification

## 1. Overview & Architectural Motivation
The **TOLA Mempool** (`tola-mempool`) serves as the staging ground for pending transactions awaiting inclusion into proposed blocks. In high-throughput distributed systems, a naive FIFO or single-lock priority queue introduces critical vulnerabilities:
1. **Denial-of-Service (DoS) and Memory Bloat:** Unchecked transaction spam can exhaust node memory.
2. **Head-of-Line Nonce Blocking:** A gap in account nonces can halt the execution of subsequent transactions.
3. **Replace-By-Fee (RBF) Sybil Cycling:** Minor gas price bumps (e.g., +1 wei) can force expensive state re-computations without paying for network resources.
4. **Parallel Execution Abort Cascades:** Packing hundreds of consecutive transactions targeting the exact same smart contract address destroys Block-STM parallel concurrency, degrading multi-threaded execution to worse than single-threaded performance.

To eliminate these vulnerabilities, TOLA Network implements a dual-queue priority mempool coupled with **Block-STM Dependency Spreading**.

---

## 2. Queue Architecture: Ready vs. Parked

Each sender account maintains an independent `AccountQueue` inside the global pool:

```
               [ Incoming Transaction ]
                          |
        +-----------------+-----------------+
        |                                   |
[ Nonce == Next Expected ]       [ Nonce > Next Expected ]
        |                                   |
        v                                   v
   READY QUEUE                         PARKED QUEUE
(Globally prioritized)             (Awaiting prior nonces)
        |                                   |
        | [ Nonce committed / promoted ]    |
        +<----------------------------------+
        |
        v
 [ Block Assembler ]
```

### 2.1 Ready Queue
- Contains transactions whose nonce matches the account's canonical on-chain state or consecutive sequence of ready transactions.
- Transactions are globally ordered in a `BTreeSet<PriorityEntry>` by `(effective_gas_price, -timestamp, tx_hash)` descending.
- The highest priority fee transaction is always available at $O(\log N)$ extraction.

### 2.2 Parked Queue
- Contains future-nonce transactions where intermediate transactions have not yet been received or verified ($N_{\text{incoming}} > N_{\text{expected}}$).
- Nonce gap bounded by `max_future_nonce_gap` (default 64) to prevent long-range storage spam.
- Whenever a transaction at nonce $k$ is executed or admitted, the account chain evaluates `parked` transactions and automatically promotes contiguous nonces $k+1, k+2, \dots$ into the global `ready` queue.

---

## 3. Replace-By-Fee (RBF) Policy

To prevent mempool replacement thrashing, incoming transactions replacing an existing transaction with the identical `(sender, nonce)` must satisfy:

$$\text{EffectiveGasPrice}_{\text{new}} \ge \text{EffectiveGasPrice}_{\text{old}} \times \left(1 + \frac{\text{bump\_bps}}{10000}\right)$$

Where default `bump_bps = 1000` (representing a mandatory $+10\%$ minimum fee escalation).
- If the replacement fee bump is insufficient, the transaction is rejected with `MempoolError::FeeTooLowForReplacement`.
- Upon successful replacement, the existing transaction is removed from the priority set and account queue, and the replacement is inserted while preserving the account's ready sequence invariant.

---

## 4. Admission Control & Capacity Eviction

### 4.1 Admission Invariants
Before entering the mempool, every transaction is evaluated against:
1. **Stateless Validation:** Signature verification, chain ID check, gas limit bounds (`gas_limit <= MAX_BLOCK_GAS`).
2. **Upfront Cost Solvency:** The sender's on-chain balance must cover `value + (gas_limit * max_fee_per_gas)`.
3. **Minimum Base Fee:** `max_fee_per_gas >= min_base_fee`.
4. **Account Limit:** Number of transactions from a single sender must not exceed `max_per_account` (default 100).
5. **Nonce Bounds:** Nonce must be $\ge$ on-chain nonce, and nonce gap must be $\le$ `max_future_nonce_gap`.

### 4.2 Pool Eviction Policy
When the mempool reaches `max_capacity` (default 10,000 transactions):
- The lowest-priority transaction in the pool is inspected.
- If the incoming transaction has a lower effective gas price than the lowest incumbent, the incoming transaction is rejected with `MempoolError::PoolFull`.
- If the incoming transaction has a higher fee, the lowest incumbent is evicted, and the incoming transaction is admitted.

---

## 5. Block-STM Dependency Spreading Assembler

### 5.1 The Parallel Abort Cascade Problem
When hundreds of concurrent transactions invoke the same contract (e.g. popular DEX liquidity pool or viral NFT mint), naive sequential packing causes Block-STM optimistic threads to repeatedly encounter read-after-write conflicts on the same memory locations, triggering up to $78\%$ abort rates.

### 5.2 Round-Robin Address Interleaving
The `BlockAssembler` implements dependency spreading:
1. **Partition:** Fetches the top-ranked ready transactions up to candidate limit and groups them into per-destination FIFO queues (`dest_queues: HashMap<Address, VecDeque<Transaction>>`).
2. **Round-Robin Interleaving:** Cycles through distinct destination addresses, selecting one transaction per address in each round until `max_block_gas` or `max_tx_count` is reached.
3. **Result:** Adjacent transactions in the proposed block access disjoint account storage slots, maximizing multi-core Block-STM concurrency and reducing validation aborts to $< 8\%$.
