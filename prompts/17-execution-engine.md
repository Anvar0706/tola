# SUBSYSTEM: Transaction Execution Engine & State Transition

## Objective
Design the deterministic execution pipeline that applies transactions to state, computes state transitions, tracks gas consumption, and emits verifiable transaction receipts.

## Why It Exists
The execution engine transforms raw transactions into updated blockchain state. It must guarantee absolute cross-platform determinism, accurate gas metering, and clean rollback mechanics.

## Dependencies
- `prompts/04-account-model.md`
- `prompts/06-transaction-model.md`
- `prompts/07-state-model.md`
- `prompts/18-smart-contract-vm.md`

## Non-Goals
- Consensus message ordering (transactions arrive pre-ordered in proposed blocks).

## Requirements
- Deterministic State Transitions: Given identical initial state $S$ and block $B$, the engine must produce identical state $S'$, gas used, and receipts root across all operating systems.
- Transaction Execution Steps:
  1. Deduct upfront gas fee: $\text{Balance} -= \text{GasLimit} \times \text{GasPrice}$.
  2. Increment sender nonce.
  3. Execute native transfer or VM contract invocation.
  4. Refund unspent gas: $\text{Balance} += (\text{GasLimit} - \text{GasUsed}) \times \text{GasPrice}$.
  5. Distribute consumed fee to proposer/treasury and execute fee burning.
  6. Generate and append execution receipt and logs.
- Intra-Block Atomicity: An individual transaction revert must cleanly roll back its state changes without aborting the entire block.

## Architecture
```
                  ┌───────────────────────────────┐
                  │    Transaction Execution Bus  │
                  └───────────────┬───────────────┘
                                  │
         ┌────────────────────────┴────────────────────────┐
         │                                                 │
┌────────▼──────────────┐                         ┌────────▼──────────────┐
│ Native Value Transfer │                         │   Smart Contract VM   │
│ - Balance debit/credit│                         │ - Bytecode execution  │
│ - Nonce increment     │                         │ - Gas metering        │
└───────────────────────┘                         └───────────────────────┘
         │                                                 │
         └────────────────────────┬────────────────────────┘
                                  │
                  ┌───────────────▼───────────────┐
                  │   State Delta & Receipt Log   │
                  └───────────────────────────────┘
```

## Interfaces
```rust
pub trait ExecutionEngine {
    fn execute_transaction(
        &self,
        state: &mut dyn StateJournal,
        tx: &Transaction,
        block_ctx: &BlockContext,
    ) -> Result<ExecutionReceipt, ExecutionError>;
}
```

## Data Structures
```rust
pub struct ExecutionReceipt {
    pub success: bool,
    pub gas_used: u64,
    pub logs: Vec<LogEntry>,
    pub return_data: Vec<u8>,
    pub state_root_snapshot: Option<[u8; 32]>,
}

pub struct LogEntry {
    pub address: Address,
    pub topics: Vec<[u8; 32]>,
    pub data: Vec<u8>,
}
```

## Security Requirements
- Sandboxed execution: Transactions cannot execute native OS syscalls, access system clocks, or read non-deterministic hardware states.
- Re-entrancy protection across intra-contract invocations.

## Failure Modes
- Non-deterministic execution due to uninitialized memory or floating-point usage.
- Stack overflow during deep contract recursion.

## Testing Requirements
- Differential state tests executing 100,000 synthetic transaction variations.
- Exact gas metering validation against known execution vectors.

## Performance Requirements
- Native transfer throughput $> 10,000$ tx/sec in-memory.
- VM dispatch overhead $< 2\mu s$.

## Migration Requirements
- Pluggable execution router supporting multiple VM backends simultaneously.

## Acceptance Criteria
- Execution engine specification documented in `docs/execution/execution-engine.md`.
- Determinism test suite passing across Linux, macOS, and Windows.

## Open Questions
- What parallel transaction execution model (Block-STM vs access-list parallelism) should be adopted in Phase 16?

## Implementation Tasks
1. Author execution journal and rollback checkpoint stack.
2. Specify transaction receipt and log encoding.
3. Define execution context boundaries.

## Completion Criteria
Execution engine formal specification complete and integrated with state journal.
