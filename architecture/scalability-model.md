# TOLA Network: Scalability Model & Roadmap

## 1. Principles of Scalable Design

1. **Optimize Single-Chain Execution First**: Premature sharding adds massive distributed complexity. TOLA maximizes single-core and multi-core throughput of the base protocol before fragmenting state.
2. **Deterministic State Transitions**: State access patterns must be predictable and parallelizable.
3. **Decoupled Consensus and Execution**: Consensus engines order blocks without executing transactions; execution pipelines process transactions asynchronously.
4. **Gradual Path to Multi-Shard Scale**: Architecture is designed with shard readiness from Day 1, but multi-shard consensus is activated only when traffic metrics justify it.

---

## 2. Scaling Phases

```
┌─────────────────────────────────┐
│  Phase A: Single-Chain Peak     │
│  - Parallel VM Execution        │
│  - In-Memory State Cache        │  Target: 2,000 - 5,000 TPS
│  - Optimized LSM Storage (MDBX) │
└────────────────┬────────────────┘
                 │
┌────────────────▼────────────────┐
│  Phase B: Modular Scaling       │
│  - Off-Chain Indexing (Graph)   │
│  - Dedicated RPC Read Replicas  │  Target: 10,000+ Queries/sec
│  - State Snapshot Pruning       │
└────────────────┬────────────────┘
                 │
┌────────────────▼────────────────┐
│  Phase C: Dynamic Sharding      │
│  - Coordination Layer           │
│  - Execution Shards (0..N)      │  Target: 50,000+ TPS
│  - Asynchronous Cross-Shard Txs │
└─────────────────────────────────┘
```

---

## 3. Dynamic Sharding Architecture (Future Phase)

### 3.1 Coordination Layer
- Maintains global validator registry, shard assignments, and cross-shard transaction commitments.
- Validators are pseudo-randomly assigned to shard committees using Verifiable Random Functions (VRF) and rotated periodically to prevent committee bribery.

### 3.2 Asynchronous Cross-Shard Communication
- When Account in Shard A sends funds to Account in Shard B:
  1. Shard A executes the debit and emits an **Outbound Cross-Shard Receipt** with state root proof.
  2. The receipt is verified by the Coordination Layer.
  3. Shard B ingests the verified receipt and credits the recipient account.
  4. Nonce tracking and nullifier sets prevent receipt replay.
