# TOLA Network — Native Seed Phrase Wallet Implementation

## Phase 1: Cryptographic Core & Client SDK

This document describes the design, architecture, and security properties of the native BIP-39 / BIP-32 / BIP-44 seed phrase wallet implementation for the TOLA Network.

---

## 1. Architectural Derivation Pipeline

The canonical user wallet lifecycle strictly follows standard deterministic derivation while preserving 100% compatibility with TOLA's high-performance transaction execution pipeline:

```
+-------------------------------------------------------------+
|                CSPRNG Entropy (128 / 256 bits)              |
+-------------------------------------------------------------+
                              |
                              v
+-------------------------------------------------------------+
|              BIP-39 Mnemonic (12 or 24 words)               |
|            (NFKD normalized, lowercase english)             |
+-------------------------------------------------------------+
                              |
                              | PBKDF2-HMAC-SHA512 (2048 iter)
                              | Salt: "mnemonic" + passphrase
                              v
+-------------------------------------------------------------+
|                     512-bit Master Seed                     |
|                   (ZeroizeOnDrop protected)                 |
+-------------------------------------------------------------+
                              |
                              | BIP-32 HMAC-SHA512 ("Bitcoin seed")
                              | BIP-44 Derivation Path
                              v
+-------------------------------------------------------------+
|               Secp256k1 Private Key (32 bytes)              |
|                   (ZeroizeOnDrop protected)                 |
+-------------------------------------------------------------+
                              |
                              | Elliptic curve scalar multiplication
                              v
+-------------------------------------------------------------+
|          Secp256k1 Public Key (33B comp / 65B uncomp)       |
+-------------------------------------------------------------+
                              |
                              | Keccak-256(Uncompressed[1..65])
                              | Take last 20 bytes [12..32]
                              v
+-------------------------------------------------------------+
|                   TOLA Address (20 bytes)                   |
|              Hex: 0x... / Bech32m: tola1...                 |
+-------------------------------------------------------------+
```

---

## 2. Derivation Paths & Network Separation (ADR-0013)

Per [`ADR-0013`](../../decisions/ADR-0013-tola-wallet-derivation.md), TOLA introduces proposed internal coin types aligned with Chain IDs while offering fallback compatibility:

| Network | Chain ID | Derivation Path | Status |
| :--- | :--- | :--- | :--- |
| **Mainnet** | `9999` | `m/44'/9999'/0'/0/0` | Proposed (Canonical Internal) |
| **Testnet** | `102` | `m/44'/102'/0'/0/0` | Proposed (Canonical Internal) |
| **Devnet** | `1337` | `m/44'/1337'/0'/0/0` | Proposed |
| **Fallback** | Any | `m/44'/60'/0'/0/0` | Supported (Ethereum standard) |

Path format follows the standard BIP-44 specification:
$$\text{m} / 44' / \text{coin\_type}' / \text{account}' / \text{change} / \text{address\_index}$$

Helper function in `tola-crypto`:
```rust
pub fn format_bip44_path(coin_type: u32, account: u32, change: u32, address_index: u32) -> String
```

---

## 3. Cryptographic Primitives & Dependencies

The core implementation utilizes hardened, audited, standard Rust cryptography:
- `bip39` (v2.0): Standard BIP-39 mnemonic phrase generation, dictionary verification, and PBKDF2 seed expansion.
- `bip32` (v0.5): Extended private keys (`XPrv`), hardened and non-hardened child derivation paths, with `secp256k1` and `alloc` features.
- `k256`: Curve arithmetic, ECDSA signing and verification, and public key serialization.
- `tiny-keccak`: Keccak-256 hashing for 20-byte address derivation.
- `zeroize`: Automatic cryptographic zeroization of sensitive memory upon drop.

---

## 4. Security Guarantees & Memory Hardening

1. **Zeroization on Drop:**
   - Both `Mnemonic` and `Seed` structures implement `ZeroizeOnDrop`. When instances go out of scope, the underlying byte buffers in RAM are overwritten with zeroes using compiler-fence guarantees.
   - Private key buffers in `TolaWallet` are zeroized upon drop.

2. **Redacted Debug Formats:**
   - `Mnemonic` implements `Debug` producing `Mnemonic([REDACTED])`.
   - `Seed` implements `Debug` producing `Seed([REDACTED])`.
   - Neither `Mnemonic` nor `Seed` implement `Display`, preventing accidental formatting in loggers, panic strings, or telemetry handlers.
   - Access to plaintext phrases requires explicit `.phrase()` invocation.

3. **Input Sanitization & Normalization:**
   - Leading, trailing, and duplicate intermediate whitespace are collapsed.
   - Input strings are converted to lowercase before parsing against the BIP-39 English dictionary.
   - Validates word count ($12$ or $24$) and checksum integrity.

---

## 5. Public APIs

### `tola-crypto`
```rust
pub enum MnemonicCount { Words12, Words24 }

pub struct Mnemonic { /* zeroized internal buffer */ }
impl Mnemonic {
    pub fn generate(count: MnemonicCount) -> Result<Self, CryptoError>;
    pub fn from_phrase(phrase: &str) -> Result<Self, CryptoError>;
    pub fn phrase(&self) -> &str;
    pub fn word_count(&self) -> usize;
    pub fn to_seed(&self, passphrase: &str) -> Seed;
}

pub struct Seed { /* zeroized 64-byte buffer */ }
impl Seed {
    pub fn from_bytes(bytes: [u8; 64]) -> Self;
    pub fn as_bytes(&self) -> &[u8; 64];
    pub fn derive_secp256k1_keypair(&self, path: &str) -> Result<Secp256k1KeyPair, CryptoError>;
}
```

### `tola-sdk`
```rust
pub struct TolaWallet { /* private key, public key, address */ }
impl TolaWallet {
    // Mnemonic creation & restoration
    pub fn generate_with_mnemonic(count: MnemonicCount) -> Result<(Self, Mnemonic), String>;
    pub fn from_mnemonic(phrase: &str, passphrase: &str, network: &Network) -> Result<Self, String>;
    pub fn from_mnemonic_with_path(phrase: &str, passphrase: &str, path: &str) -> Result<Self, String>;
    pub fn from_seed(seed: &Seed, path: &str) -> Result<Self, String>;

    // Existing backward-compatible constructors
    pub fn generate() -> Self;
    pub fn from_private_key_bytes(bytes: &[u8; 32]) -> Result<Self, String>;
    pub fn from_hex_private_key(hex_str: &str) -> Result<Self, String>;

    // Accessors and signing
    pub fn address(&self) -> Address;
    pub fn address_bech32m(&self, network: &Network) -> String;
    pub fn public_key_bytes(&self) -> [u8; 33];
    pub fn public_key(&self) -> PublicKey;
    pub fn sign_transaction(&self, tx: &mut Transaction) -> Result<(), String>;
}
```

---

## 6. Verification & Test Suite

The implementation is verified by unit and integration tests across the workspace:
1. **BIP-39 Test Vectors:** Verified with official Trezor vectors (Vector 1 with empty passphrase and `"TREZOR"` passphrase).
2. **Deterministic Roundtrips:** Generated 12-word and 24-word phrases cleanly serialize, parse, and restore identical addresses and public keys.
3. **Transaction Signing & Recovery:** Transactions signed by wallets generated via mnemonic recover the exact originating address (`tx.from`) in `tola-execution` and `tola-mempool`.
4. **Backward Compatibility:** All existing tests for raw private key importing, transaction construction, RPC, and consensus pass without modification.
