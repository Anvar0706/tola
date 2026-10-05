# SUBSYSTEM: Consensus Engine & BFT Finality

## Objective
Design and implement the high-assurance Proof-of-Stake consensus engine with deterministic Byzantine Fault Tolerant (BFT) finality for TOLA Network.

## Why It Exists
Consensus is the core mechanism enabling untrusted, geographically distributed nodes to achieve infallible agreement on the canonical order and validity of blocks.

## Dependencies
- `prompts/03-cryptography.md`
- `prompts/10-block-engine.md`
- `prompts/12-p2p.md`
- `prompts/14-validator-system.md`

## Non-Goals
- Proof-of-Work mining.
- Probabilistic Nakamoto longest-chain forks.

## Requirements
- Protocol Family: **Deterministic BFT Consensus** (Tendermint/HotStuff paradigm).
- Fault Tolerance: Safely tolerates up to $f < n/3$ Byzantine or crashed nodes.
- Quorum Rule: Proposals and commits require $\ge 2f + 1$ voting power.
- Two-Phase Voting: Pre-vote (Prepare) and Pre-commit (Commit) rounds per block height.
- Deterministic Finality: Once a block is committed with $\ge 2/3 + 1$ signatures, it can NEVER be reorganized.
- Proposer Selection: Weighted round-robin based on active bonded validator stake.

## Architecture
```
Consensus State Machine (per Height H, Round R):
NewRound -> ProposeBlock -> Prevote (Gossip) -> Precommit (Gossip)
         -> Commit (2/3+1 Quorum) -> Next Height
(Timeout at any stage triggers Round Step-Up with Pacemaker)
```

## Interfaces
```rust
pub trait ConsensusEngine {
    fn handle_proposal(&mut self, proposal: BlockProposal) -> Result<(), ConsensusError>;
    fn handle_prevote(&mut self, vote: Vote) -> Result<(), ConsensusError>;
    fn handle_precommit(&mut self, vote: Vote) -> Result<(), ConsensusError>;
    fn on_timeout(&mut self, timeout_type: TimeoutType);
}
```

## Data Structures
```rust
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Vote {
    pub vote_type: VoteType, // Prevote | Precommit
    pub height: u64,
    pub round: u32,
    pub block_hash: [u8; 32],
    pub validator_address: Address,
    pub signature: [u8; 64],
}

pub struct CommitCertificate {
    pub height: u64,
    pub block_hash: [u8; 32],
    pub signatures: Vec<VoteSignature>,
}
```

## Security Requirements
- Equivocation (double-signing) detection: Emits immediate cryptographic slashing evidence.
- Nothing-at-Stake prevention: Slashing ensures that voting on two conflicting proposals at the same height/round destroys validator stake.
- Liveness under partition: Network halts safely if $< 2/3$ stake is reachable, preserving state safety.

## Failure Modes
- Stalled round due to offline proposer (pacemaker triggers timeout and advances round).
- Asynchronous Byzantine collusion attempting double spend.

## Testing Requirements
- Jepsen-style formal consensus safety verification under simulated network splits.
- Byzantine node injection tests (nodes proposing invalid blocks or double-voting).

## Performance Requirements
- Consensus round completion latency $< 1.0\text{ second}$ with 100 active validators.

## Migration Requirements
- Fork-free consensus parameter adjustments via on-chain governance epoch updates.

## Acceptance Criteria
- Formal consensus specification documented in `docs/consensus/bft-engine.md`.
- Safety and liveness proofs verified against BFT literature.

## Open Questions
- Should we use HotStuff's 3-phase pipelined commit or Tendermint's 2-phase lock mechanism?

## Implementation Tasks
1. Specify state machine round transitions and timeout rules.
2. Define vote message structures and BLS/Ed25519 signature formats.
3. Formulate slashing evidence ingestion pipeline.

## Completion Criteria
Consensus specification validated, state transitions mathematically proven, and test scenarios mapped.
