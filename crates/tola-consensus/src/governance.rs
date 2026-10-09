//! On-Chain Governance Engine, proposal lifecycle, and timelocked protocol parameter updates.
//!
//! Complies with ADR-0001, ADR-0003, and Approved Decisions D01–D15.
//!
//! Lifecycle:
//! 1. Proposal Submission: Requires min deposit to activate voting.
//! 2. Voting Window: 7 days (302,400 blocks @ 2s) per Decision D03.
//! 3. Quorum & Tally: Requires >= 5% quorum (500 bp), >= 60% approval (6000 bp), < 33.4% veto.
//! 4. Timelock: Mandatory 14 days (604,800 blocks @ 2s) timelock before automated on-chain execution.
//! 5. Emergency: 3-of-5 Security Council multisig for emergency parameter changes.

use std::collections::HashMap;
use tola_types::{Address, Amount, BlockHeight, Gas};

/// Configurable protocol parameter changes via governance
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParamChange {
    MaxBlockGasLimit(Gas),
    TargetBaseFee(Amount),
    MinValidatorSelfStake(Amount),
    UnbondingPeriodBlocks(u64),
    SlashingPenaltyBp(u16),
}

/// Dynamic active protocol parameters managed by governance
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveProtocolParams {
    pub max_block_gas_limit: Gas,
    pub target_base_fee: Amount,
    pub min_validator_self_stake: Amount,
    pub unbonding_period_blocks: u64,
    pub slashing_penalty_bp: u16,
}

impl Default for ActiveProtocolParams {
    fn default() -> Self {
        Self {
            max_block_gas_limit: Gas(30_000_000),
            target_base_fee: Amount(1_000_000_000), // 1 Gwei
            min_validator_self_stake: Amount(10_000 * 1_000_000_000_000_000_000), // 10k TOLA per Decision D02
            unbonding_period_blocks: 907_200, // 21 days @ 2s blocks per Decision D02
            slashing_penalty_bp: 500,         // 5% per Decision D02/D06
        }
    }
}

/// Voting options available to bonded stakers
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoteOption {
    Yes,
    No,
    NoWithVeto,
    Abstain,
}

/// Lifecycle status of a governance proposal
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProposalStatus {
    DepositPending,
    VotingActive,
    Passed,
    Rejected { reason: String },
    Vetoed,
    Timelocked { execution_height: BlockHeight },
    Executed,
}

/// Full specification of a governance proposal
#[derive(Debug, Clone)]
pub struct Proposal {
    pub id: u64,
    pub proposer: Address,
    pub title: String,
    pub description: String,
    pub changes: Vec<ParamChange>,
    pub deposit: Amount,
    pub min_deposit: Amount,
    pub start_block: BlockHeight,
    pub end_block: BlockHeight,
    pub status: ProposalStatus,
    pub votes: HashMap<Address, (VoteOption, Amount)>,
}

/// Governance configuration parameters
#[derive(Debug, Clone)]
pub struct GovernanceConfig {
    pub min_proposal_deposit: Amount,
    pub voting_period_blocks: u64,
    pub timelock_delay_blocks: u64,
    pub quorum_bp: u16,             // e.g. 500 = 5% per Decision D03
    pub approval_threshold_bp: u16, // e.g. 6000 = 60% per Decision D03
    pub veto_threshold_bp: u16,     // e.g. 3340 = 33.4%
}

impl Default for GovernanceConfig {
    fn default() -> Self {
        Self {
            min_proposal_deposit: Amount(10_000 * 1_000_000_000_000_000_000), // 10k TOLA
            voting_period_blocks: 302_400, // 7 days @ 2s blocks per Decision D03
            timelock_delay_blocks: 604_800, // 14 days @ 2s blocks per Decision D03
            quorum_bp: 500,                // 5% per Decision D03
            approval_threshold_bp: 6000,   // 60% per Decision D03
            veto_threshold_bp: 3340,       // 33.4%
        }
    }
}

/// Emergency Security Council multisig configuration (Decision D03: 3-of-5 threshold)
#[derive(Debug, Clone)]
pub struct EmergencyCouncilConfig {
    pub council_members: Vec<Address>, // Exactly 5 authorized members
    pub threshold: usize,              // Exactly 3 approvals required (3-of-5)
}

impl Default for EmergencyCouncilConfig {
    fn default() -> Self {
        Self {
            council_members: vec![
                Address([0x01; 20]),
                Address([0x02; 20]),
                Address([0x03; 20]),
                Address([0x04; 20]),
                Address([0x05; 20]),
            ],
            threshold: 3, // 3-of-5 required per Decision D03
        }
    }
}

