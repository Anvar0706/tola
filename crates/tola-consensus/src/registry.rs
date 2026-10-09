//! Decentralized validator registry, bonded staking pools, and dynamic committee rotation.
//!
//! Enforces minimum self-stake, share-based pool math for O(1) reward/slash absorption,
//! and unbonding locks against weak subjectivity attacks.

use std::collections::{HashMap, HashSet};
use tola_core::StakingError;
use tola_types::{
    Address, Amount, BlockHeight, Epoch, PublicKey, SlashReceipt, ValidatorId, ValidatorInfo,
    ValidatorSet, ValidatorStatus,
};

/// 10,000 base TOLA minimum self-stake (10,000 * 10^18 attotola) per approved Decision D02.
pub const DEFAULT_MIN_SELF_STAKE: u128 = 10_000 * 1_000_000_000_000_000_000;
pub const DEFAULT_MAX_ACTIVE_VALIDATORS: usize = 100;
pub const DEFAULT_UNBONDING_BLOCKS: u64 = 907_200; // ~21 days at 2.0s blocks per Decision D02
pub const DEFAULT_JAILING_BLOCKS: u64 = 1_296_000; // ~30 days at 2.0s blocks

/// Deterministic validator performance tracking for composite reward scoring (Decision D02).
///
/// Composite score weighting:
/// - 50% uptime (full 5000 bp at >= 99% uptime, 0 below 80% uptime)
/// - 40% successful assigned consensus duties (proposing + voting)
/// - 10% protocol compliance (default 1000 bp)
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidatorPerformance {
    pub duties_assigned: u64,
    pub duties_completed: u64,
    pub blocks_proposed: u64,
    pub missed_proposals: u64,
    pub compliance_score_bp: u16, // Default 1000 bp (10%)
    pub under_status_review: bool,
}

impl Default for ValidatorPerformance {
    fn default() -> Self {
        Self {
            duties_assigned: 0,
            duties_completed: 0,
            blocks_proposed: 0,
            missed_proposals: 0,
            compliance_score_bp: 1000,
            under_status_review: false,
        }
    }
}

impl ValidatorPerformance {
    /// Computes uptime percentage in basis points (10000 = 100.00%).
    pub fn uptime_bp(&self) -> u16 {
        if self.duties_assigned == 0 {
            return 10000;
        }
        let completed = self.duties_completed as u128;
        let assigned = self.duties_assigned as u128;
        ((completed * 10000) / assigned).min(10000) as u16
    }

    /// Evaluates composite performance score (0 to 10,000 basis points) per Decision D02.
    /// Below 80% uptime: returns 0 (no performance reward) and triggers status review.
    pub fn compute_performance_score(&mut self) -> u32 {
        let uptime = self.uptime_bp();
        if uptime < 8000 {
            self.under_status_review = true;
            return 0; // Excluded from performance reward below 80% uptime per Decision D02
        }

        // 1. 50% Uptime component (5,000 bp max)
        let uptime_component = if uptime >= 9900 {
            5000 // Full uptime score at 99% or above per Decision D02
        } else {
            let delta = (uptime - 8000) as u32; // 0 to 1900
            (delta * 5000) / 1900
        };

        // 2. 40% Assigned duties component (4,000 bp max)
        let duty_component = if self.duties_assigned == 0 {
            4000
        } else {
            ((self.duties_completed as u32) * 4000) / (self.duties_assigned as u32)
        };

        // 3. 10% Protocol compliance component (1,000 bp max)
        let compliance_component = (self.compliance_score_bp as u32).min(1000);

        uptime_component + duty_component + compliance_component
    }
}

