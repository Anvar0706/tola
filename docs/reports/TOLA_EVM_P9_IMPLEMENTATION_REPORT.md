# TOLA EVM Phase 9 — Call-Depth Boundary Test Remediation Report

**Phase Status**: **OPEN — NOT APPROVED** (Pending Independent Forensic Audit)  
**Remediation Target**: EVM Maximum Call-Depth (1,024-Frame) Boundary Verification  
**Scope**: Strictly restricted to EVM call-depth boundary test coverage (`crates/tola-node/tests/p9_ethereum_conformance_test.rs`). Zero changes to protocol code, consensus, state schemas, storage, or P0–P8 phases.

---

## 1. Executive Summary & Root Cause Confirmation

In the previous P9 conformance suite, the test `test_p9_canonical_gst_call_depth_limits` executed a recursive contract call with an input depth parameter of $5$, inherited from the fixture structure of `call_depth_limit.json`. While this test exercised basic recursion, CALL stack unwinding, and return value validation, it did not exercise the EVM's maximum call-stack limit ($1,024$ frames) or verify the rejection behavior at depth $1,025$.

To resolve this conformance gap, we:
1. Conducted an authoritative audit of call-depth semantics in the Ethereum Shanghai specification and REVM 43.0.3.
2. Formulated and deployed a custom recursive bytecode contract that reaches frame $1,024$, executes observable state changes, attempts an over-limit call to frame $1,025$, verifies the EVM depth-limit failure return code ($0$), records confirmation to storage, and unwinds cleanly.
3. Resolved the EIP-150 ($63/64$ rule) gas decay challenge by utilizing P9's dynamic block gas limit capability ($1 \text{ Trillion gas}$).
4. Established two-sided boundary verification ensuring that off-by-one errors in either direction fail the test deterministically.

---

## 2. EVM & REVM 43.0.3 Call-Depth Semantics

### 2.1 The 1,024 Frame Limit Convention

Under the Ethereum Yellow Paper and Shanghai EVM specification:
- The maximum call depth is defined as $\text{CALL\_STACK\_LIMIT} = 1024$.
- The root transaction frame starts at `depth = 0`.
- Each subsequent nested execution frame (`CALL`, `CALLCODE`, `DELEGATECALL`, `STATICCALL`) is spawned with `caller_depth + 1`.

In REVM 43.0.3 (`revm-primitives/src/constants.rs` and `revm-handler/src/frame.rs`):
```rust
pub const CALL_STACK_LIMIT: u64 = 1024;

// revm-handler/src/frame.rs:
if depth > CALL_STACK_LIMIT as usize {
    return return_result(InstructionResult::CallTooDeep);
}
```

### 2.2 Reconciled Boundary Mechanics

| Frame Index / Depth | Action | Expected REVM Result |
| :--- | :--- | :--- |
| **0** (Root frame) | Receives transaction calldata (`0x0`). Initiates call to self with $D = 1$. | Permitted (`depth: 0 <= 1024`) |
| **1 .. 1023** | Recursive subcalls incrementing $D \leftarrow D + 1$. | Permitted (`depth <= 1024`) |
| **1024** (Boundary leaf) | Frame 1024 executes: stores `0x42` at slot 1024. Initiates call with $D = 1025$. | Permitted (`depth: 1024 <= 1024`) |
| **1025** (Over-limit call) | REVM frame check evaluates `1025 > 1024`. Returns `InstructionResult::CallTooDeep`. | **REJECTED** |
| **1024** (Handling failure) | CALL opcode pushes `0` (failure) to stack; returns unused gas. Frame 1024 checks status == `0`, stores `0x99` at slot 1025, and returns `1` (success). | Permitted |
| **1023 .. 0** | Each frame validates child returned `1`, pushes `1` as return data, and unwinds cleanly. | Permitted |

Under EVM CALL semantics, an over-limit nested call pushes `0` onto the caller's operand stack and returns all unused gas to the caller frame. It does **not** revert the caller unless the caller explicitly executes `REVERT`.

---

## 3. The EIP-150 63/64 Gas Decay Mechanics & Dynamic Gas Limit

Under EIP-150 (Tangerine Whistle), a CALL opcode forwards at most:
$$\text{gas\_forwarded} = \left\lfloor \frac{63}{64} \times \text{gas\_available} \right\rfloor$$

