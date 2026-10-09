# TOLA Network Cross-Language Wallet Compatibility & Parity Matrix

## 1. Executive Summary

This document certifies that the **Rust (`tola-crypto` / `tola-sdk`)**, **TypeScript (`@tola/sdk`)**, and **Python (`tola`)** wallet implementations achieve **100% bit-for-bit cryptographic parity** across mnemonic derivation, key generation, address serialization, transaction signing, and message clear-signing.

---

## 2. Canonical Derivation Vector 1 (12 Words)

| Property | Rust (`tola-sdk`) | TypeScript (`@tola/sdk`) | Python (`tola`) | Parity Status |
| :--- | :--- | :--- | :--- | :---: |
| **Mnemonic** | `abandon abandon ... about` | `abandon abandon ... about` | `abandon abandon ... about` | **IDENTICAL** |
| **Derivation Path** | `m/44'/9999'/0'/0/0` | `m/44'/9999'/0'/0/0` | `m/44'/9999'/0'/0/0` | **IDENTICAL** |
| **Private Key Hex** | `0xf77999a4...a750` | `0xf77999a4...a750` | `0xf77999a4...a750` | **IDENTICAL** |
| **Compressed Pubkey** | `03e95745...b1bf96` | `03e95745...b1bf96` | `03e95745...b1bf96` | **IDENTICAL** |
| **Uncompressed Pubkey** | `04e95745...17f` | `04e95745...17f` | `04e95745...17f` | **IDENTICAL** |
| **Raw Address** | `0x3b45664e195dbacd0035046b511491e09bd3ca8a` | `0x3b45664e195dbacd0035046b511491e09bd3ca8a` | `0x3b45664e195dbacd0035046b511491e09bd3ca8a` | **IDENTICAL** |
| **Mainnet Address** | `tola18dzkvnsetkav6qp4q344z9y3uzda8j5263ez6j` | `tola18dzkvnsetkav6qp4q344z9y3uzda8j5263ez6j` | `tola18dzkvnsetkav6qp4q344z9y3uzda8j5263ez6j` | **IDENTICAL** |
| **Testnet Address** | `ttola18dzkvnsetkav6qp4q344z9y3uzda8j527xgjrh` | `ttola18dzkvnsetkav6qp4q344z9y3uzda8j527xgjrh` | `ttola18dzkvnsetkav6qp4q344z9y3uzda8j527xgjrh` | **IDENTICAL** |
| **Devnet Address** | `dtola18dzkvnsetkav6qp4q344z9y3uzda8j52x0hhl4` | `dtola18dzkvnsetkav6qp4q344z9y3uzda8j52x0hhl4` | `dtola18dzkvnsetkav6qp4q344z9y3uzda8j52x0hhl4` | **IDENTICAL** |
| **Message Signature** *(Genesis 2026)* | `0xb2c3fd06...01` | `0xb2c3fd06...01` | `0xb2c3fd06...01` | **IDENTICAL** |

Exact 65-byte RFC 6979 signature for message `"Hello TOLA Network Genesis 2026"`:
```text
0xb2c3fd06ac1b3044e522b6e0c8e9aa43f931825bac33cb0230399d1b4098d9bf0810ab4014e2f1b7e73e36afedbda48b26a1be47abab0dd63ae198ef7915c37d01
```

---

## 3. Canonical Derivation Vector 2 (24 Words)

| Property | Rust (`tola-sdk`) | TypeScript (`@tola/sdk`) | Python (`tola`) | Parity Status |
| :--- | :--- | :--- | :--- | :---: |
| **Mnemonic** | `abandon abandon ... art` | `abandon abandon ... art` | `abandon abandon ... art` | **IDENTICAL** |
| **512-bit Seed Hex** | `408b285c...0840` | `408b285c...0840` | `408b285c...0840` | **IDENTICAL** |
| **Derivation Path** | `m/44'/9999'/0'/0/0` | `m/44'/9999'/0'/0/0` | `m/44'/9999'/0'/0/0` | **IDENTICAL** |
| **Compressed Pubkey** | `03d20a0a...6e3f` | `03d20a0a...6e3f` | `03d20a0a...6e3f` | **IDENTICAL** |
| **Raw Address** | `0x37061e043e1b89972ed615c46ff3e5090514c909` | `0x37061e043e1b89972ed615c46ff3e5090514c909` | `0x37061e043e1b89972ed615c46ff3e5090514c909` | **IDENTICAL** |
| **Mainnet Address** | `tola1xurpupp7rwyewtkkzhzxlul9pyz3fjgf4wludn` | `tola1xurpupp7rwyewtkkzhzxlul9pyz3fjgf4wludn` | `tola1xurpupp7rwyewtkkzhzxlul9pyz3fjgf4wludn` | **IDENTICAL** |

---

## 4. Legacy Testnet Provisional Path (`m/44'/102'/0'/0/0`)

| Vector | Raw Address | Testnet Bech32m Address | Parity Status |
| :--- | :--- | :--- | :---: |
| **Vector 1 (12 words)** | `0x8e3719da26b9408f27fa9602c4362d9fbc49dc73` | `ttola13cm3nk3xh9qg7fl6jcpvgd3dn77ynhrnukw78v` | **IDENTICAL** |
| **Vector 2 (24 words)** | `0x1073f501701b4a14dd4bc6a1f1746fca1fb8de64` | `ttola1vpeeyvshypq27dfg8h9z554e2q04x9m3g3q9s0` | **IDENTICAL** |

---

## 5. Verification Commands

To independently reproduce the complete parity verification across all runtimes:

```powershell
# 1. Rust workspace test suite (168 tests)
cargo test --workspace

# 2. TypeScript SDK test suite (32 tests)
node --test sdk/typescript/test/sdk.test.mjs

# 3. Python SDK test suite (31 tests)
python -m unittest discover -s sdk/python/tests -p "*.py"
```
All suites pass 100% with zero warnings.
