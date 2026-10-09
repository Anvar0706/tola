# TOLA NETWORK — RUST & PYTHON NATIVE WALLET AUDIT REPORT

**Date:** 2026-10-07  
**Status:** AUDIT COMPLETED — GAP ANALYSIS & IMPLEMENTATION PLAN  
**Standard Reference:** [TOLA Wallet Derivation Standard v1.0](./tola-wallet-derivation-standard-v1.md) (FROZEN)  
**Reference Implementation:** `@tola/sdk` (TypeScript Native Wallet SDK — Phase 18.x)

---

## 1. Executive Summary

This audit evaluates the current state of native wallet support across the TOLA Network stack:
1. **Rust Protocol & SDK** (`crates/tola-crypto`, `crates/tola-sdk`, `crates/tola-cli`)
2. **Python Client SDK** (`sdk/python/tola`)
3. **TypeScript Client SDK** (`sdk/typescript/src` — Reference Implementation)

The objective is to achieve **100% cryptographic parity**, **identical deterministic derivation**, **identical Bech32m address encoding**, **cross-compatible transaction and message signing**, and **zero secret leakage** across all three platforms without disrupting existing protocol rules or consensus cryptography.

---

## 2. Platform-by-Platform Audit

### 2.1. Rust (`crates/tola-crypto`, `crates/tola-sdk`, `crates/tola-cli`)

#### What Exists & Is Production-Ready:
- **`crates/tola-crypto/src/mnemonic.rs`**:
  - BIP-39 mnemonic generation (`MnemonicCount::Words12`, `MnemonicCount::Words24`) via system CSPRNG.
  - PBKDF2-HMAC-SHA512 seed derivation (2048 iterations) with NFKD normalization.
  - BIP-44 HD key derivation for Secp256k1 using `CANONICAL_DERIVATION_PATH = "m/44'/9999'/0'/0/0"`.
  - Secret memory protection (`Zeroize`, `ZeroizeOnDrop`) and redacted `Debug` implementations (`Mnemonic([REDACTED])`, `Seed([REDACTED])`).
  - Canonical test vectors (12-word Vector 1 and 24-word Vector 2) passing.
- **`crates/tola-crypto/src/lib.rs`**:
  - `Secp256k1Keypair` with RFC 6979 deterministic nonce generation, Low-S enforcement, recoverable signatures (`r`, `s`, `v`), and address derivation (`Keccak256(pubkey[1..65])[12..32]`).
  - Strict domain-separated BLAKE3 hashing (`DOMAIN_TRANSACTION_PAYLOAD`).
- **`crates/tola-sdk/src/wallet.rs`**:
  - `TolaWallet::generate()`, `TolaWallet::generate_with_mnemonic()`.
  - `TolaWallet::from_mnemonic()`, `TolaWallet::from_mnemonic_canonical()`, `TolaWallet::from_mnemonic_with_path()`, `TolaWallet::from_mnemonic_legacy_testnet()`.
  - `TolaWallet::from_private_key_bytes()`, `TolaWallet::from_hex_private_key()`.
  - In-place transaction signing (`sign_transaction`) matching protocol mempool and execution.
- **`crates/tola-cli/src/wallet.rs`**:
  - Subcommands: `wallet generate`, `wallet recover`, `wallet address`, `wallet public-key`, `wallet sign`.
  - Terminal security: `EchoGuard` (Windows & Unix) disabling terminal echo during seed phrase entry.

#### What Is Missing in Rust:
1. **Message Signing (`sign_message`)**:
   - TypeScript implements `\x19TOLA Signed Message:\n<length><message>` with Keccak-256 pre-image and recoverable Secp256k1 signature.
   - Rust `TolaWallet` currently lacks this message signing method and recovery helper.
2. **Ergonomic Factory Aliases**:
   - `TolaWallet::recover(phrase, passphrase)` alias.
   - `TolaWallet::from_private_key(...)` polymorphic or standardized helper.
3. **Debug Redaction for Wallet**:
   - While `Mnemonic` redacts `Debug`, `TolaWallet` and `Secp256k1Keypair` should explicitly implement `Debug` with `TolaWallet([REDACTED: 0x...])` to prevent accidental logging of private keys when wallets are embedded in application contexts.

---

### 2.2. Python (`sdk/python/tola`)

