# TOLA Network: EVM Core Specification & Operational Architecture

**Status:** IMPLEMENTED & VERIFIED (EVM Core P0–P10)  
**Engine:** REVM v43.0.3  
**EVM Target Revision:** Shanghai (`SpecId::SHANGHAI`)  
**Solidity Baseline:** `solc 0.8.24` (`--evm-version shanghai`)  
**Libraries Baseline:** `@openzeppelin/contracts v5.0.2`  

---

## 1. Overview & Architectural Scope

The **TOLA EVM Core** (`crates/tola-execution`, `crates/tola-codec`, `crates/tola-rpc`, `crates/tola-storage`) embeds a high-performance, deterministic Ethereum Virtual Machine directly into TOLA's sovereign Layer-1 architecture. 

Rather than wrapping an external EVM daemon or relying on an out-of-process bridge, TOLA executes Ethereum smart contracts natively inside `tola-execution` using **REVM 43.0.3**, unified with TOLA's persistent MDBX storage trie, EIP-1559 fee accounting, pipelined BFT consensus, and zero-dependency JSON-RPC 2.0 dispatcher.

---

## 2. Supported Ethereum Transaction Types & Revision Baseline

### 2.1 Supported Envelope Formats
TOLA ingests raw signed Ethereum wire transactions via `eth_sendRawTransaction`, decoding them into canonical `tola_types::Transaction` instances with `Signature::Secp256k1Eth`:

| Type | Specification | Wire Prefix | Payload Description |
|:---:|:---|:---:|:---|
| **Type 0** | Legacy (EIP-155) | `>= 0xc0` | `rlp([nonce, gas_price, gas_limit, to, value, data, v, r, s])`<br>Replay protection via $v = \text{chain\_id} \times 2 + 35 + y\_parity$. |
| **Type 1** | EIP-2930 Access Lists | `0x01` | `0x01 \|\| rlp([chain_id, nonce, gas_price, gas_limit, to, value, data, access_list, y_parity, r, s])`<br>Pre-warms specified accounts (2,400 gas) and storage keys (1,900 gas). |
| **Type 2** | EIP-1559 Dynamic Fee | `0x02` | `0x02 \|\| rlp([chain_id, nonce, max_priority_fee_per_gas, max_fee_per_gas, gas_limit, to, value, data, access_list, y_parity, r, s])`. |

### 2.2 EVM Revision: Shanghai
The virtual machine operates strictly under the **Shanghai** execution specification:
* **EIP-3651 (Warm COINBASE):** Proposer/coinbase address is pre-warmed at transaction initialization (100 gas).
* **EIP-3855 (`PUSH0`):** `0x5f` opcode pushes a 0 constant onto the stack for 2 gas.
* **EIP-3860 (Initcode Metering & Limits):** Contract initcode length is strictly bounded to $49{,}152$ bytes ($48\text{ KB}$), metered at 2 gas per 32-byte chunk.
* **EIP-170 (Contract Code Size Limit):** Runtime bytecode deployed to the state is bounded to $24{,}576$ bytes ($24\text{ KB}$).
* **EIP-2929 / EIP-2930 (Cold/Warm Storage & Access Lists):** Warm storage slot reads cost 100 gas; cold reads cost 2,100 gas. Max access list items capped at 256.

### 2.3 Shanghai Precompiles (`0x01`–`0x09`)
All standard Ethereum Shanghai precompiles evaluate statelessly with mathematical precision against independent cryptographic oracles:
* `0x01` **`ecrecover`**: Secp256k1 ECDSA public key recovery ($v \in \{27, 28\}$, $s < \text{SECP256K1\_N}/2$).
* `0x02` **`SHA-256`**: FIPS 180-2 hash.
* `0x03` **`RIPEMD-160`**: 160-bit hash left-padded to 32 bytes.
* `0x04` **`identity`**: Verbatim memory copy.
* `0x05` **`modexp`** (EIP-198 / EIP-2565): Arbitrary precision big-integer modular exponentiation.
* `0x06` **`ecAdd`** (EIP-196 / EIP-1108): alt_bn128 curve addition.
* `0x07` **`ecMul`** (EIP-196 / EIP-1108): alt_bn128 curve scalar multiplication.
* `0x08` **`ecPairing`** (EIP-197 / EIP-1108): alt_bn128 bilinear pairing check.
* `0x09` **`blake2f`** (EIP-152): Blake2b F-compression function.

---

## 3. Ingestion, Execution Pipeline & State Lifecycle

```
[Web3 Client] 
     │ (eth_sendRawTransaction)
     ▼
[JSON-RPC Dispatcher] ──> [RLP Decode & Secp256k1 Recovery]
     │
     ▼
[Mempool Pool] ──> [Block Assembler]
     │
     ▼
[Sequential / Parallel BlockEngine]
     │
     ├─► [REVM EvmExecutor (Shanghai)] ──> [Gas & Revert Metering]
     │
     ▼
[StateDelta Generation] ──> [Pending Working Cache]
     │
     ▼ (Validator Vote & 2-Chain QC Assembly)
[PersistentStateDB] ──> [MDBX Tables: Accounts, Storage, Code, Receipts]
```

