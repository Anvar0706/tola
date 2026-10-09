# ADR-0017: TOLA EVM Ethereum Conformance & Differential Testing

## Status
ACCEPTED (TOLA EVM Phase 9)

## Context
Across Phases 0 through 8 (P0–P8), TOLA established a high-performance Layer-1 EVM implementation executing within a unified StateDB architecture under sequential consensus:
- **P0–P2:** Core EVM state model, receipt persistence, log indexing, and bloom filters.
- **P3–P5:** JSON-RPC compatibility, WebSocket subscriptions, tooling APIs, and Type-2 fee economics.
- **P6–P7:** Hand-crafted and compiler-generated Solidity 0.8.24 bytecode execution, OpenZeppelin v5.0.2 contracts (ERC-20, ERC-721, ERC-1155, UUPS proxy), and Shanghai precompiles (0x01–0x09).
- **P8:** Historical state queries (`eth_getBalance`, `eth_getCode`, `eth_getStorageAt`, `eth_call`, `eth_estimateGas`) across block tags, numbers, and EIP-1898 objects via copy-on-write JMT account nodes and reverse-seek storage history.

A forensic audit identified three correctness gaps and the need for a formal conformance testing framework:
1. **Dynamic `block.gaslimit` Binding:** Canonical execution and simulation previously hardcoded `30_000_000` rather than dynamically extracting `gas_limit` from the block header.
2. **Block-Level EIP-1559 Base Fee Validation:** Block validation lacked an explicit consensus check rejecting EVM transactions whose `max_fee_per_gas` falls below `block.header.base_fee_per_gas`.
3. **EIP-3860 Initcode Size Bound:** Stateless validation did not enforce the Shanghai limit of 49,152 bytes (`MAX_INITCODE_SIZE = 49152`) for contract creation transactions.
4. **Differential & Conformance Testing Rigor:** Formal verification against canonical Ethereum Shanghai test vectors and diagnostic state oracles is required to guarantee semantic conformance without divergence.

---

## Decision

### 1. Goal A — EVM Correctness Remediation

#### 1.1 Dynamic `block.gaslimit` Binding (P9-FND-01)
- **Canonical Execution:** In `SequentialBlockEngine::execute_block`, `EvmExecutor::execute_transaction` receives `block.header.gas_limit`. REVM's `BlockEnv.gas_limit` is set directly to `block_gas_limit.as_u64()`.
- **Simulation (`eth_call` & `eth_estimateGas`):** `RpcDispatcher::resolve_block_context` returns a 5-tuple `(BlockHeight, Timestamp, Amount, Address, Gas)`. The target block's gas limit is passed to `EvmExecutor::simulate_call`, ensuring simulation accurately reflects the target block's gas limit across both latest and historical block queries.
- **Opcode Verification:** The EVM `GASLIMIT` opcode (`0x45`) returns the exact block gas limit configured in the target block header.

#### 1.2 Block-Level EIP-1559 Base Fee Validation (P9-FND-02)
- In `SequentialBlockEngine::validate_block`, every transaction is validated prior to execution:
  ```rust
  if tx.is_evm() && tx.max_fee_per_gas < block.header.base_fee_per_gas {
      return Err(BlockValidationError::FeeBelowBaseFee);
  }
  ```
- Any block containing an underpriced EVM transaction is rejected by consensus with `BlockValidationError::FeeBelowBaseFee`.
- Native TOLA transactions maintain their decoupled validator fee model and are unaffected by this EVM-specific constraint.

#### 1.3 EIP-3860 Initcode Size Stateless Validation (P9-FND-05)
- In `tola-types/src/transaction.rs`, `MAX_INITCODE_SIZE` is declared as constant `49152` (48 KiB).
- `Transaction::stateless_validate()` enforces:
  ```rust
  if self.is_contract_creation() && self.data.len() > MAX_INITCODE_SIZE {
      return Err(TransactionError::InitcodeTooLarge {
          max_bytes: MAX_INITCODE_SIZE,
          got_bytes: self.data.len(),
      });
  }
  ```
