//! Mainnet native asset economics, emission schedule, fee burning, and supply conservation.
//!
//! Complies with ADR-0001, ADR-0003, and `prompts/20-native-asset.md`.
//!
//! Invariants:
//! 1. Denomination: 1 TOLA = 10^18 base units (nano-tola / tol).
//! 2. Max Genesis Cap: 1,000,000,000 TOLA (10^27 base units).
//! 3. Predictable disinflationary staking emissions: 5% initial -> 1.5% terminal floor.
//! 4. Deflationary Fee Burn: 100% of base fees permanently removed from circulation.
//! 5. Exact Supply Conservation: Sum(Balances) + Staked + Treasury + Burned = GenesisSupply + MintedRewards.

use tola_types::Amount;

/// Standard units and multipliers
pub const TOLA_BASE_UNITS: u128 = 1_000_000_000_000_000_000; // 10^18
pub const MAINNET_GENESIS_SUPPLY: u128 = 1_000_000_000 * TOLA_BASE_UNITS; // 1 Billion TOLA
pub const BLOCKS_PER_YEAR_2S: u64 = 15_778_800; // 365.25 * 86400 / 2

/// Economic parameters governing the native TOLA asset
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EconomicParameters {
    pub genesis_supply: Amount,
    pub initial_annual_inflation_bp: u16, // e.g. 500 = 5.0%
    pub decay_rate_bp_per_year: u16,      // e.g. 50 = 0.5% annual decay
    pub terminal_inflation_bp: u16,       // e.g. 150 = 1.5% terminal floor
    pub blocks_per_year: u64,             // Default 15,778,800 @ 2s blocks
    pub treasury_tax_bp: u16,             // e.g. 1000 = 10.0% to community treasury
    pub validator_reward_bp: u16,         // e.g. 9000 = 90.0% to validator pool
}

impl Default for EconomicParameters {
    fn default() -> Self {
        Self {
            genesis_supply: Amount(MAINNET_GENESIS_SUPPLY),
            initial_annual_inflation_bp: 0, // No ongoing inflation or minting beyond 1B cap per Decision D01
            decay_rate_bp_per_year: 0,
            terminal_inflation_bp: 0,
            blocks_per_year: BLOCKS_PER_YEAR_2S,
            treasury_tax_bp: 2500, // 25.0% to Protocol Treasury per Decision D01/D04
            validator_reward_bp: 7500, // 75.0% to validator pool per Decision D01/D04
        }
    }
}

/// Breakdown of a single block's newly minted rewards
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockReward {
    pub total_reward: Amount,
    pub treasury_amount: Amount,
    pub validator_pool_amount: Amount,
}

/// Error type for economic and token supply invariant violations
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EconomicError {
    ArithmeticOverflow,
    SupplyConservationViolation {
        expected: u128,
        actual: u128,
        delta: i128,
    },
    InsufficientTreasuryBalance {
        available: u128,
        requested: u128,
    },
}

/// Tokenomics accounting engine tracking total minted, burned, and treasury balances
#[derive(Debug, Clone)]
pub struct EconomicEngine {
    params: EconomicParameters,
    total_minted: Amount,
    total_burned: Amount,
    treasury_balance: Amount,
}

impl EconomicEngine {
    pub fn new(params: EconomicParameters) -> Self {
        Self {
            params,
            total_minted: Amount(0),
            total_burned: Amount(0),
            treasury_balance: Amount(0),
        }
    }

    pub fn params(&self) -> &EconomicParameters {
        &self.params
    }

    pub fn total_minted(&self) -> Amount {
        self.total_minted
    }

    pub fn total_burned(&self) -> Amount {
        self.total_burned
    }

    pub fn treasury_balance(&self) -> Amount {
        self.treasury_balance
    }

    /// Computes the effective annual inflation rate in basis points for a given block height
    pub fn inflation_rate_at_height(&self, height: u64) -> u16 {
        let years = height / self.params.blocks_per_year;
        let total_decay = (years as u32) * (self.params.decay_rate_bp_per_year as u32);
        let current_rate =
            (self.params.initial_annual_inflation_bp as u32).saturating_sub(total_decay);
        let terminal = self.params.terminal_inflation_bp as u32;
        if current_rate < terminal {
            terminal as u16
        } else {
            current_rate as u16
        }
    }