Compounded over $1,024$ nested frames:
$$\left(\frac{63}{64}\right)^{1024} \approx 9.223 \times 10^{-8} \approx \frac{1}{10,841,922}$$

If a standard transaction gas limit of $30,000,000$ or even $150,000,000$ were used, the gas available at frame 1024 would be:
- At 30M gas: $30{,}000{,}000 \times 9.223 \times 10^{-8} \approx 2.76 \text{ gas}$ (Immediate Out-Of-Gas).
- At 150B gas: $150{,}000{,}000{,}000 \times 9.223 \times 10^{-8} \approx 13{,}834 \text{ gas}$, which ran out of gas when performing a cold `SSTORE` (cost 22,100 gas).

### Solution via P9 Dynamic Block Gas Limit
P9 implemented dynamic block gas limit propagation (`commit_block_with_gas_limit`). We set:
$$\text{boundary\_gas} = \text{Gas}(1{,}000{,}000{,}000{,}000) \quad (1 \text{ Trillion gas})$$
- Gas available at frame 1024: $\approx 89,550 \text{ gas}$.
- Frame 1024 easily executes:
  - `SSTORE(1024, 0x42)`: 22,100 gas
  - `CALL(1025)`: 2,600 gas base call
  - `SSTORE(1025, 0x99)`: 22,100 gas
  - Stack unwind returns: ~1,500 gas per frame.
- Economic cost in TOLA: At $\text{base\_fee} = 10 \text{ wei}$, $1 \text{T gas} \times 10 \text{ wei} = 10^{13} \text{ wei} = 0.00001 \text{ TOLA}$, funded trivially by test account with 100 TOLA.
- Execution speed: REVM executes the entire 1,024 frame stack in **0.04 seconds**.

---

## 4. Bytecode Architecture & Execution Path

The deployed recursive runtime bytecode:
```hex
6000358061040014603e5760010160005260206000602060006000305af11560335760005115603357600160005260206000f35b600060005260206000f35b60426104005561040160005260206000602060006000305af160001415603357609961040155600160005260206000f3
```

### Disassembly & Control Flow

```assembly
// Entry: Read input depth
[00] PUSH1 00
[02] CALLDATALOAD            // stack: [depth]
[03] DUP1                    // stack: [depth, depth]
[04] PUSH2 0400              // stack: [1024, depth, depth]
[07] EQ                      // depth == 1024 ?
[08] PUSH1 3e                // PC 0x3e (leaf_1024_handler)
[0a] JUMPI                   // If depth == 1024, jump to leaf handler

// Recursive Step: depth < 1024
[0b] PUSH1 01
[0d] ADD                     // next_depth = depth + 1
[0e] PUSH1 00
[10] MSTORE                  // memory[0..32] = next_depth
[11] PUSH1 20                // out_size = 32
[13] PUSH1 00                // out_offset = 0
[15] PUSH1 20                // in_size = 32
[17] PUSH1 00                // in_offset = 0
[19] PUSH1 00                // value = 0
[1b] ADDRESS                 // to = self (0x30)
[1c] GAS                     // forward all available gas
[1d] CALL                    // call self(next_depth)
[1e] ISZERO
[1f] PUSH1 33                // PC 0x33 (fail_dest)
[21] JUMPI                   // If call failed, jump to fail_dest
[22] PUSH1 00
[24] MLOAD                   // load returned word from memory[0..32]
[25] ISZERO
[26] PUSH1 33                // PC 0x33 (fail_dest)
[28] JUMPI                   // If ret_val == 0, jump to fail_dest
[29] PUSH1 01
[2b] PUSH1 00
[2d] MSTORE                  // memory[0..32] = 1
[2e] PUSH1 20
[30] PUSH1 00
[32] RETURN                  // return 1 (success)

// fail_dest:
[33] JUMPDEST
[34] PUSH1 00
[36] PUSH1 00
[38] MSTORE
[39] PUSH1 20
[3b] PUSH1 00
[3d] RETURN                  // return 0 (failure)

// leaf_1024_handler: (Executed ONLY at depth 1024)
[3e] JUMPDEST
[3f] PUSH1 42
[41] PUSH2 0400
[44] SSTORE                  // storage[1024] = 0x42 (Proves frame 1024 reached!)
[45] PUSH2 0401
[48] PUSH1 00
[4a] MSTORE                  // memory[0..32] = 1025
[4b] PUSH1 20                // out_size = 32
[4d] PUSH1 00                // out_offset = 0
[4f] PUSH1 20                // in_size = 32
[51] PUSH1 00                // in_offset = 0
[53] PUSH1 00                // value = 0
[55] ADDRESS                 // to = self
[56] GAS                     // gas
[57] CALL                    // call self(1025) -> MUST FAIL (CallTooDeep)!
[58] PUSH1 00
[5a] EQ                      // call_status == 0 ?
[5b] ISZERO
[5c] PUSH1 33                // PC 0x33 (fail_dest)
[5e] JUMPI                   // If call succeeded (!= 0), jump to fail_dest (FAIL TEST!)
[5f] PUSH1 99
[61] PUSH2 0401
[64] SSTORE                  // storage[1025] = 0x99 (Proves call 1025 failure verified!)
[65] PUSH1 01
[67] PUSH1 00
[69] MSTORE                  // memory[0..32] = 1
[6a] PUSH1 20
[6c] PUSH1 00
[6e] RETURN                  // return 1 (unwind stack successfully)
```

