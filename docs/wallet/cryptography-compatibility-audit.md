# TOLA NETWORK — WALLET CRYPTOGRAPHY COMPATIBILITY AUDIT REPORT

**Date:** 2026-10-07  
**Auditor:** Independent Cryptography & Protocol Architecture Auditor  
**Scope:** Verification of Native TOLA Protocol Cryptography vs Seed-Phrase Wallet Derivation  
**Repository State:** Phase 0–18 Complete, Pre-Mainnet Blocker Remediation Complete, Phase 2A CLI Wallet Integrated  
**Final Verdict:** **COMPATIBLE**

---

## EXECUTIVE SUMMARY

A strict, independent cryptographic compatibility audit was conducted across the TOLA Network codebase to evaluate whether the native seed-phrase wallet implementation (`BIP-39 → BIP-32 / BIP-44 → Secp256k1 → TOLA Address`) is fully compatible with the existing node, mempool, execution engine, RPC dispatcher, and validator consensus mechanisms.

The audit verified that TOLA Network operates a deliberate, mathematically sound **dual-cryptography model**:
1. **Consensus Layer:** Ed25519 (RFC 8032) for Jolteon/HotStuff BFT proposals, votes, Quorum Certificates (QCs), validator equivocation slashing proofs, and 5-of-7 Security Council multi-sig.
2. **Account / Execution Layer:** Secp256k1 ECDSA (RFC 6979, 65-byte recoverable signature `[r || s || v]`, strict Low-S check) for user accounts, transaction signing, EVM/Block-STM state execution, and mempool admission.

The new BIP-39 / BIP-44 seed phrase derivation derives native Secp256k1 private keys and computes EVM-compatible Keccak-256 addresses identical to the existing account model (`tola-cli account new` and `tola-sdk::wallet::TolaWallet`).

A live, end-to-end integration test was executed against a running standalone node daemon over HTTP JSON-RPC, proving that transactions derived from a seed phrase are signed locally, admitted to the mempool, verified by the node, executed by Block-STM, and committed to persistent state without any protocol changes.

---

## SECTION A: EXISTING PROTOCOL CRYPTOGRAPHY

Direct code inspection of the TOLA Network repository reveals the following cryptographic primitives:

### 1. Account & Transaction Layer (User Space)
* **Signature Algorithm:** ECDSA over curve **Secp256k1** (`k256` crate in Rust, pure JavaScript in `@tola/sdk`, pure Python in `tola`).
* **Signature Format:** 65 bytes serialized as `[r (32 bytes) || s (32 bytes) || v (1 byte)]`.
  * `v`: Recovery ID (`0` or `1`, normalized to `27`/`28` for EVM compatibility).
  * Strict BIP-146 Low-S enforcement: Signatures where $s > N / 2$ (half curve order) are strictly rejected to eliminate signature malleability.
  * Deterministic nonce generation: RFC 6979 deterministic $k$-generation.
* **Digest Construction:** Transactions are hashed using domain-separated BLAKE3:
  $$\text{digest} = \text{BLAKE3}(\text{DOMAIN\_TRANSACTION\_PAYLOAD} \parallel \text{chain\_id} \parallel \text{epoch} \parallel \text{payload\_hash})$$
  This binds chain identity and replay protection directly into the signing digest.
* **Signer Recovery:** The execution engine and mempool call `recover_signer(&tx.payload_hash, &tx.signature)`:
  * Reconstructs the 65-byte uncompressed public key point via ECDSA public key recovery.
  * Eliminates the need to transmit public keys on the wire (saving 33–65 bytes per transaction).
  * Hashes the recovered public key to verify the sender address.

### 2. Consensus & Validator Layer (Consensus Space)
* **Signature Algorithm:** **Ed25519** (RFC 8032, `ed25519-dalek` crate).
* **Usage:** Jolteon/HotStuff 2-chain BFT consensus proposals, votes, Quorum Certificates (QCs), validator registration Proof of Possession (PoP), equivocation slashing evidence, and Security Council emergency pause signals.
* **Rationale:** Ed25519 provides ultra-fast batch signature verification, deterministic nonces, zero malleability, and pure threshold aggregation essential for high-throughput 2.0s block cadences.

