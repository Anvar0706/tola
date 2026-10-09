# TOLA EVM P8 — HISTORICAL STATE & BLOCK-TAG QUERY FIDELITY IMPLEMENTATION REPORT

**Phase:** P8 — Historical State & Block-Tag Query Fidelity  
**Status:** IMPLEMENTATION COMPLETE  
**Repository:** TOLA Network  
**EVM Baseline:** REVM 43.0.3 (Shanghai)  
**Database Architecture:** Unified `StateDB` / Single Canonical State Root  

---

## 1. Executive Summary

Phase 8 (P8) of the TOLA EVM Core implementation has been completed in exact conformance with the approved P8 Architecture Audit. 

Prior to P8, state queries (`eth_getBalance`, `eth_getCode`, `eth_getStorageAt`, `eth_getTransactionCount`) and simulation RPCs (`eth_call`, `eth_estimateGas`) evaluated exclusively against the chain head (`HEAD`).

With P8, TOLA delivers full historical state query fidelity, historical call simulation, named block tags (`latest`, `earliest`, `pending`, `safe`, `finalized`), explicit hex/dec heights, and EIP-1898 object queries while preserving all core protocol invariants:
1. **Zero Second EVM Database:** EVM and native TOLA state remain strictly unified inside a single `PersistentStateDB`.
2. **Single Canonical State Root:** No shadow trees, second state roots, or secondary Merkle tries were introduced.
3. **No Sequential Block Replay:** Historical state lookups complete in $O(1)$ to $O(\log \text{history})$ time without re-executing blocks.
4. **Historical Simulation Fidelity:** `eth_call` and `eth_estimateGas` execute in the exact environment context (`block.number`, `timestamp`, `basefee`, `coinbase`, and `BLOCKHASH` 256-block window) of target block $N$.
5. **Strict Future Block Rejection:** Block heights exceeding the committed head return standard RPC error `-32000` (`header not found`), preventing silent fallback to latest.

---

## 2. Architectural Implementation Details

### 2.1 Historical Accounts via 16-ary Jellyfish Merkle Tree (JMT)
- In TOLA, account state (`balance`, `nonce`, `code_hash`, `flags`) is committed into a 16-ary Jellyfish Merkle Tree persisted in `Table::TreeNodes` content-addressed by BLAKE3 hash.
- Because tree nodes are copy-on-write and never overwritten or pruned, the `state_root` in `BlockHeader(N)` points to an immutable, fully traversable snapshot of all accounts at block $N$.
- Implemented `PersistentJmtReader` in `crates/tola-storage/src/jmt.rs` providing read-only point queries into `Table::TreeNodes` anchored to historical `BlockHeader(N).state_root`.

### 2.2 Historical Storage via `Table::StorageHistory`
- Active contract storage slots in `Table::Storage` remain flat and unversioned for maximum consensus write throughput.
- Added canonical table `Table::StorageHistory` (`"storage_history"`) in `crates/tola-storage/src/schema.rs`:
  - **Key Format (60 bytes):**  
    `[Address (20B) || Slot (32B) || BlockHeight Big-Endian (8B)]`
  - **Value Format (32 bytes):** Storage slot value (`Hash`).
  - **Lookup Mechanism:** Added `get_prev_or_equal` to `KeyValueStore` and `DiskKvStore`. Queries form the maximum key `(Address, Slot, Height_BE)` and perform a reverse range seek to find the latest mutation at or before the requested height in $O(\log \text{history})$ time.
  - **Zero-Value Deletion Invariant:** When a slot is cleared to zero, `Table::Storage` deletes the flat record, but `Table::StorageHistory` explicitly records `Hash::ZERO` to distinguish cleared slots from unwritten slots.

### 2.3 REVM Execution & Simulation Fidelity
- Updated `RevmReadOnlyAdapter` in `crates/tola-execution/src/evm_adapter.rs`:
  - Account queries resolve via `state.get_historical_account(BlockHeight(current_height), addr)`.
  - Storage queries resolve via `state.get_historical_storage(BlockHeight(current_height), addr, slot)`.
  - `block_hash(number)` opcode evaluates relative to target height $N$. Queries for $k \ge N$ or $k < N - 256$ return `0x0`. Queries in $[N-256, N-1]$ return the canonical hash of block $k$.
  - Contract bytecode resolves via `state.get_code(&acct.code_hash)` (immutable content addressing).