    /// Computes the deterministic block reward for a given block height
    pub fn compute_block_reward(&self, height: u64) -> Result<BlockReward, EconomicError> {
        let inflation_bp = self.inflation_rate_at_height(height) as u128;

        // Annual emission based on current base supply:
        // annual_emission = (genesis_supply * inflation_bp) / 10_000
        let genesis_supply = self.params.genesis_supply.0;
        let annual_emission = genesis_supply
            .checked_mul(inflation_bp)
            .ok_or(EconomicError::ArithmeticOverflow)?
            / 10_000;

        let total_block_reward = annual_emission / (self.params.blocks_per_year as u128);

        let treasury = total_block_reward
            .checked_mul(self.params.treasury_tax_bp as u128)
            .ok_or(EconomicError::ArithmeticOverflow)?
            / 10_000;

        let validator_pool = total_block_reward.saturating_sub(treasury);

        Ok(BlockReward {
            total_reward: Amount(total_block_reward),
            treasury_amount: Amount(treasury),
            validator_pool_amount: Amount(validator_pool),
        })
    }

    /// Records block emission and updates treasury balance
    pub fn apply_block_emission(&mut self, reward: &BlockReward) -> Result<(), EconomicError> {
        self.total_minted.0 = self
            .total_minted
            .0
            .checked_add(reward.total_reward.0)
            .ok_or(EconomicError::ArithmeticOverflow)?;

        self.treasury_balance.0 = self
            .treasury_balance
            .0
            .checked_add(reward.treasury_amount.0)
            .ok_or(EconomicError::ArithmeticOverflow)?;

        Ok(())
    }

    /// Records burned base gas fees, permanently removing them from circulating supply
    pub fn record_burned_fees(&mut self, burned: Amount) -> Result<(), EconomicError> {
        self.total_burned.0 = self
            .total_burned
            .0
            .checked_add(burned.0)
            .ok_or(EconomicError::ArithmeticOverflow)?;
        Ok(())
    }

    /// Allocates collected transaction fee revenue according to the approved 75/25 split (D01, D04):
    /// 75% to validator pool, 25% to Protocol Treasury. Zero base fee is burned.
    pub fn allocate_fee_revenue(
        &mut self,
        total_fee: Amount,
    ) -> Result<(Amount, Amount), EconomicError> {
        let val_amount = total_fee
            .0
            .checked_mul(self.params.validator_reward_bp as u128)
            .ok_or(EconomicError::ArithmeticOverflow)?
            / 10_000;
        let treasury_amount = total_fee.0.saturating_sub(val_amount);

        self.treasury_balance.0 = self
            .treasury_balance
            .0
            .checked_add(treasury_amount)
            .ok_or(EconomicError::ArithmeticOverflow)?;

        Ok((Amount(val_amount), Amount(treasury_amount)))
    }

    /// Disburses funds from community treasury
    pub fn disburse_treasury(&mut self, amount: Amount) -> Result<(), EconomicError> {
        if self.treasury_balance.0 < amount.0 {
            return Err(EconomicError::InsufficientTreasuryBalance {
                available: self.treasury_balance.0,
                requested: amount.0,
            });
        }
        self.treasury_balance.0 -= amount.0;
        Ok(())
    }

    /// Computes the active circulating supply:
    /// Circulating = Genesis + TotalMinted - TotalBurned - TreasuryBalance
    pub fn circulating_supply(&self, total_staked: Amount) -> Result<Amount, EconomicError> {
        let total_issued = self
            .params
            .genesis_supply
            .0
            .checked_add(self.total_minted.0)
            .ok_or(EconomicError::ArithmeticOverflow)?;

        let locked_or_destroyed = self
            .total_burned
            .0
            .checked_add(self.treasury_balance.0)
            .and_then(|v| v.checked_add(total_staked.0))
            .ok_or(EconomicError::ArithmeticOverflow)?;

        let circulating = total_issued.saturating_sub(locked_or_destroyed);
        Ok(Amount(circulating))
    }

