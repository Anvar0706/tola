# ADR-0016: Historical EVM State & Block-Tag Query Architecture

## Status
ACCEPTED (TOLA EVM Phase 8)

## Context
Phases 0 through 7 (P0–P7) established TOLA's EVM execution engine (REVM 43.0.3, Shanghai revision), unified StateDB persistence, transaction envelopes, receipt persistence, log filters, WebSocket subscriptions, tooling RPCs, hand-crafted bytecode compatibility, and real Solidity / OpenZeppelin v5.0.2 / Shanghai precompiles execution.

Prior to Phase 8, JSON-RPC state queries (`eth_getBalance`, `eth_getCode`, `eth_getStorageAt`, `eth_getTransactionCount`) and call simulations (`eth_call`, `eth_estimateGas`) evaluated exclusively against the latest committed state (`HEAD`).

Developer tooling (Foundry, Hardhat, Tenderly, viem, ethers.js, Web3.py, MetaMask) and institutional indexing pipelines (The Graph, indexing subgraphs, analytics nodes) require query fidelity across arbitrary historical block heights and block tags:
1. Historical accounts and balances at block $N$;
2. Historical contract code at block $N$ (distinguishing pre-deployment from post-deployment);
3. Historical contract storage slots at block $N$ across multiple mutations and zeroing events;
4. Historical `eth_call` simulation executing inside the exact block context of block $N$ (`block.number`, `block.timestamp`, `block.basefee`, `block.coinbase`, and `BLOCKHASH` window);
5. Named block tags (`latest`, `earliest`, `pending`, `safe`, `finalized`), explicit hex/dec heights, and EIP-1898 object queries;
6. Strict rejection of future blocks ($H > H_{\text{latest}}$) without silent fallback.

All historical fidelity requirements must be satisfied while maintaining TOLA's core invariants:
- **Zero Second EVM Database**: EVM state and native TOLA state remain strictly unified.
- **Single Canonical State Root**: Consensus state root remains authoritative; no shadow trees or secondary roots.
- **Constant-Time / Logarithmic Reads**: Historical queries must not replay $O(N)$ blocks sequentially.
- **Atomic Persistence**: Block bundles commit state, history, headers, bodies, receipts, and indexes in a single atomic batch.

---

## Decision

### 1. Unified Storage Architecture & Zero Second EVM DB
TOLA preserves its single unified `StateDB` architecture:
- **Historical Accounts via Jellyfish Merkle Tree (JMT):** Account states (`balance`, `nonce`, `code_hash`, `flags`) are stored in immutable, copy-on-write 16-ary JMT nodes in `Table::TreeNodes`, content-addressed by BLAKE3 hash. Because tree nodes are never overwritten or pruned, any historical block header's `state_root` points to an immutable, fully traversable account trie at height $N$.
- **Historical Storage via `Table::StorageHistory`:** Contract storage slots in `Table::Storage` are flat and unversioned for optimal consensus write throughput. Historical values are recorded in `Table::StorageHistory` (`"storage_history"`).
  - **Key Structure (60 bytes):**
    $$\text{Key} = \text{Address (20 bytes)} \,\|\, \text{Slot (32 bytes)} \,\|\, \text{Height (8 bytes Big-Endian)}$$
  - **Value Structure (32 bytes):** Storage slot value (`Hash`).
  - **Lookup Semantics:** To retrieve slot $S$ of address $A$ at height $H$, the engine forms the maximum key $(A, S, H_{\text{BE}})$ and performs a reverse-range seek (`get_prev_or_equal`). If the resulting key prefix matches $(A, S)$, the recorded value is the authoritative slot value at height $H$. If no matching key exists, the slot evaluates to `Hash::ZERO`. Lookups complete in $O(\log \text{history})$ time.
- **Explicit Zero-Value Storage Persistence:** When a contract clears a storage slot to zero (`Hash::ZERO`), `Table::Storage` deletes the flat entry, but `Table::StorageHistory` explicitly records `Hash::ZERO` at height $H$. This ensures historical queries accurately distinguish cleared slots from slots that were never written.

