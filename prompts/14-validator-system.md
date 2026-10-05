# SUBSYSTEM: Validator Registry & Committee Lifecycle

## Objective
Design the validator registration, active committee selection, key management, and lifecycle state machine for TOLA Network.

## Why It Exists
Validators maintain network integrity and security by proposing blocks and signing consensus votes. The validator subsystem governs who can participate in consensus and how voting power is distributed.

## Dependencies
- `prompts/13-consensus.md`
- `prompts/15-staking.md`

## Non-Goals
- Delegator rewards accounting (governed by `prompts/15-staking.md`).

## Requirements
- Minimum Self-Stake: Enforce minimum stake threshold to register as a candidate validator.
- Active Set Sizing: Dynamically select the top $K$ validators (e.g. $K=100$) ranked by total effective stake.
- Epoch Transitions: Validator sets are rotated at epoch boundaries (e.g., every 3,600 blocks / ~1 hour).
- Dual-Key Architecture:
  - **Operator Key**: Used for administrative functions, fee recipient updates, and governance.
  - **Consensus Signing Key**: Isolated, high-security Ed25519 key used exclusively for block and vote signing.

## Architecture
```
Candidate Registration -> Stake Accumulation -> Active Set Selection (Epoch N)
                       -> Consensus Participation -> Jail / Unbonding / Ejection
```

## Interfaces
```rust
pub trait ValidatorRegistry {
    fn register_validator(&mut self, req: RegisterRequest) -> Result<(), RegistryError>;
    fn get_active_set(&self, epoch: u64) -> Vec<ValidatorInfo>;
    fn update_voting_power(&mut self, epoch: u64);
    fn jail_validator(&mut self, addr: &Address, duration_blocks: u64);
}
```

## Data Structures
```rust
pub struct ValidatorInfo {
    pub operator_address: Address,
    pub consensus_pubkey: [u8; 32],
    pub total_stake: u128,
    pub commission_rate_basis_points: u16,
    pub status: ValidatorStatus, // Active | Inactive | Jailed
    pub jailing_height: Option<u64>,
}
```

## Security Requirements
- Anti-Double-Signing Guard: Validator software must maintain local persistent disk state of highest signed height/round to prevent equivocation even after crash reboot.
- Remote Signer Support: Consensus signing key must support isolation via external HSM / KMS over mutual TLS.

## Failure Modes
- Accidental dual-instance deployment of a single validator causing immediate self-slashing.
- Validator offline during consensus proposal leading to missed block penalties.

## Testing Requirements
- Unit tests for epoch transitions and active set sorting.
- Jailing and unjailing lifecycle integration tests.

## Performance Requirements
- Active set recalculation $< 5\text{ ms}$ at epoch boundary.

## Migration Requirements
- Dynamic expansion of active set size parameter via governance without hard forks.

## Acceptance Criteria
- Validator system specification documented in `docs/validators/validator-system.md`.
- Anti-double-signing local persistence specification ratified.

## Open Questions
- What is the optimal active committee size for Phase 8 ($N=50$ vs $N=100$)?

## Implementation Tasks
1. Define validator lifecycle states and transitions.
2. Author validator registration and key rotation specs.
3. Design remote signer communication interface.

## Completion Criteria
Validator subsystem specification approved, state transitions verified, and security guards defined.
