# TOLA Network — Native CLI Wallet & Recovery Guide

The `tola-cli wallet` command group provides native, production-grade wallet operations for TOLA Network:
- Cryptographically secure BIP-39 mnemonic generation (`generate`)
- Secure terminal-based wallet recovery with echo suppression (`recover`)
- Public address inspection (`address`)
- Public key inspection (`public-key`)
- Local transaction clear signing with interactive confirmation (`sign`)

---

## 1. Architectural Overview & Security Invariants

All user wallet keys in TOLA Network derive deterministically according to BIP-39 / BIP-32 / BIP-44:
- **Default Word Count:** 12 words (128-bit entropy CSPRNG).
- **High-Security Word Count:** 24 words (256-bit entropy CSPRNG).
- **Derivation Paths (ADR-0013):**
  - Mainnet (Chain ID `9999`): `m/44'/9999'/0'/0/0` (Address format: `tola1...`)
  - Testnet (Chain ID `102`): `m/44'/102'/0'/0/0` (Address format: `ttola1...`)
  - Devnet (Chain ID `1337`): `m/44'/1337'/0'/0/0` (Address format: `dtola1...`)
- **Address Algorithm:** `Keccak256(UncompressedPublicKey[1..65])[12..32]`.
- **Memory Security:** Plaintext recovery phrases, seeds, and private keys implement `ZeroizeOnDrop` and are erased from RAM when out of scope.
- **Air-Gapped Signing:** The private key NEVER leaves the local CLI process. It is never transmitted over RPC, telemetry, logs, or network pipes.

---

## 2. Command Reference

### `tola-cli wallet generate`

Generates a fresh wallet with a BIP-39 recovery phrase.

```bash
# Default: 12 words, Mainnet
tola-cli wallet generate

# Optional 24 words
tola-cli wallet generate --words 24

# Network selection
tola-cli wallet generate --network testnet
tola-cli wallet generate --network devnet

# Optional BIP-39 passphrase prompt
tola-cli wallet generate --passphrase
```

#### Output Example:
```text
TOLA Wallet Created
Network: Mainnet
Chain ID: 9999
Address:
tola18dzkvnsetkav6qp4q344z9y3uzda8j5263ez6j
Recovery Phrase:
abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about
IMPORTANT:
Your recovery phrase controls this wallet.
Store it offline.
Never share it with anyone.
```

> [!WARNING]
> The private key is never displayed. Your 12 or 24-word recovery phrase is the only backup needed to restore access to your funds.

---

### `tola-cli wallet recover`

Restores a wallet from an existing recovery phrase.

```bash
# Mainnet recovery
tola-cli wallet recover

# Testnet recovery
tola-cli wallet recover --network testnet
```

#### Terminal Interaction:
```text
Recovery phrase: [terminal echo disabled]
BIP-39 passphrase (optional): [terminal echo disabled]

Wallet recovered successfully.
Network: Mainnet
Address:
tola18dzkvnsetkav6qp4q344z9y3uzda8j5263ez6j
```

> [!IMPORTANT]
> For security, recovery phrases **cannot** be passed via command-line arguments (e.g. `--mnemonic`). Input is securely prompted through the terminal with echo disabled where supported. The phrase is never echoed or printed back.

---

### `tola-cli wallet address`

Inspects the public address of a wallet.

```bash
tola-cli wallet address
# Or for testnet:
tola-cli wallet address --network testnet
```

#### Output Example:
```text
tola18dzkvnsetkav6qp4q344z9y3uzda8j5263ez6j
```

Only the public address is output. No secret keys or mnemonic phrases are printed.

---

### `tola-cli wallet public-key`

Displays the compressed Secp256k1 public key (33 bytes hex).

```bash
tola-cli wallet public-key
```

#### Output Example:
```text
0x03e95745eb5f028b4e23fb4c5af97fb49426d522c4228fe6dd922f2585e6b1bf96
```

---

### `tola-cli wallet sign`

Constructs and signs a transaction entirely within the local process with clear signing verification.

```bash
# Sign local transfer (air-gapped or offline)
tola-cli wallet sign \
    --to tola18dzkvnsetkav6qp4q344z9y3uzda8j5263ez6j \
    --amount 100 \
    --nonce 42 \
    --network mainnet

# Sign and broadcast to local RPC
tola-cli wallet sign \
    --to tola18dzkvnsetkav6qp4q344z9y3uzda8j5263ez6j \
    --amount 100 \
    --rpc-url 127.0.0.1:8545 \
    --broadcast
```

#### Clear Signing Display & Confirmation:
```text
=======================================================
                   TOLA TRANSACTION                   
=======================================================
Network:   Mainnet
Chain ID:  9999
From:      tola18dzkvnsetkav6qp4q344z9y3uzda8j5263ez6j
To:        tola18dzkvnsetkav6qp4q344z9y3uzda8j5263ez6j
Amount:    100 TOLA
Max Fee:   0.01 TOLA
Nonce:     42
Action:    Transfer
=======================================================
Sign this transaction? [y/N]: y

[SUCCESS] Transaction signed locally!
Transaction Hash: 0x32f94669e75eaf7c3d4d68388eb100c9c7884a86e1d16af2ee5c7e808281b830
Signed Transaction (Hex):
0x...
```

> [!NOTE]
> The clear signing prompt defaults to `[N]`. If the user presses Enter or enters any response other than `y` / `yes`, signing is aborted immediately.

---

## 3. Relationship with `tola-cli account new`

| Command | Purpose | Target Audience | Status |
| :--- | :--- | :--- | :--- |
| `tola-cli wallet generate` | BIP-39 Mnemonic + BIP-44 Derivation | End users, wallets, signers | **Canonical Primary Standard** |
| `tola-cli account new` | Raw Secp256k1 Keypair (Hex) | Low-level automation, legacy scripts | **Backward Compatible (Retained)** |

`tola-cli account new` remains fully functional and unmodified for backward compatibility. In future major protocol updates, `account new` may be aliased to `wallet generate`.