/// Errors occurring in the governance lifecycle
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GovernanceError {
    ProposalNotFound(u64),
    VotingNotActive,
    VotingAlreadyEnded,
    VotingNotEndedYet,
    InsufficientDeposit,
    TimelockNotMatured {
        current: BlockHeight,
        required: BlockHeight,
    },
    InvalidStatus(String),
    ZeroVotingPower,
}

/// On-Chain Governance Engine managing proposals, votes, and parameters
#[derive(Debug, Clone)]
pub struct GovernanceEngine {
    config: GovernanceConfig,
    active_params: ActiveProtocolParams,
    proposals: HashMap<u64, Proposal>,
    next_proposal_id: u64,
    burned_deposits: Amount,
}

impl GovernanceEngine {
    pub fn new(config: GovernanceConfig, active_params: ActiveProtocolParams) -> Self {
        Self {
            config,
            active_params,
            proposals: HashMap::new(),
            next_proposal_id: 1,
            burned_deposits: Amount(0),
        }
    }

    pub fn config(&self) -> &GovernanceConfig {
        &self.config
    }

    pub fn active_params(&self) -> &ActiveProtocolParams {
        &self.active_params
    }

    pub fn burned_deposits(&self) -> Amount {
        self.burned_deposits
    }

    pub fn get_proposal(&self, id: u64) -> Option<&Proposal> {
        self.proposals.get(&id)
    }

    pub fn proposals(&self) -> &HashMap<u64, Proposal> {
        &self.proposals
    }

    pub fn list_proposals(&self) -> Vec<Proposal> {
        let mut list: Vec<Proposal> = self.proposals.values().cloned().collect();
        list.sort_by_key(|p| p.id);
        list
    }

    /// Submits a new proposal. If initial deposit meets minimum, voting activates immediately.
    pub fn submit_proposal(
        &mut self,
        proposer: Address,
        title: String,
        description: String,
        changes: Vec<ParamChange>,
        initial_deposit: Amount,
        current_height: BlockHeight,
    ) -> u64 {
        let id = self.next_proposal_id;
        self.next_proposal_id += 1;

        let meets_min = initial_deposit.0 >= self.config.min_proposal_deposit.0;
        let status = if meets_min {
            ProposalStatus::VotingActive
        } else {
            ProposalStatus::DepositPending
        };

        let start_block = current_height;
        let end_block = BlockHeight(current_height.0 + self.config.voting_period_blocks);

        let proposal = Proposal {
            id,
            proposer,
            title,
            description,
            changes,
            deposit: initial_deposit,
            min_deposit: self.config.min_proposal_deposit,
            start_block,
            end_block,
            status,
            votes: HashMap::new(),
        };

        self.proposals.insert(id, proposal);
        id
    }

    /// Deposits additional tokens toward a proposal's activation threshold
    pub fn add_deposit(
        &mut self,
        proposal_id: u64,
        amount: Amount,
        current_height: BlockHeight,
    ) -> Result<(), GovernanceError> {
        let proposal = self
            .proposals
            .get_mut(&proposal_id)
            .ok_or(GovernanceError::ProposalNotFound(proposal_id))?;

        if proposal.status != ProposalStatus::DepositPending {
            return Err(GovernanceError::InvalidStatus(
                "Proposal is not in deposit pending state".into(),
            ));
        }

        proposal.deposit.0 += amount.0;
        if proposal.deposit.0 >= proposal.min_deposit.0 {
            proposal.status = ProposalStatus::VotingActive;
            proposal.start_block = current_height;
            proposal.end_block = BlockHeight(current_height.0 + self.config.voting_period_blocks);
        }

        Ok(())
    }

    /// Casts a vote on an active proposal with snapshot staked voting power
    pub fn cast_vote(
        &mut self,
        proposal_id: u64,
        voter: Address,
        option: VoteOption,
        voting_power: Amount,
        current_height: BlockHeight,
    ) -> Result<(), GovernanceError> {
        let proposal = self
            .proposals
            .get_mut(&proposal_id)
            .ok_or(GovernanceError::ProposalNotFound(proposal_id))?;

        if proposal.status != ProposalStatus::VotingActive {
            return Err(GovernanceError::VotingNotActive);
        }

        if current_height.0 > proposal.end_block.0 {
            return Err(GovernanceError::VotingAlreadyEnded);
        }

        if voting_power.0 == 0 {
            return Err(GovernanceError::ZeroVotingPower);
        }

        // Record vote (overwrites previous vote from same address)
        proposal.votes.insert(voter, (option, voting_power));
        Ok(())
    }

