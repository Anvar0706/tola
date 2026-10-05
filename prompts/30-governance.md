# SUBSYSTEM: On-Chain Governance & Protocol Parameters

## Objective
Design the decentralized governance mechanism for TOLA Network, enabling token-weighted proposal submission, community voting, and automated execution of parameter updates.

## Why It Exists
Blockchains must adapt over time without fracturing the community into chaotic hard forks. Governance provides a transparent, decentralized decision-making framework.

## Dependencies
- `prompts/04-account-model.md`
- `prompts/15-staking.md`
- `prompts/31-upgradeability.md`

## Non-Goals
- Governance micro-management of individual smart contracts or account freezes (censorship resistance must be preserved).

## Requirements
- Parameter Updates: Dynamically update protocol parameters (e.g. gas limits, base fee targets, slashing percentages, unbonding periods) without node re-compilation.
- Lifecycle:
  1. **Proposal Submission**: Requires a minimum TOLA proposal deposit to prevent spam.
  2. **Voting Period**: Fixed voting window (e.g. 7 days). Voting power proportional to bonded stake.
  3. **Quorum & Threshold**: Requires $\ge 40\%$ quorum of total stake and $> 50\%$ affirmative votes (or $> 66.7\%$ for critical constitutional changes).
  4. **Timelock**: Approved proposals enter a mandatory 48-hour timelock before on-chain execution to allow dissenting participants to exit.

## Architecture
```
Deposit Period -> Active Voting (7 Days) -> Quorum & Tally Check
               -> Approved -> Timelock (48h) -> Automated Parameter Execution
```

## Interfaces
```rust
pub trait GovernanceEngine {
    fn submit_proposal(&mut self, proposal: Proposal) -> Result<u64, GovError>;
    fn cast_vote(&mut self, proposal_id: u64, voter: Address, vote: VoteOption) -> Result<(), GovError>;
    fn execute_proposal(&mut self, proposal_id: u64) -> Result<(), GovError>;
}
```

## Data Structures
```rust
pub struct Proposal {
    pub id: u64,
    pub proposer: Address,
    pub title: String,
    pub description: String,
    pub parameter_changes: Vec<ParamChange>,
    pub deposit: u128,
    pub start_block: u64,
    pub end_block: u64,
    pub status: ProposalStatus,
}

pub enum VoteOption {
    Yes,
    No,
    NoWithVeto,
    Abstain,
}
```

## Security Requirements
- Flash-loan attack prevention: Voting power is strictly snapshot at proposal start block height; flash-borrowed tokens cannot participate.
- Veto protection: "NoWithVeto" votes exceeding 33.4% reject proposals and burn spam deposits.

## Failure Modes
- Low voter turnout failing to meet quorum on critical emergency parameter adjustments.
- Malicious proposal passing due to apathy (mitigated by 48-hour timelock).

## Testing Requirements
- Simulation tests of voting scenarios: passing, failing, vetoing, and timelock cancellations.
- Snapshot balance verification invariant tests.

## Performance Requirements
- Vote tally calculation $< 5\text{ ms}$ at voting conclusion block.

## Migration Requirements
- Governance contract upgradeable via self-referential governance proposals.

## Acceptance Criteria
- Governance specification documented in `docs/protocol/governance.md`.
- Security invariants against flash loan attacks verified.

## Open Questions
- Should delegators inherit validator votes automatically with an option to override?

## Implementation Tasks
1. Specify proposal lifecycle state machine.
2. Define parameter dictionary and execution hooks.
3. Formulate timelock queue contracts.

## Completion Criteria
Governance specification approved, threat model analyzed, and state transitions verified.
