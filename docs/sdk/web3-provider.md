# TOLA Network Web3 Provider Integration (EIP-1193)

Comprehensive guide for integrating TOLA Network with Web3 wallets, dApps, Telegram Mini Apps, and standard Ethereum provider tooling.

---

## 1. Overview & Standard Compliance

TOLA Network RPC endpoints implement full Ethereum JSON-RPC 2.0 compatibility (`eth_*`, `net_*`, `web3_*`) alongside high-performance native extensions (`tola_*`).

The TypeScript SDK provides `TolaProvider`, an [EIP-1193](https://eips.ethereum.org/EIPS/eip-1193) compliant JavaScript provider interface exposing the canonical `.request({ method, params })` interface.

---

## 2. Supported JSON-RPC Methods

### Standard Ethereum Methods
- `eth_chainId`: Returns the chain ID (e.g. `0x270f` for 9999 Mainnet, `0x66` for 102 Testnet).
- `eth_blockNumber`: Returns the latest committed block height in hex.
- `eth_getBalance`: Returns account balance in base units.
- `eth_getTransactionCount`: Returns account nonce for replay protection.
- `eth_sendRawTransaction`: Ingests a signed transaction into the mempool.
- `eth_getBlockByNumber`: Returns block header and transaction lists.
- `eth_getBlockByHash`: Returns block details by hash.
- `eth_getTransactionByHash`: Returns transaction details by hash.
- `eth_getTransactionReceipt`: Returns block inclusion receipt and status.
- `net_version`: Network identifier string.
- `net_peerCount`: Current active P2P connected peers.
- `web3_clientVersion`: Client software version string.

### Native TOLA Extensions
- `tola_getHealth`: Real-time node operational health, view, finalized height, state root.
- `tola_getValidators`: Active validator set and voting power.
- `tola_getQuorumCertificate`: Quorum Certificate for a finalized block.

---

## 3. Integrating with Web3 Libraries

### Connecting with Viem / Wagmi
```typescript
import { createPublicClient, custom } from 'viem';
import { TolaProvider } from '@tola/sdk';

const tolaProvider = new TolaProvider('https://testnet-rpc.tola.network');

const client = createPublicClient({
  transport: custom(tolaProvider),
});

const blockNumber = await client.getBlockNumber();
```

### Connecting with Ethers.js (v6)
```typescript
import { BrowserProvider } from 'ethers';
import { TolaProvider } from '@tola/sdk';

const tolaProvider = new TolaProvider('https://testnet-rpc.tola.network');
const provider = new BrowserProvider(tolaProvider);

const feeData = await provider.getFeeData();
```