#[derive(Clone, Debug)]
pub struct ValidatorRecord {
    pub operator_address: Address,
    pub consensus_pubkey: PublicKey,
    pub status: ValidatorStatus,
    pub commission_rate_bp: u16,
    pub total_bonded_stake: Amount,
    pub total_pool_shares: Amount,
    pub self_stake_shares: Amount,
    pub jailing_height: Option<BlockHeight>,
    pub unjail_allowed_height: Option<BlockHeight>,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct UnbondingEntry {
    pub delegator: Address,
    pub validator: ValidatorId,
    pub amount: Amount,
    pub completion_height: BlockHeight,
}

#[derive(Clone, Debug)]
pub struct RegistryConfig {
    pub min_self_stake: Amount,
    pub max_active_validators: usize,
    pub unbonding_delay_blocks: u64,
    pub jailing_duration_blocks: u64,
}

impl Default for RegistryConfig {
    fn default() -> Self {
        Self {
            min_self_stake: Amount(DEFAULT_MIN_SELF_STAKE),
            max_active_validators: DEFAULT_MAX_ACTIVE_VALIDATORS,
            unbonding_delay_blocks: DEFAULT_UNBONDING_BLOCKS,
            jailing_duration_blocks: DEFAULT_JAILING_BLOCKS,
        }
    }
}

pub struct ValidatorRegistry {
    config: RegistryConfig,
    validators: HashMap<ValidatorId, ValidatorRecord>,
    /// Maps (delegator, validator) -> shares.
    delegations: HashMap<(Address, ValidatorId), Amount>,
    /// FIFO queue of unbonding withdrawals.
    unbonding_entries: Vec<UnbondingEntry>,
    /// Deterministic validator performance logs.
    performances: HashMap<ValidatorId, ValidatorPerformance>,
    /// Last height at which 24-hour validator pool settlement occurred.
    pub last_settlement_height: Option<BlockHeight>,
    /// Last timestamp at which 24-hour validator pool settlement occurred.
    pub last_settlement_timestamp: Option<u64>,
}

pub const SETTLEMENT_INTERVAL_BLOCKS: u64 = 43_200; // 24 hours @ 2s blocks
pub const SETTLEMENT_INTERVAL_SECS: u64 = 86_400; // 24 hours in seconds

impl ValidatorRegistry {
    pub fn new(config: RegistryConfig) -> Self {
        Self {
            config,
            validators: HashMap::new(),
            delegations: HashMap::new(),
            unbonding_entries: Vec::new(),
            performances: HashMap::new(),
            last_settlement_height: None,
            last_settlement_timestamp: None,
        }
    }

    pub fn validator_count(&self) -> usize {
        self.validators.len()
    }

    pub fn get_validator(&self, id: &ValidatorId) -> Option<&ValidatorRecord> {
        self.validators.get(id)
    }

    pub fn get_validator_mut(&mut self, id: &ValidatorId) -> Option<&mut ValidatorRecord> {
        self.validators.get_mut(id)
    }

    /// Registers a new candidate validator with self-stake and commission parameters.
    pub fn register_validator(
        &mut self,
        operator: Address,
        consensus_pubkey: PublicKey,
        self_stake: Amount,
        commission_bp: u16,
    ) -> Result<(), StakingError> {
        let val_id = ValidatorId(operator);

        if self.validators.contains_key(&val_id) {
            return Err(StakingError::AlreadyRegistered);
        }

        if self_stake.0 < self.config.min_self_stake.0 {
            return Err(StakingError::InsufficientSelfStake);
        }

        if commission_bp > 10000 {
            return Err(StakingError::InvalidCommissionRate);
        }

        // Initialize validator record with 1:1 shares
        let record = ValidatorRecord {
            operator_address: operator,
            consensus_pubkey,
            status: ValidatorStatus::Candidate,
            commission_rate_bp: commission_bp,
            total_bonded_stake: self_stake,
            total_pool_shares: self_stake,
            self_stake_shares: self_stake,
            jailing_height: None,
            unjail_allowed_height: None,
        };

        self.validators.insert(val_id, record);
        self.delegations.insert((operator, val_id), self_stake);

        Ok(())
    }

    /// Delegates tokens to an existing active or candidate validator.
    ///
    /// Per approved Decision D02: Third-party delegation is disabled in V1.
    /// Only the validator's operator address may add to their self-stake.
    pub fn delegate(
        &mut self,
        delegator: Address,
        validator: ValidatorId,
        amount: Amount,
    ) -> Result<Amount, StakingError> {
        if amount.0 == 0 {
            return Err(StakingError::InsufficientBalance);
        }

        let val = self
            .validators
            .get_mut(&validator)
            .ok_or(StakingError::ValidatorNotFound(validator))?;

        // Decision D02: No delegation in V1
        if delegator != val.operator_address {
            return Err(StakingError::DelegationDisabledInV1);
        }

        if val.status == ValidatorStatus::Jailed {
            return Err(StakingError::ValidatorJailed);
        }

        let shares_minted = if val.total_bonded_stake.0 == 0 || val.total_pool_shares.0 == 0 {
            amount
        } else {
            let num = amount
                .0
                .checked_mul(val.total_pool_shares.0)
                .ok_or(StakingError::MathOverflow)?;
            Amount(num / val.total_bonded_stake.0)
        };

        val.total_bonded_stake = val
            .total_bonded_stake
            .checked_add(amount)
            .ok_or(StakingError::MathOverflow)?;
        val.total_pool_shares = val
            .total_pool_shares
            .checked_add(shares_minted)
            .ok_or(StakingError::MathOverflow)?;
        val.self_stake_shares = val
            .self_stake_shares
            .checked_add(shares_minted)
            .ok_or(StakingError::MathOverflow)?;

        let entry = self
            .delegations
            .entry((delegator, validator))
            .or_insert(Amount(0));
        *entry = entry
            .checked_add(shares_minted)
            .ok_or(StakingError::MathOverflow)?;

        Ok(shares_minted)
    }

