# TOLA Network: Key Derivation Specification

**Specification Version:** 1.0.0  
**Status:** RATIFIED & CANONICAL  
**Date:** 2026-10-07  
**Subsystem:** Cryptographic Key Derivation & Recovery (`tola-crypto`, `tola-sdk`)  

---

## 1. Overview

This document specifies the exact, deterministic mathematical algorithms used across the TOLA Network ecosystem for generating mnemonics, calculating master seeds, and deriving cryptographic keys for user accounts and validator identities.

---

## 2. BIP-39 Mnemonic Specification

### 2.1 Entropy & Word Count
| Word Count | Entropy Bits | Checksum Bits | Total Bits | Word Indices (11-bit) |
| :---: | :---: | :---: | :---: | :---: |
| **12 (Default)** | 128 | 4 | 132 | 12 |
| **24 (Optional)** | 256 | 8 | 264 | 24 |

1. **CSPRNG Generation:** Entropy $E$ is generated using an OS-level Cryptographically Secure Pseudorandom Number Generator (CSPRNG, e.g., `rand_core::OsRng` in Rust, `crypto.getRandomValues` in browsers/Node.js, `secrets.token_bytes` in Python).
2. **Checksum Calculation:**
   $$\text{Checksum} = \text{SHA-256}(E)[0 \dots \text{CS\_bits}]$$
3. **Word Mapping:** The combined bitstring $E \parallel \text{Checksum}$ is split into 11-bit integer chunks ($0 \dots 2047$), each mapping to an entry in the canonical English BIP-39 wordlist.
4. **Validation:** On import, the user phrase is normalized (trimmed, single space separator, lowercase, NFKD normalization), the word count verified, and the SHA-256 checksum recalculated. Invalid phrases or failed checksums are strictly rejected.

---

## 3. Seed Derivation (PBKDF2-HMAC-SHA512)

A 512-bit (64-byte) cryptographic master seed is generated from the normalized mnemonic phrase using PBKDF2:

$$\text{Seed} = \text{PBKDF2}(\text{HMAC-SHA512}, \text{Password} = \text{NFKD}(\text{Mnemonic}), \text{Salt} = \text{NFKD}(\text{"mnemonic"} \parallel \text{Passphrase}), \text{Iterations} = 2048, \text{Length} = 64)$$

- **Default Passphrase:** Empty string `""` (resulting in salt `b"mnemonic"`).
- **Optional Passphrase:** User-supplied extra passphrase for plausible deniability (BIP-39 13th / 25th word).

---

## 4. BIP-44 Hierarchical Deterministic Key Derivation (Secp256k1)

User accounts use BIP-32 / BIP-44 key derivation on the Secp256k1 elliptic curve, ensuring 100% compatibility with all existing TOLA transactions, 20-byte accounts, EVM smart contracts, and recoverable signatures.

### 4.1 Master Key Derivation
From the 64-byte Seed:
$$I = \text{HMAC-SHA512}(\text{Key} = \text{b"Bitcoin seed"}, \text{Data} = \text{Seed})$$
- Master Private Key scalar: $k_L = I[0 \dots 32]$
- Master Chain Code: $c_R = I[32 \dots 64]$

