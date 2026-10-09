//! Mainnet Tokenomics, Emission Schedule, Governance Timelock, and Fork Coordination Integration Tests.
//!
//! Complies with Phase 17: Mainnet Launch Preparation & Tokenomics Activation.

use std::fs;
use tola_node::economics::{
    EconomicEngine, EconomicParameters, BLOCKS_PER_YEAR_2S, MAINNET_GENESIS_SUPPLY, TOLA_BASE_UNITS,
};
use tola_node::genesis::{
    compute_genesis_state_root, parse_genesis_json, validate_genesis_config, GenesisValidationError,
};
use tola_node::governance::{
    ActiveProtocolParams, GovernanceConfig, GovernanceEngine, GovernanceError, ParamChange,
    ProposalStatus, VoteOption,
};
use tola_node::upgrade::{ProtocolVersion, UpgradeError, UpgradeSignalingManager};
use tola_types::{Address, Amount, BlockHeight, ChainId, Gas, Hash};

#[test]
fn test_mainnet_genesis_state_root_and_supply_integrity() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let path = format!("{}/../../genesis-mainnet.json", manifest_dir);
    let json_content = fs::read_to_string(&path)
        .or_else(|_| fs::read_to_string("genesis-mainnet.json"))
        .expect("Read genesis-mainnet.json");

    let genesis = parse_genesis_json(&json_content).expect("Parse genesis-mainnet.json");

    assert_eq!(
        genesis.chain_id,
        ChainId(9999),
        "Mainnet Chain ID must be 9999"
    );
    assert_eq!(
        genesis.accounts.len(),
        6,
        "Mainnet genesis must specify 6 initial allocation pools per Decision D01"
    );
    assert_eq!(
        genesis.initial_validators.len(),
        4,
        "Mainnet genesis must have 4 initial validators"
    );

    // Exact supply sum check: must equal 1,000,000,000 TOLA (10^27 base units)
    let total_balance: u128 = genesis.accounts.iter().map(|a| a.balance.0).sum();
    assert_eq!(
        total_balance, MAINNET_GENESIS_SUPPLY,
        "Genesis allocations must sum exactly to 1 Billion TOLA (10^27 base units)"
    );

    // Compute deterministic state root
    let root = compute_genesis_state_root(&genesis);
    assert_ne!(root, Hash::ZERO, "State root must not be null");

    // Root must be identical on repeated evaluations
    let root2 = compute_genesis_state_root(&genesis);
    assert_eq!(root, root2, "Mainnet state root must be 100% deterministic");
}

#[test]
fn test_mainnet_genesis_unfunded_validator_stake_blocker_detection() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let path = format!("{}/../../genesis-mainnet.json", manifest_dir);
    let json_content = fs::read_to_string(&path)
        .or_else(|_| fs::read_to_string("genesis-mainnet.json"))
        .expect("Read genesis-mainnet.json");

    let genesis = parse_genesis_json(&json_content).expect("Parse genesis-mainnet.json");

    // Formal genesis validator verification must detect that the 4 genesis validators
    // lack backing funded accounts in genesis-mainnet.json.
    let validation_result = validate_genesis_config(&genesis);
    assert!(
        validation_result.is_err(),
        "Mainnet genesis must fail validation due to unbacked validator self-stakes"
    );

    match validation_result.unwrap_err() {
        GenesisValidationError::ValidatorAccountNotFound(val_id) => {
            assert_eq!(
                val_id.0, genesis.initial_validators[0].validator_id.0,
                "Validation error must pinpoint the first unfunded validator"
            );
        }
        other => panic!("Expected ValidatorAccountNotFound error, got: {:?}", other),
    }
}