    /// Records a consensus duty (vote participation or block proposal) for performance scoring.
    pub fn record_duty(&mut self, validator: &ValidatorId, completed: bool) {
        let perf = self.performances.entry(*validator).or_default();
        perf.duties_assigned = perf.duties_assigned.saturating_add(1);
        if completed {
            perf.duties_completed = perf.duties_completed.saturating_add(1);
        } else {
            perf.missed_proposals = perf.missed_proposals.saturating_add(1);
        }
    }

    /// Records a successful block proposal.
    pub fn record_block_proposed(&mut self, validator: &ValidatorId) {
        let perf = self.performances.entry(*validator).or_default();
        perf.blocks_proposed = perf.blocks_proposed.saturating_add(1);
        perf.duties_assigned = perf.duties_assigned.saturating_add(1);
        perf.duties_completed = perf.duties_completed.saturating_add(1);
    }

    /// Records a protocol compliance incident docking the compliance score (out of 1000 bp).
    pub fn record_compliance_violation(&mut self, validator: &ValidatorId, penalty_bp: u16) {
        let perf = self.performances.entry(*validator).or_default();
        perf.compliance_score_bp = perf.compliance_score_bp.saturating_sub(penalty_bp);
    }

    /// Gets a validator's performance record.
    pub fn get_performance(&self, validator: &ValidatorId) -> Option<&ValidatorPerformance> {
        self.performances.get(validator)
    }

    /// Distributes collected gas fee validator pool rewards among active validators
    /// according to the approved 80% stake / 20% performance model (Decision D02).
    ///
    /// - 80% distributed proportionally to bonded stake.
    /// - 20% distributed proportionally to composite performance score.
    /// - Validators with < 80% uptime receive 0 from the performance component.
    pub fn distribute_pool_rewards(
        &mut self,
        total_pool_fees: Amount,
    ) -> Result<HashMap<ValidatorId, Amount>, StakingError> {
        if total_pool_fees.0 == 0 || self.validators.is_empty() {
            return Ok(HashMap::new());
        }

        let stake_pool = total_pool_fees.0 * 80 / 100;
        let perf_pool = total_pool_fees.0.saturating_sub(stake_pool);

        let mut total_active_stake = 0u128;
        let mut total_perf_score = 0u32;

        let active_ids: Vec<ValidatorId> = self
            .validators
            .iter()
            .filter(|(_, v)| v.status == ValidatorStatus::Active)
            .map(|(id, _)| *id)
            .collect();

        let mut validator_scores = Vec::new();
        for id in &active_ids {
            let stake = self
                .validators
                .get(id)
                .map(|v| v.total_bonded_stake.0)
                .unwrap_or(0);
            let perf = self
                .performances
                .entry(*id)
                .or_default()
                .compute_performance_score();
            total_active_stake = total_active_stake.saturating_add(stake);
            total_perf_score = total_perf_score.saturating_add(perf);
            validator_scores.push((*id, stake, perf));
        }

        if total_active_stake == 0 {
            return Ok(HashMap::new());
        }

        let mut distributions = HashMap::new();
        for (id, stake, perf) in validator_scores {
            let stake_reward = (stake_pool * stake) / total_active_stake;
            let perf_reward = if total_perf_score > 0 {
                (perf_pool * (perf as u128)) / (total_perf_score as u128)
            } else {
                0
            };

            let validator_total = stake_reward + perf_reward;
            if validator_total > 0 {
                if let Some(val) = self.validators.get_mut(&id) {
                    val.total_bonded_stake = val
                        .total_bonded_stake
                        .checked_add(Amount(validator_total))
                        .ok_or(StakingError::MathOverflow)?;
                }
                distributions.insert(id, Amount(validator_total));
            }
        }

        Ok(distributions)
    }

