# TOLA Network: Scalability Specification v0.1

## 1. Architecture Ceiling & Phased Roadmap

TOLA Network achieves horizontal scale through a strictly phased engineering roadmap that maximizes monolithic performance before introducing distributed state partitioning.

---

## 2. Quantitative Performance Milestones

| Milestone | Phase | Architectural Mechanism | Target TPS | Finality Latency |
| :--- | :--- | :--- | :--- | :--- |
| **Milestone 1** | Mainnet v1.0 | Pipelined BFT + Block-STM Parallel EVM + MDBX | **3,000 - 8,000** | $2.0 - 4.0\text{ sec}$ |
| **Milestone 2** | Mainnet v1.5 | Off-chain read replicas + Micro-payment state channels | **15,000+** | $< 100\text{ ms}$ (Off-chain) |
| **Milestone 3** | Mainnet v2.0 | Application Rollups / Coordination Layer Sharding | **50,000+** | $4.0 - 8.0\text{ sec}$ (Cross-domain) |

---

## 3. Monolithic Execution Optimization Parameters

1. **Storage Concurrency:** MDBX multi-reader zero-copy design allows all CPU worker threads to execute read transactions in parallel without mutex locks.
2. **Speculative MVCC:** Block-STM divides transaction blocks into parallel batches; non-conflicting transfers execute concurrently across all available physical CPU cores.
3. **Pipelined Consensus:** Block execution for height $H-1$ overlaps with BFT leader proposal and voting for height $H$, eliminating execution idle time from the consensus round.
