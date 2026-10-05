# SUBSYSTEM: Protocol Research & Comparative Analysis

## Objective
Conduct thorough, objective technical research across modern Layer-1 and Layer-2 architectures (including TON, Ethereum, Solana, Cosmos/Tendermint, Polkadot, Aptos/Sui) to evaluate consensus, execution, networking, and data structures for TOLA Network.

## Why It Exists
To avoid "reinventing the wheel" poorly. Protocol decisions must be grounded in peer-reviewed cryptography, proven distributed systems literature, and empirical performance data.

## Dependencies
- `prompts/00-project-foundation.md`

## Non-Goals
- Blindly copying proprietary or closed implementations.
- Prematurely standardizing on unproven academic cryptographic schemes.

## Requirements
- Produce formal comparative trade-off matrices for consensus, virtual machines, storage engines, and P2P layers.
- Evaluate developer ergonomics, ecosystem tooling, auditability, and node resource requirements.
- Analyze TON's asynchronous actor model vs synchronous composability.

## Architecture
Research pipeline:
Literature Review -> Benchmark Analysis -> Trade-Off Matrix Formulation -> Architectural Decision Record (ADR).

## Interfaces
- Research documents authored in `research/` and `docs/research/`.
- Markdown benchmark reports.

## Data Structures
Comparative evaluation schema:
```rust
pub struct ArchitectureComparison {
    pub subsystem: String,
    pub candidate_systems: Vec<String>,
    pub latency_profile: LatencyMetrics,
    pub throughput_profile: ThroughputMetrics,
    pub security_assumptions: Vec<String>,
    pub developer_friction: FrictionLevel,
    pub recommendation: String,
}
```

## Security Requirements
- Verify mathematical proofs of liveness and safety for candidate consensus protocols.
- Analyze vulnerability histories of evaluated virtual machines and serialization formats.

## Failure Modes
- Selecting a protocol based on marketing TPS claims rather than real-world Byzantine fault tolerance.
- Adopting a virtual machine paradigm that alienates developers.

## Testing Requirements
- Benchmarking candidate libraries (e.g. RocksDB vs MDBX; revm vs Wasmer) on standardized hardware.

## Performance Requirements
- Comparative benchmarks must specify exact CPU, RAM, disk IOPS, and OS environments.

## Migration Requirements
- N/A (Research Phase).

## Acceptance Criteria
- Comprehensive research papers published on:
  1. Consensus mechanisms (`research/01-consensus-mechanisms.md`)
  2. VM & execution environments (`research/02-vm-and-execution.md`)
  3. P2P networking protocols (`research/03-networking-p2p.md`)
  4. Storage engines (`research/04-storage-engines.md`)
  5. State sharding paradigms (`research/05-scalability-and-sharding.md`)
  6. TON architecture and decoupling principles (`docs/research/ton-integration-research.md`).

## Open Questions
- Does pipelined HotStuff offer measurable latency advantages over Tendermint under typical validator counts ($N=100$)?

## Implementation Tasks
1. Evaluate consensus algorithms (PBFT, Tendermint, HotStuff, Narwhal-Bullshark).
2. Evaluate VM alternatives (EVM, WASM, Actor TVM).
3. Evaluate P2P stacks (libp2p, devp2p).
4. Evaluate DB engines (RocksDB, Pebble, MDBX).
5. Document findings in dedicated markdown research files.

## Completion Criteria
All research dossiers completed with explicit trade-off matrices and clear engineering recommendations.
