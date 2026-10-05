# SUBSYSTEM: Staking, Delegation & Slashing Mechanics

## Objective
Design the economic staking system, delegation pool, reward distribution, unbonding locks, and slashing penalty enforcement for TOLA Network.

## Why It Exists
Proof-of-Stake derives its security from capital at risk. Staking aligns economic incentives: honest validators and delegators earn staking yields, while malicious actors face severe slashing of their bonded capital.

## Dependencies
- `prompts/14-validator-system.md`
- `prompts/20-native-asset.md`

## Non-Goals
- Liquid staking derivatives (LSD) protocols (left to ecosystem smart contract developers).

## Requirements
- Delegation: Token holders can delegate native TOLA to active validators without giving up custody.
- Unbonding Period: Withdrawing bonded stake requires an unbonding delay (e.g. 14 or 21 days) to prevent long-range attacks.
- Reward Distribution: Block rewards and transaction fees distributed proportionally to bonded stake minus validator commission.
- Slashing Rules:
  - **Equivocation (Double Signing)**: Severe penalty (e.g. $5\% - 20\%$ stake burned, permanent jailing).
  - **Liveness Fault (Downtime)**: Minor penalty (e.g. $0.01\%$ stake burned or temporary jailing without slashing).

## Architecture
```
Delegator Bonds TOLA -> Added to Validator Stake Pool -> Earns Pro-Rata Rewards
                    -> Requests Unbond -> 14-Day Lockup -> Claim Released Funds
```

## Interfaces
```rust
pub trait StakingEngine {
    fn delegate(&mut self, delegator: Address, validator: Address, amount: u128) -> Result<(), StakingError>;
    fn undelegate(&mut self, delegator: Address, validator: Address, amount: u128) -> Result<UndelegateReceipt, StakingError>;
    fn distribute_block_rewards(&mut self, proposer: Address, fees: u128);
    fn execute_slashing(&mut self, evidence: SlashingEvidence) -> Result<u128, SlashingError>;
}
```

## Data Structures
```rust
pub struct DelegationRecord {
    pub delegator: Address,
    pub validator: Address,
    pub shares: u128,
}

pub struct UnbondingEntry {
    pub delegator: Address,
    pub amount: u128,
    pub completion_height: u64,
}

pub struct SlashingEvidence {
    pub height: u64,
    pub round: u32,
    pub vote_a: Vote,
    pub vote_b: Vote,
}
```

## Security Requirements
- Weak Subjectivity enforcement: Node sync checkpoints must be more recent than the unbonding period.
- Re-entrancy prevention on undelegation claims.

## Failure Modes
- Slashing cascade due to buggy validator client update.
- Rounding dust accumulation in proportional reward calculation.

## Testing Requirements
- Math invariant tests: Total distributed rewards must exactly match inflation schedule plus transaction fees.
- Slashing verification tests against genuine and forged equivocation evidence.

## Performance Requirements
- Reward distribution state updates must be $O(1)$ per block (using pool share accounting).

## Migration Requirements
- Unbonding duration and slashing percentages configurable via governance.

## Acceptance Criteria
- Staking specification documented in `docs/staking/staking-and-slashing.md`.
- Mathematical share-accounting formulas proven against precision loss.

## Open Questions
- Should slashing burn tokens permanently or deposit them into the network community treasury?

## Implementation Tasks
1. Formalize share-based pool math for $O(1)$ reward distribution.
2. Specify unbonding queue data structure.
3. Formulate cryptographic verification of slashing evidence.

## Completion Criteria
Staking and slashing specification approved and mathematically verified.