### 3.1 Transaction Ingestion
1. A raw signed hex string arrives via `eth_sendRawTransaction`.
2. `tola_codec::rlp::decode_raw_eth_transaction` parses the envelope (0x02, 0x01, or Legacy RLP).
3. The cryptographic signing hash is computed; the sender's 20-byte address is recovered via `tola_crypto::recover_secp256k1_address`.
4. The transaction enters `tola-mempool::pool::Mempool`, verifying nonces, upfront fee balances, and gas limits.

### 3.2 REVM Execution Dispatch
When a block is constructed or validated, `SequentialBlockEngine` executes transactions:
1. `tx.is_evm()` detects `Signature::Secp256k1Eth`.
2. The transaction is converted to `revm::primitives::TxEnv`:
   - `caller = tx.from`
   - `transact_to = if tx.to.is_none() { TxKind::Create } else { TxKind::Call(tx.to) }`
   - `value = tx.value`
   - `data = tx.data`
   - `gas_limit = tx.gas_limit`
   - `gas_price = tx.effective_gas_price(block_base_fee)`
   - `access_list = tx.access_list`
   - `tx_type = tx.tx_type`
3. State is queried via `EvmStateAdapter`, reading account balances, nonces, code, and storage from `PersistentStateDB`.
4. REVM executes bytecodes, evaluates gas, handles sub-call frames (depth limit 1,024), logs events, and emits a commit delta.

### 3.3 Contract Creation & Address Derivation
* **`CREATE` (Deployment):** Contract address is derived deterministically from the deployer's address and account nonce:
  $$\text{Address} = \text{Keccak-256}(\text{RLP}([ \text{sender}, \text{nonce} ]))[12..32]$$
* **`CREATE2`:** Contract address is derived deterministically from salt and initcode hash:
  $$\text{Address} = \text{Keccak-256}(0xff \parallel \text{sender} \parallel \text{salt} \parallel \text{Keccak-256}(\text{initcode}))[12..32]$$

---

## 4. Contract State, Gas, Nonces, Receipts, and Logs

### 4.1 State Persistence Architecture
State is stored in LMDB/MDBX through `PersistentStateDB`:
* **`Table::Accounts`:** Key: 20-byte `Address`; Value: `AccountState` (`balance`, `nonce`, `code_hash`, `storage_root`, `flags`).
* **`Table::Storage`:** Key: `Address (20B) || StorageSlot (32B)`; Value: `32B StorageValue`.
* **`Table::Code`:** Key: 32-byte `code_hash`; Value: raw runtime bytecode.
* **`Table::Receipts`:** Key: 32-byte `TransactionId`; Value: `Receipt` (`status`, `cumulative_gas_used`, `gas_used`, `logs`, `contract_address`).

### 4.2 Gas & Nonce Accounting
* **Intrinsic Gas:** Calculated as $21{,}000$ ($+32{,}000$ for create) $+ 4 \times \text{zero\_bytes} + 16 \times \text{nonzero\_bytes} + 2 \times \lceil\text{initcode}/32\rceil + 2{,}400 \times N_{\text{addresses}} + 1{,}900 \times N_{\text{keys}}$.
* **Dynamic EIP-1559 Fee:**
  $$\text{EffectivePrice} = \text{BaseFee} + \min(\text{MaxPriorityFeePerGas}, \text{MaxFeePerGas} - \text{BaseFee})$$
* **Exact Supply Invariant:**
  $$\text{UpfrontDebit} = (\text{GasUsed} \times \text{BaseFee})_{\text{Burned}} + (\text{GasUsed} \times (\text{EffectivePrice} - \text{BaseFee}))_{\text{ProposerFee}} + \text{Refund}$$
* **Nonce Advance:** Nonce increments monotonically by 1 upon transaction inclusion, regardless of whether execution succeeded or reverted.

### 4.3 Logs & Bloom Filters
* Smart contract events (`LOG0`–`LOG4`) are captured in the execution receipt.
* A 256-byte Ethereum logs bloom filter is computed for each receipt and aggregated into the block header (`BlockHeader::logs_bloom`).
* `eth_getLogs` supports block-tag filtering (`fromBlock`, `toBlock`), address filters, and topic matching (`topics[0]..topics[3]`).

---

## 5. Consensus Integration & Canonical State Commit Boundary

### 5.1 Speculative Proposal Validation vs. Canonical Commit
1. **Proposal Phase (Speculative):** When a block proposal is received, the node re-executes all transactions speculatively in a volatile working cache.
2. **Commitment Checks:** The node independently verifies:
   - `state_root == block.header.state_root`
   - `txs_root == block.header.txs_root`
   - `receipts_root == block.header.receipts_root`
   - `gas_used == block.header.gas_used`
3. **No Premature Persistence:** If the block proposal is invalid or rejected, speculative state changes are discarded. **No database write occurs.**
4. **Canonical Commit Boundary:** State deltas, account balances, contract storage, runtime code, headers, and receipts are committed to persistent MDBX tables **only when a valid Quorum Certificate (QC)** satisfies Jolteon's 2-chain commit rule.

---

## 6. Developer Connectivity & SDK Support