- **Boundary Semantics:** Contract creations with `data.len() <= 49152` pass stateless validation; creations with `data.len() == 49153` are rejected immediately before mempool admission.
- **Calldata Isolation:** Non-creation CALL transactions (`to.is_some()`) are permitted to submit calldata exceeding 49,152 bytes up to the general transaction size limit (128 KiB).

#### 1.4 RPC Simulation Gas-Limit Policy (Task B)
- **Problem Statement:** Historical simulation previously silently clamped caller gas limits (`tx_env.gas_limit = tx.gas_limit.min(block_gas_limit)`), masking caller configuration errors and diverging from Ethereum JSON-RPC client expectations.
- **Adopted Policy:**
  1. **Omitted Caller Gas:** When the `gas` field is omitted from an `eth_call` or `eth_estimateGas` request object, simulation defaults the gas limit to the target block's dynamic gas limit (`block_gas_limit`).
  2. **Explicit Caller Gas Within Bounds:** When caller specifies explicit `gas <= block_gas_limit`, simulation passes the exact caller gas limit to the EVM (`tx_env.gas_limit = tx.gas_limit.as_u64()`).
  3. **Explicit Caller Gas Exceeding Bounds:** When caller specifies explicit `gas > block_gas_limit`, the RPC dispatcher immediately rejects the request with JSON-RPC error code `-32602` (`invalid_params: "gas limit exceeds block gas limit"`).
  4. **Execution Out of Gas:** When execution exhausts available gas, the engine reports an execution halt (`EvmExecutionStatus::Halt("OutOfGas")`) returning JSON-RPC server error `-32000` (`"execution halted: OutOfGas"`).
  5. **Canonical Execution Invariant:** In `EvmExecutor::execute_transaction`, transactions with `gas_limit > block_gas_limit` are strictly rejected, preserving consensus block gas boundaries.
  6. **Historical Query Context:** Historical simulations (`eth_call` at block height $H$) query the target block's canonical header and bind `block_gas_limit` to $H$'s gas limit.

---

### 2. Goal B — Conformance & Differential Testing Architecture

#### 2.1 Tier 1: Official Ethereum Shanghai Execution Fixtures (Task A)
TOLA integrates a bounded, curated catalog of 12 official Ethereum Shanghai GeneralStateTest (GST) fixtures under `crates/tola-node/tests/fixtures/shanghai/` with explicit provenance documented in `PROVENANCE.md` (upstream `ethereum/tests` v17.2 and `ethereum/execution-spec-tests` v5.4.0 under CC0-1.0 and MIT licenses):
1. `add11.json` — Basic arithmetic and cold SSTORE allocation.
2. `mulmod_zero.json` — Modulo zero boundary condition handling.
3. `sdiv_min_int.json` — Signed division edge case: two's complement $MIN\_INT / -1$ overflow boundary.
4. `exp_basic.json` — EIP-160 exponential gas calculation and exponentiation.
5. `shl_sar_basic.json` — EIP-145 bitwise shift opcodes (`SHL`, `SAR`) with negative operands.
6. `memory_expansion.json` — Quadratic memory expansion formula and `MSIZE` opcode accounting.
7. `revert_with_data.json` — EIP-140 state rollback and returndata bubbling.
8. `eip170_code_size.json` — EIP-170 24,576-byte deployment code size boundary.
9. `eip3860_initcode.json` — EIP-3860 49,152-byte creation initcode size boundary.
10. `stack_depth_limit.json` — 1024 stack items limit enforcement.
11. `call_depth_limit.json` — 1024 recursive call frame depth limit.
12. `sha3_basic.json` — SHA3 (Keccak-256) memory hashing computation.

