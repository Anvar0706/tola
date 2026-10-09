# ADR-006: Solidity Smart Contract Execution & Storage Architecture

## Status
ACCEPTED (TOLA EVM Phase 6)

## Context
TOLA is an independent Layer-1 blockchain supporting native consensus and high-throughput transactions. To support the global Web3 smart contract ecosystem without bifurcating developer workflows or state storage, TOLA requires an Ethereum-compatible smart contract execution environment that integrates natively into its consensus-controlled state machine.

## Decision
1. **Execution Engine:** Adopt **REVM 43.0.3** configured to `SpecId::SHANGHAI` as the definitive EVM execution engine. All opcode execution, gas metering, call depth boundaries (1024), stack limits (1024), and initcode metering (EIP-3860) are governed strictly by REVM.
2. **Unified StateDB:** Smart contracts execute against TOLA's single consensus-controlled `StateDB`. There is **no separate EVM database** and **no separate EVM state root**. All validator nodes reach consensus on a single state root covering both native accounts and EVM contract accounts.
3. **Account Representation:** Contract accounts are stored in `Table::Accounts` with `flags & FLAG_CONTRACT != 0`, retaining standard `nonce`, `balance`, `code_hash`, and `storage_root` fields.
4. **Bytecode Storage:** Contract bytecode is persisted in `Table::Code`, indexed by its 32-byte Keccak-256 code hash (`Hash`).
5. **Contract Storage:** Contract storage slots modified by `SSTORE` are persisted in `Table::Storage`, indexed by `(Address, Hash)` (contract address and 32-byte slot key).
6. **Creation Semantics:**
   - **CREATE (Top-Level & Opcode 0xF0):** Derives address via `keccak256(rlp([sender, nonce]))[12..32]`.
   - **CREATE2 (Opcode 0xF5):** Derives address via `keccak256(0xff ++ deployer ++ salt ++ keccak256(init_code))[12..32]`.
7. **Persistence & Restart Parity:** Block execution deltas (`StateDelta`) are committed atomically with block bodies, headers, receipts, and indexes via `PersistentStateDB::commit_block_bundle`. State recovers with byte-for-byte fidelity upon node restart.
8. **Token Standards as User-Space Bytecode:** ERC-20, ERC-721, and ERC-1155 are executed as ordinary Solidity bytecode inside REVM. **No native ERC subsystems exist.** Native TLS-721 digital asset management remains independent.
9. **Canonical BLOCKHASH:** `BLOCKHASH(N)` resolves against TOLA's canonical committed block history via `StateDB::get_block_hash_by_height`. Adheres strictly to the EVM 256-block visibility window (current block, future blocks, and blocks older than 256 return zero).
10. **PREVRANDAO Semantics:** Under Shanghai, `PREVRANDAO` (opcode 0x44) evaluates deterministically to zero (`B256::ZERO`), matching the absence of an Ethereum Beacon Chain RANDAO beacon in TOLA's BFT consensus.

## Consequences
- Developers can deploy ordinary Solidity contracts (compiled with solc 0.8.x targeting Shanghai) using standard tools (Foundry, Hardhat, Remix).
- Total supply conservation, state proofs, and block rollback mechanisms operate uniformly across native and EVM state.
- No storage duplication or consensus divergence.