    /// Settles the accumulated gas fees from the canonical Validator Reward Pool
    /// every 24 hours per approved Decisions D01 & D02.
    ///
    /// Rewards are distributed using the approved 80% stake weight / 20% performance score model.
    /// Deducts total distributed amount from the on-chain Validator Reward Pool account and credits
    /// active validators' operator addresses in `state`, strictly conserving total token supply.
    ///
    /// Replay-safe: returns `Err(StakingError::AlreadySettled)` if invoked before 24h have elapsed.
    /// Handles empty pool (0 balance) safely.
    pub fn settle_daily_validator_pool(
        &mut self,
        current_height: BlockHeight,
        current_timestamp: u64,
        state: &mut dyn tola_core::StateDB,
    ) -> Result<Amount, StakingError> {
        // Enforce 24-hour interval
        if let Some(last_h) = self.last_settlement_height {
            if current_height.0 < last_h.0 + SETTLEMENT_INTERVAL_BLOCKS {
                return Err(StakingError::AlreadySettled);
            }
        }
        if let Some(last_ts) = self.last_settlement_timestamp {
            if current_timestamp < last_ts + SETTLEMENT_INTERVAL_SECS {
                return Err(StakingError::AlreadySettled);
            }
        }

        let pool_addr = Address::VALIDATOR_REWARD_POOL;
        let pool_acct = state
            .get_account(&pool_addr)
            .map_err(|_| StakingError::StorageError)?
            .unwrap_or(tola_types::AccountState::EMPTY);

        let total_pool = pool_acct.balance;
        if total_pool.0 == 0 {
            self.last_settlement_height = Some(current_height);
            self.last_settlement_timestamp = Some(current_timestamp);
            return Ok(Amount::ZERO);
        }

        // Distribute pool rewards using 80% stake / 20% performance model
        let distributions = self.distribute_pool_rewards(total_pool)?;
        if distributions.is_empty() {
            self.last_settlement_height = Some(current_height);
            self.last_settlement_timestamp = Some(current_timestamp);
            return Ok(Amount::ZERO);
        }

        let mut dirty_accounts = Vec::new();
        let mut total_distributed = 0u128;

        for (val_id, reward) in &distributions {
            if let Some(record) = self.validators.get(val_id) {
                let op_addr = record.operator_address;
                let mut op_acct = state
                    .get_account(&op_addr)
                    .map_err(|_| StakingError::StorageError)?
                    .unwrap_or(tola_types::AccountState::EMPTY);
                op_acct
                    .credit(*reward)
                    .map_err(|_| StakingError::MathOverflow)?;
                dirty_accounts.push((op_addr, Some(op_acct)));
                total_distributed = total_distributed.saturating_add(reward.0);
            }
        }

        // Debit the exact distributed amount from the validator pool account
        let mut updated_pool_acct = pool_acct;
        updated_pool_acct
            .debit(Amount(total_distributed))
            .map_err(|_| StakingError::MathOverflow)?;
        dirty_accounts.push((pool_addr, Some(updated_pool_acct)));

        dirty_accounts.sort_by_key(|(addr, _)| *addr);

        state
            .apply_delta(tola_core::StateDelta {
                accounts: dirty_accounts,
                storage: vec![],
                code: vec![],
            })
            .map_err(|_| StakingError::StorageError)?;

        self.last_settlement_height = Some(current_height);
        self.last_settlement_timestamp = Some(current_timestamp);

        Ok(Amount(total_distributed))
    }

    /// Requests undelegation of shares, computing unbonded tokens and placing them in the unbonding queue.
    ///
    /// amount_redeemed = (shares * total_bonded_stake) / total_shares
    pub fn undelegate(
        &mut self,
        delegator: Address,
        validator: ValidatorId,
        shares: Amount,
        current_height: BlockHeight,
    ) -> Result<Amount, StakingError> {
        let entry = self
            .delegations
            .get_mut(&(delegator, validator))
            .ok_or(StakingError::InsufficientShares)?;

        if entry.0 < shares.0 {
            return Err(StakingError::InsufficientShares);
        }

        let val = self
            .validators
            .get_mut(&validator)
            .ok_or(StakingError::ValidatorNotFound(validator))?;

        let amount_redeemed = shares
            .0
            .checked_mul(val.total_bonded_stake.0)
            .ok_or(StakingError::MathOverflow)?
            .checked_div(val.total_pool_shares.0)
            .map(Amount)
            .unwrap_or(Amount(0));

        // Deduct from pool
        *entry = entry
            .checked_sub(shares)
            .ok_or(StakingError::MathOverflow)?;
        val.total_pool_shares = val
            .total_pool_shares
            .checked_sub(shares)
            .ok_or(StakingError::MathOverflow)?;
        val.total_bonded_stake = val
            .total_bonded_stake
            .checked_sub(amount_redeemed)
            .ok_or(StakingError::MathOverflow)?;

        // If delegator is the operator, verify self-stake invariant
        if delegator == val.operator_address {
            val.self_stake_shares = val
                .self_stake_shares
                .checked_sub(shares)
                .ok_or(StakingError::MathOverflow)?;
            if val.total_bonded_stake.0 < self.config.min_self_stake.0
                && val.status == ValidatorStatus::Active
            {
                val.status = ValidatorStatus::Inactive;
            }
        }

        // Add to unbonding queue
        let completion_height = BlockHeight(current_height.0 + self.config.unbonding_delay_blocks);
        self.unbonding_entries.push(UnbondingEntry {
            delegator,
            validator,
            amount: amount_redeemed,
            completion_height,
        });

        Ok(amount_redeemed)
    }

