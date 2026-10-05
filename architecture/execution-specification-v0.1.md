# TOLA Network: Execution Specification v0.1

## 1. Virtual Machine & Execution Pipeline

- **Core Engine:** Rust EVM runtime (`revm`).
- **Compatibility:** Full support for EVM Cancun/Prague bytecode specifications.
- **Concurrency Paradigm:** **Block-STM (Software Transactional Memory)** for multi-threaded speculative execution.

---

## 2. Block-STM Parallel Execution Lifecycle

```
Input Block Transactions [Tx 0 .. Tx N]
                     │
    ┌────────────────┼────────────────┐
    ▼                ▼                ▼
Thread 1 (Tx 0)  Thread 2 (Tx 1)  Thread 3 (Tx 2)
  Speculative      Speculative      Speculative
  Read/Write       Read/Write       Read/Write
    │                │                │
    └────────────────┼────────────────┘
                     ▼
          Multi-Version Concurrency Control (MVCC)
          - Detect Read-After-Write conflicts
          - Re-execute conflicting transactions
          - Commit sequential state delta
```

---

## 3. Execution Journal & State Delta

```rust
pub struct StateDelta {
    pub accounts: HashMap<Address, AccountUpdate>,
    pub storage_slots: HashMap<(Address, [u8; 32]), [u8; 32]>,
    pub logs: Vec<ExecutionLog>,
    pub gas_consumed: u64,
}

pub enum AccountUpdate {
    Modified(AccountState),
    Deleted,
}
```

---

## 4. Gas Accounting & Precompiles

1. Standard EVM precompiles (`0x01` through `0x0A`) supported natively.
2. **TOLA Native Precompile (`0x0100`):**
   - Address: `0x0000000000000000000000000000000000000100`
   - Method: `verifyEd25519(bytes32 pubkey, bytes message, bytes64 signature) -> bool`
   - Gas: 3,000 gas flat.
