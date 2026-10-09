# TOLA Network: Parallel Execution & Block-STM Engine Specification

## 1. Overview & Architectural Motivation
The **TOLA Parallel Execution Engine** (`tola-execution`) implements optimistic Software Transactional Memory (Block-STM) conforming to **ADR-0012**, **research/07-execution-validation.md**, and **research/20-evm-block-stm-benchmark.md**.

Single-threaded EVM state transitions limit validator throughput to $\sim 1,500 - 2,350\text{ TPS}$ due to sequential memory locks. While architectures like Solana achieve parallelism via static access lists, they force developers to manually declare all accessed accounts upfront, breaking dynamic smart contract composability and standard EVM tooling.

TOLA solves this by implementing **Block-STM optimistic concurrency**:
1. **Zero Access Lists Required:** Developers deploy standard Solidity contracts without modifying contract code or declaring static storage keys.
2. **Speculative Multi-Threaded Execution:** Transactions execute optimistically across available CPU cores, recording read dependencies in Multi-Version Memory (MV-Memory).
3. **Dynamic Conflict Abort & Re-Execution:** When read-write conflicts occur, conflicting transactions are rolled back, marked as estimates, and re-executed with bumped incarnations.
4. **Deterministic Sequential Equivalence:** The final state root, account balances, nonces, and transaction receipts are mathematically identical to single-threaded sequential execution.
5. **Bounded Worst-Case Fallback:** If conflict aborts exceed a configurable threshold, the engine automatically falls back to deterministic sequential execution.

---

## 2. Multi-Version Concurrency Memory (MV-Memory)

The MV-Memory structure (`tola-execution::mv_memory`) manages versioned state access without acquiring global read/write locks on the underlying database.

### 2.1 Keys & Values
State accesses are tracked under canonical keys:
* `MemoryKey::Account(Address)`: Account balance, nonce, and flags.
* `MemoryKey::Storage(Address, Hash)`: Contract 256-bit storage slot.

Values stored at each version:
* `MemoryValue::Account(Option<AccountState>)`
* `MemoryValue::Storage(Hash)`

### 2.2 Versioning Tuple
Every write produced in the execution wave is indexed by a version tuple:
$$V = (i, \text{incarnation})$$
where $i \in [0, N-1]$ is the canonical transaction index within the block, and $\text{incarnation} \ge 0$ is the execution attempt counter.

### 2.3 Read Semantics
When transaction $i$ reads key $K$:
1. MV-Memory searches for the highest write index $j < i$ for key $K$.
2. **If found:**
   - If entry is `MemoryEntry::Value(V)`, return value and record $(K, \text{Some}(V))$ in transaction $i$'s `ReadSet`.
   - If entry is `MemoryEntry::Estimate`, abort execution and yield to prevent reading uncommitted speculative state.
3. **If not found:** Read from the base `StateDB` snapshot, returning value and recording $(K, \text{None})$ in `ReadSet`.

### 2.4 Read Set Validation
Validation of transaction $i$ verifies that every entry in its `ReadSet`:
$$\forall (K, V_{\text{expected}}) \in \text{ReadSet}(i): \quad \text{CurrentVersion}(K, < i) == V_{\text{expected}}$$
If any recorded version differs (e.g. an earlier transaction $j < i$ committed a new write to key $K$), validation **fails**, triggering an abort.

---

## 3. Block-STM Concurrency Scheduler

The scheduler (`tola-execution::scheduler`) drives worker threads through execution and validation waves:

```
                      +-----------------------------+
                      |   Execution Queue (Min-Heap)|
                      +-----------------------------+
                                     |
                         pop_task()  | Priority 1
                                     v
                           [ Worker Executes ]
                                     |
                   writes recorded   |
                                     v
                      +-----------------------------+
                      |  Validation Queue (Min-Heap)|
                      +-----------------------------+
                                     |
                         pop_task()  | Priority 2
                                     v
                          [ Worker Validates ]
                             /             \
                   Valid == true         Valid == false
                           /                 \
                  [ Kept Executed ]     [ Abort & Incarnation++ ]
                                               |
                                     Re-enqueue into Execution Queue &
                                     Re-validate dependent Tx (j > i)
```

### 3.1 Task Prioritization
1. **Execution Priority:** Lowest transaction index in the execution queue is always scheduled first.
2. **Validation Priority:** Lowest transaction index in the validation queue is scheduled next.
3. **Cascading Validation:** Whenever transaction $i$ finishes execution or aborts, all subsequent executed transactions $j \in [i+1, N-1]$ are enqueued for validation.

### 3.2 Lock Hierarchy & Deadlock Prevention
To guarantee zero deadlocks across scoped worker threads:
* `MvMemory` holds at most ONE lock at any given time (`locations`, `read_sets`, or `write_sets`).
* Intermediate read/write guards are dropped before secondary locks are acquired.
* Worker threads coordinate via `Condvar` on the unified `SchedulerState` mutex.

---

## 4. Sequential Fallback & Contention Boundaries

Under extreme contention (e.g. Workload C: 500+ transactions calling the exact same AMM pool or token faucet within one block):
* Conflict aborts can degrade parallel speedup to $\sim 1.1\times$ (`research/20-evm-block-stm-benchmark.md`).
* If cumulative aborts exceed `max_aborts` (default 1,000):
  1. `scheduler.is_fallback_required()` triggers.
  2. Workers exit cleanly.
  3. The block is immediately executed via `SequentialBlockEngine`.
* **Bounded Complexity Guarantee:** The execution time of any block is strictly bounded by sequential processing time:
  $$T_{\text{execution}} \le T_{\text{sequential}} + O(\text{thread\_exit})$$

---

## 5. Invariants & Deterministic Equivalence

1. **Deterministic State Root:**
   $$\text{ParallelStateRoot} \equiv \text{SequentialStateRoot}$$
   Verified across disjoint, dependent, and fallback test suites.
2. **Attotola Supply Conservation:**
   $$\sum \text{FinalBalances} + \text{BurnedGasFees} \equiv \sum \text{InitialBalances}$$
3. **Sequential Receipt Ordering:**
   Transaction receipts $[R_0, R_1, \dots, R_{N-1}]$ have strictly increasing cumulative gas:
   $$\text{CumulativeGas}_i = \sum_{k=0}^i \text{GasUsed}_k$$