Each fixture is executed through TOLA's canonical EVM engine (`SequentialBlockEngine`), asserting pre-state initialization, transaction execution success/revert, and normalized post-state (account balances, nonces, deployed bytecode, and individual storage slots).
1. **Arithmetic & Logic Edge Cases:**
   - `ADDMOD` / `MULMOD` with modulus 0 returns 0 without division-by-zero panics.
   - `SDIV` with `MIN_INT` ($-2^{255}$) divided by $-1$ overflows to `MIN_INT`.
   - `SDIV` and `SMOD` by 0 return 0.
   - `SIGNEXTEND` across 1-byte, multi-byte, and boundary byte sizes.
   - `EXP` exponentiation gas and value calculation at $2^{256}-1$ boundaries.
   - Shift opcodes (`SHL`, `SHR`, `SAR`) with negative and $>255$ shift amounts.
2. **Memory Expansion Quadratic Gas:**
   - Verifies the canonical quadratic formula:
     $$\text{Cost}(words) = 3 \times words + \left\lfloor \frac{words^2}{512} \right\rfloor$$
   - Enforces precise gas deductions on memory reads/writes across 0, 32, 1024, and 4096-byte boundaries.
3. **Stack Depth Limits:**
   - 1024 stack items: successfully completes.
   - 1025 stack items: stack overflow halts execution, consumes allotted gas, and sets failure receipt.
   - Stack underflow: empty stack POP halts execution.
4. **Call Frame Depth Limits:**
   - Nested CALL frames up to depth 1024 succeed.
   - Depth 1025 returns 0 (failure flag) on caller stack without engine panic.
5. **Contract Creation & Size Boundaries:**
   - CREATE / CREATE2 address collisions on existing code/nonces return address 0.
   - EIP-170 runtime code size limit (24,576 bytes) enforced on deployment.
6. **Revert Propagation:**
   - Sub-call revert preserves return data payload, burns expended gas, reverts child state deltas, and propagates return bytes to caller.

#### 2.2 Tier 2: Standalone REVM 43.0.3 Diagnostic Oracle
- **Methodological Role:** Pure in-memory REVM 43.0.3 execution is designated as an authoritative **diagnostic adapter fidelity oracle**, verifying that TOLA's `RevmStateAdapter` and `RevmReadOnlyAdapter` translate state access and deltas with zero divergence compared to an in-memory reference environment.
- Canonical Ethereum GST vectors provide external specification compliance, while Tier 2 provides internal adapter-level verification.
- Both engines are evaluated against identical initial states and transaction environments, verifying bit-for-bit equivalence in:
  - `status` (`Success`, `Revert`, `Halt`)
  - `gas_used`
  - `output` / `return_data`
  - `created_address`
  - Mutated account balances, nonces, and storage slots.

#### 2.3 State Root & Trie Normalization
- Ethereum test vectors define post-state roots derived from Keccak256 Modified Patricia Tries (MPT).
- TOLA utilizes a unified BLAKE3 Jellyfish Merkle Tree (JMT) for account commitments and flat versioned storage history.
- Conformance validation performs **normalized state comparison**:
  - Account balances, nonces, deployed code, and storage key-value pairs are compared against post-state expectations.
  - This avoids false negatives arising from trie structure differences while proving complete semantic state equivalence.

---

## Consequences

### Positive
- Resolves all three post-P8 correctness findings (dynamic gas limit, block base fee validation, EIP-3860 initcode bounds).
- Guarantees full Ethereum Shanghai EVM execution compatibility across all arithmetic, memory, stack, and call boundaries.
- Establishes automated, repeatable conformance testing with both canonical vectors and diagnostic reference oracles.
- Preserves all invariants from P0–P8, including unified state DB, single state root, and cold restart persistence.

### Considerations
- EIP-3860 bounds apply strictly to initcode during deployment transactions (`to.is_none()`). Smart contracts deploying child contracts via `CREATE` / `CREATE2` inside EVM execution rely on REVM's internal Shanghai EIP-3860 gas metering and size validation.
- Conformance tests verify normalized state representations rather than raw MPT root hashes due to TOLA's high-throughput JMT storage architecture.
