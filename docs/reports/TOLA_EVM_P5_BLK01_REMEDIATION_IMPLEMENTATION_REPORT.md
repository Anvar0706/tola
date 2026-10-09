# TOLA EVM P5-BLK-01 — REMEDIATION IMPLEMENTATION REPORT

**Phase:** P5-BLK-01 Remediation Implementation  
**Status:** IMPLEMENTATION COMPLETE — READY FOR FORENSIC AUDIT  
**Date:** October 8, 2026  
**Target:** Cryptographic Envelope Resolution for Ethereum Transaction Type Persistence  

---

## 1. EXECUTIVE SUMMARY

The critical blocker **P5-BLK-01** (Legacy Ethereum transaction envelope type corruption across canonical serialization and persistent reload) has been successfully remediated.

Prior to this remediation, canonical deserialization (`Transaction::read_canonical`) unconditionally hardcoded `tx_type = 2` whenever `signature` was `Signature::Secp256k1Eth`. Consequently, upon node restart or cold-cache disk reload, any legacy Ethereum transaction (`tx_type = 0`) was deserialized with `tx_type = 2`, corrupting JSON-RPC responses (`eth_getTransactionByHash`, `eth_getBlockByNumber`), receipt generation (`eth_getTransactionReceipt`), and raw transaction re-encoding.

Under the approved **Cryptographic Envelope Resolution** architecture, `crates/tola-codec/src/lib.rs` now deterministically reconstructs the correct Ethereum transaction envelope type (`tx_type = 0` vs `tx_type = 2`) during `Transaction::read_canonical` without altering a single byte of the canonical serialization format, without appending length prefixes, without codec versioning, and without requiring database migrations.

---

## 2. REMEDIATION ARCHITECTURE & RESOLUTION MECHANISM

### 2.1 The Two-Tiered Resolution Algorithm

When `Transaction::read_canonical` deserializes a transaction signed with `Signature::Secp256k1Eth { r, s, y_parity, chain_id }`:

1. **Tier 1 — $O(1)$ Fast-Path (Fee Asymmetry Check):**
   $$\text{if } \text{max\_fee\_per\_gas} \neq \text{max\_priority\_fee\_per\_gas} \implies \text{tx\_type} = 2$$
   *Proof of Soundness:* In Ethereum Legacy transactions (EIP-155), there is only a single `gasPrice` parameter. When mapped into TOLA's internal data model, `max_fee_per_gas` and `max_priority_fee_per_gas` are set to identical values (`gasPrice`). A legacy transaction can **never** have asymmetric fees. Therefore, fee asymmetry guarantees Type-2 envelope classification at zero cryptographic overhead.

2. **Tier 2 — Dual-Recovery Verification (Equal Fee Edge Case):**
   When `max_fee_per_gas == max_priority_fee_per_gas`:
   - A candidate Type-2 transaction envelope ($0\text{x}02 \parallel \text{RLP}$) is constructed in-memory.
   - The Type-2 signing hash $H_{\text{type2}} = \text{keccak256}(0\text{x}02 \parallel \text{RLP}([...]))$ is computed via `tola_codec::rlp::compute_type2_signing_hash`.
   - The 65-byte recoverable ECDSA signature $[r \parallel s \parallel y\text{\_parity}]$ is evaluated against $H_{\text{type2}}$ via `tola_crypto::recover_secp256k1_address`.
   - If the recovered 20-byte address equals the consensus-authenticated `tx.from`, the transaction is cryptographically proven to be Type-2 ($\text{tx\_type} = 2$).
   - Otherwise, the transaction is proven to be Legacy ($\text{tx\_type} = 0$).

3. **Fallback Safety:**
   - Any non-Secp256k1Eth signature (`Signature::Ed25519`, `Signature::Mock`, etc.) resolves to $\text{tx\_type} = 0$.
   - Any unrecoverable or malformed ECDSA point with equal fees safely resolves to $\text{tx\_type} = 0$ without panicking or failing deserialization.

