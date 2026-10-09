# TOLA Network Python SDK (`tola`)

Official Python client library for backend integrations, trading bots, indexing pipelines, and validator automation.

---

## 1. Installation & Environment

The Python SDK uses only the Python standard library with no external dependencies required for core functionality.

```bash
pip install ./sdk/python
```

---

## 2. Key Management & Wallets

```python
from tola import TolaWallet, Network

# Generate random wallet
wallet = TolaWallet.generate()
print("Private Key:", wallet.private_key_hex)
print("Address Hex:", wallet.address.to_hex())
print("Mainnet Address:", wallet.address_bech32m(Network.MAINNET))
print("Testnet Address:", wallet.address_bech32m(Network.TESTNET))

# Import from existing private key hex
imported = TolaWallet.from_hex_private_key("0x4242424242424242424242424242424242424242424242424242424242424242")
```

---

## 3. Composing & Broadcasting Transactions

```python
from tola import TolaClient, TransactionBuilder

client = TolaClient("http://127.0.0.1:8545")

# Check node health
assert client.check_health()

# Query latest nonce
nonce = client.get_nonce(wallet.address.to_hex())

# Compose transaction
tx = (
    TransactionBuilder(chain_id=102)
    .nonce(nonce)
    .to("ttola1zlz3s5t8gq0dqr847ke0e97eh07m05p90jzepq")
    .value(1_000_000_000_000_000_000)  # 1 TOLA
    .gas_limit(21000)
    .max_fee_per_gas(1_000_000_000)
    .max_priority_fee_per_gas(1_000_000_000)
    .valid_until_block(1_000_000)
    .build(wallet.address)
)

# Sign with wallet
signed_tx = wallet.sign_transaction(tx)

# Broadcast
tx_hash = client.send_transaction(signed_tx)
print("Broadcasted TX Hash:", tx_hash)

# Wait for receipt
receipt = client.wait_for_transaction_receipt(tx_hash, timeout=30.0)
print(f"Transaction included in block #{receipt.block_number}")
```

---

## 4. Querying Balances & Blocks

```python
# Query account balance
balance = client.get_balance(wallet.address.to_hex())
print(f"Balance: {balance / 10**18:.4f} TOLA")

# Query block height
height = client.get_block_number()
print("Current Block Height:", height)
```