---

## 5. Two-Sided Boundary Assertions & Sensitivity Proof

The test asserts three explicit storage invariants plus receipt success:
```rust
assert!(receipts[0].success, "Boundary transaction must succeed");

// Invariant 1: Slot 1024 contains 0x42
let s1024 = db.get_storage(&contract_addr, &Hash(pad32_u64(1024))).unwrap();
assert_eq!(s1024.0, pad32_u64(0x42), "Frame 1024 must execute and store 0x42");

// Invariant 2: Slot 1025 contains 0x99
let s1025 = db.get_storage(&contract_addr, &Hash(pad32_u64(1025))).unwrap();
assert_eq!(s1025.0, pad32_u64(0x99), "Frame 1024 must observe call 1025 failure and store 0x99");

// Invariant 3: Slot 1026 remains Hash::ZERO
let s1026 = db.get_storage(&contract_addr, &Hash(pad32_u64(1026))).unwrap();
assert_eq!(s1026, Hash::ZERO, "Slot 1026 must remain unwritten");
```

### Deterministic Sensitivity Demonstration:
1. **Under-limit failure sensitivity (e.g. limit < 1024)**:
   If the EVM rejected calls at depth 1023 or 1024, frame 1024 would never execute. Slot 1024 would remain zero, failing Invariant 1.
2. **Over-limit failure sensitivity (e.g. limit > 1024)**:
   If the EVM incorrectly permitted depth 1025, the nested call would return `1` instead of `0`. The opcode sequence `EQ(0) -> ISZERO -> JUMPI(fail_dest)` would jump to `fail_dest`, returning `0` without writing `0x99` to slot 1025. Invariant 2 would fail.
3. **Failure isolation**:
   Depth limit rejection at frame 1025 returned `0` without corrupting frame 1024's context, confirming that depth limits do not cause unwanted transaction panics or rollbacks of prior mutations.

---

## 6. Authoritative Test Results & Workspace Verification

### 6.1 Targeted Test Execution
Command:
```bash
cargo test --test p9_ethereum_conformance_test test_p9_canonical_evm_call_depth_1024_boundary
```
Output:
```
running 1 test
test test_p9_canonical_evm_call_depth_1024_boundary ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 38 filtered out; finished in 0.04s
```

