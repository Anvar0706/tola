# SUBSYSTEM: Canonical Serialization & Wire Protocols

## Objective
Design and implement the high-performance, deterministic canonical binary serialization format for all TOLA Network data structures (transactions, blocks, state nodes, P2P messages).

## Why It Exists
Non-deterministic serialization produces differing byte hashes for identical semantic data, splitting consensus. Inefficient serialization inflates P2P bandwidth and drags down block propagation times.

## Dependencies
- `prompts/06-transaction-model.md`
- `prompts/07-state-model.md`

## Non-Goals
- Human-readable serialization on the wire (JSON/YAML is restricted strictly to RPC endpoints).

## Requirements
- Deterministic: Identical data structures must ALWAYS encode to the exact same byte sequence.
- Zero-Copy Deserialization: Maximize memory efficiency during high-throughput transaction decoding.
- Compact Wire Footprint: Minimize byte padding.
- Candidate Formats: Scale Codec (Parity), Borsh (Near/Solana), or custom compact binary encoding.

## Architecture
```
Rust In-Memory Struct <──Zero-Copy Borrow──> Canonical Byte Buffer (Wire / DB)
```

## Interfaces
```rust
pub trait CanonicalSerialize {
    fn serialize_canonical<W: std::io::Write>(&self, writer: &mut W) -> Result<(), SerializeError>;
    fn to_canonical_bytes(&self) -> Vec<u8>;
}

pub trait CanonicalDeserialize: Sized {
    fn deserialize_canonical<R: std::io::Read>(reader: &mut R) -> Result<Self, DeserializeError>;
    fn from_canonical_bytes(bytes: &[u8]) -> Result<Self, DeserializeError>;
}
```

## Data Structures
```rust
pub struct CompactVarInt(pub u64); // Variable-length integer encoding (1 to 9 bytes)
```

## Security Requirements
- Bounded deserialization: Enforce strict recursion depth limits and maximum allocation caps to prevent memory exhaustion (Zip-bomb style attacks).
- Strict trailing byte validation: Reject any payload containing unparsed trailing bytes.

## Failure Modes
- Deserialization panic caused by crafted out-of-bounds inputs.
- Ambiguous integer encoding breaking hash determinism.

## Testing Requirements
- Continuous fuzz testing using `cargo-fuzz` (libFuzzer) on all deserialization entry points.
- Round-trip invariant testing: $\forall X, \text{Deserialize}(\text{Serialize}(X)) == X$.

## Performance Requirements
- Deserialization throughput $> 500\text{ MB/sec}$.

## Migration Requirements
- Schema evolvability via explicit version tags in message envelopes.

## Acceptance Criteria
- Wire protocol specification documented in `docs/protocol/serialization.md`.
- Zero fuzz panics across 10,000,000 generated inputs.

## Open Questions
- Is Borsh preferred over Scale codec for our Rust ecosystem integration?

## Implementation Tasks
1. Benchmarking candidate codecs.
2. Formalizing canonical encoding rules for variable-length integers and collections.
3. Configuring continuous fuzzing harness.

## Completion Criteria
Serialization specification completed, benchmarked, and committed with zero fuzz crash reports.