---

## 3. EXACT CODE MODIFICATIONS

### File: `crates/tola-codec/src/lib.rs`

```rust
// In Transaction::read_canonical:
let tx_type = resolve_eth_tx_type(
    chain_id,
    nonce,
    from,
    to,
    value,
    gas_limit,
    max_fee_per_gas,
    max_priority_fee_per_gas,
    valid_until_block,
    &data,
    memo.as_deref(),
    &signature,
);

#[allow(clippy::too_many_arguments)]
fn resolve_eth_tx_type(
    chain_id: ChainId,
    nonce: Nonce,
    from: Address,
    to: Option<Address>,
    value: Amount,
    gas_limit: Gas,
    max_fee_per_gas: Amount,
    max_priority_fee_per_gas: Amount,
    valid_until_block: BlockHeight,
    data: &[u8],
    memo: Option<&str>,
    signature: &Signature,
) -> u8 {
    match signature {
        Signature::Secp256k1Eth {
            r,
            s,
            y_parity,
            chain_id: sig_chain_id,
        } => {
            // Fast-path: In Legacy EIP-155 transactions, gas_price is singular and aliases
            // into both fee fields. If max_fee != max_priority, it cannot be Legacy.
            if max_fee_per_gas != max_priority_fee_per_gas {
                2
            } else {
                // Equal-fee path: construct Type-2 signing preimage and test signature recovery
                let mut sig_65 = [0u8; 65];
                sig_65[0..32].copy_from_slice(r);
                sig_65[32..64].copy_from_slice(s);
                sig_65[64] = *y_parity;

                let candidate_tx = Transaction {
                    chain_id,
                    nonce,
                    from,
                    to,
                    value,
                    gas_limit,
                    max_fee_per_gas,
                    max_priority_fee_per_gas,
                    valid_until_block,
                    data: data.to_vec(),
                    memo: memo.map(|s| s.to_string()),
                    signature: Signature::Secp256k1Eth {
                        r: *r,
                        s: *s,
                        y_parity: *y_parity,
                        chain_id: *sig_chain_id,
                    },
                    tx_type: 0,
                };

                let h_type2 = rlp::compute_type2_signing_hash(&candidate_tx);
                match tola_crypto::recover_secp256k1_address(h_type2.as_bytes(), &sig_65) {
                    Ok(recovered) if recovered == from => 2,
                    _ => 0,
                }
            }
        }
        _ => 0,
    }
}
```

---

## 4. VERIFICATION & TEST SUITE METRICS

### 4.1 Dedicated Remediation Tests (`crates/tola-node/tests/p5_rpc_batch_and_tooling_test.rs`)

Three new comprehensive test functions with end-to-end multi-transaction verification were implemented:

1. **`test_p5_blk01_codec_and_block_roundtrip`**:
   - **Test 1 (Legacy roundtrip):** Validates that a Legacy transaction (`tx_type = 0`, equal fees) preserves `tx_type == 0`, `TransactionId`, and `compute_eth_tx_hash` across canonical serialization and deserialization.
   - **Test 2 (Type-2 diff-fee):** Validates that a Type-2 transaction with asymmetric fees (`max_fee != max_priority`) preserves `tx_type == 2` via Tier-1 fast path.
   - **Test 3 (Type-2 equal-fee):** Validates that a Type-2 transaction with identical fees (`max_fee == max_priority`) preserves `tx_type == 2` via Tier-2 cryptographic recovery.
   - **Test 4 (Mixed block `[0, 2, 2, 0]`):** Constructs a canonical block with 4 transactions in alternating sequence: Legacy, Type-2 (diff), Type-2 (equal), Legacy. Serializes the entire block to canonical bytes, deserializes, and asserts exact type array `[0, 2, 2, 0]`, matching `TransactionId`s, matching raw Ethereum wire encodings, and invariant block `txs_root`.