### 2. Block-Tag Resolution & EIP-1898 Specification
All RPC endpoints accepting block tags (`eth_getBalance`, `eth_getCode`, `eth_getStorageAt`, `eth_getTransactionCount`, `eth_call`, `eth_estimateGas`) resolve parameters through `RpcDispatcher::resolve_block_tag`:
1. **Named Tags:**
   - `"latest"`, `"safe"`, `"finalized"`: Resolve to the latest committed block height $H_{\text{latest}}$.
   - `"pending"`: Resolves to $H_{\text{latest}}$ (committed state), with mempool pending overlays applied for nonce tracking where applicable.
   - `"earliest"`: Resolves to height 0 (genesis).
2. **Explicit Numbers:**
   - Hexadecimal strings (`"0x0"`, `"0x1"`, ...) and decimal strings/integers.
3. **EIP-1898 Object Parameter:**
   - `{ "blockNumber": "0x..." }`
   - `{ "blockHash": "0x...", "requireCanonical": bool }`: Validates that the requested hash exists and (if `requireCanonical` is true) matches the canonical chain at that height.
4. **Future Block Rejection:**
   - Any query where $H > H_{\text{latest}}$ returns standard RPC error `-32000` (`"header not found: block height X exceeds latest height Y"`). Silent fallback to latest is strictly prohibited.

### 3. Historical Simulation Context & REVM Invariants
When executing `eth_call` or `eth_estimateGas` at historical block $N$:
- **Context Resolution:** The block header at height $N$ is retrieved from `Table::Headers`. The simulation environment is configured with:
  - `block.number = N`
  - `block.timestamp = header.timestamp`
  - `block.basefee = header.base_fee_per_gas`
  - `block.coinbase = header.proposer`
- **Historical Database Adapter (`RevmReadOnlyAdapter`):**
  - Account queries resolve via `state.get_historical_account(BlockHeight(N), addr)`.
  - Storage queries resolve via `state.get_historical_storage(BlockHeight(N), addr, slot)`.
  - Code queries resolve via `state.get_code(&acct.code_hash)` (immutable content-addressing).
- **BLOCKHASH Opcode Fidelity:**
  - Evaluated strictly relative to target block $N$.
  - For query height $k$:
    - If $k \ge N$: Returns `0x0` (EVM forbids querying current or future blocks).
    - If $k < N$ and $k \ge N.saturating_sub(256)$: Returns canonical hash of block $k$.
    - If $k < N.saturating_sub(256)$: Returns `0x0` (outside the 256-block window).

### 4. Cold Restart Invariance & Atomic Bundle Persistence
- `PersistentStateDB::open` re-hydrates `current_height` from `Table::Metadata` key `"latest_height"`.
- `RpcDispatcher::new` initializes its head tracker from `state_db.current_height()`, eliminating cold restart desynchronization.
- All block writes execute in `commit_block_bundle` via a single `StorageBatch`:
  - `Table::Accounts`
  - `Table::Storage`
  - `Table::StorageHistory`
  - `Table::Code`
  - `Table::TreeNodes` (JMT nodes)
  - `Table::Headers`
  - `Table::BlockBodies`
  - `Table::Transactions` / `Table::EthTxLookup`
  - `Table::Receipts`
  - `Table::Metadata`
  Guarantees zero orphan records or partial state transitions across crashes or restarts.

---

## Consequences

### Positive
- **Full Historical Query Fidelity:** Developers can query arbitrary historical blocks for balances, nonces, bytecodes, storage slots, and simulated calls.
- **Tooling Compatibility:** Full compatibility with Hardhat, Foundry, The Graph, and MetaMask historical queries.
- **Zero Consensus Overhead:** Historical state reads never trigger consensus re-execution or block replay.
- **Architectural Integrity:** Preserves the unified TOLA StateDB; avoids second database synchronization hazards.

### Negative / Trade-Offs
- **Storage Growth:** `Table::StorageHistory` appends 60-byte keys and 32-byte values on each modified storage slot. This is standard and required for archive node fidelity. (Pruning policies for full vs. archive nodes will be governed by future node configuration phases).
