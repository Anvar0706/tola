# TOLA Network: JSON-RPC 2.0 Engine & Ethereum Compatibility Specification

## 1. Overview & Architectural Motivation
The **TOLA JSON-RPC Engine** (`tola-rpc`) implements a production-grade, zero-dependency JSON-RPC 2.0 server providing native Ethereum compatibility alongside TOLA protocol extensions.

To achieve seamless developer adoption and ecosystem composability (wallets, DEX frontends, explorers, and development frameworks):
1. **100% Web3/Ethers Compatibility:** Standard Ethereum JSON-RPC 2.0 calls (`eth_*`, `net_*`, `web3_*`) are supported out-of-the-box. Developers can point MetaMask, Foundry, Hardhat, ethers.js, and viem directly to a TOLA node.
2. **Dual Address Format Ingestion:** The dispatcher seamlessly decodes both standard Ethereum 20-byte hex addresses (`0x...`) and canonical TOLA Bech32m addresses (`tola1...`, `ttola1...`, `dtola1...`).
3. **Zero-Dependency Security:** Implemented with a custom, zero-dependency JSON parser/serializer (`tola-rpc::json`), eliminating supply chain vulnerabilities and runtime bloat.
4. **Native Protocol Observability:** Protocol extensions (`tola_*`) expose pipelined BFT committee states, epoch boundaries, and threshold Quorum Certificates (QCs).

---

## 2. Standard Ethereum API Surface

### 2.1 Web3 & Network Methods
* **`web3_clientVersion`**: Returns client banner string (e.g., `"TOLA/v0.1.0-alpha"`).
* **`web3_sha3(data)`**: Computes standard 32-byte Keccak-256 hash.
* **`net_version`**: Returns decimal chain ID string (e.g., `"9999"` for Mainnet, `"102"` for Testnet).
* **`net_listening`**: Returns `true`.
* **`net_peerCount`**: Returns current active P2P peer count in hex.

### 2.2 Ethereum Ledger & State Queries
* **`eth_chainId`**: Returns hex-encoded ChainId (e.g., `"0x270f"` for 9999 Mainnet, `"0x66"` for 102 Testnet).
* **`eth_blockNumber`**: Returns the latest finalized or committed block height in hex.
* **`eth_gasPrice`**: Returns the current base fee per gas.
* **`eth_getBalance(address, block_tag)`**: Queries account liquid balance in wei/attotola (`0x...`).
* **`eth_getTransactionCount(address, block_tag)`**: Returns monotonic account nonce (`0x...`).
* **`eth_getCode(address, block_tag)`**: Returns smart contract bytecode or `"0x"` for EOAs.
* **`eth_getStorageAt(address, slot, block_tag)`**: Reads state storage slot value.
* **`eth_estimateGas(transaction_call)`**: Calculates intrinsic and execution gas requirements.
* **`eth_sendRawTransaction(raw_tx_hex)`**: Decodes canonical transaction payload, performs stateless validation, and submits to the mempool priority queue.
* **`eth_getBlockByNumber(tag, full_txs)`**: Formats block header, gas metrics, and transaction list.
* **`eth_getBlockByHash(hash, full_txs)`**: Queries block by 32-byte hash.
* **`eth_getTransactionByHash(tx_hash)`**: Queries transaction payload and location.
* **`eth_getTransactionReceipt(tx_hash)`**: Formats canonical receipt with status code, gas consumed, and logs.

---

## 3. TOLA Protocol Extensions (`tola_*`)

| Method | Parameters | Return Type | Description |
| :--- | :--- | :--- | :--- |
| **`tola_getEpoch`** | `[]` | `HexQuantity` | Returns current consensus epoch number. |
| **`tola_getValidators`** | `[]` | `Array<ValidatorInfo>` | Returns active consensus committee: validator ID, Ed25519 consensus pubkey, voting power, and commission rate. |
| **`tola_getQuorumCertificate`** | `[height]` | `QuorumCertificate` | Returns threshold QC: block hash, round, validator bitfield, and aggregated signatures. |

---

## 4. JSON-RPC 2.0 Error Specifications

Standard JSON-RPC 2.0 errors are mapped according to RFC specifications:

| Error Code | Error Constant | Trigger Condition |
| :--- | :--- | :--- |
| **`-32700`** | `PARSE_ERROR` | Malformed JSON payload received on wire. |
| **`-32600`** | `INVALID_REQUEST` | Missing `method` or invalid JSON-RPC structure. |
| **`-32601`** | `METHOD_NOT_FOUND` | Unregistered RPC method requested. |
| **`-32602`** | `INVALID_PARAMS` | Address decoding failure, odd hex length, or out-of-bounds parameter. |
| **`-32603`** | `INTERNAL_ERROR` | Storage or state database access failure. |
| **`-32000`** | `SERVER_ERROR` | Mempool admission failure (underpriced, nonce gap, pool full). |
