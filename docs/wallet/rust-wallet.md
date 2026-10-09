# TOLA Network Native Rust Wallet Specification & Guide

## 1. Overview & Architecture

The TOLA Network Rust wallet stack provides production-grade, cryptographically secure key management, deterministic derivation, transaction signing, and arbitrary message clear-signing.

The implementation is modularized across three workspace crates:
- **`crates/tola-crypto`**: Implements BIP-39 mnemonic generation & validation, BIP-32 hierarchical deterministic derivation, RFC 6979 deterministic ECDSA signing on Secp256k1, Low-S malleability defense, and Keccak-256 / BLAKE3 hashing.
- **`crates/tola-sdk`**: Higher-level developer SDK providing `TolaWallet`, `TransactionBuilder`, Bech32m address handling, and end-to-end transaction generation.
- **`crates/tola-cli`**: Command-line wallet subcommands (`tola-cli wallet generate`, `recover`, `address`, `public-key`, `sign`).

---

## 2. Derivation Standard Conformance

The Rust wallet strictly adheres to the frozen **TOLA Wallet Derivation Standard v1.0**:

```
BIP-39 Mnemonic (12 / 24 words)
       │
       ▼ (PBKDF2-HMAC-SHA512, 2048 iterations)
512-bit Binary Seed
       │
       ▼ (BIP-32 HMAC-SHA512 master derivation)
Master Node (m)
       │
       ▼ (m/44'/9999'/0'/0/0)
Secp256k1 Private Scalar (32 bytes)
       │
       ▼ (Elliptic curve scalar multiplication k * G)
Secp256k1 Uncompressed Public Key (65 bytes: 0x04 || X || Y)
       │
       ▼ (Keccak-256 of uncompressed pubkey[1..65])
32-byte Keccak-256 Digest
       │
       ▼ (Last 20 bytes: digest[12..32])
Canonical Raw Account Address (20 bytes)
       │
       ▼ (BIP-350 Bech32m encoding with network HRP)
Bech32m Address (tola1... / ttola1... / dtola1...)
```

### Derivation Paths
- **Canonical Derivation Path**: `m/44'/9999'/0'/0/0` (Unified across all networks under Model A).
- **Legacy Testnet Provisional Path**: `m/44'/102'/0'/0/0` (Retained exclusively for legacy migration verification).

---

## 3. Rust API Reference (`tola-sdk`)

### Core Types & Constructors

```rust
use tola_sdk::TolaWallet;
use tola_types::Network;

// 1. Generate fresh 12-word wallet
let (wallet, mnemonic) = TolaWallet::generate_with_mnemonic(12, "")?;
println!("Mnemonic: {}", mnemonic);
println!("Address: {}", wallet.address_bech32m(&Network::Mainnet));

// 2. Restore wallet from mnemonic
let wallet = TolaWallet::recover("abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about", "")?;
assert_eq!(wallet.address().to_hex(), "0x3b45664e195dbacd0035046b511491e09bd3ca8a");

// 3. Import from raw 32-byte private key or hex
let wallet = TolaWallet::from_hex_private_key("0xf77999a4d727d792213ae012f18621208ea91a7246e5bf69a4317942abeca750")?;
```

### Transaction Signing

```rust
let mut tx = Transaction {
    chain_id: ChainId(9999),
    nonce: Nonce(0),
    from: wallet.address(),
    to: Some(recipient_addr),
    value: Amount(1_000_000_000_000_000_000), // 1 TOLA
    gas_limit: Gas(21000),
    max_fee_per_gas: Amount(1_000_000_000),
    max_priority_fee_per_gas: Amount(1_000_000_000),
    valid_until_block: BlockHeight(100),
    data: vec![],
    signature: Signature::Secp256k1 { r: [0; 32], s: [0; 32], v: 0 },
};

wallet.sign_transaction(&mut tx)?;
```

### Arbitrary Message Clear-Signing & Recovery

Conforms to standard prefixed envelope: `\x19TOLA Signed Message:\n<length><message>`.

```rust
let message = b"Hello TOLA Network Genesis 2026";
let sig_bytes: [u8; 65] = wallet.sign_message(message)?;
let sig_hex: String = wallet.sign_message_hex(message)?;

// Recover signer address
let recovered_addr = TolaWallet::recover_message_address(message, &sig_bytes)?;
assert_eq!(recovered_addr, wallet.address());
```

---

## 4. Security & Isolation Guarantees

1. **Zero Secret Leakage in Debug / Display**:
   `TolaWallet` implements `core::fmt::Debug` redacting all private key material:
   ```rust
   // Output: TolaWallet([REDACTED: 0x3b45664e195dbacd0035046b511491e09bd3ca8a])
   ```
2. **Deterministic Nonces (RFC 6979)**: Nonce collision attacks and biased RNG vulnerabilities are mathematically eliminated.
3. **Malleability Immunity (BIP-62 Low-S)**: Any signature with $s > N/2$ is flipped to $N - s$ with normalized recovery ID $v \in \{0, 1\}$.
4. **Network Replay Isolation**: Chain ID is bound into the BLAKE3 transaction signing envelope.