    /// Claims matured tokens from the unbonding queue.
    pub fn claim_unbonded(&mut self, delegator: Address, current_height: BlockHeight) -> Amount {
        let mut claimed = Amount(0);
        let mut remaining = Vec::with_capacity(self.unbonding_entries.len());

        for entry in self.unbonding_entries.drain(..) {
            if entry.delegator == delegator && entry.completion_height.0 <= current_height.0 {
                claimed = claimed.checked_add(entry.amount).unwrap_or(claimed);
            } else {
                remaining.push(entry);
            }
        }

        self.unbonding_entries = remaining;
        claimed
    }

    /// Slashes a validator for Byzantine equivocation or protocol violation.
    ///
    /// Deducts `penalty_bp` basis points (e.g. 1000 bp = 10%) from `total_bonded_stake`,
    /// leaves `total_pool_shares` constant (absorbing the loss across all delegators in O(1)),
    /// slashes pending unbonding queues proportionally, and jails the validator for
    /// `jailing_duration_blocks`.
    pub fn slash_validator(
        &mut self,
        validator: &ValidatorId,
        penalty_bp: u16,
        current_height: BlockHeight,
        reason: String,
    ) -> Result<SlashReceipt, StakingError> {
        let val = self
            .validators
            .get_mut(validator)
            .ok_or(StakingError::ValidatorNotFound(*validator))?;

        if penalty_bp > 10000 {
            return Err(StakingError::InvalidSlashingEvidence(
                "Penalty basis points exceed 10000".into(),
            ));
        }

        let slashed_amount = val
            .total_bonded_stake
            .0
            .checked_mul(penalty_bp as u128)
            .ok_or(StakingError::MathOverflow)?
            / 10000;

        val.total_bonded_stake = val
            .total_bonded_stake
            .checked_sub(Amount(slashed_amount))
            .ok_or(StakingError::MathOverflow)?;

        val.status = ValidatorStatus::Jailed;
        val.jailing_height = Some(current_height);
        val.unjail_allowed_height = Some(BlockHeight(
            current_height.0 + self.config.jailing_duration_blocks,
        ));

        // Proportional slash on pending unbonding entries to prevent unbonding front-running
        for entry in self.unbonding_entries.iter_mut() {
            if entry.validator == *validator {
                let entry_slash =
                    entry.amount.0.checked_mul(penalty_bp as u128).unwrap_or(0) / 10000;
                entry.amount = entry.amount.saturating_sub(Amount(entry_slash));
            }
        }

        Ok(SlashReceipt {
            validator_id: *validator,
            slashed_amount: Amount(slashed_amount),
            jailing_height: current_height,
            reason,
        })
    }

    /// Releases a jailed validator back into Candidate status once the jailing duration has elapsed.
    pub fn unjail_validator(
        &mut self,
        validator: &ValidatorId,
        current_height: BlockHeight,
    ) -> Result<(), StakingError> {
        let val = self
            .validators
            .get_mut(validator)
            .ok_or(StakingError::ValidatorNotFound(*validator))?;

        if val.status != ValidatorStatus::Jailed {
            return Ok(());
        }

        if let Some(allowed_height) = val.unjail_allowed_height {
            if current_height.0 < allowed_height.0 {
                return Err(StakingError::ValidatorJailed);
            }
        }

        // Must still satisfy min self-stake threshold
        if val.total_bonded_stake.0 < self.config.min_self_stake.0 {
            return Err(StakingError::InsufficientSelfStake);
        }

        val.status = ValidatorStatus::Candidate;
        val.jailing_height = None;
        val.unjail_allowed_height = None;

        Ok(())
    }