### 6.2 Full P9 Integration Target Execution
Command:
```bash
cargo test --test p9_ethereum_conformance_test
```
Output:
```
running 39 tests
test test_p9_block_level_base_fee_validation_native_tx_isolation ... ok
test test_p9_eip3860_initcode_size_boundary_49151_49152_49153 ... ok
test test_p9_eip3860_non_creation_call_above_49152_permitted ... ok
test test_p9_block_level_base_fee_validation_rejects_underpriced_evm_tx ... ok
test test_p9_block_level_base_fee_validation_accepts_valid_evm_tx ... ok
test test_p9_canonical_gst_arithmetic_modulo_edge_cases ... ok
test test_p9_canonical_gst_call_depth_limits ... ok
test test_p9_official_shanghai_gst_02_mulmod_zero ... ok
test test_p9_dynamic_block_gas_limit_historical_simulation ... ok
test test_p9_official_shanghai_gst_01_add11 ... ok
test test_p9_dynamic_block_gas_limit_canonical_execution ... ok
test test_p9_canonical_gst_stack_depth_limits ... ok
test test_p9_canonical_evm_call_depth_1024_boundary ... ok
test test_p9_canonical_gst_revert_propagation_and_gas_bubbling ... ok
test test_p9_canonical_gst_memory_expansion_quadratic_gas ... ok
test test_p9_official_shanghai_gst_04_exp_basic ... ok
test test_p9_official_shanghai_gst_03_sdiv_min_int ... ok
test test_p9_official_shanghai_gst_07_revert_with_data ... ok
test test_p9_official_shanghai_gst_06_memory_expansion ... ok
test test_p9_official_shanghai_gst_05_shl_sar_basic ... ok
test test_p9_official_shanghai_gst_08_eip170_code_size ... ok
test test_p9_canonical_gst_create_collision_and_eip170_code_limit ... ok
test test_p9_official_shanghai_gst_11_call_depth_limit ... ok
test test_p9_sim_gas_canonical_block_strict_enforcement ... ok
test test_p9_official_shanghai_gst_10_stack_depth_limit ... ok
test test_p9_official_shanghai_gst_12_sha3_basic ... ok
test test_p9_regression_p5_equal_fee_persistence ... ok
test test_p9_regression_cold_restart_persistence ... ok
test test_p9_tier2_revm_diagnostic_oracle_arithmetic_equivalence ... ok
test test_p9_tier2_revm_diagnostic_oracle_state_mutation_equivalence ... ok
test test_p9_sim_gas_explicit_exceeds_block_gas_limit ... ok
test test_p9_sim_gas_execution_out_of_gas ... ok
test test_p9_sim_gas_explicit_within_limit ... ok
test test_p9_sim_gas_omitted_defaults_to_block_gas_limit ... ok
test test_p9_official_shanghai_gst_09_eip3860_initcode ... ok
test test_p9_sim_gas_historical_block_context ... ok
test test_p9_regression_p7_solidity_and_precompiles ... ok
test test_p9_regression_p8_historical_state_queries ... ok
test test_p9_official_shanghai_all_curated_suite ... ok

test result: ok. 39 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.36s
```

### 6.3 Toolchain Quality Checks
- `cargo fmt -- --check`: **Clean** (Exit code 0).
- `cargo clippy --workspace --all-targets -- -D warnings`: **Clean** (Exit code 0, 0 warnings).

### 6.4 Complete Workspace Target Breakdown
Command:
```bash
cargo test --workspace
```

