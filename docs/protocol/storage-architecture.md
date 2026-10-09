# TOLA Network: Storage Architecture & Database Specification

## 1. Overview & Architectural Motivation
Compliant with **ADR-0008**, TOLA Network enforces a strict **Two-Tier Storage Architecture** designed to eliminate LSM-tree write amplification and compaction stalls while guaranteeing durable crash recovery:

1. **Tier 1 (State Storage):** Memory-mapped B+Tree key-value engine (MDBX model) for mutable active accounts, contract storage slots, and authenticated tree nodes.
2. **Tier 2 (History Storage):** Sequential flat append-only segment files (`segment_XXXXX.seg`) with cryptographic BLAKE3 checksums for immutable block headers, block bodies, and execution receipts.

---

## 2. Table Schemas & Key Layouts

| Table Namespace | Key Schema | Value Schema | Primary Access Pattern |
| :--- | :--- | :--- | :--- |
| `Table::Accounts` | `Address` (20B) | `AccountState` (92B canonical Borsh) | Sub-microsecond point reads & updates |
| `Table::Storage` | `Address` (20B) $\parallel$ `Slot` (32B) [52B] | `Value` (32B) | Smart contract storage reads & writes |
| `Table::TreeNodes` | `NodeHash` (32B BLAKE3) | Canonical JMT Node bytes | Cryptographic Merkle tree traversal & proofs |
| `Table::BlockHeaders` | `BlockHash` (32B) | Canonical `BlockHeader` bytes | Block verification & JSON-RPC |
| `Table::BlockHeightIndex` | `BlockHeight` (8B Big-Endian) | `BlockHash` (32B) | Lexicographical height lookups |
| `Table::BlockHashIndex` | `BlockHash` (32B) | `BlockHeight` (8B Big-Endian) | Canonical chain verification |
| `Table::Transactions` | `TransactionId` (32B) | `SegmentLocation` (16B) | Transaction lookups (`eth_getTransactionByHash`) |
| `Table::Receipts` | `TransactionId` (32B) | Canonical `Receipt` bytes | Transaction receipts (`eth_getTransactionReceipt`) |
| `Table::Metadata` | System Key ASCII string | Arbitrary bytes (e.g. `latest_height`, `state_root`) | Node initialization & startup sync |

---

## 3. Flat Append-Only Segment Architecture

Historical blocks and transactions are immutable. Writing them to B+Trees or LSM trees causes unnecessary page fragmentation and write amplification. 

### 3.1 Segment File Binary Format
All historical records are written to sequential files: `segments/segment_{id:05}.seg` (e.g. `segment_00000.seg`).

```
[0..4]   : Magic Bytes (0x544F4C41 = "TOLA")
[4..8]   : Payload Length (u32 LE)
[8..9]   : Record Type (u8: 0x01=Header, 0x02=Body, 0x03=Receipt)
[9..41]  : Cryptographic Checksum (32 bytes BLAKE3)
[41..]   : Canonical Payload Bytes
```

### 3.2 Segment Pointer & Indexing
Every record location is represented by a compact 16-byte `SegmentLocation`:
```rust
pub struct SegmentLocation {
    pub segment_id: u32,       // 4 bytes LE
    pub byte_offset: u64,      // 8 bytes LE
    pub payload_len: u32,      // 4 bytes LE
}
```
Upon disk read, the node validates magic bytes and asserts that:
$$\text{BLAKE3}(\text{ReadPayload}) == \text{StoredChecksum}$$
Any single-bit corruption or storage medium degradation immediately halts with `StorageError::ChecksumMismatch`, preventing silent state corruption.
