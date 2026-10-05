# SUBSYSTEM: Protocol Versioning & Upgrade Coordination

## Objective
Design the deterministic protocol versioning, fork-free activation signaling, coordinated node software upgrades, and emergency rollback safeguards for TOLA Network.

## Why It Exists
Coordinating decentralized node upgrades without network splits is one of the hardest challenges in distributed systems. An undisciplined upgrade can divide the validator set and destroy consensus.

## Dependencies
- `prompts/13-consensus.md`
- `prompts/14-validator-system.md`
- `prompts/30-governance.md`

## Non-Goals
- Silent or automatic software updates on third-party validator servers.

## Requirements
- Semantic Versioning: Strict SemVer (`MAJOR.MINOR.PATCH`) for node daemon releases.
- On-Chain Upgrade Signaling:
  - Validators signal readiness for an upgrade by encoding the target protocol version in their block proposals.
  - When $\ge 80\%$ of proposals over an evaluation window (e.g. 10,000 blocks) signal readiness, the upgrade is scheduled for an activation height $H_{\text{upgrade}}$.
- Hard Fork vs Soft Fork Rules:
  - **Soft Fork**: Restricts valid blocks; backward compatible.
  - **Hard Fork**: Expands valid blocks; requires non-upgraded nodes to halt safely rather than corrupting local state.
- Rollback & Safeguard Strategy: Node state captures a database snapshot prior to $H_{\text{upgrade}}$ to allow rapid recovery in case of critical bugs.

## Architecture
```
Proposal Approved -> Code Released -> Validator Signaling (>= 80% Threshold)
                  -> Activation Height Scheduled -> Automated Protocol Switch
```

## Interfaces
```rust
pub trait UpgradeManager {
    fn record_signal(&mut self, proposer: Address, version: ProtocolVersion);
    fn check_activation(&self, height: u64) -> Option<ProtocolVersion>;
    fn get_active_protocol_version(&self, height: u64) -> ProtocolVersion;
}
```

## Data Structures
```rust
pub struct ProtocolVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

pub struct UpgradePlan {
    pub target_version: ProtocolVersion,
    pub activation_height: u64,
    pub description: String,
}
```

## Security Requirements
- Replay prevention across forks: Chain ID or transaction envelope version updated if a permanent fork occurs.
- Safe halt: Un-upgraded nodes must log clear error messages and halt execution upon reaching an unhandled hard-fork height without corrupting disk state.

## Failure Modes
- Validator set split due to premature activation without requisite quorum.
- State migration script failure at activation height.

## Testing Requirements
- Multi-node upgrade simulation tests: 10 nodes signaling, reaching threshold, and transitioning to new protocol rules seamlessly at block $H$.

## Performance Requirements
- Version check overhead in block execution loop $< 1\mu s$.

## Migration Requirements
- Every breaking protocol change MUST be accompanied by an approved ADR.

## Acceptance Criteria
- Upgrade coordination specification documented in `docs/protocol/upgradeability.md`.
- Upgrade signaling simulation successfully executed in test harness.

## Open Questions
- What length should the upgrade signaling evaluation window be (24 hours vs 72 hours)?

## Implementation Tasks
1. Define protocol version structures and signaling headers.
2. Implement threshold signaling accumulator.
3. Author emergency snapshot creation hook prior to activation block.

## Completion Criteria
Upgrade specification completed, reviewed against network split scenarios, and approved.