### 3. Hash Functions & Commitments
* **State Tree:** Jellyfish Merkle Tree (JMT) keyed by BLAKE3 hashes of 20-byte account addresses.
* **P2P Wire Framing:** BLAKE3 32-byte frame checksums.
* **Address Generation:** Keccak-256 (standard Ethereum/EVM specification).

---

## SECTION B: EXISTING ACCOUNT / ADDRESS MODEL

### 1. Address Derivation Pipeline
Inspected in `crates/tola-crypto/src/lib.rs` (`pubkey_to_address`), `crates/tola-types/src/address.rs`, and cross-language SDKs:
$$\text{Secp256k1 Secret Key (32 bytes)} \xrightarrow{\text{point multiplication}} \text{Uncompressed Public Key (65 bytes: } [0x04 \parallel X \parallel Y])$$
$$\text{Address (20 bytes)} = \text{Keccak-256}(X \parallel Y)[12..32]$$

### 2. Address Encodings
* **Raw Hex:** `0x` followed by 40 hexadecimal characters (case-insensitive or EIP-55 checksummed).
* **Canonical Bech32m:** BIP-350 Bech32m checksummed string encoding the 20 raw bytes with network Human-Readable Part (HRP):
  * **Mainnet (Chain ID 9999):** HRP `tola` $\rightarrow$ `tola1...` (e.g. `tola18dzkvnsetkav6qp4q344z9y3uzda8j5263ez6j`)
  * **Testnet (Chain ID 102):** HRP `ttola` $\rightarrow$ `ttola1...` (e.g. `ttola13cm3nk3xh9qg7fl6jcpvgd3dn77ynhrnukw78v`)
  * **Devnet (Chain ID 1337):** HRP `dtola` $\rightarrow$ `dtola1...` (e.g. `dtola1...`)
* All RPC endpoints accept both hex (`0x...`) and Bech32m encodings.

### 3. Legacy Account Compatibility (`tola-cli account new`)
`tola-cli account new` generates a raw 32-byte Secp256k1 private key using `OsRng`. Its public key and address generation use the exact same `pubkey_to_address` pipeline.
* **Verdict:** Legacy accounts and seed-derived accounts use the **exact same address space, state representation, and signature format**.

---

## SECTION C: NEW SEED WALLET CRYPTOGRAPHY

Implemented in `crates/tola-crypto/src/mnemonic.rs` and integrated in `crates/tola-sdk/src/wallet.rs` and `crates/tola-cli/src/wallet.rs`:

### 1. Standard Compliance
* **BIP-39:** Mnemonic generation from 128 or 256 bits of CSPRNG entropy (`rand::rngs::OsRng`), 12 or 24 words from the standard English wordlist (2048 words), NFKD lowercased normalization, and strict checksum validation.
* **BIP-39 Seed Generation:** PBKDF2-HMAC-SHA512 with salt `b"mnemonic" + passphrase` and 2048 iterations yielding a 64-byte (512-bit) binary seed.
* **BIP-32 HD Key Derivation:**
  * Master node generation: $\text{HMAC-SHA512}(b"\text{Bitcoin seed}", \text{seed})$.
  * Child key derivation: Standard BIP-32 derivation supporting hardened ($i \ge 2^{31}$) and unhardened child indices.
  * Curve: Secp256k1.
* **Security & Memory Hardening:**
  * Sensitive structs (`Mnemonic`, `Seed`, `TolaWallet`) implement `zeroize::ZeroizeOnDrop` to scrub memory on deallocation.
  * `Debug` implementations emit `[REDACTED]` to prevent secret leaks to logs.
  * Terminal echo suppression on Windows (`SetConsoleMode`) and Unix (`termios`) prevents shoulder-surfing during recovery.

### 2. Derivation Paths Supported
* **Mainnet:** `m/44'/9999'/0'/0/0`
* **Testnet:** `m/44'/102'/0'/0/0`
* **Devnet:** `m/44'/1337'/0'/0/0`
* **Standard EVM Fallback:** `m/44'/60'/0'/0/0`

---

## SECTION D: COMPATIBILITY RESULT