### 2.4 Canonical Block-Tag & EIP-1898 Resolution
- Implemented `resolve_block_tag` and `resolve_block_context` in `crates/tola-rpc/src/dispatcher.rs`:
  - Named tags: `"latest"`, `"safe"`, `"finalized"` resolve to latest committed block height; `"pending"` resolves to latest committed state with mempool nonce overlay; `"earliest"` resolves to block 0.
  - Integer / hex block tags: parsed into `BlockHeight`.
  - EIP-1898 object tags: parses `{ "blockNumber": "0x..." }` and `{ "blockHash": "0x...", "requireCanonical": bool }`.
  - Future block rejection: Any height exceeding the chain head returns RPC error `-32000`.

### 2.5 Cold Restart & Atomic Bundle Persistence
- `PersistentStateDB::open` re-hydrates `current_height` from `Table::Metadata` key `"latest_height"`.
- `RpcDispatcher::new` initializes its head tracker dynamically from `state_db.current_height()`, preventing cold restart desynchronization.
- `commit_block_bundle` writes accounts, storage, `StorageHistory`, code, JMT nodes, headers, bodies, receipts, and metadata in a single atomic `StorageBatch`.

---

## 3. Integration Test Suite (`p8_historical_state_test.rs`)

The test suite in `crates/tola-node/tests/p8_historical_state_test.rs` covers all 19 approved verification targets (Tests A through S):

| Test Identifier | Test Name | Invariant Verified | Result |
|:---|:---|:---|:---:|
| **Test A** | `test_p8_a_historical_native_balance_fidelity` | Exact native balance at intermediate blocks across transfers | **PASS** |
| **Test B** | `test_p8_b_historical_contract_code_fidelity` | Pre-deploy returns `0x`, post-deploy returns runtime bytecode | **PASS** |
| **Test C** | `test_p8_c_historical_storage_fidelity` | `eth_getStorageAt` reflects mutations across blocks 1 through 4 | **PASS** |
| **Test D** | `test_p8_d_historical_eth_call_simulation` | `eth_call` reads historical storage slot at blocks 2 and 3 | **PASS** |
| **Test E** | `test_p8_e_historical_nonce_fidelity` | `eth_getTransactionCount` reflects exact nonce progression | **PASS** |
| **Test F** | `test_p8_f_historical_gas_estimation` | `eth_estimateGas` simulates against historical target blocks | **PASS** |
| **Test G** | `test_p8_g_named_block_tags_fidelity` | `latest`, `earliest`, `pending`, `safe`, `finalized` resolution | **PASS** |
| **Test H** | `test_p8_h_historical_execution_opcode_fidelity` | `NUMBER`, `TIMESTAMP`, `BASEFEE`, `COINBASE` match block header | **PASS** |
| **Test I** | `test_p8_i_blockhash_256_window_anchored_to_target` | `BLOCKHASH` 256-block window anchored to historical target $N$ | **PASS** |
| **Test J** | `test_p8_j_cold_restart_historical_state_invariance` | Database reopened from disk; historical queries match exactly | **PASS** |
| **Test K** | `test_p8_k_future_block_rejection` | Query at $H > H_{\text{latest}}$ returns `-32000` (no fallback) | **PASS** |
| **Test L** | `test_p8_l_non_existent_account_slot_historical_consistency` | Non-existent accounts/slots return 0/empty gracefully | **PASS** |
| **Test M** | `test_p8_m_storage_zeroing_invariance` | Slot modified then cleared to 0; historical value preserved | **PASS** |
| **Test N** | `test_p8_n_multiple_contracts_independent_historical_states` | Two distinct contracts maintain isolated historical storage | **PASS** |
| **Test O** | `test_p8_o_eip1898_blocknumber_and_blockhash_queries` | EIP-1898 `blockNumber` and `blockHash` query objects | **PASS** |
| **Test P** | `test_p8_p_historical_nested_call_trace` | Contract A calls Contract B via `STATICCALL` on historical state | **PASS** |
| **Test Q** | `test_p8_q_unified_state_root_monotonic_determinism` | Historical queries perform zero state mutations; root unchanged | **PASS** |
| **Test R** | `test_p8_r_mempool_pending_overlay_isolation` | Mempool tx only visible in `pending`; `latest` remains committed | **PASS** |
| **Test S** | `test_p8_s_atomic_block_bundle_commit_invariance` | All tables, JMT nodes, and history written in single atomic batch | **PASS** |

