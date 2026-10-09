# TOLA Network — Official Wallet Recovery Test Vectors

This document establishes the official test vectors for TOLA Network native seed phrase generation, key derivation, and address encoding. Any client SDK (Rust, TypeScript, Python, Swift, Go), CLI wallet, hardware wallet, or ecosystem signer MUST match these outputs exactly.

---

## Specification Summary

- **Mnemonic Standard:** BIP-39 (English wordlist, NFKD normalized, lowercased).
- **Mnemonic Lengths:** 12 words (128-bit entropy, default) and 24 words (256-bit entropy, optional high security).
- **Seed Derivation:** PBKDF2-HMAC-SHA512 (2048 iterations) with salt `"mnemonic" + passphrase`.
- **Master HD Derivation:** BIP-32 Hierarchical Deterministic Wallets using HMAC-SHA512 (`"Bitcoin seed"`).
- **Coin Types (Proposed - ADR-0013):**
  - Mainnet (Chain ID 9999): `m/44'/9999'/0'/0/0`
  - Testnet (Chain ID 102): `m/44'/102'/0'/0/0`
  - Devnet (Chain ID 1337): `m/44'/1337'/0'/0/0`
  - Fallback / Ethereum compatibility: `m/44'/60'/0'/0/0`
- **Curve:** Secp256k1.
- **Address Generation Algorithm:** `Address = Keccak256(UncompressedPubKey[1..65])[12..32]`.
- **Address Encoding:**
  - Hex: `0x` + 40 lowercase hexadecimal characters.
  - Bech32m Mainnet: HRP `tola`, 20-byte payload with Bech32m checksum (`tola1...`).
  - Bech32m Testnet: HRP `ttola`, 20-byte payload with Bech32m checksum (`ttola1...`).

---

## Canonical Test Vector 1 (12 Words — Default)

### Input
- **Word Count:** 12 words
- **Mnemonic Phrase:**
  ```text
  abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about
  ```
- **Passphrase:** `""` (empty string)

### BIP-39 512-bit Seed (PBKDF2-HMAC-SHA512)
```text
5eb00bbddcf069084889a8ab9155568165f5c453ccb85e70811aaed6f6da5fc19a5ac40b389cd370d086206dec8aa6c43daea6690f20ad3d8d48b2d2ce9e38e4
```

### Derivation Path A: Mainnet (`m/44'/9999'/0'/0/0`)
- **Path:** `m/44'/9999'/0'/0/0`
- **Compressed Public Key (33 bytes):**
  ```text
  03e95745eb5f028b4e23fb4c5af97fb49426d522c4228fe6dd922f2585e6b1bf96
  ```
- **Address (20 bytes hex):**
  ```text
  0x3b45664e195dbacd0035046b511491e09bd3ca8a
  ```
- **Bech32m Address (Mainnet):**
  ```text
  tola18dzkvnsetkav6qp4q344z9y3uzda8j5263ez6j
  ```

### Derivation Path B: Testnet (`m/44'/102'/0'/0/0`)
- **Path:** `m/44'/102'/0'/0/0`
- **Compressed Public Key (33 bytes):**
  ```text
  02b74bcb92d52c5cfe20ece37e6cc6ae0b12dd58fc328c509e8f904dfc62977403
  ```
- **Address (20 bytes hex):**
  ```text
  0x8e3719da26b9408f27fa9602c4362d9fbc49dc73
  ```
- **Bech32m Address (Testnet):**
  ```text
  ttola13cm3nk3xh9qg7fl6jcpvgd3dn77ynhrnukw78v
  ```

### BIP-39 Standard Passphrase Vector ("TREZOR")
- **Passphrase:** `"TREZOR"`
- **BIP-39 Seed:**
  ```text
  c55257c360c07c72029aebc1b53c05ed0362ada38ead3e3e9efa3708e53495531f09a6987599d18264c1e1c92f2cf141630c7a3c4ab7c81b2f001698e7463b04
  ```

---

## Canonical Test Vector 2 (24 Words — High Security)

### Input
- **Word Count:** 24 words
- **Mnemonic Phrase:**
  ```text
  abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon art
  ```
- **Passphrase:** `""` (empty string)

### BIP-39 512-bit Seed (PBKDF2-HMAC-SHA512)
```text
408b285c123836004f4b8842c89324c1f01382450c0d439af345ba7fc49acf705489c6fc77dbd4e3dc1dd8cc6bc9f043db8ada1e243c4a0eafb290d399480840
```

### Derivation Path A: Mainnet (`m/44'/9999'/0'/0/0`)
- **Path:** `m/44'/9999'/0'/0/0`
- **Compressed Public Key (33 bytes):**
  ```text
  03d20a0a3b709e063b279a5810d91b922c50b5b62ba09ef16b6c3916f1e5d66e3f
  ```
- **Address (20 bytes hex):**
  ```text
  0x37061e043e1b89972ed615c46ff3e5090514c909
  ```
- **Bech32m Address (Mainnet):**
  ```text
  tola1xurpupp7rwyewtkkzhzxlul9pyz3fjgf4wludn
  ```

### Derivation Path B: Testnet (`m/44'/102'/0'/0/0`)
- **Path:** `m/44'/102'/0'/0/0`
- **Compressed Public Key (33 bytes):**
  ```text
  02909cbd77e782de759b59b670c5b8eaab0ba25aa8bf063f75e2ee806d27a0eb96
  ```
- **Address (20 bytes hex):**
  ```text
  0x1073f501701b4a14dd4bc6a1f1746fca1fb8de64
  ```
- **Bech32m Address (Testnet):**
  ```text
  ttola1zpel2qtsrd9pfh2tc6slzar0eg0m3hnyflc270
  ```

---

## Canonical Test Vector 3: Raw Private Key Backward Compatibility

### Input
- **Raw Private Key (32 bytes):**
  ```text
  0x4242424242424242424242424242424242424242424242424242424242424242
  ```

### Public Key & Address Outputs
- **Compressed Public Key (33 bytes):**
  ```text
  0324653eac434488002cc06bbfb7f10fe18991e35f9fe4302dbea6d2353dc0ab1c
  ```
- **Address (20 bytes hex):**
  ```text
  0x17c5185167401ed00cf5f5b2fc97d9bbfdb7d025
  ```
- **Bech32m Mainnet:**
  ```text
  tola1zlz3s5t8gq0dqr847ke0e97eh07m05p9t9nfc9
  ```
- **Bech32m Testnet:**
  ```text
  ttola1zlz3s5t8gq0dqr847ke0e97eh07m05p90jzepq
  ```

---

## Verification in Rust Suite

These vectors are asserted continuously in:
- `crates/tola-crypto/src/mnemonic.rs` (`test_bip39_official_test_vector_1_seed`, `test_canonical_test_vector_outputs`)
- `crates/tola-sdk/src/wallet.rs` (`test_canonical_vector_values`, `test_wallet_mnemonic_sign_and_verify_transaction`)
