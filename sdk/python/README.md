# tola: TOLA Network Python SDK

Official Python client SDK and bot tooling for the **TOLA Network**.

Pure Python implementation designed for backend automation, automated bots, algorithmic services, and testing infrastructure.

---

## Features

- **Wallet & Key Derivation**:
  - Full BIP-39 implementation with English wordlist (12 or 24 words) and seed derivation.
  - BIP-44 multi-network path derivation:
    - Mainnet (`9999`): `m/44'/9999'/0'/0/0`
    - Testnet (`102`): `m/44'/102'/0'/0/0`
    - Devnet (`1337`): `m/44'/1337'/0'/0/0`
  - Secp256k1 key generation, signature recovery, and message signing.
  - Bech32m address formatting (`tola1...`, `tola_test1...`, `tola_dev1...`) and raw hex (`0x...`).
- **Transaction Composition & Signing**:
  - `TransactionBuilder` with strict fee calculation, chain ID validation, and nonce tracking.
  - Native memo support (UTF-8 up to 256 bytes) with cryptographic tamper resistance.
- **Client & JSON-RPC Communication**:
  - `TolaClient` for communicating with TOLA node RPC endpoints.
  - Standard queries: block height, balances, nonces, raw transaction submission, receipts, health checks.
- **Optional Application & Bot Infrastructure (TOLA Connect)**:
  - `ClearSigningPrompt` verification and human-readable summarization.
  - Multi-device authorization and revocation management (`DeviceRegistry`).
  - Encrypted end-to-end session protocol (`TolaConnectSession`).
- **Dynamic Sharding & External Assets (TLS-721)**:
  - Shard partition calculator and Merkle receipt proof verification.
  - Calldata builders and cryptographic attestation verifiers for external assets.

---

## Installation

```bash
pip install tola
```

Or from source:

```bash
git clone https://github.com/tolanetwork/tola.git
cd tola/sdk/python
pip install -e .
```

---

## Quickstart

### 1. Generating and Recovering a Wallet

```python
from tola import TolaWallet

# Generate a new 12-word wallet (or pass 24 for 24 words)
wallet = TolaWallet.generate(word_count=12)
print("Mnemonic:", wallet.mnemonic)
print("Address (Hex):", wallet.address.to_hex())
print("Mainnet Address:", wallet.address_bech32m("mainnet"))

# Recover wallet from mnemonic
restored = TolaWallet.from_mnemonic(wallet.mnemonic)
assert restored.address.to_hex() == wallet.address.to_hex()
```

### 2. Composing and Signing a Transaction

```python
from tola import TolaWallet, TransactionBuilder

wallet = TolaWallet.generate()

builder = (
    TransactionBuilder()
    .set_chain_id(9999) # Mainnet: 9999, Testnet: 102, Devnet: 1337
    .set_nonce(0)
    .set_to("0x70997970C51812dc3A010C7d01b50e0d17dc79C8")
    .set_value(1_000_000_000) # 1 TOLA in nanoTOLA
    .set_max_fee(21_000)
    .set_memo("Payment for invoice #1042")
)

tx = builder.build()
signed_tx = wallet.sign_transaction(tx)
print("Transaction Digest:", signed_tx.digest.hex())
```

### 3. Communicating with a TOLA Node

```python
from tola import TolaClient, TolaWallet, TransactionBuilder

client = TolaClient("http://127.0.0.1:8545")

# Check node health
health = client.get_node_health()
print(f"Node is healthy: {health.healthy}, block height: {health.block_height}")

# Query balance and nonce
address = "0x70997970C51812dc3A010C7d01b50e0d17dc79C8"
balance = client.get_balance(address)
nonce = client.get_nonce(address)
print(f"Balance: {balance} nanoTOLA, Nonce: {nonce}")
```

### 4. Bot & Automation Integration Example

```python
from tola import TolaClient, TolaWallet, TransactionBuilder, ClearSigningPrompt

# Fast non-custodial clear-signing validation for bot commands
prompt = ClearSigningPrompt(
    domain="https://bot.tola.network",
    to="0x70997970C51812dc3A010C7d01b50e0d17dc79C8",
    amount=5_000_000_000,
    memo="Bot order #88",
    max_fee=50_000,
    nonce=1,
    chain_id=9999,
)

prompt.validate()
print("Clear sign summary:", prompt.format_summary())
```

---

## Network Parameters

| Network | Chain ID | Bech32m HRP | Default Derivation Path |
|:---|:---:|:---:|:---|
| **Mainnet** | `9999` | `tola` | `m/44'/9999'/0'/0/0` |
| **Testnet** | `102` | `tola_test` | `m/44'/102'/0'/0/0` |
| **Devnet** | `1337` | `tola_dev` | `m/44'/1337'/0'/0/0` |

---

## Running Tests

```bash
python -m unittest discover -s tests
```

46 unit, cryptographic vector, RPC simulation, and security test cases.

---

## License

Apache-2.0
