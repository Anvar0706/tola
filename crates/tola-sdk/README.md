# tola-sdk

Official Rust client SDK for the **TOLA Network**.

High-performance, strongly-typed client library for native Rust services, high-throughput pipelines, validator automation, and CLI tools.

---

## Features

- **Wallet & Cryptography**:
  - BIP-39 mnemonic phrase generation (12 or 24 words) and seed derivation via `tola-crypto`.
  - BIP-44 path derivation:
    - Mainnet: `m/44'/9999'/0'/0/0`
    - Testnet: `m/44'/102'/0'/0/0`
    - Devnet: `m/44'/1337'/0'/0/0`
  - Secp256k1 ECDSA signing, address recovery, and arbitrary message clear-signing.
  - Secret isolation: Debug redaction prevents sensitive keys and mnemonics from leaking to logs.
- **Transaction Composition**:
  - `TransactionBuilder` for building native transactions and EVM smart contract interactions.
  - Built-in transaction memo bounds enforcement (up to 256 bytes UTF-8).
- **Client & JSON-RPC Integration**:
  - `TolaClient` providing direct bindings to TOLA node RPC methods.
  - Account balance queries, nonces, raw transaction submission, and receipt lookups.

---

## Usage

Add `tola-sdk` to your `Cargo.toml`:

```toml
[dependencies]
tola-sdk = { path = "../tola-sdk", version = "0.1.0" }
```

---

## Quickstart

### 1. Generating or Restoring a Wallet

```rust
use tola_sdk::{TolaWallet, MnemonicCount};

// Generate a new 12-word wallet
let wallet = TolaWallet::generate(MnemonicCount::Words12, None).expect("failed to generate");
println!("Mnemonic: {}", wallet.mnemonic().unwrap());
println!("Hex Address: {}", wallet.address_hex());
println!("Mainnet Address: {}", wallet.address_bech32m("tola").unwrap());

// Restore wallet from mnemonic
let restored = TolaWallet::from_mnemonic(wallet.mnemonic().unwrap(), None).expect("failed to restore");
assert_eq!(wallet.address(), restored.address());
```

### 2. Building and Signing a Transaction

```rust
use tola_sdk::{TolaWallet, TransactionBuilder};
use tola_types::Address;

let wallet = TolaWallet::generate_random();
let recipient = Address::from_hex("0x70997970C51812dc3A010C7d01b50e0d17dc79C8").unwrap();

let tx = TransactionBuilder::new()
    .chain_id(9999) // Mainnet
    .nonce(0)
    .to(recipient)
    .value(1_000_000_000) // 1 TOLA in nanoTOLA
    .max_fee(21_000)
    .memo("Payment invoice #1042")
    .build_and_sign(&wallet)
    .expect("failed to sign transaction");

println!("Transaction digest: {:?}", tx.digest());
```

### 3. Interacting with a Node via `TolaClient`

```rust
use tola_sdk::TolaClient;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = TolaClient::new("http://127.0.0.1:8545");

    let block_height = client.get_block_number().await?;
    println!("Current block height: {}", block_height);

    let address = "0x70997970C51812dc3A010C7d01b50e0d17dc79C8";
    let balance = client.get_balance(address).await?;
    println!("Balance: {} nanoTOLA", balance);

    Ok(())
}
```

---

## Running Tests

```bash
cargo test -p tola-sdk
```

---

## License

Apache-2.0
