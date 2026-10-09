# @tola/sdk

Official cross-platform TypeScript SDK and Web3 Provider for the **TOLA Network**.

Supports Node.js, Web browsers, React Native, and Telegram Mini Apps.

---

## Features

- **Key Management & Standards**:
  - Full BIP-39 mnemonic phrase generation (12 or 24 words) and seed recovery.
  - BIP-44 multi-network derivation:
    - Mainnet: `m/44'/9999'/0'/0/0`
    - Testnet: `m/44'/102'/0'/0/0`
    - Devnet: `m/44'/1337'/0'/0/0`
  - Secp256k1 elliptic curve signatures and address recovery.
  - Dual address representation: 20-byte raw hex (`0x...`) and human-readable Bech32m (`tola1...`, `tola_test1...`, `tola_dev1...`).
- **Transaction Composition & Signing**:
  - Fluent `TransactionBuilder` for native transfers and EVM contract calls.
  - User memo/comment support with UTF-8 validation (up to 256 bytes).
  - Explicit replay protection via chain ID binding.
- **Provider & Web3 Tooling**:
  - `JsonRpcProvider` for typed RPC queries, block headers, state queries, and transaction broadcasting.
  - EIP-1193 compliant `TolaProvider` for seamless integration into Ethereum/Web3 libraries (ethers.js, viem, web3.js).
- **TOLA Connect & Mini Apps**:
  - Non-custodial Mini App Bridge (`MiniAppBridge` / `TelegramMiniAppBridge`).
  - E2EE session management with ECDH + AES-256-GCM.
  - Mandatory clear-signing prompts with bounds checking and domain binding.
  - Multi-device authorization and revocation registry (`DeviceRegistry`).
- **Dynamic Sharding & State Proofs**:
  - Deterministic shard calculation, cross-shard receipt nullifiers, and Merkle inclusion proof verification.
- **Tokenized External Assets (TLS-721)**:
  - Calldata builders and cryptographic attestation signers for external assets (usernames, badges, collectibles).

---

## Installation

```bash
npm install @tola/sdk
```

---

## Quickstart

### 1. Generating and Recovering a Wallet

```typescript
import { TolaWallet } from '@tola/sdk';

// Generate a new 12-word wallet (or 24-word by passing 24)
const wallet = TolaWallet.generate(12);
console.log('Mnemonic:', wallet.mnemonic);
console.log('Hex Address:', wallet.address.toHex());
console.log('Mainnet Address:', wallet.addressBech32m('mainnet'));

// Recover from mnemonic
const restored = TolaWallet.fromMnemonic(wallet.mnemonic);
console.log('Restored Address:', restored.address.toHex());
```

### 2. Composing and Signing a Transaction

```typescript
import { TolaWallet, TransactionBuilder } from '@tola/sdk';

const wallet = TolaWallet.generate();

const tx = new TransactionBuilder()
  .setChainId(9999) // Mainnet: 9999, Testnet: 102, Devnet: 1337
  .setNonce(0)
  .setTo('0x70997970C51812dc3A010C7d01b50e0d17dc79C8')
  .setValue(1_000_000_000n) // in nanoTOLA (1 TOLA = 10^9 nanoTOLA)
  .setMaxFee(21000n)
  .setMemo('Invoice #1042 payment')
  .build();

const signedTx = await wallet.signTransaction(tx);
console.log('Transaction Digest:', signedTx.digest);
console.log('Signature:', signedTx.signature);
```

### 3. Querying the Network via JSON-RPC

```typescript
import { JsonRpcProvider } from '@tola/sdk';

const provider = new JsonRpcProvider('http://127.0.0.1:8545');

const blockNumber = await provider.getBlockNumber();
console.log('Current Block:', blockNumber);

const balance = await provider.getBalance('0x70997970C51812dc3A010C7d01b50e0d17dc79C8');
console.log('Balance:', balance);
```

### 4. EIP-1193 Web3 Provider Integration

```typescript
import { TolaProvider, TolaWallet } from '@tola/sdk';

const wallet = TolaWallet.generate();
const provider = new TolaProvider({
  rpcUrl: 'http://127.0.0.1:8545',
  wallet: wallet,
  chainId: 9999,
});

// Standard EIP-1193 request interface
const accounts = await provider.request({ method: 'eth_accounts' });
console.log('Connected accounts:', accounts);
```

### 5. Telegram Mini App & Clear-Signing Bridge

```typescript
import { MiniAppBridge, TolaWallet } from '@tola/sdk';

const wallet = TolaWallet.generate();
const bridge = new MiniAppBridge(wallet, {
  expectedDomain: 'https://app.tola.network',
  onClearSign: async (prompt) => {
    // Present clear-signing details to user in UI
    console.log(`Authorize transfer of ${prompt.amount} to ${prompt.to}?`);
    return true; // Return false to reject
  },
});

const result = await bridge.requestPayment({
  domain: 'https://app.tola.network',
  to: '0x70997970C51812dc3A010C7d01b50e0d17dc79C8',
  amount: 5_000_000_000n,
  memo: 'Order #42',
});
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
npm test
```

49 unit, cryptographic parity, and integration test cases covering wallet generation, BIP-39 vectors, cross-SDK parity, EIP-1193, clear-signing, and external asset calldata.

---

## License

Apache-2.0
