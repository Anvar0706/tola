# TOLA Network TypeScript SDK (`@tola/sdk`)

Official client SDK and Web3 provider library for interacting with TOLA Network in Node.js, Web browsers, React Native, and Telegram Mini Apps.

---

## 1. Architecture & Design Principles

- **Zero Heavy Cryptographic Dependencies:** Self-contained pure implementations of Keccak-256, BLAKE3, Secp256k1 (RFC 6979), and BIP-350 Bech32m.
- **Cross-Platform Compatibility:** Runs seamlessly across Node.js 18+, modern evergreen browsers, and sandboxed WebView runtimes (Telegram WebApps).
- **Protocol Parity:** Matches Rust `tola-codec` and `tola-crypto` byte-for-byte in serialization and domain-separated signing digests.
- **EIP-1193 Standard Provider:** Plug-and-play compatibility with standard Web3 libraries (viem, wagmi, ethers).

---

## 2. Installation & Quick Start

```bash
npm install @tola/sdk
```

### Initializing a Wallet

```typescript
import { TolaWallet, Address } from '@tola/sdk';

// Generate a fresh random wallet
const wallet = TolaWallet.generate();
console.log('Private Key:', wallet.privateKeyHex);
console.log('Hex Address:', wallet.address.toHex());
console.log('Mainnet Address:', wallet.addressBech32m('mainnet')); // tola1...
console.log('Testnet Address:', wallet.addressBech32m('testnet')); // ttola1...

// Import from existing private key
const imported = TolaWallet.fromPrivateKeyHex('0x4242424242424242424242424242424242424242424242424242424242424242');
```

---

## 3. Constructing & Signing Transactions

```typescript
import { TransactionBuilder, JsonRpcProvider } from '@tola/sdk';

const provider = new JsonRpcProvider('http://127.0.0.1:8545');

// Query latest nonce
const nonce = await provider.getTransactionCount(wallet.address.toHex());

// Build transfer transaction
const tx = new TransactionBuilder(102n) // ChainId 102 = Testnet
  .nonce(nonce)
  .to('ttola1zlz3s5t8gq0dqr847ke0e97eh07m05p90jzepq')
  .value(1_000_000_000_000_000_000n) // 1 TOLA
  .gasLimit(21000n)
  .maxFeePerGas(1_000_000_000n) // 1 Gwei
  .maxPriorityFeePerGas(1_000_000_000n)
  .validUntilBlock(1_000_000n)
  .build(wallet.address.toHex());

// Sign transaction (enforces RFC 6979 and canonical domain separation)
const signedTx = await wallet.signTransaction(tx);

// Broadcast to network
const txHash = await provider.sendRawTransaction(signedTx);
console.log('Transaction Broadcasted:', txHash);

// Wait for confirmation
const receipt = await provider.waitForTransactionReceipt(txHash);
console.log('Confirmed in Block:', receipt.blockNumber);
```

---

## 4. Querying Node State

```typescript
// Query balance
const balance = await provider.getBalance(wallet.address.toHex());
console.log('Balance (base units):', balance);

// Query node health
const health = await provider.getHealth();
console.log('Chain ID:', health.chainId);
console.log('Block Height:', health.blockHeight);
console.log('Finalized Height:', health.finalizedHeight);
console.log('Active Peers:', health.peerCount);
```

---

## 5. Telegram Mini App Integration

`@tola/sdk` includes `TolaProvider`, an EIP-1193 compatible provider that works inside Telegram Mini Apps without browser extension dependencies:

```typescript
import { TolaProvider } from '@tola/sdk';

const provider = new TolaProvider('https://testnet-rpc.tola.network');

// Dispatch standard EIP-1193 requests
const chainId = await provider.request({ method: 'eth_chainId' });
const blockNumber = await provider.request({ method: 'eth_blockNumber' });

// Listen for network events
provider.on('chainChanged', (chainId) => {
  console.log('Switched to chain:', chainId);
});
```
