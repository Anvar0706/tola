# SUBSYSTEM: Finality Engine & Reorganization Guard

## Objective
Design and implement the deterministic finality rules, commit certificates, light-client finality proofs, and reorganization guards for TOLA Network.

## Why It Exists
In probabilistic blockchains, deep reorganizations can reverse confirmed transactions, resulting in catastrophic double-spending in payment gateways and exchanges. TOLA requires instant, mathematically irrevocable finality.

## Dependencies
- `prompts/13-consensus.md`
- `prompts/14-validator-system.md`

## Non-Goals
- Long-range proof-of-work chain reorganizations.

## Requirements
- Instant Economic Finality: Once a block achieves a quorum certificate ($\ge 2/3 + 1$ validator voting power), it is finalized and irrevocable.
- Reorganization Guard: Node execution engine strictly rejects any alternate branch or reorg attempt rooted before or at a finalized block height.
- Commit Certificate: Compact cryptographic proof containing aggregated signatures that light clients can verify in constant time.
- Weak Subjectivity: Client sync requires a trusted header not older than the unbonding period (e.g. 14 days) to prevent long-range attacks.

## Architecture
```
Block Proposed -> Prevote Round -> Precommit Round
               -> Commit Certificate Generated (>= 2/3 + 1 Signatures)
               -> Irrevocable Finalization -> State Frozen
```

## Interfaces
```rust
pub trait FinalityEngine {
    fn verify_commit_certificate(&self, cert: &CommitCertificate) -> Result<(), FinalityError>;
    fn is_finalized(&self, height: u64, block_hash: &Hash256) -> bool;
    fn get_last_finalized_height(&self) -> u64;
}
```

## Data Structures
```rust
pub struct CommitCertificate {
    pub height: u64,
    pub round: u32,
    pub block_hash: [u8; 32],
    pub validator_bitfield: Vec<u8>,
    pub aggregated_signature: Vec<u8>,
}
```

## Security Requirements
- Mathematical finality: A finalized block cannot be reverted unless $> 1/3$ of total validator stake maliciously signs a conflicting commit, resulting in verifiable slashing.
- Light client verifiability: Finality proofs must be standalone and verifiable without executing transactions.

## Failure Modes
- Network partition causing liveness halt (finality pauses safely; zero divergent forks formed).
- Corrupted commit certificate submitted by malicious peer.

## Testing Requirements
- Chaos tests simulating 30% offline validators (system continues finalization).
- Byzantine tests simulating 34% malicious voting (system halts safely without fork).

## Performance Requirements
- Finality proof verification latency $< 1\text{ ms}$.

## Migration Requirements
- Seamless support for signature aggregation scheme upgrades (e.g. Ed25519 to BLS12-381).

## Acceptance Criteria
- Finality specification documented in `docs/consensus/finality.md`.
- Invariant tests prove zero reorgs occur past finalized height.

## Open Questions
- Is BLS threshold aggregation required immediately or can individual signature bitfields suffice for Phase 8?

## Implementation Tasks
1. Specify commit certificate binary encoding.
2. Author reorganization guard validation logic.
3. Design light-client finality verification endpoint.

## Completion Criteria
Finality engine specification ratified and integrated into block engine pipeline.