| Dimension | Native Protocol Expectation | Seed-Phrase Wallet Derivation | Status |
| :--- | :--- | :--- | :--- |
| **Account Key Type** | Secp256k1 (32-byte scalar) | Secp256k1 (32-byte scalar) | **MATCH** |
| **Public Key Format** | 65-byte uncompressed point | 65-byte uncompressed point | **MATCH** |
| **Address Algorithm** | Keccak-256(pubkey[1..65])[12..32] | Keccak-256(pubkey[1..65])[12..32] | **MATCH** |
| **Signature Format** | 65-byte recoverable ECDSA `[r\|\|s\|\|v]` | 65-byte recoverable ECDSA `[r\|\|s\|\|v]` | **MATCH** |
| **Signature Verification** | Low-S canonical check + ecdsa recovery | Low-S canonical check + ecdsa recovery | **MATCH** |
| **RPC Compatibility** | `eth_sendRawTransaction` / `tola_*` | Compatible with raw transaction builder | **MATCH** |
| **Mempool Admission** | Nonce tracking + balance debit | Validated and admitted identically | **MATCH** |
| **Execution Engine** | Block-STM parallel execution | Executed and applied to state identically | **MATCH** |
| **Legacy Key Compatibility** | Raw Secp256k1 hex import | Interoperable in same state trie | **MATCH** |

### **Overall Result: FULLY COMPATIBLE**

---

## SECTION E: ANY MISMATCH FOUND

### 1. Consensus vs User Account Key Type (Intentional Design Separation)
* **Finding:** Validator consensus nodes use **Ed25519** signing keys. User wallets use **Secp256k1** signing keys.
* **Audit Evaluation:** This is **NOT a defect or mismatch**. It is an intentional, industry-standard architectural separation (similar to Near Protocol, Aptos, and Ethereum PoS where execution and validator consensus keys are isolated).
* **Implication:** User seed phrases MUST NOT be used to derive validator consensus keys directly without a separate consensus key derivation specification. Validator keys must continue to be generated via `tola-cli ceremony` with Proof of Possession.

### 2. BIP-44 Coin Type vs Chain ID Namespace Conflation
* **Finding:** The provisional derivation path utilizes `m/44'/9999'/0'/0/0` on Mainnet and `m/44'/102'/0'/0/0` on Testnet.
* **Audit Evaluation:**
  * In standard multi-chain wallets (e.g., MetaMask, Ledger, Trezor), BIP-44 `coin_type` is an SLIP-0044 registered identifier (e.g. `60'` for Ethereum, `0'` for Bitcoin).
  * Replay protection in TOLA is **already cryptographically guaranteed** at the transaction digest layer via `ChainId` domain separation:
    $$\text{BLAKE3}(\text{DOMAIN\_TRANSACTION\_PAYLOAD} \parallel \text{chain\_id} \parallel \text{epoch} \parallel \text{payload\_hash})$$
  * A transaction signed on Testnet (`chain_id = 102`) can **never** be replayed on Mainnet (`chain_id = 9999`), even if the exact same private key and address are used.
  * Parameterizing `coin_type` with `chain_id` means that the same 12-word mnemonic yields a *different* address on Mainnet than on Testnet.

---

## SECTION F: REQUIRED CORRECTION & RECOMMENDATIONS

### 1. Derivation Path Standardization Policy
* **Correction:** No code breaking changes are required. The codebase already supports arbitrary derivation paths in `crates/tola-crypto/src/mnemonic.rs`.
* **Standardization Policy:**
  1. **Canonical TOLA Native Path:** `m/44'/9999'/0'/0/0` (ADR-0013 proposed standard for TOLA-native wallets).
  2. **Universal Compatibility Path:** `m/44'/60'/0'/0/0` (supported for third-party multi-chain wallets and Ledger/Trezor hardware integration where coin type `60'` is default).
  3. **Documentation Clarification:** Clarify in `docs/wallet/key-derivation.md` that network replay protection is enforced by Chain ID in transaction digests, not by changing the BIP-44 path between Testnet and Mainnet.

---

## SECTION G: FINAL DERIVATION PATH

