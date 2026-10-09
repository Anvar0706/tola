# ADR-0018: EIP-2930 Access Lists, Intrinsic Gas Metering, and Ethereum Developer Compatibility (EVM-P10)

## Status
ACCEPTED (TOLA EVM Phase 10)

## Context
Following the successful completion of EVM Core phases P0–P9, TOLA demonstrated robust Shanghai execution semantics and verified boundary behaviors against official Ethereum GeneralStateTest (GST) vectors and REVM 43.0.3 reference oracles.

However, an in-depth architectural audit identified three developer-compatibility and conformance gaps required for full parity with the Ethereum ecosystem:
1. **Access List Support (EIP-2930 & EIP-1559):** While Type-2 transactions were admitted into the mempool and executed, access lists were discarded or unsupported at the envelope decoding and execution boundaries. Specifically, Type-1 (EIP-2930) envelopes were unsupported, and Type-2 access list entries were not passed into REVM `TxEnv`, preventing smart contracts relying on pre-warmed storage slots (e.g., cross-contract reentrancy guards, DeFi routers) from benefiting from EIP-2930 warm-access gas discounts.
2. **Intrinsic Gas Accounting & Boundary Validation:** `calculate_intrinsic_gas` did not meter access list entries (2,400 gas per address and 1,900 gas per storage key), nor did `stateless_validate` bound the maximum number of access list items, risking memory blowup under adversarial transactions.
3. **`eth_estimateGas` Payable Simulation Failure:** The JSON-RPC dispatcher hardcoded `value: Amount::ZERO` during `eth_estimateGas` calls, causing any payable contract function requiring `msg.value > 0` to revert with `execution reverted` during estimation even when the caller explicitly provided a non-zero `value`.
4. **Fixture Post-State Depth:** The Shanghai GST runner validated post-execution receipts and return data, but lacked exhaustive verification of all normalized post-state accounts (`balance`, `nonce`, runtime `code`, and all `storage` slots).

---

## Decision

### 1. Work Package 1: Access List Primitives (`tola-types`)
- **Primitive Definition:**
  ```rust
  pub const MAX_ACCESS_LIST_ITEMS: usize = 256;

  #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
  pub struct AccessListItem {
      pub address: Address,
      pub storage_keys: Vec<Hash>,
  }
  ```
- **Transaction Extension:** Added `pub access_list: Vec<AccessListItem>` to `Transaction`. Backward compatibility is preserved across all native TOLA wire decoders by defaulting to `Vec::new()`.
- **Intrinsic Gas Metering:** Updated `Transaction::intrinsic_gas()` to strictly compute:
  $$\text{IntrinsicGas} = G_{\text{tx}} + G_{\text{calldata}} + G_{\text{create}} + (2400 \times N_{\text{addresses}}) + (1900 \times N_{\text{storage\_keys}})$$
  Where $G_{\text{create}} = 32{,}000 + 2 \times \lceil\text{initcode\_len} / 32\rceil$ if contract creation.
- **Stateless Validation:** Enforced `access_list.len() <= MAX_ACCESS_LIST_ITEMS` (rejecting with `TransactionError::AccessListTooLarge`), and `gas_limit >= intrinsic_gas` (rejecting with `TransactionError::GasLimitTooLow`).

### 2. Work Package 2: RLP Envelopes & Codecs (`tola-codec`)
- Implemented `encode_access_list(&[AccessListItem]) -> Vec<u8>` and `decode_access_list(&RlpItem) -> Result<Vec<AccessListItem>, CodecError>`.
- Implemented `compute_type1_signing_hash(&Transaction) -> Hash` adhering to EIP-2930:
  $$\text{Keccak-256}(0x01 \parallel \text{rlp}([ \text{chainId}, \text{nonce}, \text{gasPrice}, \text{gasLimit}, \text{to}, \text{value}, \text{data}, \text{accessList} ]))$$