    /// Rotates the active consensus committee at an epoch boundary.
    ///
    /// Elects the top K validators ranked by total bonded stake, updates statuses,
    /// and generates the canonical `ValidatorSet` for the new epoch.
    pub fn rotate_committee(&mut self, next_epoch: Epoch) -> ValidatorSet {
        // Collect eligible validators (not Jailed, not Inactive, meeting min self-stake)
        let mut eligible: Vec<(ValidatorId, Amount, PublicKey, u16)> = Vec::new();

        for (id, val) in self.validators.iter() {
            if val.status != ValidatorStatus::Jailed
                && val.total_bonded_stake.0 >= self.config.min_self_stake.0
            {
                eligible.push((
                    *id,
                    val.total_bonded_stake,
                    val.consensus_pubkey.clone(),
                    val.commission_rate_bp,
                ));
            }
        }

        // Sort descending by total bonded stake
        eligible.sort_by_key(|a| std::cmp::Reverse(a.1 .0));

        // Truncate to max active set size
        eligible.truncate(self.config.max_active_validators);

        let active_set: HashSet<ValidatorId> = eligible.iter().map(|(id, ..)| *id).collect();

        // Update validator statuses: selected become Active, others become Inactive (unless Jailed)
        for (id, val) in self.validators.iter_mut() {
            if active_set.contains(id) {
                val.status = ValidatorStatus::Active;
            } else if val.status != ValidatorStatus::Jailed {
                val.status = ValidatorStatus::Inactive;
            }
        }

        // Normalize voting power (minimum 1 unit per 1,000 TOLA, bounded to u64)
        let mut committee_infos = Vec::with_capacity(eligible.len());
        let mut total_power = 0u64;

        for (id, stake, pubkey, commission) in eligible {
            // Power = stake / 10^18 (1 base TOLA = 1 voting power unit)
            let power = (stake.0 / 1_000_000_000_000_000_000).max(1) as u64;
            total_power = total_power.saturating_add(power);

            committee_infos.push(ValidatorInfo {
                validator_id: id,
                consensus_pubkey: pubkey,
                voting_power: power,
                commission_bp: commission,
            });
        }

        ValidatorSet {
            epoch: next_epoch,
            validators: committee_infos,
            total_voting_power: total_power,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_test_registry() -> ValidatorRegistry {
        let config = RegistryConfig {
            min_self_stake: Amount(10_000),
            max_active_validators: 2,
            unbonding_delay_blocks: 100,
            jailing_duration_blocks: 500,
        };
        ValidatorRegistry::new(config)
    }

    #[test]
    fn test_validator_registration_and_self_stake_threshold() {
        let mut reg = setup_test_registry();
        let op1 = Address([0x01; 20]);
        let pk1 = PublicKey::Ed25519([0x11; 32]);

        // Below min self-stake rejected
        let err = reg
            .register_validator(op1, pk1.clone(), Amount(9_999), 500)
            .unwrap_err();
        assert_eq!(err, StakingError::InsufficientSelfStake);

        // Valid registration accepted
        assert!(reg
            .register_validator(op1, pk1, Amount(10_000), 500)
            .is_ok());
        assert_eq!(reg.validator_count(), 1);

        // Duplicate registration rejected
        let dup_err = reg
            .register_validator(op1, PublicKey::Ed25519([0x11; 32]), Amount(10_000), 500)
            .unwrap_err();
        assert_eq!(dup_err, StakingError::AlreadyRegistered);
    }

    #[test]
    fn test_share_based_pool_math_and_undelegation_queue() {
        let mut reg = setup_test_registry();
        let op1 = Address([0x01; 20]);
        let pk1 = PublicKey::Ed25519([0x11; 32]);
        reg.register_validator(op1, pk1, Amount(10_000), 500)
            .unwrap();

        let val_id = ValidatorId(op1);
        let third_party_delegator = Address([0xdd; 20]);

        // Decision D02: Third-party delegation is disabled in V1
        let err = reg
            .delegate(third_party_delegator, val_id, Amount(5_000))
            .unwrap_err();
        assert_eq!(err, StakingError::DelegationDisabledInV1);

        // Operator adding to self-stake is allowed
        let shares = reg.delegate(op1, val_id, Amount(5_000)).unwrap();
        assert_eq!(shares, Amount(5_000));

        let val = reg.get_validator(&val_id).unwrap();
        assert_eq!(val.total_bonded_stake, Amount(15_000));
        assert_eq!(val.total_pool_shares, Amount(15_000));

        // Undelegate 2,500 shares at Height 10 -> Enqueued until Height 110
        let redeemed = reg
            .undelegate(op1, val_id, Amount(2_500), BlockHeight(10))
            .unwrap();
        assert_eq!(redeemed, Amount(2_500));

        // Claim at Height 50 -> Not yet matured!
        assert_eq!(reg.claim_unbonded(op1, BlockHeight(50)), Amount(0));

        // Claim at Height 110 -> Matured and claimed!
        assert_eq!(reg.claim_unbonded(op1, BlockHeight(110)), Amount(2_500));
    }

    #[test]
    fn test_epoch_committee_rotation_ranks_by_stake() {
        let mut reg = setup_test_registry(); // Max 2 active validators
        let op1 = Address([0x01; 20]);
        let op2 = Address([0x02; 20]);
        let op3 = Address([0x03; 20]);

        reg.register_validator(op1, PublicKey::Ed25519([0x11; 32]), Amount(20_000), 500)
            .unwrap();
        reg.register_validator(op2, PublicKey::Ed25519([0x22; 32]), Amount(50_000), 500)
            .unwrap();
        reg.register_validator(op3, PublicKey::Ed25519([0x33; 32]), Amount(30_000), 500)
            .unwrap();

        // Rotate committee for Epoch 1: top 2 are op2 (50k) and op3 (30k)
        let committee = reg.rotate_committee(Epoch(1));
        assert_eq!(committee.validators.len(), 2);
        assert_eq!(committee.validators[0].validator_id, ValidatorId(op2));
        assert_eq!(committee.validators[1].validator_id, ValidatorId(op3));

        assert_eq!(
            reg.get_validator(&ValidatorId(op2)).unwrap().status,
            ValidatorStatus::Active
        );
        assert_eq!(
            reg.get_validator(&ValidatorId(op3)).unwrap().status,
            ValidatorStatus::Active
        );
        assert_eq!(
            reg.get_validator(&ValidatorId(op1)).unwrap().status,
            ValidatorStatus::Inactive
        );
    }

    #[test]
    fn test_validator_slashing_and_unjailing_lifecycle() {
        let mut reg = setup_test_registry();
        let op1 = Address([0x01; 20]);
        let pk1 = PublicKey::Ed25519([0x11; 32]);
        let val_id = ValidatorId(op1);

        // Register with 20,000 stake (min self-stake is 10,000)
        reg.register_validator(op1, pk1, Amount(20_000), 500)
            .unwrap();

        // Operator adds 10,000 self-stake
        reg.delegate(op1, val_id, Amount(10_000)).unwrap();

        // Operator requests unbonding 5,000 at height 100
        reg.undelegate(op1, val_id, Amount(5_000), BlockHeight(100))
            .unwrap();

        // Validator has bonded stake: 25,000
        assert_eq!(
            reg.get_validator(&val_id).unwrap().total_bonded_stake,
            Amount(25_000)
        );

        // Slash 5% (500 bp) at height 200 per Decision D02/D06
        let receipt = reg
            .slash_validator(&val_id, 500, BlockHeight(200), "Double voting".into())
            .unwrap();
        assert_eq!(receipt.slashed_amount, Amount(1_250)); // 5% of 25,000
        assert_eq!(receipt.jailing_height, BlockHeight(200));

        let val = reg.get_validator(&val_id).unwrap();
        assert_eq!(val.status, ValidatorStatus::Jailed);
        assert_eq!(val.total_bonded_stake, Amount(23_750));
        assert_eq!(val.unjail_allowed_height, Some(BlockHeight(700))); // 200 + 500

        // Check that pending unbonding was also slashed 5% (5,000 -> 4,750)
        assert_eq!(reg.claim_unbonded(op1, BlockHeight(200)), Amount(4_750));

        // Attempting to unjail before height 700 fails
        assert_eq!(
            reg.unjail_validator(&val_id, BlockHeight(699)).unwrap_err(),
            StakingError::ValidatorJailed
        );

        // Unjail at height 700 succeeds (remaining stake 23,750 >= min self-stake 10,000)
        assert!(reg.unjail_validator(&val_id, BlockHeight(700)).is_ok());
        assert_eq!(
            reg.get_validator(&val_id).unwrap().status,
            ValidatorStatus::Candidate
        );
    }

    #[test]
    fn test_validator_performance_scoring_and_80_20_reward_split() {
        let mut reg = setup_test_registry();
        let op1 = Address([0x01; 20]);
        let op2 = Address([0x02; 20]);
        let val1 = ValidatorId(op1);
        let val2 = ValidatorId(op2);

        reg.register_validator(op1, PublicKey::Ed25519([0x11; 32]), Amount(10_000), 500)
            .unwrap();
        reg.register_validator(op2, PublicKey::Ed25519([0x22; 32]), Amount(10_000), 500)
            .unwrap();

        let _ = reg.rotate_committee(Epoch(1));

        // Validator 1 has 100/100 duties completed (100% uptime >= 99% -> full performance score)
        for _ in 0..100 {
            reg.record_duty(&val1, true);
        }

        // Validator 2 has 75/100 duties completed (75% uptime < 80% -> 0 performance score per D02)
        for _ in 0..75 {
            reg.record_duty(&val2, true);
        }
        for _ in 0..25 {
            reg.record_duty(&val2, false);
        }

        assert_eq!(reg.get_performance(&val1).unwrap().uptime_bp(), 10000);
        assert_eq!(reg.get_performance(&val2).unwrap().uptime_bp(), 7500);

        // Distribute 1000 TOLA in validator pool gas fees
        // 80% stake pool = 800 TOLA. Since both have equal 10,000 stake: 400 TOLA each from stake pool.
        // 20% performance pool = 200 TOLA. Val1 has >99% uptime, Val2 has <80% uptime (score 0).
        // Therefore Val1 receives 100% of the performance pool (200 TOLA), Val2 receives 0 TOLA.
        // Total expected: Val1 = 400 + 200 = 600 TOLA; Val2 = 400 + 0 = 400 TOLA.
        let rewards = reg.distribute_pool_rewards(Amount(1000)).unwrap();
        assert_eq!(rewards.get(&val1).unwrap().0, 600);
        assert_eq!(rewards.get(&val2).unwrap().0, 400);

        // Check that validator 2 was flagged under status review due to <80% uptime
        assert!(reg.get_performance(&val2).unwrap().under_status_review);
    }

    #[test]
    fn test_daily_validator_pool_settlement_replay_and_conservation() {
        use tola_core::StateDB;
        use tola_state::MemoryStateJournal;
        use tola_types::AccountState;

        let mut reg = setup_test_registry();
        let op1 = Address([0x01; 20]);
        let op2 = Address([0x02; 20]);
        let val1 = ValidatorId(op1);
        let val2 = ValidatorId(op2);

        reg.register_validator(op1, PublicKey::Ed25519([0x11; 32]), Amount(10_000), 500)
            .unwrap();
        reg.register_validator(op2, PublicKey::Ed25519([0x22; 32]), Amount(10_000), 500)
            .unwrap();
        let _ = reg.rotate_committee(Epoch(1));

        for _ in 0..100 {
            reg.record_duty(&val1, true);
        }
        for _ in 0..75 {
            reg.record_duty(&val2, true);
        }
        for _ in 0..25 {
            reg.record_duty(&val2, false);
        }

        let mut state = MemoryStateJournal::new();
        // Fund Validator Reward Pool with 1,000 TOLA in state_db
        let initial_pool_balance = Amount(1_000);
        state.set_account(
            Address::VALIDATOR_REWARD_POOL,
            AccountState::new_with_balance(initial_pool_balance),
        );

        // First settlement at 24 hours (height 43,200, timestamp 86,400)
        let distributed = reg
            .settle_daily_validator_pool(BlockHeight(43_200), 86_400, &mut state)
            .expect("First daily settlement succeeds");
        assert_eq!(distributed, Amount(1_000));

        // Verify on-chain account balances
        let op1_acct = state.get_account(&op1).unwrap().unwrap();
        let op2_acct = state.get_account(&op2).unwrap().unwrap();
        let pool_acct = state
            .get_account(&Address::VALIDATOR_REWARD_POOL)
            .unwrap()
            .unwrap();

        assert_eq!(op1_acct.balance, Amount(600)); // 400 stake + 200 perf
        assert_eq!(op2_acct.balance, Amount(400)); // 400 stake + 0 perf
        assert_eq!(pool_acct.balance, Amount::ZERO); // Pool depleted exactly

        // Exact supply conservation: op1 + op2 + pool == initial_pool_balance
        assert_eq!(
            op1_acct.balance.0 + op2_acct.balance.0 + pool_acct.balance.0,
            initial_pool_balance.0
        );

        // Replay defense: Attempting immediate second settlement before 24h fails with AlreadySettled
        let replay_err = reg
            .settle_daily_validator_pool(BlockHeight(43_201), 86_401, &mut state)
            .unwrap_err();
        assert_eq!(replay_err, StakingError::AlreadySettled);

        // Subsequent settlement after another 24h with empty pool safely returns 0 without panic
        let next_settlement = reg
            .settle_daily_validator_pool(BlockHeight(86_400), 172_800, &mut state)
            .expect("Second 24h settlement with empty pool succeeds");
        assert_eq!(next_settlement, Amount::ZERO);
    }
}
