# TOLA Network: State Journal, Speculative Checkpoints & Merkle Commitment

## 1. Overview & Architecture
TOLA Network utilizes an in-memory transactional state journal (`MemoryStateJournal`) backed by persistent zero-copy MDBX tables and a Jellyfish Merkle Tree (JMT) accumulator.

During transaction and block execution, speculative modifications to accounts and storage slots are tracked inside a multi-level checkpoint stack. If a sub-execution or transaction reverts, only the speculative frames are discarded, guaranteeing zero database corruption.

---

## 2. Multi-Level Checkpoint Mechanism

```
    Initial State (Accounts: A=100, B=50)
              │
    ┌─────────▼─────────┐
    │  begin_checkpoint │ ──> Checkpoint 0 pushed
    └─────────┬─────────┘
              │ Debit A: A=80, Credit B: B=70
    ┌─────────▼─────────┐
    │  begin_checkpoint │ ──> Checkpoint 1 pushed (e.g. inner contract call)
    └─────────┬─────────┘
              │ Debit B: B=10 (Reverted due to EVM error!)
    ┌─────────▼─────────┐
    │revert_to_checkpoint 1│ ──> Restores B=70, discards Checkpoint 1
    └─────────┬─────────┘
              │ commit_checkpoint 0
    ┌─────────▼─────────┐
    │ Finalized State   │ ──> Accounts: A=80, B=70
    └───────────────────┘
```

---

## 3. Cryptographic State Root Accumulation

The state root commitment is computed deterministically across all active accounts using BLAKE3 tree hashing:

### 3.1 Leaf Hash Formulation
For each active account, with domain separator `b"TOLA-JMT-LEAF-v1"`:
$$\text{LeafHash}(A) = \text{BLAKE3}(\text{DOMAIN\_LEAF} \parallel A.\text{as\_bytes}() \parallel A.\text{to\_canonical\_bytes}())$$

### 3.2 Pairwise Reduction
Accounts are lexicographically sorted by `Address`. Pairs of adjacent hashes are recursively merged with domain separator `b"TOLA-JMT-NODE-v1"`:
$$\text{NodeHash} = \text{BLAKE3}(\text{DOMAIN\_NODE} \parallel \text{LeftChild} \parallel \text{RightChild})$$
Odd un-paired elements are balanced with themselves.

### 3.3 Inclusion Proofs
The state journal generates compact cryptographic inclusion proofs (`MerkleProof`):
```rust
pub struct MerkleProof {
    pub leaf_index: u64,
    pub sibling_hashes: Vec<Hash>,
}
```
Light clients and external verification bridges verify account inclusion in logarithmic time:
$$\mathcal{O}(\log_2 N)$$
without downloading the full state.