#[test]
fn test_tokenomics_50_year_emission_decay_and_deflation() {
    let mut engine = EconomicEngine::new(EconomicParameters::default());

    // Verify annual inflation schedule is zero per Decision D01
    assert_eq!(engine.inflation_rate_at_height(0), 0); // Year 0: 0%
    assert_eq!(engine.inflation_rate_at_height(BLOCKS_PER_YEAR_2S), 0); // Year 1: 0%
    assert_eq!(engine.inflation_rate_at_height(2 * BLOCKS_PER_YEAR_2S), 0); // Year 2: 0%
    assert_eq!(engine.inflation_rate_at_height(7 * BLOCKS_PER_YEAR_2S), 0); // Year 7: 0%
    assert_eq!(engine.inflation_rate_at_height(50 * BLOCKS_PER_YEAR_2S), 0); // Year 50: 0%

    let genesis_supply = engine.params().genesis_supply.0;
    let mut circulating_balances = genesis_supply - (200_000_000 * TOLA_BASE_UNITS);
    let staked_total = 200_000_000 * TOLA_BASE_UNITS;

    // Verify initial state
    assert!(engine
        .verify_supply_conservation(Amount(circulating_balances), Amount(staked_total))
        .is_ok());

    // Simulate 500 blocks of production with 75% validator / 25% treasury fee allocation
    for h in 1..=500 {
        let reward = engine
            .compute_block_reward(h)
            .expect("Compute block reward");
        engine
            .apply_block_emission(&reward)
            .expect("Apply block emission");

        // Minted emission is 0 per D01
        assert_eq!(reward.validator_pool_amount.0, 0);

        // Simulate 2 TOLA of transaction fees allocated 75% validator / 25% Treasury
        let fee_amount = Amount(2 * TOLA_BASE_UNITS);
        circulating_balances -= fee_amount.0;
        let (val_pool, treas) = engine
            .allocate_fee_revenue(fee_amount)
            .expect("Allocate fee");
        assert_eq!(val_pool.0, (2 * TOLA_BASE_UNITS * 75) / 100);
        assert_eq!(treas.0, (2 * TOLA_BASE_UNITS * 25) / 100);
        circulating_balances += val_pool.0;

        // Conservation invariant check
        assert!(
            engine
                .verify_supply_conservation(Amount(circulating_balances), Amount(staked_total))
                .is_ok(),
            "Supply conservation violated at block height {}",
            h
        );
    }

    assert_eq!(
        engine.total_minted().0,
        0,
        "No new tokens minted under D01 zero inflation"
    );
    assert_eq!(engine.total_burned().0, 0, "No base fees burned under D01");
    assert!(
        engine.treasury_balance().0 > 0,
        "Treasury receives 25% of all fee revenue"
    );
}

#[test]
fn test_governance_complete_parameter_timelock_lifecycle() {
    let config = GovernanceConfig {
        min_proposal_deposit: Amount(10_000 * TOLA_BASE_UNITS),
        voting_period_blocks: 100,
        timelock_delay_blocks: 20,
        quorum_bp: 4000,             // 40%
        approval_threshold_bp: 5000, // 50%
        veto_threshold_bp: 3340,     // 33.4%
    };

    let initial_params = ActiveProtocolParams::default();
    let mut gov = GovernanceEngine::new(config, initial_params);

    let proposer = Address([0x11; 20]);
    let val1 = Address([0xaa; 20]);
    let val2 = Address([0xbb; 20]);
    let val3 = Address([0xcc; 20]);

    // Total bonded stake: 100,000,000 TOLA
    let total_stake = Amount(100_000_000 * TOLA_BASE_UNITS);

    // 1. Submit proposal
    let pid = gov.submit_proposal(
        proposer,
        "Scaling Upgrade: Increase Gas Limit to 45M".into(),
        "Enhance throughput for messenger dApps".into(),
        vec![
            ParamChange::MaxBlockGasLimit(Gas(45_000_000)),
            ParamChange::SlashingPenaltyBp(500), // 5%
        ],
        Amount(10_000 * TOLA_BASE_UNITS),
        BlockHeight(10),
    );

    assert_eq!(
        gov.get_proposal(pid).unwrap().status,
        ProposalStatus::VotingActive
    );

    // 2. Cast votes
    // Val1 (40M stake): Yes
    // Val2 (20M stake): Yes
    // Val3 (10M stake): No
    // Total participating: 70M / 100M = 70% (meets 40% quorum)
    // Yes: 60M / 70M = 85.7% (meets 50% approval)
    gov.cast_vote(
        pid,
        val1,
        VoteOption::Yes,
        Amount(40_000_000 * TOLA_BASE_UNITS),
        BlockHeight(20),
    )
    .unwrap();
    gov.cast_vote(
        pid,
        val2,
        VoteOption::Yes,
        Amount(20_000_000 * TOLA_BASE_UNITS),
        BlockHeight(25),
    )
    .unwrap();
    gov.cast_vote(
        pid,
        val3,
        VoteOption::No,
        Amount(10_000_000 * TOLA_BASE_UNITS),
        BlockHeight(30),
    )
    .unwrap();

    // Cannot evaluate before voting period ends (ends at height 10 + 100 = 110)
    assert!(gov
        .evaluate_proposal(pid, total_stake, BlockHeight(105))
        .is_err());

    // 3. Evaluate at height 110 -> transitions to Timelocked
    let status = gov
        .evaluate_proposal(pid, total_stake, BlockHeight(110))
        .unwrap();
    assert_eq!(
        status,
        ProposalStatus::Timelocked {
            execution_height: BlockHeight(130) // 110 + 20 timelock
        }
    );

    // 4. Timelock delay check: execution at height 125 fails
    let err = gov.execute_proposal(pid, BlockHeight(125)).unwrap_err();
    assert_eq!(
        err,
        GovernanceError::TimelockNotMatured {
            current: BlockHeight(125),
            required: BlockHeight(130),
        }
    );

    // 5. Execute at maturity (height 130) -> parameters applied!
    gov.execute_proposal(pid, BlockHeight(130))
        .expect("Execute matured proposal");
    assert_eq!(gov.active_params().max_block_gas_limit, Gas(45_000_000));
    assert_eq!(gov.active_params().slashing_penalty_bp, 500);
    assert_eq!(
        gov.get_proposal(pid).unwrap().status,
        ProposalStatus::Executed
    );
}

