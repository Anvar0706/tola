# Research 08: Persistent Storage Engine & Database Validation

## 1. Executive Summary & Challenge of Phase 0 Assumptions
Phase 0 provisionally selected RocksDB/Pebble as the storage layer. This paper challenges that decision by evaluating the critical failure modes of LSM-trees in high-throughput blockchain environments: **Write Amplification (WA)** and **Compaction Stalls**.

**Core Findings:**
1. **The RocksDB Trap:** In an LSM-tree (Log-Structured Merge-tree), every written key is repeatedly re-written across Levels $L_0$ through $L_6$. Write amplification commonly exceeds $15\times - 30\times$. At 3,000 TPS producing 1 KB per transaction state delta, an LSM writes $> 50 - 90\text{ MB/sec}$ of continuous churn to disk, destroying consumer NVMe SSD endurance within months.
2. **Compaction Latency Spikes:** When $L_0$ fills faster than background threads can compact into $L_1$, RocksDB halts all writes ("write stall"). In a BFT consensus loop, a 500 ms write stall causes the validator to miss its proposal deadline, triggering unnecessary view changes and jailing.
3. **The MDBX Alternative:** **MDBX (`libmdbx`)** is a battle-tested, memory-mapped B+Tree key-value database:
   - **Zero-Copy Reads:** Reads return direct pointers into the operating system page cache, achieving sub-microsecond point lookups ($< 1\mu s$).
   - **Zero Background Compactions:** Empty pages are tracked via an in-database freelist and recycled immediately. Zero compaction stalls ever occur.
   - **ACID Transactions:** Full crash-consistency without complex Write-Ahead Log (WAL) replay loops.
4. **Recommendation:** Adopt a **Tiered Storage Architecture**:
   - **Tier 1 (Hot State & Account DB):** Use **MDBX (`libmdbx`)** for rapid account balances, contract storage slots, and Merkle tree nodes.
   - **Tier 2 (Append-Only Historical Chain DB):** Use flat, pre-allocated, chunked append-only log files for historical block headers, bodies, and receipts.

---

## 2. Storage Engine Comparison Matrix

| Storage Engine | Engine Type | Read Latency (p99) | Write Amplification | Compaction Stalls | Memory Architecture | Rust Bindings Quality |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| **RocksDB** | LSM-Tree | $\sim 50 - 150\mu s$ | Very High ($15-30\times$)| **Yes (Frequent)** | User-space Block Cache | Mature (`rocksdb-rs`, C FFI) |
| **Pebble** | LSM-Tree | $\sim 60 - 180\mu s$ | Very High ($15-25\times$)| **Yes (Frequent)** | Go Memory Pool | Poor (Go-only, no native Rust) |
| **MDBX (`libmdbx`)**| Memory-Mapped B+Tree | **$< 1\mu s$ (Zero-copy)**| Low ($2-4\times$) | **Zero (No Compaction)** | Kernel Page Cache | Outstanding (`mdbx-rs`, Reth-tested) |
| **SQLite (WAL)** | B-Tree | $\sim 20 - 80\mu s$ | Medium ($5-8\times$) | Minor checkpoint pauses| Buffer Pool | Very Mature |
| **Flat File Append**| Log | $\sim 10\mu s$ (Index lookup)| **1.0x (Optimal)** | **Zero** | OS Page Cache | Native Rust (`std::fs`) |

---

## 3. Storage Layout Specification

```
/var/lib/tola/data/
├── state/                   <── MDBX Database (Single file: mdbx.dat)
│   ├── table_accounts       (Address -> AccountState)
│   ├── table_storage        ((Address, Slot) -> Value)
│   └── table_tree_nodes     (NodeHash -> JMTNode)
│
├── chain/                   <── Append-Only Segment Files
│   ├── blocks_000000.bin    (Block records, 1 GB segments)
│   ├── blocks_000001.bin
│   └── block_index.mdbx     (Height / Hash -> Segment Offset)
│
└── receipts/                <── Prunable Append-Only Logs
    └── receipts_000000.bin  (Receipt records per block)
```

---

## 4. State Pruning & Archival Policy

1. **Full Nodes (Default Validator):**
   - Retain complete state at the latest block height $H$.
   - Retain historical state snapshots for the last $K = 50,000$ blocks ($\sim 1.5$ days, sufficient for consensus reorg guards and unbonding dispute resolution).
   - Prune obsolete MDBX historical versions via copy-on-write page recycling.
   - Total disk footprint: $< 150\text{ GB}$ after 1 year of active operation.
2. **Archive Nodes:**
   - Retain all historical state deltas and raw blocks from Genesis.
   - Disk footprint: Grows linearly with chain history; deployed on dedicated high-capacity storage servers.

---

## 5. Architectural Decision Recommendation
**MODIFY ADR-0007**: Formally replace RocksDB with **MDBX (`libmdbx`)** for the hot state database and an **append-only flat file log** for historical block storage. Create **ADR-0008**.
