# SUBSYSTEM: System Architecture & Layered Specification

## Objective
Define the overarching, modular architecture of TOLA Network, establishing strict boundary interfaces between P2P, consensus, execution, storage, RPC, and client application layers.

## Why It Exists
Monolithic blockchains are notoriously fragile, difficult to test, and impossible to upgrade safely. A clean layered architecture allows independent evolution, fuzz testing, and component replacement without protocol hard forks.

## Dependencies
- `prompts/00-project-foundation.md`
- `prompts/01-protocol-research.md`

## Non-Goals
- Microservice decomposition of the validator node (core node must remain a high-performance single daemon).
- Hardcoding specific third-party client integrations into the protocol.

## Requirements
- Five-layer modular architecture: Layer 0 (Networking), Layer 1 (Consensus), Layer 2 (State & Execution), Layer 3 (Interfaces & Tooling), Layer 4 (Ecosystem & Clients).
- Deterministic interaction interfaces between consensus and execution (ABCI-like abstraction).
- Strict protocol decoupling: The core node has zero awareness of messaging or social features.

## Architecture
```
Layer 4: Decoupled Clients (TOLA Messenger, Web Wallets, Explorer)
                         │ (TOLA Connect / E2EE)
Layer 3: Interfaces (JSON-RPC 2.0, WebSockets, Client SDKs)
                         │
Layer 2: State & Execution (VM Runner, Mempool, State DB)
                         │ (Consensus-Execution Interface)
Layer 1: Consensus (BFT Engine, Validator Set, Slashing)
                         │
Layer 0: Networking (libp2p, GossipSub, Noise Encryption)
```

## Interfaces
- Core Protocol Interface: `ConsensusEngine <-> ExecutionEngine` via state transition functions.
- State Storage Interface: `StateDB` trait for abstract key-value lookups.
- P2P Interface: `NetworkService` pub-sub event bus.

## Data Structures
```rust
pub trait ExecutionEngine: Send + Sync {
    fn validate_transaction(&self, tx: &Transaction) -> Result<(), TxValidationError>;
    fn execute_block(&self, block: &Block, state: &mut StateDB) -> Result<ExecutionResult, ExecError>;
    fn compute_state_root(&self) -> Hash256;
}
```

## Security Requirements
- Strict isolation of untrusted network inputs from state execution logic.
- Defense-in-depth: Memory safety (Rust), input bounds checking, gas metering.

## Failure Modes
- Tight coupling causing memory leaks across subsystem boundaries.
- Deadlocks in async message passing between consensus and mempool.

## Testing Requirements
- Architecture validation via dependency graph linting (`cargo-deny` / crate isolation).
- End-to-end integration mocks between layers.

## Performance Requirements
- Inter-layer dispatch overhead must be $< 100\mu s$ per block.

## Migration Requirements
- Modular layers allow VM engine upgrades without changing BFT consensus engine.

## Acceptance Criteria
- Formal system architecture overview published (`architecture/system-overview.md`).
- Core layered traits and lifecycle states defined.

## Open Questions
- Should we use a customized ABCI-rs interface or design a zero-allocation native Rust trait bridge?

## Implementation Tasks
1. Draft `architecture/system-overview.md`.
2. Define trait boundaries for consensus, execution, and networking.
3. Establish error handling and backpressure propagation contracts.

## Completion Criteria
Architectural overview document reviewed, approved, and integrated with `PROCESS.md`.