#### What Exists & Is Production-Ready:
- **`sdk/python/tola/crypto.py`**:
  - Pure Python Keccak-256 (NIST sponge with `0x01 .. 0x80` padding).
  - Pure Python BLAKE3 (chunk trees, root compression, sub-tree hashing).
  - Domain separation: `domain_separated_hash` with `DOMAIN_TRANSACTION_PAYLOAD`.
  - Pure Python Secp256k1 elliptic curve arithmetic:
    - Point addition, point doubling, scalar multiplication.
    - Compressed and uncompressed public key encoding.
    - Public key to 20-byte address derivation.
    - RFC 6979 deterministic ECDSA signing with Low-S enforcement.
    - Public key and address recovery (`recover_public_key`, `recover_address`).
- **`sdk/python/tola/address.py`**:
  - Full BIP-350 Bech32m encoder and decoder (`encode_bech32m`, `decode_bech32m`).
  - Network HRP resolution (`tola1...`, `ttola1...`, `dtola1...`).
- **`sdk/python/tola/transaction.py`**:
  - Unsigned and signed transaction wire serialization matching `tola-codec`.
  - Transaction signing digest computation matching protocol mempool.
  - Fluent `TransactionBuilder`.
- **`sdk/python/tola/client.py`**:
  - JSON-RPC client (`TolaClient`) with `eth_getBalance`, `eth_sendRawTransaction`, `tola_getHealth`, etc.

#### What Is Missing in Python:
1. **BIP-39 Mnemonic Module**:
   - Missing official English 2048-word BIP-39 wordlist.
   - Missing CSPRNG mnemonic generation (`12` or `24` words).
   - Missing checksum validation and NFKD Unicode normalization.
   - Missing PBKDF2-HMAC-SHA512 seed derivation (2048 iterations).
2. **BIP-32 Hierarchical Deterministic Derivation Module**:
   - Missing master node derivation from seed (`HMAC-SHA512(b"Bitcoin seed", seed)`).
   - Missing child key derivation for Secp256k1 along `m/44'/9999'/0'/0/0`.
   - Missing path parsing and support for canonical and legacy paths.
3. **Hardened `TolaWallet` API**:
   - Missing `TolaWallet.from_mnemonic(phrase, passphrase="", network=...)`.
   - Missing `TolaWallet.generate(word_count=12, passphrase="")` returning mnemonic.
   - Missing `TolaWallet.from_private_key(...)` supporting `int`, `bytes`, and `str`.
   - Missing `sign_message(message)` with `\x19TOLA Signed Message:\n` prefix.
   - Missing `mnemonic` and `derivation_path` properties.
   - Missing secret exposure protection: currently `private_key` is a public attribute; `__repr__` and `__str__` must not expose secrets.
4. **Automated Cross-SDK Parity & Security Tests**:
   - Missing Canonical Vector 1 (12 words) and Vector 2 (24 words) unit tests.
   - Missing BIP-39 recovery tests 1–9.
   - Missing signature tampering and cross-chain replay protection tests.
   - Missing live node JSON-RPC integration test.

---

### 2.3. TypeScript (`sdk/typescript/` — Reference Model)

- **Completed in Phase 18.x**:
  - `wordlist.js`: Pure JS 2048-word BIP-39 English wordlist.
  - `bip39.js`: Mnemonic generation, validation, and PBKDF2 seed derivation.
  - `bip32.js`: Secp256k1 child derivation with `CANONICAL_DERIVATION_PATH = "m/44'/9999'/0'/0/0"`.
  - `wallet.js`: Hardened `TolaWallet` with private fields (`#privateKey`, `#mnemonic`), zero secret leakage, `signTransaction`, `signMessage`, `toJSON`, and `util.inspect` redaction.
  - 32/32 tests passing in `sdk.test.mjs`.

---

## 3. Parity & Invariant Matrix