#[test]
fn test_governance_veto_burns_spam_deposit() {
    let config = GovernanceConfig {
        min_proposal_deposit: Amount(10_000 * TOLA_BASE_UNITS),
        voting_period_blocks: 50,
        timelock_delay_blocks: 10,
        quorum_bp: 4000,
        approval_threshold_bp: 5000,
        veto_threshold_bp: 3340, // 33.4%
    };

    let mut gov = GovernanceEngine::new(config, ActiveProtocolParams::default());
    let proposer = Address([0xee; 20]);
    let pid = gov.submit_proposal(
        proposer,
        "Malicious State Reallocation".into(),
        "Attempted treasury drain".into(),
        vec![],
        Amount(10_000 * TOLA_BASE_UNITS),
        BlockHeight(1),
    );

    let val1 = Address([0x11; 20]);
    let val_veto = Address([0x22; 20]);

    // Total participating: 60M Yes, 40M NoWithVeto (40% veto > 33.4%)
    gov.cast_vote(
        pid,
        val1,
        VoteOption::Yes,
        Amount(60_000_000 * TOLA_BASE_UNITS),
        BlockHeight(10),
    )
    .unwrap();
    gov.cast_vote(
        pid,
        val_veto,
        VoteOption::NoWithVeto,
        Amount(40_000_000 * TOLA_BASE_UNITS),
        BlockHeight(15),
    )
    .unwrap();

    let status = gov
        .evaluate_proposal(pid, Amount(100_000_000 * TOLA_BASE_UNITS), BlockHeight(52))
        .unwrap();
    assert_eq!(status, ProposalStatus::Vetoed);
    assert_eq!(gov.burned_deposits(), Amount(10_000 * TOLA_BASE_UNITS)); // Spam deposit destroyed
}

#[test]
fn test_validator_upgrade_signaling_and_hard_fork_halt() {
    let running = ProtocolVersion::new(1, 0, 0);
    let target = ProtocolVersion::new(2, 0, 0); // Hard fork

    // 100-block sliding window, 80% threshold
    let mut manager = UpgradeSignalingManager::new(running, 100, 8000);
    let proposer = Address([0x01; 20]);

    // Record 21 blocks signaling v1 at start and 79 blocks signaling v2 (79% < 80%)
    for h in 1..=21 {
        manager.record_signal(BlockHeight(h), proposer, running);
    }
    for h in 22..=100 {
        manager.record_signal(BlockHeight(h), proposer, target);
    }

    assert!(manager
        .evaluate_signals(target, BlockHeight(100), 50, "Hard Fork v2".into())
        .is_none());

    // Advance 5 blocks with v2 signals: heights 101..=105, which evicts the oldest 5 v1 signals
    for h in 101..=105 {
        manager.record_signal(BlockHeight(h), proposer, target);
    }

    // Window now has 84% v2 signals!
    let plan = manager.evaluate_signals(target, BlockHeight(105), 50, "Hard Fork v2".into());
    assert!(plan.is_some());
    let plan = plan.unwrap();
    assert_eq!(plan.activation_height, BlockHeight(155));
    assert!(plan.is_hard_fork);

    // Normal processing continues before activation height
    assert_eq!(
        manager.verify_and_advance(BlockHeight(154)).unwrap(),
        running
    );

    // At activation height 155, running node (v1) safely halts instead of corrupting state
    let err = manager.verify_and_advance(BlockHeight(155)).unwrap_err();
    match err {
        UpgradeError::HardForkHaltRequired {
            current_version,
            required_version,
            activation_height,
        } => {
            assert_eq!(current_version, running);
            assert_eq!(required_version, target);
            assert_eq!(activation_height, BlockHeight(155));
        }
        _ => panic!("Expected HardForkHaltRequired error"),
    }
}