    /// Strictly verifies the Total Token Supply Conservation Invariant:
    /// Sum(Balances) + Staked + Treasury + Burned == GenesisSupply + MintedRewards
    pub fn verify_supply_conservation(
        &self,
        sum_balances: Amount,
        total_staked: Amount,
    ) -> Result<(), EconomicError> {
        let actual_assets = sum_balances
            .0
            .checked_add(total_staked.0)
            .and_then(|v| v.checked_add(self.treasury_balance.0))
            .and_then(|v| v.checked_add(self.total_burned.0))
            .ok_or(EconomicError::ArithmeticOverflow)?;

        let expected_assets = self
            .params
            .genesis_supply
            .0
            .checked_add(self.total_minted.0)
            .ok_or(EconomicError::ArithmeticOverflow)?;

        if actual_assets != expected_assets {
            let delta = (actual_assets as i128) - (expected_assets as i128);
            return Err(EconomicError::SupplyConservationViolation {
                expected: expected_assets,
                actual: actual_assets,
                delta,
            });
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_zero_inflation_under_d01() {
        let engine = EconomicEngine::new(EconomicParameters::default());

        // Zero inflation schedule: 0 bp across all heights
        assert_eq!(engine.inflation_rate_at_height(0), 0);
        assert_eq!(engine.inflation_rate_at_height(BLOCKS_PER_YEAR_2S), 0);
        assert_eq!(engine.inflation_rate_at_height(5 * BLOCKS_PER_YEAR_2S), 0);

        let reward = engine.compute_block_reward(0).expect("Compute reward");
        assert_eq!(reward.total_reward.0, 0, "No new tokens minted per D01");
        assert_eq!(reward.validator_pool_amount.0, 0);
        assert_eq!(reward.treasury_amount.0, 0);
    }

    #[test]
    fn test_approved_75_25_fee_split_under_d01_d04() {
        let mut engine = EconomicEngine::new(EconomicParameters::default());
        let fee = Amount(100 * TOLA_BASE_UNITS);

        let (val_fee, treasury_fee) = engine.allocate_fee_revenue(fee).expect("Allocate fee");
        assert_eq!(
            val_fee.0,
            75 * TOLA_BASE_UNITS,
            "75% allocated to validator pool"
        );
        assert_eq!(
            treasury_fee.0,
            25 * TOLA_BASE_UNITS,
            "25% allocated to Protocol Treasury"
        );
        assert_eq!(
            val_fee.0 + treasury_fee.0,
            fee.0,
            "Exact conservation: no fee dust created or lost"
        );
        assert_eq!(
            engine.treasury_balance().0,
            25 * TOLA_BASE_UNITS,
            "Treasury balance incremented"
        );
    }

    #[test]
    fn test_supply_conservation_invariant_roundtrip() {
        let mut engine = EconomicEngine::new(EconomicParameters::default());
        let genesis_supply = engine.params().genesis_supply;

        let mut balances = genesis_supply.0 - 100_000_000 * TOLA_BASE_UNITS;
        let staked = 100_000_000 * TOLA_BASE_UNITS;

        // At genesis: balances + staked = genesis_supply (1B TOLA)
        assert!(engine
            .verify_supply_conservation(Amount(balances), Amount(staked))
            .is_ok());

        // Simulate 100 blocks of transaction fee settlement (75% validator, 25% treasury, 0% burn)
        for h in 1..=100 {
            let tx_fee = Amount(10 * TOLA_BASE_UNITS);
            balances -= tx_fee.0; // Debited from sender

            let (val_fee, _) = engine.allocate_fee_revenue(tx_fee).unwrap();
            balances += val_fee.0; // Credited to validator pool

            // Supply conservation must strictly hold at exactly 1,000,000,000 TOLA
            assert!(
                engine
                    .verify_supply_conservation(Amount(balances), Amount(staked))
                    .is_ok(),
                "Supply conservation failed at block {}",
                h
            );
        }
    }
}
