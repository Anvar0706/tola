# SUBSYSTEM: Smart Contract Virtual Machine

## Objective
Design the sandboxed virtual machine runtime for executing decentralized smart contracts, with initial full EVM compatibility and an extensible architecture for secondary runtimes.

## Why It Exists
Smart contracts enable autonomous applications, tokens, collectibles, and identity registries. Choosing a widely adopted execution environment (EVM) allows TOLA to bootstrap an immediate developer ecosystem.

## Dependencies
- `prompts/17-execution-engine.md`
- `prompts/19-gas-and-fees.md`

## Non-Goals
- Proprietary bytecode formats that alienate existing developer communities.

## Requirements
- Foundation: Modular EVM runtime (utilizing the battle-tested Rust `revm` engine).
- Compatibility: Support standard Ethereum opcodes, EVM Cancun/Prague specification, Solidity, Vyper, and standard tooling (Hardhat, Foundry).
- Storage Access: Contract storage integrated with TOLA's high-performance Sparse Merkle Tree state backend.
- Gas Metering: Strict opcode gas pricing matching standard EVM economics, supplemented by custom storage costs.
- Precompiled Contracts: Support standard EVM precompiles (`ecrecover`, `sha256`, `ripemd160`, `bn256`, `blake2f`) plus native TOLA extensions.

## Architecture
```
Contract Bytecode -> Bytecode Analysis -> EVM Stack/Memory Runtime
                 -> Gas Metering -> Opcode Dispatch -> State Access
                 -> Receipts & Event Logs
```

## Interfaces
```rust
pub trait VirtualMachine {
    fn deploy(
        &self,
        state: &mut dyn StateDB,
        deployer: Address,
        bytecode: &[u8],
        gas_limit: u64,
    ) -> Result<DeployResult, VMError>;

    fn call(
        &self,
        state: &mut dyn StateDB,
        caller: Address,
        contract: Address,
        value: u128,
        calldata: &[u8],
        gas_limit: u64,
    ) -> Result<CallResult, VMError>;
}
```

## Data Structures
```rust
pub struct CallResult {
    pub exit_reason: VMExitReason,
    pub gas_consumed: u64,
    pub output: Vec<u8>,
    pub logs: Vec<LogEntry>,
}

pub enum VMExitReason {
    Success,
    Revert,
    OutOfGas,
    InvalidOpcode,
    StackOverflow,
}
```

## Security Requirements
- Sandboxing: Complete isolation from the host filesystem, network, and system memory.
- Call depth limit: Enforce maximum stack depth (1,024 frames) to prevent host thread stack exhaustion.
- Re-entrancy protection at the virtual machine dispatch boundary.

## Failure Modes
- Infinite loop exploits (halted deterministically by gas exhaustion).
- Gas estimation discrepancies between client and validator nodes.

## Testing Requirements
- Full execution of the official Ethereum State Tests suite (`ethereum/tests`).
- Fuzzing with malformed bytecode and edge-case memory expansions.

## Performance Requirements
- Smart contract execution $> 3,000$ complex contract calls/sec.

## Migration Requirements
- Pluggable VM trait allows deploying a secondary high-performance WASM runtime (e.g., Wasmer / Wasmtime) alongside EVM.

## Acceptance Criteria
- Virtual machine specification documented in `docs/smart-contracts/vm-specification.md`.
- 100% pass rate on core EVM state transition test suite.

## Open Questions
- What custom TOLA precompiles (e.g. Ed25519 verification, ZK Poseidon hash) should be exposed at Genesis?

## Implementation Tasks
1. Integrate `revm` crate into execution harness.
2. Map EVM storage slots to TOLA Sparse Merkle Tree state.
3. Author custom TOLA precompiles.

## Completion Criteria
EVM integration verified against official test suite with zero discrepancies.