2. **`test_p5_blk01_persistent_storage_and_rpc_restart`**:
   - Creates a temporary RocksDB/KV directory on disk.
   - Initializes a `PersistentStateDB` and commits a block containing the mixed `[0, 2, 2, 0]` sequence along with execution receipts using `commit_block_bundle`.
   - Simulates node shutdown by dropping the in-memory database and dispatcher.
   - Reopens `PersistentStateDB` from disk.
   - Reads the block from disk via `get_block_by_height(42)` and asserts that all 4 transaction types are preserved as `[0, 2, 2, 0]`.
   - Reconstructs a cold `RpcDispatcher` backed by the restarted persistent database (all in-memory lookup caches empty).
   - Queries `eth_getTransactionByHash` for Legacy (returns `"type": "0x0"`) and Type-2 equal-fee (returns `"type": "0x2"` and `"accessList": []`).
   - Queries `eth_getTransactionReceipt` for Legacy (returns `"type": "0x0"`) and Type-2 equal-fee (returns `"type": "0x2"`).
   - Queries `eth_getBlockByNumber("0x2a", true)` and verifies all 4 full transaction JSON objects retain their original types (`"0x0"`, `"0x2"`, `"0x2"`, `"0x0"`).

3. **`test_p5_blk01_mock_and_malformed_signature_fallback`**:
   - Verifies that non-secp256k1 signatures (e.g., `Ed25519`) safely resolve to `0`.
   - Verifies that corrupted or unrecoverable Secp256k1Eth signatures with equal fees safely fall back to `0` without panic or error.
   - Verifies that unequal fees with Secp256k1Eth resolve to `2` via fast path.

### 4.2 Complete Regression Execution Results

| Test Suite | Command | Total Tests | Passed | Failed | Status |
| :--- | :--- | :---: | :---: | :---: | :---: |
| **P5 Integration Suite** | `cargo test -p tola-node --test p5_rpc_batch_and_tooling_test` | 11 | 11 | 0 | **GREEN** |
| **Rust Workspace Regression** | `cargo test --workspace` | 247 | 247 | 0 | **GREEN** |
| **Code Formatting** | `cargo fmt --all -- --check` | - | - | 0 | **CLEAN** |
| **Clippy Lint Verification** | `cargo clippy --workspace --all-targets -- -D warnings` | - | - | 0 | **CLEAN** |
| **TypeScript SDK Suite** | `node --test test/sdk.test.mjs` | 49 | 49 | 0 | **GREEN** |
| **Python SDK Suite** | `python -m unittest discover -s sdk/python/tests` | 46 | 46 | 0 | **GREEN** |

---

## 5. HARD CONSTRAINT & SAFETY INVARIANT AUDIT

1. **Zero Canonical Byte Layout Modification:**
   `Transaction::serialize_canonical` and `Block::serialize_canonical` were **not modified**. Canonical bytes remain 100% bit-for-bit identical to pre-remediation bytes.
2. **Consensus Hash Invariance:**
   Because `Transaction::to_canonical_bytes()` output is unchanged:
   - `TransactionId(blake3_hash(&wire_bytes))` is invariant.
   - Block `txs_root` is invariant.
   - Consensus `compute_block_hash(&block)` is invariant.
3. **No Migration / Table Changes:**
   No RocksDB/DiskKvStore tables or schemas were created or modified.
4. **Collision Resistance:**
   The ECDSA signature on a Type-2 transaction commits to $H_{\text{type2}} = \text{keccak256}(0\text{x}02 \parallel \text{RLP})$. For an attacker to forge a Legacy transaction whose signature also recovers the sender address under $H_{\text{type2}}$ requires finding a signature collision under distinct Keccak256 preimages ($p \approx 2^{-256}$).

---

## 6. FINAL VERDICT

All requirements of the authorized P5-BLK-01 remediation architecture have been implemented, tested, and validated across persistent disk storage, JSON-RPC, receipt generators, and full regression suites.

```
IMPLEMENTATION COMPLETE — READY FOR FORENSIC AUDIT
```