- Updated `compute_type2_signing_hash` to include the encoded `accessList` in the 9-element signing tuple.
- Updated `decode_raw_eth_transaction` to parse Type-1 envelopes (`raw[0] == 0x01`, 11 elements) and recover ECDSA `y_parity` signatures, and parse `items[8]` in Type-2 envelopes.
- Updated `encode_raw_eth_transaction` to serialize both Type-1 and Type-2 envelopes with full access lists.

### 3. Work Package 3: Execution Engine Wiring (`tola-execution`)
- In `EvmExecutor::execute_transaction` and `simulate_call`, mapped `tx.access_list` to `revm::context_interface::transaction::AccessList`.
- Set `tx_env.access_list = revm_access_list;`.
- **Crucial REVM Invariant:** Explicitly assigned `tx_env.tx_type = tx.tx_type;`. When `tx_type` remains default 0 (Legacy), REVM bypasses access list warming even when entries are present in `tx_env.access_list`. Assigning `tx.tx_type` (1 or 2) enables REVM's EIP-2930 warm storage slot mechanics.
- In `verify_stateless` and `Mempool::admit`, wired Type-1 transactions through `compute_type1_signing_hash` for sender recovery.

### 4. Work Package 4: JSON-RPC Dispatcher Enhancements (`tola-rpc`)
- Added `parse_access_list_json` to parse `accessList` arrays from `eth_call`, `eth_estimateGas`, and transaction payloads.
- In `handle_eth_estimate_gas`, correctly extracted `value` from request params, resolving the payable estimation failure.
- In `handle_eth_get_transaction_receipt`, set `"type": "0x1"` for Type-1 receipts.
- In `format_transaction_json`, serialized `"type": "0x1"` and populated `"accessList": [ { "address": "0x...", "storageKeys": [ "0x..." ] } ]` for both Type-1 and Type-2 transactions.

### 5. Work Package 5: Shanghai GST Fixture Runner Upgrade (`tola-node`)
- Upgraded `run_official_shanghai_fixture_json` in `crates/tola-node/tests/p9_ethereum_conformance_test.rs` to verify full normalized post-state:
  - Account `balance` matches expected hex amount.
  - Account `nonce` matches expected hex integer.
  - Deployed runtime `code` matches expected bytecode.
  - All declared storage slots match expected 32-byte values.

### 6. Work Package 6: Dedicated Integration Testing (`crates/tola-node/tests/p10_access_list_and_compatibility_test.rs`)
- Implemented comprehensive integration test suite verifying:
  1. `test_p10_eip2930_access_list_warm_slot_gas_discount`: Proves warm SLOAD (100 gas vs 2,100 gas cold) produces exact gas delta (+4,300 intrinsic - 2,000 execution = +2,300 net gas).
  2. `test_p10_access_list_size_limit_rejection`: Proves 256 items pass, 257 items reject with `AccessListTooLarge`.
  3. `test_p10_intrinsic_gas_access_list_metering`: Proves exact gas calculation (27,200 gas) and rejection at 27,199.
  4. `test_p10_payable_eth_estimate_gas_accuracy`: Proves contracts asserting `msg.value > 0` succeed in `eth_estimateGas`.
  5. `test_p10_full_rpc_roundtrip_type1_and_type2_with_access_list`: Proves end-to-end block commitment, receipt generation, RPC format, and wire decode.

---

## Consequences & Verification
- **Full Workspace Green:** 46 targets compiled, 366 passing tests (356 prior + 10 new), 0 failed, 0 ignored.
- **Linter & Style Clean:** 0 clippy warnings (`cargo clippy --all-targets -- -D warnings`), 0 formatting differences (`cargo fmt --all -- --check`).
- **Cross-Language SDKs Verified:** TypeScript SDK (49/49 passing), Python SDK (46/46 passing).
- **Zero Regressions:** All prior P0–P9 conformance invariants, Block-STM concurrency, and L1 consensus sovereignty remain intact.