### 4.2 Canonical Derivation Path
$$\mathbf{m / 44' / \text{coin\_type}' / \text{account}' / \text{change} / \text{address\_index}}$$

| Element | Value (Mainnet) | Value (Testnet) | Description |
| :--- | :--- | :--- | :--- |
| **Purpose** | `44'` (`0x8000002C`) | `44'` (`0x8000002C`) | BIP-44 Multi-Account Standard |
| **Coin Type** | `9999'` (`0x8000270F`) [PROPOSED] | `102'` (`0x80000066`) or `1'` | Proposed TOLA Coin Type (per ADR-0013) |
| **Account** | `0'` (`0x80000000`) | `0'` (`0x80000000`) | Primary Account Index (Hardened) |
| **Change** | `0` (`0x00000000`) | `0` (`0x00000000`) | External Receiving Address (Non-hardened) |
| **Index** | `0` (`0x00000000`) | `0` (`0x00000000`) | Primary Address Index (Non-hardened) |

> [!NOTE]
> Coin Type `9999'` is formally classified as **PROPOSED (INTERNAL SPECIFICATION)** per [`decisions/ADR-0013-tola-wallet-derivation.md`](../../decisions/ADR-0013-tola-wallet-derivation.md) pending upstream inclusion in SatoshiLabs SLIP-0044.

**Default Proposed Canonical Mainnet Path:**
$$\mathbf{m/44'/9999'/0'/0/0}$$

**Default Proposed Canonical Testnet Path:**
$$\mathbf{m/44'/102'/0'/0/0}$$

### 4.3 Child Key Derivation Function (CKD)
For hardened child indices ($i \ge 2^{31}$):
$$I = \text{HMAC-SHA512}(c_{parent}, \text{b}\text{"\x00"} \parallel k_{parent} \parallel i_{32\text{B BE}})$$
$$k_{child} = (I[0 \dots 32] + k_{parent}) \pmod n$$
$$c_{child} = I[32 \dots 64]$$

For non-hardened child indices ($i < 2^{31}$):
$$I = \text{HMAC-SHA512}(c_{parent}, \text{CompressPubKey}(K_{parent}) \parallel i_{32\text{B BE}})$$
$$k_{child} = (I[0 \dots 32] + k_{parent}) \pmod n$$
$$c_{child} = I[32 \dots 64]$$

---

## 5. Address Derivation from Derived Key

From the derived Secp256k1 private key $k_{child}$:
1. Compute public point $K = k_{child} \cdot G$.
2. Form uncompressed public key point:
   $$P_{\text{uncompressed}} = \text{0x04} \parallel X \parallel Y \quad (65\text{ bytes})$$
3. Compute 32-byte Keccak-256 hash:
   $$H = \text{Keccak-256}(P_{\text{uncompressed}}[1 \dots 65])$$
4. Extract 20-byte address:
   $$\text{Address} = H[12 \dots 32]$$
5. Encode in Bech32m (BIP-350):
   - Mainnet: `tola1...`
   - Testnet: `ttola1...`
   - Devnet: `dtola1...`

---

## 6. Validator Key Derivation (SLIP-0010 Ed25519)

For node operators backing up validator keys, the same master seed derives an Ed25519 signing key via SLIP-0010:

1. **Master Ed25519 Key:**
   $$I = \text{HMAC-SHA512}(\text{Key} = \text{b"ed25519 seed"}, \text{Data} = \text{Seed})$$
   - Master Private Key: $k_L = I[0 \dots 32]$
   - Master Chain Code: $c_R = I[32 \dots 64]$
2. **Derivation Path:**
   $$\mathbf{m/44'/9999'/0'/0'}$$
   *(Note: In SLIP-0010 for Ed25519, all derivation steps must be hardened)*.
3. Output 32-byte seed directly constructs `Ed25519Keypair::from_seed(&k_child)`.

---

## 7. Canonical Deterministic Test Vectors

### Canonical Vector 1 (12 Words)
- **Mnemonic:**  
  `abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about`
- **Passphrase:**  
  `""` (empty)
- **Derived Master Seed (Hex):**  
  `5eb00bbddcf069084889087b35de2c689d9a3281537ccdbfb427d215dfd30eacd4e5452b7b97340e0194443d3748f65fa8976dc3fb012cd1aa39a66b0961acf1`
- **Derivation Path:**  
  `m/44'/9999'/0'/0/0`
- **Deterministic Derivation Output:**
  - Key derivation must evaluate to identical private key, public key, and address across Rust, TypeScript, and Python SDKs.

---

## 8. Security Guarantees
1. **Zero Cold-Boot & Swap Leakage:** Seed bytes and derived private keys are kept in zeroizing buffers (`ZeroizeOnDrop`).
2. **Zero In-Transit Exposure:** Mnemonic phrases never enter log streams, RPC payloads, debug statements, or browser telemetry.
3. **Decoupled Boundary:** Full compliance with ADR-0005. Wallets sign locally; messengers and external apps receive only signed transactions.
