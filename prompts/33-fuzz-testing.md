# SUBSYSTEM: Fuzz Testing & Differential Verification

## Objective
Design and implement the automated fuzz testing harness, mutation testing pipeline, and differential execution testing framework across all critical TOLA Network entry points.

## Why It Exists
Unit tests only exercise expected code paths. Fuzz testing feeds billions of mutated, adversarial inputs into parsers, VMs, and state machines to uncover memory corruptions, infinite loops, and consensus desynchronizations.

## Dependencies
- `prompts/08-serialization.md`
- `prompts/17-execution-engine.md`
- `prompts/18-smart-contract-vm.md`

## Non-Goals
- Fuzzing third-party compiler internals.

## Requirements
- Target Engines: `libFuzzer` via `cargo-fuzz` and `AFL++`.
- Fuzzing Targets:
  1. **Canonical Deserialization**: Transaction, block header, block body, and P2P wire messages.
  2. **EVM / VM Bytecode Runner**: Arbitrary opcode sequence generation, gas calculation, and memory boundary tests.
  3. **Address & Cryptographic Parsers**: Bech32m decoding, signature verification, and pubkey decompression.
  4. **State Trie Manipulation**: Random insert, delete, and rollback operations on Sparse Merkle Tree.
- Differential Fuzzing: Compare TOLA EVM execution outputs against reference Ethereum clients (e.g. `geth` / `revm`) on identical inputs.

## Architecture
```
Corpus Generator -> Mutation Engine (AFL / libFuzzer) -> Target Function
                 -> Sanitizer Monitor (ASan / MSan) -> Crash Detected
                 -> Minimized Test Case -> Regression Suite
```

## Interfaces
```rust
// Fuzz target entrypoint contract
pub fn fuzz_target_deserialize_tx(data: &[u8]) {
    if let Ok(tx) = Transaction::from_canonical_bytes(data) {
        let serialized = tx.to_canonical_bytes();
        let tx2 = Transaction::from_canonical_bytes(&serialized).unwrap();
        assert_eq!(tx, tx2);
    }
}
```

## Data Structures
```rust
pub struct FuzzReport {
    pub target_name: String,
    pub total_executions: u64,
    pub crashes_found: usize,
    pub coverage_branches: usize,
}
```

## Security Requirements
- Sanitizers enabled: All fuzz campaigns compiled with AddressSanitizer (ASan) and MemorySanitizer (MSan) to detect silent memory corruption.
- Deterministic reproduction: Every crash must automatically output a minimized reproducer artifact.

## Failure Modes
- Fuzzer stalling on deep nested structures (mitigated by custom dictionary and grammar generation).
- False positives in differential fuzzing due to known specification divergences.

## Testing Requirements
- Continuous 24/7 fuzzing runs on dedicated CI workers achieving $> 100,000,000$ iterations per target.

## Performance Requirements
- Target execution speed $> 2,000$ executions/second per CPU core.

## Migration Requirements
- Fuzz corpus continuously versioned and expanded in Git LFS or artifact storage.

## Acceptance Criteria
- Fuzzing harnesses integrated into `fuzz/` directory.
- Zero open crash artifacts across all active fuzz targets.

## Open Questions
- What coverage-guided fuzzing tools should be used for P2P network state machines?

## Implementation Tasks
1. Initialize `cargo-fuzz` workspace.
2. Author fuzz targets for transaction and block deserialization.
3. Author differential VM execution fuzzer.
4. Establish CI nightly fuzzing pipeline.

## Completion Criteria
Fuzzing pipeline operating, achieving high branch coverage, with zero unresolved crash reports.