| # | Target Name | Target Category | Passed | Failed | Ignored |
|---|---|---|---|---|---|
| 1 | `tola_cli (src/main.rs)` | Unit / Binary | 13 | 0 | 0 |
| 2 | `wallet_integration_test.rs` | Integration Test | 13 | 0 | 0 |
| 3 | `tola_codec (src/lib.rs)` | Unit Test | 12 | 0 | 0 |
| 4 | `tola_consensus (src/lib.rs)` | Unit Test | 20 | 0 | 0 |
| 5 | `tola_core (src/lib.rs)` | Unit Test | 0 | 0 | 0 |
| 6 | `tola_crypto (src/lib.rs)` | Unit Test | 22 | 0 | 0 |
| 7 | `tola_execution (src/lib.rs)` | Unit Test | 11 | 0 | 0 |
| 8 | `tola_mempool (src/lib.rs)` | Unit Test | 6 | 0 | 0 |
| 9 | `tola_node (src/lib.rs)` | Unit Test | 21 | 0 | 0 |
| 10 | `tola_node (src/main.rs)` | Unit / Binary | 0 | 0 | 0 |
| 11 | `autonomous_testnet_audit_test.rs` | Integration Test | 11 | 0 | 0 |
| 12 | `external_asset_architecture_test.rs` | Integration Test | 5 | 0 | 0 |
| 13 | `fuzz_and_chaos_test.rs` | Integration Test | 4 | 0 | 0 |
| 14 | `mainnet_tokenomics_governance_test.rs` | Integration Test | 5 | 0 | 0 |
| 15 | `multi_process_testnet.rs` | Integration Test | 4 | 0 | 0 |
| 16 | `p1_evm_persistence_test.rs` | Integration Test | 4 | 0 | 0 |
| 17 | `p2_evm_logs_filter_test.rs` | Integration Test | 5 | 0 | 0 |
| 18 | `p3_rpc_compatibility_test.rs` | Integration Test | 1 | 0 | 0 |
| 19 | `p4_websocket_subscription_test.rs` | Integration Test | 9 | 0 | 0 |
| 20 | `p5_rpc_batch_and_tooling_test.rs` | Integration Test | 11 | 0 | 0 |
| 21 | `p6_smart_contract_test.rs` | Integration Test | 18 | 0 | 0 |
| 22 | `p7_solidity_and_precompiles_test.rs` | Integration Test | 12 | 0 | 0 |
| 23 | `p8_historical_state_test.rs` | Integration Test | 19 | 0 | 0 |
| 24 | `p9_ethereum_conformance_test.rs` | Integration Test | **39** | 0 | 0 |
| 25 | `security_audit_test.rs` | Integration Test | 7 | 0 | 0 |
| 26 | `testnet_stress_test.rs` | Integration Test | 4 | 0 | 0 |
| 27 | `tola_p2p (src/lib.rs)` | Unit Test | 12 | 0 | 0 |
| 28 | `tola_rpc (src/lib.rs)` | Unit Test | 26 | 0 | 0 |
| 29 | `tola_sdk (src/lib.rs)` | Unit Test | 10 | 0 | 0 |
| 30 | `tola_state (src/lib.rs)` | Unit Test | 4 | 0 | 0 |
| 31 | `tola_storage (src/lib.rs)` | Unit Test | 10 | 0 | 0 |
| 32 | `tola_types (src/lib.rs)` | Unit Test | 18 | 0 | 0 |
| 33 | `Doc-tests tola_codec` | Doc Test | 0 | 0 | 0 |
| 34 | `Doc-tests tola_consensus` | Doc Test | 0 | 0 | 0 |
| 35 | `Doc-tests tola_core` | Doc Test | 0 | 0 | 0 |
| 36 | `Doc-tests tola_crypto` | Doc Test | 0 | 0 | 0 |
| 37 | `Doc-tests tola_execution` | Doc Test | 0 | 0 | 0 |
| 38 | `Doc-tests tola_mempool` | Doc Test | 0 | 0 | 0 |
| 39 | `Doc-tests tola_node` | Doc Test | 0 | 0 | 0 |
| 40 | `Doc-tests tola_p2p` | Doc Test | 0 | 0 | 0 |
| 41 | `Doc-tests tola_rpc` | Doc Test | 0 | 0 | 0 |
| 42 | `Doc-tests tola_sdk` | Doc Test | 0 | 0 | 0 |
| 43 | `Doc-tests tola_state` | Doc Test | 0 | 0 | 0 |
| 44 | `Doc-tests tola_storage` | Doc Test | 0 | 0 | 0 |
| 45 | `Doc-tests tola_types` | Doc Test | 0 | 0 | 0 |
| **Total** | **45 targets (32 binary + 13 doc)** | | **356** | **0** | **0** |

---

## 7. Git Diff Summary

Only a single test file was modified: [`crates/tola-node/tests/p9_ethereum_conformance_test.rs`](../../crates/tola-node/tests/p9_ethereum_conformance_test.rs):
- Added `test_p9_canonical_evm_call_depth_1024_boundary` (104 lines of code).
- Updated documentation comments on `test_p9_canonical_gst_call_depth_limits` to clarify that it serves as a shallow recursion regression test (depth 5).
- Zero modifications to production source code or unrelated tests.

---

## 8. Conclusion & Phase Status

1. **Remediation Completed**: The EVM maximum call-depth boundary test has been implemented and verified.
2. **Phase Status**: **Phase 9 remains OPEN — NOT APPROVED**.
3. **Next Step**: Independent forensic audit of the remediated P9 implementation.
