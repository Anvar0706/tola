# ADR-007: Inter-Contract Call Semantics & Proxy Standards

## Status
ACCEPTED (TOLA EVM Phase 6)

## Context
Complex smart contract architectures (decentralized exchanges, lending protocols, governance systems, and upgradeable contracts) depend on inter-contract execution primitives (`CALL`, `STATICCALL`, `DELEGATECALL`) and deterministic contract creation (`CREATE`, `CREATE2`). TOLA must provide complete Ethereum semantic parity for these primitives.

## Decision
1. **Execution Authority:** **REVM 43.0.3** is the sole execution authority governing call stack frames, execution context switching, and internal gas forwarding (following the EIP-150 63/64th gas rule).
2. **Call Opcode Semantics:**
   - **`CALL` (0xF1):** Creates a child execution frame targeting the callee contract. Transfers specified value between accounts, executes callee bytecode in the callee's storage context, and sets `msg.sender` to the caller. State mutations are permitted.
   - **`STATICCALL` (0xFA):** Creates a read-only child frame. Any attempt by the callee or its descendants to execute state-mutating opcodes (`SSTORE`, `LOG0`–`LOG4`, `CREATE`, `CREATE2`, `SELFDESTRUCT`) halts the frame with `StateChangeDuringStaticCall` and reverts.
   - **`DELEGATECALL` (0xF4):** Executes the target logic contract's code strictly within the **caller's address and storage context**. Preserves the original `msg.sender` and `msg.value`. Enables ERC-1967, UUPS, and Transparent Upgradeable Proxy patterns.
3. **Revert Propagation:**
   - A revert in a child frame (via `REVERT` or out-of-gas) rolls back all state mutations made within that frame and returns `false` on the stack to the calling contract.
   - Revert data is made available to the caller via `RETURNDATASIZE` and `RETURNDATACOPY`.
   - The calling contract decides whether to handle the failure or bubble the revert upward.
4. **Proxy Storage Isolation:**
   - Under `DELEGATECALL`, mutations affect only the storage of the proxy (caller). The logic contract's storage remains unmutated.
   - Both proxy and logic contract state persist atomically across block commits and node restarts.
5. **Contract Creation within Execution:**
   - Contracts may deploy child contracts via `CREATE` (incrementing deployer nonce) or `CREATE2` (using deployer address, salt, and initcode hash).
   - All newly created contracts are committed with `FLAG_CONTRACT` into `Table::Accounts` and their code into `Table::Code`.

## Consequences
- Full compatibility with OpenZeppelin proxy libraries (ERC-1967, BeaconProxy, UUPSUpgradeable).
- Deterministic cross-contract composability matching Ethereum mainnet.
