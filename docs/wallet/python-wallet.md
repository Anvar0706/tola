# TOLA Network Native Python Wallet Specification & Guide

## 1. Overview & Architecture

The TOLA Network Python SDK (`tola`) provides a complete, production-ready, pure Python standard library implementation of native wallet generation, recovery, derivation, transaction signing, and JSON-RPC communication.

### Zero-Dependency Architecture
The SDK requires **zero third-party dependencies** (`pip install` free). All cryptographic operations are implemented using standard library primitives:
- `hashlib`: SHA-256, SHA-512, PBKDF2
- `hmac`: RFC 6979 deterministic nonce generation, BIP-32 child key derivation
- `secrets`: CSPRNG for entropy and mnemonic generation
- `unicodedata`: NFKD normalization for BIP-39 passwords and mnemonics
- `urllib.request` / `http.client`: JSON-RPC 2.0 communication with TOLA nodes

---

## 2. Derivation Standard Conformance

The Python wallet strictly enforces the **TOLA Wallet Derivation Standard v1.0**:

```
BIP-39 Mnemonic
      │ (NFKD normalize + PBKDF2-HMAC-SHA512, 2048 rounds)
      ▼
512-bit Binary Seed
      │ (HMAC-SHA512 with "Bitcoin seed")
      ▼
Master Node (m)
      │ (m/44'/9999'/0'/0/0)
Secp256k1 Private Scalar
      │ (Point multiplication k * G)
Uncompressed Public Key
      │ (Keccak-256 of pubkey[1..65])
20-byte Raw Address (0x3b45664e195dbacd0035046b511491e09bd3ca8a)
      │ (Bech32m encoding)
tola18dzkvnsetkav6qp4q344z9y3uzda8j5263ez6j
```

---

## 3. Python API Reference

### Core Wallet Usage

```python
from tola import TolaWallet, Network

# 1. Generate new wallet with 12-word mnemonic
wallet = TolaWallet.generate(12)
print("Mnemonic phrase:", wallet.mnemonic)
print("Mainnet Address:", wallet.get_address(Network.MAINNET))
print("Testnet Address:", wallet.get_address(Network.TESTNET))

# 2. Restore wallet from mnemonic
phrase = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about"
restored = TolaWallet.from_mnemonic(phrase)
assert restored.raw_address == "0x3b45664e195dbacd0035046b511491e09bd3ca8a"
assert restored.get_address(Network.MAINNET) == "tola18dzkvnsetkav6qp4q344z9y3uzda8j5263ez6j"
assert restored.get_address(Network.TESTNET) == "ttola18dzkvnsetkav6qp4q344z9y3uzda8j527xgjrh"
assert restored.get_address(Network.DEVNET) == "dtola18dzkvnsetkav6qp4q344z9y3uzda8j52x0hhl4"

# 3. Import from raw private key (int, 32 bytes, or hex string)
w_hex = TolaWallet.from_private_key("0xf77999a4d727d792213ae012f18621208ea91a7246e5bf69a4317942abeca750")
assert w_hex.raw_address == restored.raw_address
```

### Transaction Building & Signing

```python
from tola import TransactionBuilder, TolaClient

client = TolaClient("http://127.0.0.1:8545")

tx = (
    TransactionBuilder(102)
    .to("0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee")
    .value(2_500_000_000_000_000_000) # 2.5 TOLA
    .nonce(client.get_nonce(wallet.raw_address))
    .gas_limit(21000)
    .max_fee_per_gas(1_000_000_000)
    .max_priority_fee_per_gas(1_000_000_000)
    .valid_until_block(client.get_block_number() + 100)
    .build(wallet.raw_address)
)

signed_tx = wallet.sign_transaction(tx)
tx_hash = client.send_transaction(signed_tx)
print("Submitted tx hash:", tx_hash)
```

### Arbitrary Message Clear-Signing & Recovery

```python
msg = "Hello TOLA Network Genesis 2026"
sig_hex = wallet.sign_message(msg)
# Output: 0xb2c3fd06ac1b3044e522b6e0c8e9aa43f931825bac33cb0230399d1b4098d9bf...

recovered = TolaWallet.recover_message_address(msg, sig_hex)
assert recovered == wallet.address
```

---

## 4. Security & Isolation Guarantees

1. **Zero Secret Leakage**:
   - `repr(wallet)`: `<TolaWallet address=tola18dzkvnsetkav6qp4q344z9y3uzda8j5263ez6j>`
   - `str(wallet)`: `tola18dzkvnsetkav6qp4q344z9y3uzda8j5263ez6j`
   - `wallet.to_dict()`: Exposes only `address` and `raw_address`. Never exposes private scalar or mnemonic.
2. **RFC 6979 Deterministic Nonces**: Prevents nonce reuse attacks with exact bit parity to Rust and TypeScript.
3. **Low-S Normalization**: Canonical signatures strictly satisfy $s \le N/2$.
4. **Replay Protection**: Chain ID is bound into the BLAKE3 transaction signing envelope.
