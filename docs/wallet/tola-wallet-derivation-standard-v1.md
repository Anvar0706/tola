# TOLA NETWORK — DETERMINISTIC WALLET DERIVATION STANDARD v1.0

**Standard Identifier:** TOLA-WDS-1.0  
**Version:** 1.0  
**Status:** FROZEN  
**Date:** 2026-10-07  
**Category:** Standards Track / Core Wallet Specification  
**Canonical Derivation Path:** `m/44'/9999'/0'/0/0`  

---

## 1. ABSTRACT & OBJECTIVE

This specification defines the canonical, deterministic wallet key derivation standard for the TOLA Network. It establishes a cryptographically secure, unified pipeline:
$$\text{BIP-39 Mnemonic} \longrightarrow \text{BIP-39 Seed} \longrightarrow \text{BIP-44 HD Path} \longrightarrow \text{Secp256k1 Keypair} \longrightarrow \text{Keccak-256} \longrightarrow \text{TOLA Address} \longrightarrow \text{Bech32m}$$

The primary objective is to guarantee that:
1. A given seed phrase deterministically produces the exact same cryptographic identity across all environments (Mainnet, Testnet, Devnet).
2. Network separation and replay protection are enforced cryptographically through Chain ID domain separation during transaction signing, never by mutating the key derivation path.
3. Full backward compatibility is maintained with existing native accounts and transaction execution engines.

---

## 2. MNEMONIC SPECIFICATION (BIP-39)

### 2.1 Entropy and Word Counts
Mnemonics MUST be generated from cryptographically secure pseudo-random numbers (CSPRNG, such as `rand_core::OsRng`):
* **12 words (Default):** 128 bits of entropy + 4-bit SHA-256 checksum = 132 bits total.
* **24 words (Extended):** 256 bits of entropy + 8-bit SHA-256 checksum = 264 bits total.
* **Prohibited Counts:** Mnemonics with 15, 18, or 21 words are strictly rejected.

### 2.2 Wordlist and Language
* **Wordlist:** The standard English wordlist (2,048 words) specified in BIP-39.
* **Normalization:** All input phrases MUST undergo Unicode NFKD normalization, conversion to lowercase, trimming of leading and trailing whitespace, and collapse of repeated internal whitespace into single spaces before checksum verification or seed derivation.

### 2.3 Memory Hardening
All memory buffers containing raw entropy, mnemonic phrases, or derived seeds MUST implement automatic memory wiping (`ZeroizeOnDrop` or `memset_s`) upon deallocation. Debug formatters MUST redact phrase contents (`[REDACTED]`).

---

## 3. MASTER SEED DERIVATION (PBKDF2-HMAC-SHA512)

The 512-bit (64-byte) binary master seed is computed from the normalized mnemonic phrase using PBKDF2:
* **Pseudorandom Function (PRF):** HMAC-SHA512.
* **Password:** The NFKD-normalized mnemonic phrase.
* **Salt:** The UTF-8 string `b"mnemonic"` concatenated with the NFKD-normalized user passphrase (if no passphrase is provided, the salt is simply `b"mnemonic"`).
* **Iteration Count:** Exactly 2,048 iterations.
* **Key Length:** 64 bytes (512 bits).

---

## 4. HIERARCHICAL DETERMINISTIC KEY DERIVATION (BIP-32 / BIP-44)

Key derivation strictly follows BIP-32 and BIP-44 over the Secp256k1 elliptic curve:

### 4.1 Master Node
* **HMAC Key:** `b"Bitcoin seed"`.
* **Input Data:** The 64-byte master seed.
* **Output:** Master Secret Key (32 bytes) and Master Chain Code (32 bytes).

### 4.2 Derivation Path Structure
The BIP-44 hierarchy is structured as:
$$\text{m} / \text{purpose}' / \text{coin\_type}' / \text{account}' / \text{change} / \text{address\_index}$$

