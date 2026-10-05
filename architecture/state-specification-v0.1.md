# TOLA Network: State Specification v0.1

## 1. Storage & State Tree Architecture

- **State Database:** **MDBX (`libmdbx`)** memory-mapped B+Tree.
- **Commitment Scheme:** **Jellyfish Merkle Tree (JMT)** with BLAKE3 hashing.

---

## 2. Jellyfish Merkle Tree Node Encoding

```rust
pub enum JmtNode {
    Internal {
        children: [Option<ChildInfo>; 16], // 16-ary nibble path compression
    },
    Leaf {
        account_key: [u8; 32], // BLAKE3(Address)
        value_hash: [u8; 32],  // BLAKE3(AccountState)
    },
    Null,
}

pub struct ChildInfo {
    pub version: u64,
    pub hash: [u8; 32],
}
```

---

## 3. MDBX Table Schema

| Table Name | Key Format | Value Format | Purpose |
| :--- | :--- | :--- | :--- |
| `accounts` | `Address` (20 Bytes) | `AccountState` (92 Bytes) | Active account balances & nonces |
| `contract_storage` | `(Address, Slot)` (52 Bytes)| `Value` (32 Bytes) | Smart contract storage variables |
| `tree_nodes` | `(NodeKey, Version)` (40 Bytes)| `SerializedJmtNode` | Persistent Merkle tree branches |
| `block_headers` | `Height` (8 Bytes Big-Endian)| `BlockHeaderBytes` | Canonical block headers |
| `block_hashes` | `BlockHash` (32 Bytes) | `Height` (8 Bytes) | Hash to height index |