---

## 4. Authoritative Workspace Test Reconciliation

Authoritative command:
```bash
cargo test --workspace
```

### Exact Target-by-Target Breakdown (Sum = 317)

| Target / Binary | Passed | Failed | Ignored |
|:---|:---:|:---:|:---:|
| `tola_cli (src/main.rs)` | 13 | 0 | 0 |
| `wallet_integration_test.rs` | 13 | 0 | 0 |
| `tola_codec (src/lib.rs)` | 12 | 0 | 0 |
| `tola_consensus (src/lib.rs)` | 20 | 0 | 0 |
| `tola_core (src/lib.rs)` | 0 | 0 | 0 |
| `tola_crypto (src/lib.rs)` | 22 | 0 | 0 |
| `tola_execution (src/lib.rs)` | 11 | 0 | 0 |
| `tola_mempool (src/lib.rs)` | 6 | 0 | 0 |
| `tola_node (src/lib.rs)` | 21 | 0 | 0 |
| `tola_node (src/main.rs)` | 0 | 0 | 0 |
| `autonomous_testnet_audit_test.rs` | 11 | 0 | 0 |
| `external_asset_architecture_test.rs` | 5 | 0 | 0 |
| `fuzz_and_chaos_test.rs` | 4 | 0 | 0 |
| `mainnet_tokenomics_governance_test.rs` | 5 | 0 | 0 |
| `multi_process_testnet.rs` | 4 | 0 | 0 |
| `p1_evm_persistence_test.rs` | 4 | 0 | 0 |
| `p2_evm_logs_filter_test.rs` | 5 | 0 | 0 |
| `p3_rpc_compatibility_test.rs` | 1 | 0 | 0 |
| `p4_websocket_subscription_test.rs` | 9 | 0 | 0 |
| `p5_rpc_batch_and_tooling_test.rs` | 11 | 0 | 0 |
| `p6_smart_contract_test.rs` | 18 | 0 | 0 |
| `p7_solidity_and_precompiles_test.rs` | 12 | 0 | 0 |
| **`p8_historical_state_test.rs` (NEW P8 SUITE)** | **19** | **0** | **0** |
| `security_audit_test.rs` | 7 | 0 | 0 |
| `testnet_stress_test.rs` | 4 | 0 | 0 |
| `tola_p2p (src/lib.rs)` | 12 | 0 | 0 |
| `tola_rpc (src/lib.rs)` | 26 | 0 | 0 |
| `tola_sdk (src/lib.rs)` | 10 | 0 | 0 |
| `tola_state (src/lib.rs)` | 4 | 0 | 0 |
| `tola_storage (src/lib.rs)` | 10 | 0 | 0 |
| `tola_types (src/lib.rs)` | 18 | 0 | 0 |
| Doc-tests (13 crates $\times$ 0) | 0 | 0 | 0 |
| **TOTAL** | **317** | **0** | **0** |

$$\sum \text{passed} = 317, \quad \sum \text{failed} = 0, \quad \sum \text{ignored} = 0$$

- Code formatting check: `cargo fmt --all -- --check` $\implies$ **CLEAN**
- Clippy linting check: `cargo clippy --workspace --all-targets --all-features -- -D warnings` $\implies$ **0 WARNINGS / CLEAN**

---

## 5. Architectural Decision Record

Documented and committed:
`decisions/ADR-0016-historical-evm-state-and-block-tag-query-architecture.md`

---

P8 IMPLEMENTATION COMPLETE — READY FOR FORENSIC AUDIT