Where:
* **`purpose'`:** `44'` (Hardened: $44 + 2^{31} = 0x8000002C$). Enforces standard BIP-44 structure.
* **`coin_type'`:** `9999'` (Hardened: $9999 + 2^{31} = 0x8000270F$). Canonical registered coin type for TOLA Network.
* **`account'`:** `0'` (Hardened: $0 + 2^{31} = 0x80000000$). Default account index. Additional accounts increment: $1', 2', \dots$
* **`change`:** `0` (Unhardened: $0$). External receiving chain. (`1` reserved for internal change).
* **`address_index`:** `0` (Unhardened: $0$). Default primary address. Subsequent addresses increment: $1, 2, \dots$

### 4.3 Exact Canonical Derivation Path
$$\mathbf{m/44'/9999'/0'/0/0}$$

* **Single Standard Policy:** `m/44'/9999'/0'/0/0` is the **ONLY** canonical derivation path for TOLA Network.
* **Legacy Status of Older Paths:** Any pre-v1.0 paths (such as the provisional testnet path `m/44'/102'/0'/0/0`) are explicitly classified as **DEPRECATED LEGACY**. Implementations must not silently default to them.

---

## 5. KEY TYPE & SECP256K1 SPECIFICATION

* **Elliptic Curve:** Secp256k1 ($y^2 = x^3 + 7 \pmod p$).
* **Private Key:** 32-byte scalar $k \in [1, N-1]$, where $N$ is the curve order.
* **Public Key:** Point $P = k \times G$.
  * **Compressed (33 bytes):** `[0x02 || X]` if $Y$ is even, or `[0x03 || X]` if $Y$ is odd.
  * **Uncompressed (65 bytes):** `[0x04 || X (32 bytes) || Y (32 bytes)]`.

---

## 6. ADDRESS DERIVATION & ENCODING

### 6.1 Address Computation
The raw 20-byte TOLA address is derived identically to Ethereum/EVM:
$$\text{Raw Address (20 bytes)} = \text{Keccak-256}(\text{UncompressedPubKey}[1..65])[12..32]$$

### 6.2 Address Encodings
The 20 raw bytes are represented in two formats:
1. **Raw Hex:** `0x` followed by 40 hexadecimal characters.
2. **Bech32m (BIP-350):** The 20 raw bytes converted to 5-bit groups and checksummed using Bech32m polynomial checksum with network Human-Readable Part (HRP):
   * **Mainnet:** HRP `tola` $\longrightarrow$ `tola1...`
   * **Testnet:** HRP `ttola` $\longrightarrow$ `ttola1...`
   * **Devnet:** HRP `dtola` $\longrightarrow$ `dtola1...`

---

## 7. NETWORK BEHAVIOR: UNIFIED IDENTITY (MODEL A)

TOLA Network explicitly adopts **Model A (Unified Account Identity Across Networks)**:

| Network | Chain ID | Derivation Path | Raw Address (20B) | Bech32m Address Format |
| :--- | :--- | :--- | :--- | :--- |
| **Mainnet** | `9999` | `m/44'/9999'/0'/0/0` | Identical | `tola1...` |
| **Testnet** | `102` | `m/44'/9999'/0'/0/0` | Identical | `ttola1...` |
| **Devnet** | `1337` | `m/44'/9999'/0'/0/0` | Identical | `dtola1...` |

### Architectural Rationale for Model A:
1. **User Identity Consistency:** A user restoring a wallet on Testnet or Devnet interacts with the same cryptographic keypair and raw 20-byte address. Faucets, test scripts, and contract deployment ownership tests remain consistent.
2. **Elimination of User Error:** Switching RPC endpoints does not unexpectedly alter the user's private key or account address.
3. **Standards Compliance:** In BIP-44, `coin_type` defines the cryptocurrency asset/ecosystem (`9999'`), not transient network stages.

---

## 8. REPLAY PROTECTION & SIGNING DOMAIN

Replay protection is mathematically guaranteed at the transaction layer through Chain ID domain separation, **not** through derivation path divergence:

$$\text{Signing Digest} = \text{BLAKE3}(\text{DOMAIN\_TRANSACTION\_PAYLOAD} \parallel \text{chain\_id} \parallel \text{epoch} \parallel \text{payload\_hash})$$

* **Domain Constant:** `DOMAIN_TRANSACTION_PAYLOAD = b"TOLA_TX_PAYLOAD_V1"`
* **Chain ID Binding:** `ChainId(9999)` for Mainnet, `ChainId(102)` for Testnet, `ChainId(1337)` for Devnet.
* **Security Invariant:** Because `chain_id` is cryptographically committed inside the BLAKE3 digest before signing, a signature generated on Testnet can **never** be admitted to the mempool or executed on Mainnet, even when using the identical private key and address.

### Signature Format
* 65-byte recoverable ECDSA signature `[r (32 bytes) || s (32 bytes) || v (1 byte)]`.
* Strict BIP-146 Low-S enforcement: $s \le N / 2$ to prevent transaction malleability.
* Deterministic nonces via RFC 6979.

---

## 9. CANONICAL TEST VECTORS

### 9.1 Test Vector 1 (12 Words, Empty Passphrase)
* **Mnemonic:**  
  `abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about`
* **Passphrase:** `""` (empty)
* **Master Seed (Hex):**  
  `5eb00bbddcf069084889a8ab9155568165f5c453ccb85e70811aaed6f6da5fc19a5ac40b389cd370d086206dec8aa6c43daea6690f20ad3d8d48b2d2ce9e38e4`
* **Canonical Derivation Path:**  
  `m/44'/9999'/0'/0/0`
* **Private Key (Hex):**  
  `0xf77999a4d727d792213ae012f18621208ea91a7246e5bf69a4317942abeca750`
* **Private Key Fingerprint (SHA-256[..4]):**  
  `0x0cc22062`
* **Compressed Public Key (Hex):**  
  `0x03e95745eb5f028b4e23fb4c5af97fb49426d522c4228fe6dd922f2585e6b1bf96`
* **Uncompressed Public Key (Hex):**  
  `0x04e95745eb5f028b4e23fb4c5af97fb49426d522c4228fe6dd922f2585e6b1bf9689629921a91231749eaf52b0198c172ea8a2cce8ee2c9f62f8a6c2fed81c217f`
* **Raw Address (Hex):**  
  `0x3b45664e195dbacd0035046b511491e09bd3ca8a`
* **Mainnet Address (Bech32m):**  
  `tola18dzkvnsetkav6qp4q344z9y3uzda8j5263ez6j`
* **Testnet Address (Bech32m):**  
  `ttola18dzkvnsetkav6qp4q344z9y3uzda8j527xgjrh`
* **Devnet Address (Bech32m):**  
  `dtola18dzkvnsetkav6qp4q344z9y3uzda8j52x0hhl4`

### 9.2 Test Vector 2 (24 Words, Empty Passphrase)
* **Mnemonic:**  
  `abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon art`
* **Passphrase:** `""` (empty)
* **Master Seed (Hex):**  
  `408b285c123836004f4b8842c89324c1f01382450c0d439af345ba7fc49acf705489c6fc77dbd4e3dc1dd8cc6bc9f043db8ada1e243c4a0eafb290d399480840`
* **Canonical Derivation Path:**  
  `m/44'/9999'/0'/0/0`
* **Compressed Public Key (Hex):**  
  `0x03d20a0a3b709e063b279a5810d91b922c50b5b62ba09ef16b6c3916f1e5d66e3f`
* **Raw Address (Hex):**  
  `0x37061e043e1b89972ed615c46ff3e5090514c909`
* **Mainnet Address (Bech32m):**  
  `tola1xurpupp7rwyewtkkzhzxlul9pyz3fjgf4wludn`

---

## 10. COMPATIBILITY & IMPLEMENTATION REQUIREMENTS

1. **Rust Implementation:**
   * All default wallet creation and restoration in `crates/tola-crypto`, `crates/tola-sdk`, and `crates/tola-cli` MUST use `CANONICAL_DERIVATION_PATH` (`m/44'/9999'/0'/0/0`).
   * No silent fallback paths are permitted.
2. **SDK Parity:**
   * TypeScript `@tola/sdk` and Python `tola` wallet implementations must match these exact test vectors bit-for-bit upon implementation.
3. **Legacy Account Compatibility:**
   * Accounts created via raw 32-byte Secp256k1 keys (`tola-cli account new`) remain fully valid and compatible with the execution engine.