| Feature | Frozen Standard v1.0 | Rust SDK / CLI | TypeScript SDK | Python SDK | Audit Status |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **BIP-39 Mnemonic (12/24w)** | English 2048 words, CSPRNG | Yes (`mnemonic.rs`) | Yes (`bip39.js`) | **Missing** | To Implement in Python |
| **PBKDF2-HMAC-SHA512** | 2048 iters, NFKD, `mnemonic+salt` | Yes | Yes | **Missing** | To Implement in Python |
| **BIP-32 Path** | `m/44'/9999'/0'/0/0` (Unified) | Yes | Yes | **Missing** | To Implement in Python |
| **Model A Account Identity** | Identical 20B address across nets | Yes | Yes | Partial | To Standardize in Python |
| **Secp256k1 Curve Math** | RFC 6979, Low-S, Recoverable | Yes (`k256`) | Yes (`secp256k1.js`) | Yes (`crypto.py`) | Production-Ready |
| **Address Derivation** | `Keccak(pub[1..65])[12..32]` | Yes | Yes | Yes | Production-Ready |
| **Bech32m Encoding** | `tola1`, `ttola1`, `dtola1` | Yes | Yes | Yes | Production-Ready |
| **Tx Signing Digest** | BLAKE3 + Domain + ChainID | Yes | Yes | Yes | Production-Ready |
| **Message Signing** | `\x19TOLA Signed Message:\n` | **Missing** | Yes | **Missing** | To Implement in Rust & Python |
| **Secret Memory Protection** | Zeroize / Redacted Debug / Repr | Yes (Partial) | Yes (Private fields) | **Missing** | To Harden in Rust & Python |
| **Live Node RPC Integration** | `eth_sendRawTransaction` | Yes | Yes | **Missing** | To Implement in Python |

---

## 4. Implementation Plan

### Step 1: Rust Enhancements
1. Add `sign_message(&self, message: &[u8]) -> Result<[u8; 65], String>` and `recover_message_address(message: &[u8], sig: &[u8; 65]) -> Result<Address, String>` in `crates/tola-sdk/src/wallet.rs` mirroring TypeScript standard.
2. Add `TolaWallet::recover` and `TolaWallet::from_private_key` convenience constructors.
3. Implement `fmt::Debug` for `TolaWallet` and `Secp256k1Keypair` redacting secret material (`TolaWallet([REDACTED: 0x...])`).
4. Add unit and cross-verification tests in `crates/tola-sdk/src/wallet.rs`.

### Step 2: Python Wallet Stack Completion
1. **`sdk/python/tola/wordlist.py`**:
   - Official 2048-word BIP-39 English wordlist and fast index dictionary.
2. **`sdk/python/tola/bip39.py`**:
   - `generate_mnemonic(word_count=12)`: CSPRNG entropy + SHA-256 checksum.
   - `validate_mnemonic(mnemonic)`: NFKD normalization, wordlist validation, checksum verification.
   - `mnemonic_to_seed(mnemonic, passphrase="")`: PBKDF2-HMAC-SHA512 with 2048 iterations.
3. **`sdk/python/tola/bip32.py`**:
   - Master node derivation: `HMAC-SHA512(b"Bitcoin seed", seed)`.
   - Child node derivation (hardened & non-hardened) for Secp256k1 curve.
   - Path derivation along `m/44'/9999'/0'/0/0` and `m/44'/102'/0'/0/0`.
4. **`sdk/python/tola/wallet.py`**:
   - Refactor `TolaWallet` with private fields (`_private_key`, `_mnemonic`, `_derivation_path`).
   - `generate(word_count=12, passphrase="") -> TolaWallet`.
   - `from_mnemonic(mnemonic, passphrase="", network=...) -> TolaWallet`.
   - `from_private_key(key) -> TolaWallet` (polymorphic for `int`, `bytes`, `str`).
   - `sign_transaction(tx) -> Transaction`.
   - `sign_message(message) -> str` (`\x19TOLA Signed Message:\n` standard).
   - `get_address(network)` and `address` property.
   - Safe `__repr__` and `__str__` redacting secrets.
5. **Re-export** in `sdk/python/tola/__init__.py`.

### Step 3: Test Suite & Cross-Language Verification
1. Add comprehensive tests to `sdk/python/tests/test_tola.py`:
   - Canonical Vectors 1 and 2 bit-for-bit parity with Rust & TypeScript.
   - 9 BIP-39 recovery scenarios.
   - Signature tampering rejection.
   - Chain ID cross-network replay protection.
   - Secret leakage review.
   - Message signing and cross-verification against TypeScript/Rust test vectors.
   - End-to-end live node JSON-RPC integration test.

### Step 4: Full Documentation & Final Audit
1. Create `docs/wallet/rust-wallet.md`.
2. Create `docs/wallet/python-wallet.md`.
3. Create `docs/wallet/cross-language-wallet-compatibility.md`.
4. Create `docs/wallet/wallet-security.md`.
5. Run full workspace tests across Rust, Python, and TypeScript.
6. Generate `docs/wallet/rust-python-wallet-completion-report.md`.