    /// Evaluates proposal votes after voting period ends
    pub fn evaluate_proposal(
        &mut self,
        proposal_id: u64,
        total_bonded_stake: Amount,
        current_height: BlockHeight,
    ) -> Result<ProposalStatus, GovernanceError> {
        let proposal = self
            .proposals
            .get_mut(&proposal_id)
            .ok_or(GovernanceError::ProposalNotFound(proposal_id))?;

        if proposal.status != ProposalStatus::VotingActive {
            return Err(GovernanceError::VotingNotActive);
        }

        if current_height.0 < proposal.end_block.0 {
            return Err(GovernanceError::VotingNotEndedYet);
        }

        let mut yes_power = 0u128;
        let mut no_power = 0u128;
        let mut veto_power = 0u128;
        let mut abstain_power = 0u128;

        for (option, power) in proposal.votes.values() {
            match option {
                VoteOption::Yes => yes_power += power.0,
                VoteOption::No => no_power += power.0,
                VoteOption::NoWithVeto => veto_power += power.0,
                VoteOption::Abstain => abstain_power += power.0,
            }
        }

        let total_participating = yes_power + no_power + veto_power + abstain_power;

        // Quorum check: total_participating / total_bonded_stake >= quorum_bp / 10,000 (5% per Decision D03)
        let quorum_required = (total_bonded_stake.0 * self.config.quorum_bp as u128) / 10_000;
        if total_participating < quorum_required {
            let reason = format!(
                "Quorum not reached: participating {} < required {}",
                total_participating, quorum_required
            );
            proposal.status = ProposalStatus::Rejected { reason };
            return Ok(proposal.status.clone());
        }

        // Veto check: veto_power / total_participating >= veto_threshold_bp / 10,000 (33.4%)
        let veto_threshold = (total_participating * self.config.veto_threshold_bp as u128) / 10_000;
        if veto_power >= veto_threshold && veto_power > 0 {
            proposal.status = ProposalStatus::Vetoed;
            // Veto burns spam deposit
            self.burned_deposits.0 += proposal.deposit.0;
            return Ok(ProposalStatus::Vetoed);
        }

        // Approval check: yes_power / (yes + no + veto) >= approval_threshold_bp / 10,000 (60% per Decision D03)
        let decisive_power = yes_power + no_power + veto_power;
        let approval_threshold =
            (decisive_power * self.config.approval_threshold_bp as u128) / 10_000;
        if decisive_power > 0 && yes_power >= approval_threshold {
            let execution_height =
                BlockHeight(current_height.0 + self.config.timelock_delay_blocks);
            let status = ProposalStatus::Timelocked { execution_height };
            proposal.status = status.clone();
            Ok(status)
        } else {
            let reason = format!(
                "Insufficient approval: yes {} < threshold {}",
                yes_power, approval_threshold
            );
            let status = ProposalStatus::Rejected { reason };
            proposal.status = status.clone();
            Ok(status)
        }
    }

    /// Executes a matured timelocked proposal, applying all parameter updates
    pub fn execute_proposal(
        &mut self,
        proposal_id: u64,
        current_height: BlockHeight,
    ) -> Result<(), GovernanceError> {
        let proposal = self
            .proposals
            .get_mut(&proposal_id)
            .ok_or(GovernanceError::ProposalNotFound(proposal_id))?;

        match proposal.status {
            ProposalStatus::Timelocked { execution_height } => {
                if current_height.0 < execution_height.0 {
                    return Err(GovernanceError::TimelockNotMatured {
                        current: current_height,
                        required: execution_height,
                    });
                }

                // Apply parameters
                for change in &proposal.changes {
                    match change {
                        ParamChange::MaxBlockGasLimit(gas) => {
                            self.active_params.max_block_gas_limit = *gas;
                        }
                        ParamChange::TargetBaseFee(fee) => {
                            self.active_params.target_base_fee = *fee;
                        }
                        ParamChange::MinValidatorSelfStake(stake) => {
                            self.active_params.min_validator_self_stake = *stake;
                        }
                        ParamChange::UnbondingPeriodBlocks(blocks) => {
                            self.active_params.unbonding_period_blocks = *blocks;
                        }
                        ParamChange::SlashingPenaltyBp(bp) => {
                            self.active_params.slashing_penalty_bp = *bp;
                        }
                    }
                }

                proposal.status = ProposalStatus::Executed;
                Ok(())
            }
            _ => Err(GovernanceError::InvalidStatus(
                "Proposal is not in Timelocked status".into(),
            )),
        }
    }