| Environment | Canonical Derivation Path | Universal EVM Fallback |
| :--- | :--- | :--- |
| **Mainnet (Chain ID 9999)** | `m/44'/9999'/0'/0/0` | `m/44'/60'/0'/0/0` |
| **Testnet (Chain ID 102)** | `m/44'/102'/0'/0/0` | `m/44'/60'/0'/0/0` |
| **Devnet (Chain ID 1337)** | `m/44'/1337'/0'/0/0` | `m/44'/60'/0'/0/0` |

---

## SECTION H: TEST EVIDENCE

### 1. End-to-End Live Node Integration Test
An automated, live integration test was implemented in `crates/tola-node/tests/testnet_stress_test.rs`:
`test_seed_phrase_wallet_end_to_end_against_node`

#### Verification Steps Executed:
1. **Mnemonic Generation:** Generated canonical 12-word BIP-39 mnemonic:
   `abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about`
2. **Deterministic Derivation:** Derived Testnet account at `m/44'/102'/0'/0/0`:
   * Address: `0x8e3719da26b9408f27fa9602c4362d9fbc49dc73` (`ttola13cm3nk3xh9qg7fl6jcpvgd3dn77ynhrnukw78v`)
3. **Genesis Allocation:** Funded the derived address in genesis with 1,000,000 TOLA.
4. **Node Daemon Boot:** Initialized a full standalone `TolaNode` daemon on a dedicated port (`127.0.0.1:18552`), binding HTTP JSON-RPC.
5. **RPC Balance Query:** Queried `eth_getBalance` via HTTP RPC -> confirmed exact 1,000,000 TOLA balance.
6. **Transaction Building & Local Signing:** Constructed a transfer of 1,000 TOLA to `0x1111111111111111111111111111111111111111` with gas price 100 nTOLA and gas limit 21,000. Signed transaction locally with the seed-derived wallet.
7. **RPC Broadcast:** Submitted raw signed transaction via HTTP `eth_sendRawTransaction`.
8. **Mempool Admission:** Mempool successfully recovered the public key from the 65-byte signature, verified the low-S condition, verified the sender balance and nonce (0), and admitted the transaction to the ready queue.
9. **Block Proposal & Execution:** The node assembled a proposal block, executed transactions concurrently via Block-STM, verified zero state conflicts, and committed the block to the Jellyfish Merkle Tree.
10. **State Verification:** Queried RPC endpoints:
    * Sender nonce: Advanced from `0` to `1`.
    * Sender balance: Debited by 1,000 TOLA + gas fee.
    * Recipient balance: Credited with 1,000 TOLA.

**Test Run Output:**
```text
running 3 tests
test test_client_sdk_end_to_end_against_node ... ok
test test_seed_phrase_wallet_end_to_end_against_node ... ok
test test_concurrent_multi_account_transfer_load ... ok

test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.24s
```

### 2. Workspace Automated Test Suite Parity
* **Rust Workspace:** 166/166 passing (`cargo test --workspace`)
* **TypeScript SDK:** 12/12 passing (`node --test sdk/typescript/test/sdk.test.mjs`)
* **Python SDK:** 10/10 passing (`python -m unittest sdk/python/tests/test_tola.py`)
* **Linter & Formatting:**
  * `cargo clippy --workspace -- -D warnings`: 0 warnings (PASS)
  * `cargo fmt --check`: 0 formatting discrepancies (PASS)

---

## CONCLUSION & FINAL REPORT METRICS

```text
Native TOLA key type: Secp256k1 (accounts/tx) / Ed25519 (validators/consensus)
Existing address algorithm: Keccak256(pubkey[1..65])[12..32] -> 20 bytes -> Bech32m
Existing transaction signature algorithm: Secp256k1 ECDSA (recoverable 65-byte [r || s || v], RFC 6979, Low-S)
Seed wallet key type: Secp256k1 (32-byte scalar)
Seed derivation path: m/44'/9999'/0'/0/0 (Mainnet), m/44'/102'/0'/0/0 (Testnet), m/44'/60'/0'/0/0 (Fallback)
Address compatibility: YES
Transaction compatibility: YES
Node verification: YES
Legacy wallet compatibility: YES
Result: COMPATIBLE
```
