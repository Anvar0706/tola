# SUBSYSTEM: Native Asset Economics & Supply Schedule

## Objective
Design the native currency of TOLA Network (symbol: **TOLA**), including denomination units, initial genesis supply, emission schedule, and inflation/deflation dynamics.

## Why It Exists
The native asset powers network security through Proof-of-Stake bonding, pays for computational gas and storage, and serves as the medium of exchange across the decentralized ecosystem.

## Dependencies
- `prompts/04-account-model.md`
- `prompts/15-staking.md`
- `prompts/19-gas-and-fees.md`

## Non-Goals
- Hardcoding speculative token prices or algorithmic stablecoin mechanics.

## Requirements
- Denomination & Precision:
  - 1 TOLA = $10^{18}$ base units (named **nano-tola** or **tol**).
  - Precision permits sub-cent micropayments in the messenger ecosystem.
- Genesis Supply: Fixed initial allocation defined in `genesis.json` (e.g. 1,000,000,000 TOLA).
- Emission Schedule: Predictable disinflationary staking reward schedule:
  - Initial annual inflation (e.g. 5%) decaying smoothly toward a terminal floor (e.g. 1.5%).
- Deflationary Burn Mechanism: 100% of the EIP-1559 base fee is permanently burned, creating net deflationary pressure during high network utilization.

## Architecture
```
Genesis Allocation + Block Staking Emission - Base Fee Burns = Circulating Supply
```

## Interfaces
```rust
pub trait EconomicModel {
    fn compute_block_reward(&self, height: u64, total_staked: u128) -> u128;
    fn get_circulating_supply(&self) -> u128;
    fn record_burned_fees(&mut self, amount: u128);
}
```

## Data Structures
```rust
pub struct EconomicParameters {
    pub genesis_supply: u128,
    pub initial_annual_inflation_rate_bp: u16, // basis points (500 = 5%)
    pub decay_rate_bp: u16,
    pub terminal_inflation_rate_bp: u16,
    pub blocks_per_year: u64,
}
```

## Security Requirements
- Supply conservation invariant: At every block height $H$, $\sum \text{Balances} + \text{TotalStaked} + \text{TotalBurned} = \text{GenesisSupply} + \sum_{i=1}^H \text{BlockRewards}_i$.
- Math safety: All balance arithmetic must use checked or safe 128-bit/256-bit operations; integer overflow panics the node immediately.

## Failure Modes
- Runaway inflation due to erroneous block reward calculation.
- Precision truncation causing gradual balance leakage.

## Testing Requirements
- Long-term simulation tests modeling 50 years of block emissions and fee burns.
- Economic invariant verification in every testnet block transition.

## Performance Requirements
- Block reward calculation $O(1)$, $< 1\mu s$.

## Migration Requirements
- Economic parameters adjustable via formal community governance votes.

## Acceptance Criteria
- Token economic paper documented in `docs/protocol/native-asset.md`.
- Invariant assertions implemented in execution test suites.

## Open Questions
- What percentage of block rewards should be allocated to a decentralized ecosystem development treasury?

## Implementation Tasks
1. Model emission curves in mathematical simulation notebooks.
2. Implement supply conservation check assertions.
3. Author genesis allocation distribution parser.

## Completion Criteria
Economic specification completed, validated against inflation models, and integrated into Genesis config.