### 6.1 JSON-RPC 2.0 Interface
Standard Ethereum JSON-RPC clients (MetaMask, Foundry, Hardhat, ethers.js, viem) connect directly to `http://<node-ip>:<rpc-port>`:

* **Ledger Queries:** `eth_chainId`, `eth_blockNumber`, `net_version`, `web3_clientVersion`, `eth_gasPrice`
* **Account Queries:** `eth_getBalance`, `eth_getTransactionCount`, `eth_getCode`, `eth_getStorageAt`
* **Execution & Simulation:** `eth_call`, `eth_estimateGas`, `eth_sendRawTransaction`
* **Block & Tx Lookup:** `eth_getBlockByNumber`, `eth_getBlockByHash`, `eth_getTransactionByHash`, `eth_getTransactionReceipt`
* **Logs & Filters:** `eth_getLogs`, `eth_newFilter`, `eth_getFilterChanges`, `eth_uninstallFilter`

### 6.2 Official SDKs
* **TypeScript SDK (`@tola/sdk`):**
  Located under `sdk/typescript`. Provides `TolaWallet`, `JsonRpcProvider`, contract interaction utilities, and secp256k1 signing.
* **Python SDK (`tola`):**
  Located under `sdk/python`. Provides equivalent client bindings, wallet derivation, and RPC providers.

---

## 7. Running the Node & Executing the End-to-End Smoke Test

### 7.1 Compiling the Node Binary
```powershell
cargo build -p tola-node
```
The compiled executable is produced at `target/debug/tola-node.exe` (or `target/release/tola-node.exe`).

### 7.2 Running a Standalone Local Node
```powershell
target/debug/tola-node.exe `
  --chain-id 102 `
  --data-dir ./tmp-node-data `
  --p2p-addr 127.0.0.1:39303 `
  --rpc-addr 127.0.0.1:39545 `
  --validator `
  --genesis ./test-genesis.json `
  --log-level info
```

### 7.3 Executing the 9-Stage End-to-End Smoke Test
The end-to-end smoke test verifies the full lifecycle against a live node daemon:
```powershell
node scratch/smoke_test.mjs
```
The smoke test exercises:
1. Node process boot and health check.
2. JSON-RPC basic methods (`eth_chainId`, `eth_blockNumber`).
3. Account generation and EIP-1559 funding transaction.
4. Value transfer between accounts, receipt status `0x1`, gas and nonce checks.
5. Compilation of `SmokeTest.sol` with `solc 0.8.24` and deployment to a new contract address.
6. Reading slot 0 via `eth_call` and `eth_getStorageAt`, mutating via `setValue(42)`, and verifying updated storage.
7. Exact mathematical gas and balance reconciliation.
8. Process termination, cold node restart with identical database, and persistence verification.
9. Post-restart transaction execution (`setValue(100)`).

---

## 8. Unsupported Features & Post-Shanghai Boundaries

The TOLA EVM Core is explicitly pinned to the **Shanghai** revision. The following post-Shanghai features are **unimplemented** by the current engine:

1. **Cancun / Dencun EVM Opcodes:**
   - `MCOPY` (`0x5e`): Memory copy opcode (reverts as invalid opcode).
   - `TSTORE` / `TLOAD` (`0x5c` / `0x5d`): Transient storage opcodes (reverts as invalid opcode).
   - `BLOBHASH` (`0x49`): Blob hash opcode (reverts as invalid opcode).
   - `BLOBBASEFEE` (`0x4a`): Blob base fee opcode (reverts as invalid opcode).
2. **Blob Transaction Envelope (Type 3):** EIP-4844 transactions (`0x03` envelope with blob commitments) are not recognized or accepted.
3. **Set Code Transaction Envelope (Type 4):** EIP-7702 transactions (`0x04` envelope) are unsupported.
4. **Prague / Electra Features:** EOF (EVM Object Format) and related Prague proposals are not implemented.

> **Compilation Requirement:** When using modern Solidity compilers (`solc >= 0.8.24`), developers **must** explicitly pass `--evm-version shanghai` to prevent the compiler from emitting Cancun-specific opcodes (`MCOPY`, `TSTORE`).

---

## 9. EVM Core Completion vs. Whole-Network Production Readiness

It is essential to distinguish between **EVM Core Completion** and **Whole-Network Production Readiness**:

* **EVM Core Completion (Achieved):**
  - Single-node execution, state trie persistence, and JSON-RPC query fidelity are 100% complete and verified.
  - Transactions execute deterministically, contracts deploy, receipts persist, and restart recovery operates without data loss.
* **Whole-Network Production Readiness (Future Work):**
  - **Multi-Validator Consensus Hash Fork Gate:** While block hashing now cryptographically binds all header fields, protocol-version activation gates for rolling upgrades across mixed-version validator networks remain a whole-network hardening task.
  - **P2P Gossip Scale:** GossipSub mesh stability under hundreds of geographically distributed nodes.
  - **Validator Slashing & Jailing:** Slashing enforcement under malicious double-signing in live networks.
  - **Cross-Shard State Proofs:** Merkle inclusion proofs across separate shards for cross-shard token bridges.
