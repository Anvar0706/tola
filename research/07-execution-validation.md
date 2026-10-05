# Research 07: Execution Environment & Virtual Machine Validation

## 1. Executive Summary & Challenge of Phase 0 Assumptions
Phase 0 selected EVM compatibility (`revm`) to maximize developer adoption. This research evaluates whether EVM is technically defensible for TOLA's long-term throughput requirements and analyzes execution paradigms across modern Layer-1 networks.

**Core Findings:**
1. **The EVM Bottleneck:** Standard EVM is single-threaded, stack-based (256-bit words), and gas-metered at every instruction. A single-threaded EVM maxes out around 800 - 1,500 complex ERC-20 transfers/second on high-end hardware due to sequential state access locks.
2. **Parallel EVM Breakthroughs:** Modern execution engines (such as Aptos Block-STM, Monad, and Reth) demonstrate that the EVM *itself* is not the bottleneck; rather, the *storage engine and sequential scheduling* are. Using optimistic concurrency control (Block-STM), EVM throughput scales to **$5,000 - 10,000\text{ TPS}$** on 16-core validator CPUs without breaking Solidity compatibility.
3. **Alternative VMs:**
   - **WASM (CosmWasm / Near):** High native speed, but fragmented ecosystem; zero compatibility with Solidity/Foundry tools.
   - **SVM / eBPF (Solana):** Unmatched speed, but explicit account passing makes composability rigid; steep developer barrier.
   - **Move VM (Aptos/Sui):** Outstanding formal safety and resource types; strong candidate for Phase 2 dual-VM integration.
4. **Recommendation:** Retain **EVM compatibility as the primary execution engine** for Genesis to leverage the multi-billion dollar Solidity ecosystem, but architect the runtime using a **Modular Pluggable VM Interface** supporting **Block-STM optimistic parallel execution**.

---

## 2. Comprehensive Virtual Machine Comparison

| Metric | EVM (revm + Block-STM) | WASM (Wasmtime) | SVM (Solana eBPF) | Move VM | Actor TVM (TON) |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **Word Architecture** | 256-bit stack | 32/64-bit stack/register | 64-bit register (ELF) | Bytecode / Stack | 1023-bit Cells / Stack |
| **Composability** | Synchronous, Atomic | Synchronous, Atomic | Synchronous, Atomic | Synchronous, Atomic | Asynchronous, Non-Atomic |
| **Parallel Execution** | Optimistic (Block-STM) | Complex / Manual | Static Account Locking | Block-STM | Actor Concurrency |
| **Developer Ecosystem** | Massive (Solidity/Vyper)| Moderate (Rust/C++) | Large (Rust) | Emerging (Move) | Niche (FunC/Tact) |
| **Tooling & Auditing** | Foundry, Slither, Hardhat | Basic debuggers | Solana CLI, Anchor | Move Prover | Custom cell inspectors |
| **Security Track Record**| Battle-tested patterns | Sandboxed | High complexity bugs | Formally verified types | Async re-entrancy traps |
| **Single-Thread TPS** | $\sim 1,500$ | $\sim 8,000$ | $\sim 4,000$ | $\sim 5,000$ | $\sim 2,000$ |
| **Multi-Core TPS** | **$5,000 - 10,000$** | $15,000+$ | $20,000+$ | $15,000+$ | $10,000+$ |

---

## 3. Parallel Execution Engine Architecture: Block-STM

```
Transactions in Proposed Block [Tx 0, Tx 1, Tx 2, ..., Tx N]
                          │
         ┌────────────────┴────────────────┐
         ▼                                 ▼
   Core 0: Execute Tx 0              Core 1: Execute Tx 1
   (Speculative Read/Write Set)      (Speculative Read/Write Set)
         │                                 │
         └────────────────┬────────────────┘
                          ▼
            Validation & Conflict Detector
            - If ReadSet(Tx 1) intersects WriteSet(Tx 0):
              ABORT Tx 1 -> Re-execute with Tx 0's updated state
            - Else:
              COMMIT Speculative Deltas to State Journal
```

### Key Properties:
- **Zero Developer Overhead:** Smart contracts remain 100% standard Solidity. Developers do not need to specify manual account access lists.
- **Deterministic Outcome:** The final state is mathematically identical to sequential execution order ($0, 1, 2, \dots, N$).
- **Linear Scaling:** Achieves $4\times - 8\times$ speedup on multi-core systems when transaction dependency conflicts are low (e.g. distinct user transfers).

---

## 4. Gas Metering & Resource Pricing

1. **Opcode Gas:** Adheres to EVM standard (Cancun/Prague schedule) to maintain strict toolchain compatibility.
2. **State Creation Surcharge:** Writing a new storage slot requires an explicit, refundable byte deposit (e.g. 0.001 TOLA) to prevent state bloat attacks.
3. **Micro-Transaction Cost:** With a baseline base fee floor of $1\text{ Gwei}$, a standard 21,000 gas transfer costs:
   $$\text{Cost} = 21,000 \times 10^{-9} \text{ TOLA} = 0.000021 \text{ TOLA} \approx \$0.00002$$
   This enables viable messenger micropayments while imposing a non-trivial $\$50,000+/\text{day}$ cost on an attacker attempting to saturate 5,000 TPS.

---

## 5. Architectural Decision Recommendation
**MODIFY ADR-0004**: Confirm EVM compatibility via Rust `revm` as the initial execution engine, but explicitly specify the integration of **Block-STM optimistic parallel execution** and modular VM traits for future WASM/Move co-processors.
