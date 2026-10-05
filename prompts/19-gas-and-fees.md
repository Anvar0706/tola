# SUBSYSTEM: Gas Accounting & Dynamic Fee Market

## Objective
Design the computational gas schedule, storage pricing, and dynamic fee market for TOLA Network, providing anti-spam resistance and predictable transaction costs.

## Why It Exists
Without an economic metering system, an attacker could halt the blockchain with infinite loops or exhaust node disk space with trivial spam. The fee market dynamically balances supply and demand.

## Dependencies
- `prompts/06-transaction-model.md`
- `prompts/18-smart-contract-vm.md`

## Non-Goals
- Free or unmetered contract execution.

## Requirements
- Deterministic Metering: Every computational opcode, memory allocation, and storage write consumes an exact quantity of gas.
- Dynamic Fee Market: EIP-1559 style two-tier pricing:
  - **Base Fee**: Dynamically adjusts per block based on block fullness. Burned 100% to combat inflation.
  - **Priority Fee (Tip)**: Paid directly to the proposing validator to prioritize time-sensitive transactions.
- Micropayment Viability: Base transfers must cost fractions of a cent while retaining sufficient cost to make sustained spam attacks economically infeasible ($> \$50,000/\text{day}$ to saturate blocks).
- State Expansion Deposit: Allocating new storage slots requires an up-front refundable byte deposit.

## Architecture
```
Block Fullness > Target Capacity (50%) -> Base Fee Increases (up to +12.5%)
Block Fullness < Target Capacity (50%) -> Base Fee Decreases (up to -12.5%)
Total Tx Fee = GasUsed * BaseFee (Burned) + GasUsed * PriorityFee (Validator)
```

## Interfaces
```rust
pub trait FeeMarket {
    fn calculate_base_fee(&self, parent_block: &BlockHeader) -> u128;
    fn verify_gas_limit(&self, block_gas_limit: u64, tx_gas_limit: u64) -> Result<(), FeeError>;
}
```

## Data Structures
```rust
pub struct FeeConfig {
    pub target_gas_per_block: u64,
    pub max_gas_per_block: u64,
    pub base_fee_change_denominator: u64, // e.g. 8 (12.5% max change)
    pub min_base_fee: u128,
}
```

## Security Requirements
- Mathematical bounding: Base fee adjustments cannot jump more than $\pm 12.5\%$ per block, preventing fee manipulation attacks.
- Underpriced operations: Regular benchmarking of all opcodes to ensure gas price matches actual CPU execution time.

## Failure Modes
- Rapid fee spikes during sudden traffic surges priced out low-value micropayments.
- Spam saturation when base fee hits minimum floor.

## Testing Requirements
- Dynamic fee adjustment simulations over 1,000 blocks under varying load ($0\%$, $50\%$, $100\%$ full).
- Storage pricing stress tests.

## Performance Requirements
- Base fee calculation $< 5\mu s$ per block header.

## Migration Requirements
- Configurable fee parameters via on-chain governance proposals.

## Acceptance Criteria
- Gas and fee model documented in `docs/protocol/gas-and-fees.md`.
- Dynamic fee formula verified against historical surge models.

## Open Questions
- Should social/messenger micro-transactions enjoy a dedicated subsidized priority lane via cryptographic vouchers?

## Implementation Tasks
1. Formulate dynamic base fee algorithm.
2. Implement opcode gas counter and out-of-gas trap.
3. Specify storage deposit and refund rules.

## Completion Criteria
Gas model specification approved and integrated into transaction execution pipeline.