    /// Checks all proposals currently in `Timelocked` state and executes those whose timelock has matured
    pub fn check_and_execute_matured_proposals(&mut self, current_height: BlockHeight) -> Vec<u64> {
        let matured_ids: Vec<u64> = self
            .proposals
            .iter()
            .filter_map(|(&id, prop)| match prop.status {
                ProposalStatus::Timelocked { execution_height } => {
                    if current_height.0 >= execution_height.0 {
                        Some(id)
                    } else {
                        None
                    }
                }
                _ => None,
            })
            .collect();

        let mut executed = Vec::new();
        for id in matured_ids {
            if self.execute_proposal(id, current_height).is_ok() {
                executed.push(id);
            }
        }
        executed
    }

    /// Executes emergency protocol parameter updates via 3-of-5 Security Council multisig (Decision D03).
    ///
    /// Requires approvals from at least 3 distinct authorized council members.
    /// Narrowly scoped to emergency parameter adjustments; cannot rewrite balances or history.
    pub fn execute_emergency_param_change(
        &mut self,
        approvers: &[Address],
        changes: Vec<ParamChange>,
        council: &EmergencyCouncilConfig,
    ) -> Result<(), GovernanceError> {
        let mut distinct_approvers = std::collections::HashSet::new();
        for &approver in approvers {
            if !council.council_members.contains(&approver) {
                return Err(GovernanceError::InvalidStatus(format!(
                    "Signer {} is not an authorized Security Council member",
                    approver.to_hex()
                )));
            }
            distinct_approvers.insert(approver);
        }

        if distinct_approvers.len() < council.threshold {
            return Err(GovernanceError::InvalidStatus(format!(
                "Emergency 3-of-5 threshold not met: {} distinct approvals < {}",
                distinct_approvers.len(),
                council.threshold
            )));
        }

        for change in &changes {
            match change {
                ParamChange::MaxBlockGasLimit(gas) => {
                    self.active_params.max_block_gas_limit = *gas;
                }
                ParamChange::TargetBaseFee(fee) => {
                    self.active_params.target_base_fee = *fee;
                }
                ParamChange::MinValidatorSelfStake(stake) => {
                    self.active_params.min_validator_self_stake = *stake;
                }
                ParamChange::UnbondingPeriodBlocks(blocks) => {
                    self.active_params.unbonding_period_blocks = *blocks;
                }
                ParamChange::SlashingPenaltyBp(bp) => {
                    self.active_params.slashing_penalty_bp = *bp;
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_approved_governance_parameters_defaults_d03() {
        let config = GovernanceConfig::default();
        assert_eq!(config.quorum_bp, 500, "5% quorum per Decision D03");
        assert_eq!(
            config.approval_threshold_bp, 6000,
            "60% approval per Decision D03"
        );
        assert_eq!(
            config.timelock_delay_blocks, 604_800,
            "14-day timelock per Decision D03"
        );
        assert_eq!(
            config.voting_period_blocks, 302_400,
            "7-day voting period per Decision D03"
        );

        let params = ActiveProtocolParams::default();
        assert_eq!(
            params.min_validator_self_stake,
            Amount(10_000 * 1_000_000_000_000_000_000),
            "10,000 TOLA min self-stake per Decision D02"
        );
        assert_eq!(
            params.slashing_penalty_bp, 500,
            "5% slashing per Decision D02/D06"
        );
    }

    #[test]
    fn test_emergency_council_multisig_3_of_5() {
        let mut gov =
            GovernanceEngine::new(GovernanceConfig::default(), ActiveProtocolParams::default());
        let council = EmergencyCouncilConfig::default();

        let member1 = council.council_members[0];
        let member2 = council.council_members[1];
        let member3 = council.council_members[2];
        let rogue = Address([0x99; 20]);

        // 1. Rejected with fewer than 3 approvals (2 of 5)
        let err = gov.execute_emergency_param_change(
            &[member1, member2],
            vec![ParamChange::SlashingPenaltyBp(600)],
            &council,
        );
        assert!(err.is_err(), "2-of-5 must be rejected");

        // 2. Rejected with duplicate approvals from same member
        let err_dup = gov.execute_emergency_param_change(
            &[member1, member1, member2],
            vec![ParamChange::SlashingPenaltyBp(600)],
            &council,
        );
        assert!(
            err_dup.is_err(),
            "Duplicate votes from same member must not reach threshold"
        );

        // 3. Rejected if non-member participates
        let err_rogue = gov.execute_emergency_param_change(
            &[member1, member2, rogue],
            vec![ParamChange::SlashingPenaltyBp(600)],
            &council,
        );
        assert!(err_rogue.is_err(), "Unauthorized signer must be rejected");

        // 4. Accepted with 3 distinct authorized members
        assert!(gov
            .execute_emergency_param_change(
                &[member1, member2, member3],
                vec![ParamChange::SlashingPenaltyBp(600)],
                &council,
            )
            .is_ok());

        assert_eq!(gov.active_params().slashing_penalty_bp, 600);
    }

    #[test]
    fn test_proposal_lifecycle_pass_timelock_execute() {
        let config = GovernanceConfig {
            min_proposal_deposit: Amount(100),
            voting_period_blocks: 10,
            timelock_delay_blocks: 5,
            quorum_bp: 500,              // 5% quorum
            approval_threshold_bp: 6000, // 60% approval
            veto_threshold_bp: 3340,
        };
        let mut gov = GovernanceEngine::new(config, ActiveProtocolParams::default());

        let proposer = Address([1u8; 20]);
        let voter1 = Address([2u8; 20]);
        let voter2 = Address([3u8; 20]);

        // Submit proposal with full deposit
        let pid = gov.submit_proposal(
            proposer,
            "Increase Gas Limit".into(),
            "Scale block gas to 40M".into(),
            vec![ParamChange::MaxBlockGasLimit(Gas(40_000_000))],
            Amount(100),
            BlockHeight(0),
        );

        assert_eq!(
            gov.get_proposal(pid).unwrap().status,
            ProposalStatus::VotingActive
        );

        // Cast votes
        // Total bonded stake: 10,000. Quorum required is 5% = 500.
        // Voter1 has 700 (Yes), Voter2 has 300 (No). Total participating: 1000 (>500 quorum).
        // Yes: 700 / 1000 = 70% (>=60% approval threshold).
        gov.cast_vote(pid, voter1, VoteOption::Yes, Amount(700), BlockHeight(1))
            .unwrap();
        gov.cast_vote(pid, voter2, VoteOption::No, Amount(300), BlockHeight(2))
            .unwrap();

        // End of voting period at height 10
        let status = gov
            .evaluate_proposal(pid, Amount(10_000), BlockHeight(10))
            .unwrap();
        assert_eq!(
            status,
            ProposalStatus::Timelocked {
                execution_height: BlockHeight(15)
            }
        );

        // Cannot execute before timelock matures (height 14)
        assert!(gov.execute_proposal(pid, BlockHeight(14)).is_err());

        // Check matured auto-execution at height 14: nothing executed
        let auto14 = gov.check_and_execute_matured_proposals(BlockHeight(14));
        assert!(auto14.is_empty());

        // Check matured auto-execution at height 15: executed!
        let auto15 = gov.check_and_execute_matured_proposals(BlockHeight(15));
        assert_eq!(auto15, vec![pid]);

        assert_eq!(gov.active_params().max_block_gas_limit, Gas(40_000_000));
        assert_eq!(
            gov.get_proposal(pid).unwrap().status,
            ProposalStatus::Executed
        );
    }

    #[test]
    fn test_proposal_veto_burns_deposit() {
        let config = GovernanceConfig {
            min_proposal_deposit: Amount(100),
            voting_period_blocks: 10,
            timelock_delay_blocks: 5,
            quorum_bp: 500,
            approval_threshold_bp: 6000,
            veto_threshold_bp: 3340,
        };
        let mut gov = GovernanceEngine::new(config, ActiveProtocolParams::default());

        let proposer = Address([1u8; 20]);
        let voter_yes = Address([2u8; 20]);
        let voter_veto = Address([3u8; 20]);

        let pid = gov.submit_proposal(
            proposer,
            "Malicious Proposal".into(),
            "Draining funds".into(),
            vec![],
            Amount(100),
            BlockHeight(0),
        );

        // 600 Yes, 400 NoWithVeto (40% > 33.4% veto threshold)
        gov.cast_vote(pid, voter_yes, VoteOption::Yes, Amount(600), BlockHeight(1))
            .unwrap();
        gov.cast_vote(
            pid,
            voter_veto,
            VoteOption::NoWithVeto,
            Amount(400),
            BlockHeight(2),
        )
        .unwrap();

        let status = gov
            .evaluate_proposal(pid, Amount(1000), BlockHeight(10))
            .unwrap();
        assert_eq!(status, ProposalStatus::Vetoed);
        assert_eq!(gov.burned_deposits(), Amount(100)); // Deposit burned
    }
}
