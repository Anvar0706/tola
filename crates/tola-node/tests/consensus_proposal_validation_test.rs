//! Comprehensive integration test suite for Consensus Proposal Validation Hardening.
//!
//! Validates:
//! 1. Valid proposal: proposal with correct commitments executes, commits, and emits a valid precommit vote.
//! 2. Forged txs_root: rejected pre-execution with `ConsensusError::InvalidTxsRoot`, no execution, no vote.
//! 3. Forged receipts_root: rejected post-execution with `ConsensusError::InvalidReceiptsRoot`, state rolled back, no vote.
//! 4. Forged gas_used: rejected post-execution with `ConsensusError::InvalidGasUsed`, state rolled back, no vote.
//! 5. gas_used > gas_limit: rejected pre-execution with `ConsensusError::GasLimitExceeded`, no execution, no vote.
//! 6. Empty block: proposal with 0 transactions produces valid Hash::ZERO roots and zero gas, passes validation, and emits vote.
//! 7. Speculative state isolation: verified balances/nonces remain untouched after rejected proposals and node accepts subsequent valid blocks.
//! 8. Rejected proposal emits no vote: handle_proposal/process_proposal returns Err and no vote message is signed.
//! 9. EVM transaction proposals: proposals containing EVM transactions validate txs_root, receipts_root, and gas_used.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use tola_core::{ConsensusError, StateDB};
use tola_crypto::{Ed25519Keypair, Secp256k1Keypair};
use tola_node::config::{GenesisAccount, GenesisConfig, NodeConfig};
use tola_node::engine::{compute_receipts_root, compute_txs_root, TolaNode};
use tola_sdk::builder::TransactionBuilder;
use tola_sdk::wallet::TolaWallet;
use tola_storage::PersistentStateDB;
use tola_types::*;

fn unique_temp_dir(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("tola-test-propval-{}-{}", label, nanos));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn setup_cluster(
    chain_id: ChainId,
    test_name: &str,
    extra_genesis_accounts: Vec<GenesisAccount>,
) -> (TolaNode, TolaNode, TolaWallet) {
    let temp_root = unique_temp_dir(test_name);
    let wallet = TolaWallet::generate();
    let initial_balance = Amount(100_000_000_000_000_000); // 100 TOLA

    let dir_a = temp_root.join("node_a");
    let dir_b = temp_root.join("node_b");
    std::fs::create_dir_all(&dir_a).unwrap();
    std::fs::create_dir_all(&dir_b).unwrap();

    let state_db_a = PersistentStateDB::open(dir_a.join("state")).unwrap();
    let state_db_b = PersistentStateDB::open(dir_b.join("state")).unwrap();

    let val_kp = Ed25519Keypair::generate();
    let val_addr = Address([0x99; 20]);
    let val_info = ValidatorInfo {
        validator_id: ValidatorId(val_addr),
        consensus_pubkey: val_kp.public_key(),
        voting_power: 100_000,
        commission_bp: 0,
    };
    let committee = ValidatorSet {
        epoch: Epoch(0),
        validators: vec![val_info.clone()],
        total_voting_power: 100_000,
    };

    let mut genesis_accounts = vec![
        GenesisAccount {
            address: wallet.address(),
            balance: initial_balance,
        },
        GenesisAccount {
            address: val_addr,
            balance: initial_balance,
        },
    ];
    genesis_accounts.extend(extra_genesis_accounts);

    let genesis_cfg = GenesisConfig {
        chain_id,
        genesis_time: 1770000000,
        accounts: genesis_accounts,
        initial_validators: vec![val_info],
    };

    let cfg_a = NodeConfig {
        chain_id,
        data_dir: dir_a,
        is_validator: true,
        validator_keypair: Some(val_kp.clone()),
        operator_address: Some(val_addr),
        p2p_port: 39611,
        rpc_port: 8621,
        genesis: genesis_cfg.clone(),
    };
    let cfg_b = NodeConfig {
        chain_id,
        data_dir: dir_b,
        is_validator: true,
        validator_keypair: Some(val_kp),
        operator_address: Some(val_addr),
        p2p_port: 39612,
        rpc_port: 8622,
        genesis: genesis_cfg,
    };

    let node_a = TolaNode::new(cfg_a, state_db_a, committee.clone()).unwrap();
    let node_b = TolaNode::new(cfg_b, state_db_b, committee).unwrap();

    (node_a, node_b, wallet)
}

fn build_native_tx(
    wallet: &TolaWallet,
    chain_id: ChainId,
    recipient: Address,
    amount: Amount,
    nonce: u64,
) -> Transaction {
    TransactionBuilder::new(chain_id)
        .to(recipient)
        .value(amount)
        .nonce(Nonce(nonce))
        .gas_limit(Gas(21_000))
        .max_fee_per_gas(Amount(1_000_000_000))
        .max_priority_fee_per_gas(Amount(1_000_000_000))
        .build_and_sign(wallet)
        .expect("Build and sign native tx")
}

fn build_evm_eip1559_tx(
    kp: &Secp256k1Keypair,
    chain_id: ChainId,
    recipient: Address,
    amount: u128,
    nonce: u64,
) -> Transaction {
    let mut tx = Transaction {
        chain_id,
        nonce: Nonce(nonce),
        from: kp.address(),
        to: Some(recipient),
        value: Amount::from_u128(amount),
        gas_limit: Gas(21_000),
        max_fee_per_gas: Amount(1_000_000_000),
        max_priority_fee_per_gas: Amount(1_000_000_000),
        valid_until_block: BlockHeight(100),
        data: vec![],
        memo: None,
        signature: Signature::Ed25519([0u8; 64]),
        tx_type: 2,
        access_list: vec![],
    };
    let hash_to_sign = tola_codec::rlp::compute_type2_signing_hash(&tx);
    let sig = kp.sign_recoverable(hash_to_sign.as_bytes()).unwrap();
    let mut r = [0u8; 32];
    let mut s = [0u8; 32];
    r.copy_from_slice(&sig[0..32]);
    s.copy_from_slice(&sig[32..64]);
    let y_parity = sig[64];
    tx.signature = Signature::Secp256k1Eth {
        r,
        s,
        y_parity,
        chain_id: chain_id.0,
    };
    tx
}

/// 1. Valid proposal: proposal with correct txs_root, receipts_root, gas_used, and state_root
///    executes, commits, and emits a valid precommit vote.
#[test]
fn test_1_valid_proposal_passes_and_emits_vote() {
    let chain_id = ChainId(201);
    let (node_a, node_b, wallet) = setup_cluster(chain_id, "test1_valid", vec![]);

    let recipient = Address([0x11; 20]);
    let tx = build_native_tx(&wallet, chain_id, recipient, Amount(500_000_000), 0);

    {
        let mut mp = node_a.mempool.write().unwrap();
        let state = node_a.state_db.read().unwrap();
        mp.insert(tx, &*state, BlockHeight(0)).unwrap();
    }

    let block = node_a.propose_block(None, Round(0), 1000).unwrap();
    assert_eq!(block.header.height, BlockHeight(1));
    assert_eq!(block.transactions.len(), 1);
    assert_ne!(block.header.txs_root, Hash::ZERO);
    assert_ne!(block.header.receipts_root, Hash::ZERO);
    assert!(block.header.gas_used > Gas::ZERO);

    // Process on replica Node B
    let vote_res = node_b.process_proposal(&block);
    assert!(
        vote_res.is_ok(),
        "Proposal must be accepted: {:?}",
        vote_res
    );
    let vote = vote_res.unwrap();
    assert!(vote.is_some(), "Precommit vote must be emitted");
    let vote = vote.unwrap();
    assert_eq!(vote.height, BlockHeight(1));
    assert_eq!(vote.round, Round(0));

    // Form QC and commit block via process_qc
    let qc = node_b.process_vote(vote).unwrap().expect("QC formed");
    node_b.process_qc(&qc).expect("QC processed");

    // Replica state must be committed
    assert_eq!(node_b.current_height(), BlockHeight(1));
    assert_eq!(node_b.state_root(), block.header.state_root);

    // Verify recipient received funds
    let state_b = node_b.state_db.read().unwrap();
    let recipient_acct = state_b.get_account(&recipient).unwrap().unwrap();
    assert_eq!(recipient_acct.balance, Amount(500_000_000));
}

/// 2. Forged txs_root: proposer alters txs_root -> rejected before execution with InvalidTxsRoot,
///    no state change, no vote.
#[test]
fn test_2_forged_txs_root_rejected_pre_execution() {
    let chain_id = ChainId(202);
    let (node_a, node_b, wallet) = setup_cluster(chain_id, "test2_forged_txs_root", vec![]);

    let recipient = Address([0x22; 20]);
    let tx = build_native_tx(&wallet, chain_id, recipient, Amount(500_000_000), 0);
    {
        let mut mp = node_a.mempool.write().unwrap();
        let state = node_a.state_db.read().unwrap();
        mp.insert(tx, &*state, BlockHeight(0)).unwrap();
    }

    let mut forged_block = node_a.propose_block(None, Round(0), 1000).unwrap();
    let initial_height = node_b.current_height();
    let initial_state_root = node_b.state_root();

    // Proposer alters txs_root
    forged_block.header.txs_root = Hash([0xAA; 32]);

    let res = node_b.process_proposal(&forged_block);
    assert_eq!(res, Err(ConsensusError::InvalidTxsRoot));

    // Verify zero state change on Node B
    assert_eq!(node_b.current_height(), initial_height);
    assert_eq!(node_b.state_root(), initial_state_root);
    let state_b = node_b.state_db.read().unwrap();
    assert!(state_b.get_account(&recipient).unwrap().is_none());
}

/// 3. Forged receipts_root: proposer alters receipts_root -> rejected post-execution with
///    InvalidReceiptsRoot, speculative state rolled back, no disk commit, no vote.
#[test]
fn test_3_forged_receipts_root_rejected_post_execution() {
    let chain_id = ChainId(203);
    let (node_a, node_b, wallet) = setup_cluster(chain_id, "test3_forged_receipts_root", vec![]);

    let recipient = Address([0x33; 20]);
    let tx = build_native_tx(&wallet, chain_id, recipient, Amount(500_000_000), 0);
    {
        let mut mp = node_a.mempool.write().unwrap();
        let state = node_a.state_db.read().unwrap();
        mp.insert(tx, &*state, BlockHeight(0)).unwrap();
    }

    let mut forged_block = node_a.propose_block(None, Round(0), 1000).unwrap();
    let initial_height = node_b.current_height();
    let initial_state_root = node_b.state_root();

    // Proposer alters receipts_root
    forged_block.header.receipts_root = Hash([0xBB; 32]);

    let res = node_b.process_proposal(&forged_block);
    assert_eq!(res, Err(ConsensusError::InvalidReceiptsRoot));

    // Verify zero state change on Node B
    assert_eq!(node_b.current_height(), initial_height);
    assert_eq!(node_b.state_root(), initial_state_root);
    let state_b = node_b.state_db.read().unwrap();
    assert!(state_b.get_account(&recipient).unwrap().is_none());
}

/// 4. Forged gas_used: proposer alters gas_used -> rejected post-execution with InvalidGasUsed,
///    state rolled back, no vote.
#[test]
fn test_4_forged_gas_used_rejected_post_execution() {
    let chain_id = ChainId(204);
    let (node_a, node_b, wallet) = setup_cluster(chain_id, "test4_forged_gas_used", vec![]);

    let recipient = Address([0x44; 20]);
    let tx = build_native_tx(&wallet, chain_id, recipient, Amount(500_000_000), 0);
    {
        let mut mp = node_a.mempool.write().unwrap();
        let state = node_a.state_db.read().unwrap();
        mp.insert(tx, &*state, BlockHeight(0)).unwrap();
    }

    let mut forged_block = node_a.propose_block(None, Round(0), 1000).unwrap();
    let initial_height = node_b.current_height();
    let initial_state_root = node_b.state_root();

    // Proposer alters gas_used to a non-matching value that is still within gas_limit
    forged_block.header.gas_used = Gas(forged_block.header.gas_used.0 + 10_000);

    let res = node_b.process_proposal(&forged_block);
    assert_eq!(res, Err(ConsensusError::InvalidGasUsed));

    // Verify zero state change on Node B
    assert_eq!(node_b.current_height(), initial_height);
    assert_eq!(node_b.state_root(), initial_state_root);
    let state_b = node_b.state_db.read().unwrap();
    assert!(state_b.get_account(&recipient).unwrap().is_none());
}

/// 5. gas_used > gas_limit: proposal rejected immediately with gas limit error.
#[test]
fn test_5_gas_used_exceeds_limit_rejected_pre_execution() {
    let chain_id = ChainId(205);
    let (node_a, node_b, wallet) = setup_cluster(chain_id, "test5_gas_limit", vec![]);

    let recipient = Address([0x55; 20]);
    let tx = build_native_tx(&wallet, chain_id, recipient, Amount(500_000_000), 0);
    {
        let mut mp = node_a.mempool.write().unwrap();
        let state = node_a.state_db.read().unwrap();
        mp.insert(tx, &*state, BlockHeight(0)).unwrap();
    }

    let mut forged_block = node_a.propose_block(None, Round(0), 1000).unwrap();
    let initial_height = node_b.current_height();

    // Set gas_used to exceed gas_limit
    forged_block.header.gas_limit = Gas(30_000_000);
    forged_block.header.gas_used = Gas(30_000_001);

    let res = node_b.process_proposal(&forged_block);
    assert_eq!(res, Err(ConsensusError::GasLimitExceeded));

    // Verify height did not advance
    assert_eq!(node_b.current_height(), initial_height);
}

/// 6. Empty block: proposal with 0 transactions produces valid Hash::ZERO roots and zero gas,
///    passes validation, and emits vote.
#[test]
fn test_6_empty_block_proposal_valid_commitments() {
    let chain_id = ChainId(206);
    let (node_a, node_b, _wallet) = setup_cluster(chain_id, "test6_empty_block", vec![]);

    // Propose without any transactions in mempool
    let empty_block = node_a.propose_block(None, Round(0), 1000).unwrap();
    assert_eq!(empty_block.transactions.len(), 0);
    assert_eq!(empty_block.header.txs_root, Hash::ZERO);
    assert_eq!(empty_block.header.receipts_root, Hash::ZERO);
    assert_eq!(empty_block.header.gas_used, Gas::ZERO);

    // Verify helper functions produce Hash::ZERO for empty slices
    assert_eq!(compute_txs_root(&[]), Hash::ZERO);
    assert_eq!(compute_receipts_root(&[]), Hash::ZERO);

    // Process on replica Node B
    let vote_res = node_b.process_proposal(&empty_block);
    assert!(
        vote_res.is_ok(),
        "Empty block proposal must be accepted: {:?}",
        vote_res
    );
    let vote = vote_res.unwrap();
    assert!(
        vote.is_some(),
        "Precommit vote must be emitted for empty block"
    );
    let vote = vote.unwrap();
    let qc = node_b.process_vote(vote).unwrap().expect("QC formed");
    node_b.process_qc(&qc).expect("QC processed");
    assert_eq!(node_b.current_height(), BlockHeight(1));
}

/// 7. Speculative state isolation: verify that after a rejected proposal (e.g. bad receipts_root),
///    account balances, nonces, and storage are identical to pre-proposal state, and next block succeeds.
#[test]
fn test_7_speculative_state_isolation_on_rejection() {
    let chain_id = ChainId(207);
    let (node_a, node_b, wallet) = setup_cluster(chain_id, "test7_state_isolation", vec![]);

    let sender_addr = wallet.address();
    let recipient_addr = Address([0x77; 20]);

    let initial_sender_state = node_b
        .state_db
        .read()
        .unwrap()
        .get_account(&sender_addr)
        .unwrap()
        .expect("Sender must exist");
    let initial_root = node_b.state_root();

    let tx = build_native_tx(&wallet, chain_id, recipient_addr, Amount(1_000_000_000), 0);
    {
        let mut mp = node_a.mempool.write().unwrap();
        let state = node_a.state_db.read().unwrap();
        mp.insert(tx.clone(), &*state, BlockHeight(0)).unwrap();
    }

    let mut corrupted_block = node_a.propose_block(None, Round(0), 1000).unwrap();
    corrupted_block.header.receipts_root = Hash([0xCC; 32]);

    // Send corrupted block to Node B
    let res = node_b.process_proposal(&corrupted_block);
    assert_eq!(res, Err(ConsensusError::InvalidReceiptsRoot));

    // Confirm state isolation
    {
        let state_b = node_b.state_db.read().unwrap();
        let sender_after = state_b.get_account(&sender_addr).unwrap().unwrap();
        assert_eq!(
            sender_after.balance, initial_sender_state.balance,
            "Sender balance must NOT change after rejected proposal"
        );
        assert_eq!(
            sender_after.nonce, initial_sender_state.nonce,
            "Sender nonce must NOT advance after rejected proposal"
        );
        assert!(
            state_b.get_account(&recipient_addr).unwrap().is_none(),
            "Recipient must NOT be created after rejected proposal"
        );
    }
    assert_eq!(node_b.state_root(), initial_root);

    // Now test valid proposal at round 1 to verify Node B remained uncorrupted
    // Put transaction in Node B's mempool and propose on Node B
    {
        let mut mp = node_b.mempool.write().unwrap();
        let state = node_b.state_db.read().unwrap();
        mp.insert(tx, &*state, BlockHeight(0)).unwrap();
    }
    let _valid_block = node_b.propose_block(None, Round(1), 2000).unwrap();
    assert_eq!(node_b.current_height(), BlockHeight(1));
    let state_b = node_b.state_db.read().unwrap();
    let recipient_acct = state_b.get_account(&recipient_addr).unwrap().unwrap();
    assert_eq!(recipient_acct.balance, Amount(1_000_000_000));
}

/// 8. Rejected proposal emits no vote: verify that handle_proposal / process_proposal returns
///    Err and no vote message is signed or emitted for adversarial proposals.
#[test]
fn test_8_rejected_proposal_emits_no_vote() {
    let chain_id = ChainId(208);
    let (node_a, node_b, wallet) = setup_cluster(chain_id, "test8_no_vote", vec![]);

    let recipient = Address([0x88; 20]);
    let tx = build_native_tx(&wallet, chain_id, recipient, Amount(500_000_000), 0);
    {
        let mut mp = node_a.mempool.write().unwrap();
        let state = node_a.state_db.read().unwrap();
        mp.insert(tx, &*state, BlockHeight(0)).unwrap();
    }

    let base_block = node_a.propose_block(None, Round(0), 1000).unwrap();

    // Case A: Bad txs_root -> Err, no vote
    let mut b_txs = base_block.clone();
    b_txs.header.txs_root = Hash([0x01; 32]);
    assert!(matches!(
        node_b.process_proposal(&b_txs),
        Err(ConsensusError::InvalidTxsRoot)
    ));

    // Case B: Bad receipts_root -> Err, no vote
    let mut b_rec = base_block.clone();
    b_rec.header.receipts_root = Hash([0x02; 32]);
    assert!(matches!(
        node_b.process_proposal(&b_rec),
        Err(ConsensusError::InvalidReceiptsRoot)
    ));

    // Case C: Bad gas_used -> Err, no vote
    let mut b_gas = base_block.clone();
    b_gas.header.gas_used = Gas(100);
    assert!(matches!(
        node_b.process_proposal(&b_gas),
        Err(ConsensusError::InvalidGasUsed)
    ));

    // Case D: Gas limit exceeded -> Err, no vote
    let mut b_lim = base_block.clone();
    b_lim.header.gas_limit = Gas(10_000);
    b_lim.header.gas_used = Gas(20_000);
    assert!(matches!(
        node_b.process_proposal(&b_lim),
        Err(ConsensusError::GasLimitExceeded)
    ));

    // Case E: Bad state_root -> Err, no vote
    let mut b_state = base_block.clone();
    b_state.header.state_root = Hash([0x05; 32]);
    assert!(matches!(
        node_b.process_proposal(&b_state),
        Err(ConsensusError::InvalidStateRoot)
    ));

    // Verify pacemaker and height on Node B never advanced
    assert_eq!(node_b.current_height(), BlockHeight(0));
    assert_eq!(
        node_b
            .consensus
            .read()
            .unwrap()
            .pacemaker()
            .current_height(),
        BlockHeight(0)
    );
}

/// 9. EVM transaction proposals: verify that proposals containing legacy and EIP-1559/EIP-2930
///    EVM transactions correctly validate txs_root, receipts_root, and gas_used.
#[test]
fn test_9_evm_transaction_proposals_commitment_validation() {
    let chain_id = ChainId(209);
    let secp_kp = Secp256k1Keypair::generate();
    let eth_addr = secp_kp.address();
    let extra_accounts = vec![GenesisAccount {
        address: eth_addr,
        balance: Amount(10_000_000_000_000_000), // 10 TOLA
    }];

    let (node_a, node_b, _wallet) = setup_cluster(chain_id, "test9_evm", extra_accounts);

    let recipient = Address([0x9A; 20]);
    let evm_tx = build_evm_eip1559_tx(&secp_kp, chain_id, recipient, 2_000_000_000, 0);
    assert!(evm_tx.is_evm());

    {
        let mut mp = node_a.mempool.write().unwrap();
        let state = node_a.state_db.read().unwrap();
        mp.insert(evm_tx, &*state, BlockHeight(0)).unwrap();
    }

    let block = node_a.propose_block(None, Round(0), 1000).unwrap();
    assert_eq!(block.transactions.len(), 1);
    assert!(block.transactions[0].is_evm());
    assert_ne!(block.header.txs_root, Hash::ZERO);
    assert_ne!(block.header.receipts_root, Hash::ZERO);
    assert!(block.header.gas_used > Gas::ZERO);

    // Test forged EVM txs_root
    let mut forged_txs = block.clone();
    forged_txs.header.txs_root = Hash([0xEE; 32]);
    assert_eq!(
        node_b.process_proposal(&forged_txs),
        Err(ConsensusError::InvalidTxsRoot)
    );

    // Test forged EVM receipts_root
    let mut forged_rec = block.clone();
    forged_rec.header.receipts_root = Hash([0xFF; 32]);
    assert_eq!(
        node_b.process_proposal(&forged_rec),
        Err(ConsensusError::InvalidReceiptsRoot)
    );

    // Test forged EVM gas_used
    let mut forged_gas = block.clone();
    forged_gas.header.gas_used = Gas(block.header.gas_used.0 + 1);
    assert_eq!(
        node_b.process_proposal(&forged_gas),
        Err(ConsensusError::InvalidGasUsed)
    );

    // Valid EVM proposal succeeds on Node B
    let vote_res = node_b.process_proposal(&block);
    assert!(
        vote_res.is_ok(),
        "Valid EVM proposal must be accepted: {:?}",
        vote_res
    );
    let vote = vote_res.unwrap();
    assert!(
        vote.is_some(),
        "Precommit vote must be emitted for EVM proposal"
    );
    let vote = vote.unwrap();
    let qc = node_b.process_vote(vote).unwrap().expect("QC formed");
    node_b.process_qc(&qc).expect("QC processed");
    assert_eq!(node_b.current_height(), BlockHeight(1));

    // Verify recipient received EVM transfer funds
    let state_b = node_b.state_db.read().unwrap();
    let recipient_acct = state_b.get_account(&recipient).unwrap().unwrap();
    assert_eq!(recipient_acct.balance, Amount(2_000_000_000));
}

/// 10. Final block commit lifecycle safety: verify that an accepted, validly executed proposal
///     does NOT alter persistent canonical state or height on disk before QC formation, and that
///     state becomes durable only when the QC commit boundary is reached.
#[test]
fn test_10_uncommitted_proposal_leaves_canonical_state_unchanged_and_commits_on_qc() {
    let chain_id = ChainId(210);
    let test_dir = unique_temp_dir("test10_commit_lifecycle");
    let wallet = TolaWallet::generate();
    let initial_balance = Amount(100_000_000_000_000_000);

    let dir_a = test_dir.join("node_a");
    let dir_b = test_dir.join("node_b");
    std::fs::create_dir_all(&dir_a).unwrap();
    std::fs::create_dir_all(&dir_b).unwrap();

    let state_db_a = PersistentStateDB::open(dir_a.join("state")).unwrap();
    let state_db_b = PersistentStateDB::open(dir_b.join("state")).unwrap();

    let val_kp = Ed25519Keypair::generate();
    let val_addr = Address([0x99; 20]);
    let val_info = ValidatorInfo {
        validator_id: ValidatorId(val_addr),
        consensus_pubkey: val_kp.public_key(),
        voting_power: 100_000,
        commission_bp: 0,
    };
    let committee = ValidatorSet {
        epoch: Epoch(0),
        validators: vec![val_info.clone()],
        total_voting_power: 100_000,
    };

    let genesis_accounts = vec![
        GenesisAccount {
            address: wallet.address(),
            balance: initial_balance,
        },
        GenesisAccount {
            address: val_addr,
            balance: initial_balance,
        },
    ];

    let genesis_cfg = GenesisConfig {
        chain_id,
        genesis_time: 1770000000,
        accounts: genesis_accounts,
        initial_validators: vec![val_info],
    };

    let cfg_a = NodeConfig {
        chain_id,
        data_dir: dir_a,
        is_validator: true,
        validator_keypair: Some(val_kp.clone()),
        operator_address: Some(val_addr),
        p2p_port: 39681,
        rpc_port: 8691,
        genesis: genesis_cfg.clone(),
    };
    let cfg_b = NodeConfig {
        chain_id,
        data_dir: dir_b.clone(),
        is_validator: true,
        validator_keypair: Some(val_kp),
        operator_address: Some(val_addr),
        p2p_port: 39682,
        rpc_port: 8692,
        genesis: genesis_cfg,
    };

    let node_a = TolaNode::new(cfg_a, state_db_a, committee.clone()).unwrap();
    let node_b = TolaNode::new(cfg_b, state_db_b, committee).unwrap();

    let initial_height = node_b.current_height();
    let initial_root = node_b.state_root();
    assert_eq!(initial_height, BlockHeight(0));

    let recipient = Address([0x55; 20]);
    let transfer_amount = Amount(1_000_000_000);
    let tx = build_native_tx(&wallet, chain_id, recipient, transfer_amount, 0);

    {
        let mut mp = node_a.mempool.write().unwrap();
        let state = node_a.state_db.read().unwrap();
        mp.insert(tx, &*state, BlockHeight(0)).unwrap();
    }

    let block = node_a.propose_block(None, Round(0), 1000).unwrap();

    // 1. Process proposal on replica Node B
    let vote_res = node_b.process_proposal(&block);
    assert!(vote_res.is_ok(), "Proposal is valid and accepted");
    let vote = vote_res.unwrap().expect("Precommit vote emitted");
    assert_eq!(vote.height, BlockHeight(1));

    // 2. VERIFY SPECULATIVE ISOLATION:
    // Even though the block proposal was successfully executed speculatively and voted on,
    // it MUST NOT alter canonical height or state on Node B prior to QC commit!
    assert_eq!(
        node_b.current_height(),
        BlockHeight(0),
        "Height must remain 0 before QC"
    );
    assert_eq!(
        node_b.state_root(),
        initial_root,
        "State root must remain unchanged before QC"
    );
    {
        let state_b = node_b.state_db.read().unwrap();
        assert!(
            state_b.get_account(&recipient).unwrap().is_none(),
            "Recipient must not exist in canonical state before QC"
        );
    }

    // 3. RESTART SAFETY: Reopening storage from disk must prove no persistent write occurred
    {
        let reopened_db = PersistentStateDB::open(dir_b.join("state")).unwrap();
        assert_eq!(
            reopened_db.current_height(),
            BlockHeight(0),
            "Disk storage must remain at height 0 across restart before QC"
        );
        assert_eq!(
            reopened_db.state_root(),
            initial_root,
            "Disk state root must match genesis across restart before QC"
        );
        assert!(
            reopened_db.get_account(&recipient).unwrap().is_none(),
            "Disk account table must NOT reflect uncommitted transfer"
        );
    }

    // 4. Ingest QC at the protocol commit boundary
    let qc = node_b.process_vote(vote).unwrap().expect("QC formed");
    node_b.process_qc(&qc).expect("QC committed successfully");

    // 5. VERIFY DURABLE COMMIT:
    assert_eq!(
        node_b.current_height(),
        BlockHeight(1),
        "Height must advance to 1 upon QC commit"
    );
    assert_eq!(
        node_b.state_root(),
        block.header.state_root,
        "State root must match block state root upon QC commit"
    );
    {
        let state_b = node_b.state_db.read().unwrap();
        let recipient_acct = state_b
            .get_account(&recipient)
            .unwrap()
            .expect("Recipient exists");
        assert_eq!(recipient_acct.balance, transfer_amount);
    }

    // 6. RESTART VERIFICATION AFTER COMMIT:
    // Verify that committed state is durable across restart
    {
        let reopened_db = PersistentStateDB::open(dir_b.join("state")).unwrap();
        assert_eq!(
            reopened_db.current_height(),
            BlockHeight(1),
            "Disk height must persist as 1 after QC commit"
        );
        assert_eq!(
            reopened_db.state_root(),
            block.header.state_root,
            "Disk state root must persist matching block after QC commit"
        );
        let recipient_acct = reopened_db
            .get_account(&recipient)
            .unwrap()
            .expect("Recipient exists on disk");
        assert_eq!(recipient_acct.balance, transfer_amount);
    }

    let _ = std::fs::remove_dir_all(&test_dir);
}

/// 11. QC verification: a QC with an invalid cryptographic signature is rejected,
///     leaving state height and storage completely uncommitted.
#[test]
fn test_11_qc_invalid_signature_rejected() {
    let chain_id = ChainId(211);
    let (node_a, node_b, wallet) = setup_cluster(chain_id, "test11_invalid_sig", vec![]);

    let recipient = Address([0x22; 20]);
    let tx = build_native_tx(&wallet, chain_id, recipient, Amount(500_000_000), 0);
    {
        let mut mp = node_a.mempool.write().unwrap();
        let state = node_a.state_db.read().unwrap();
        mp.insert(tx, &*state, BlockHeight(0)).unwrap();
    }

    let block = node_a.propose_block(None, Round(0), 1000).unwrap();
    let vote = node_b.process_proposal(&block).unwrap().unwrap();
    let mut bad_qc = node_b.process_vote(vote).unwrap().unwrap();

    // Tamper with the aggregated signature
    bad_qc.aggregated_signature[0] ^= 0xFF;

    let res = node_b.process_qc(&bad_qc);
    assert_eq!(res, Err(ConsensusError::InvalidSignature));
    assert_eq!(node_b.current_height(), BlockHeight(0));

    let state_b = node_b.state_db.read().unwrap();
    assert!(state_b.get_account(&recipient).unwrap().is_none());
}

/// 12. QC verification: a QC with insufficient voting power (below quorum threshold)
///     is rejected with InvalidQuorum.
#[test]
fn test_12_qc_insufficient_quorum_rejected() {
    let chain_id = ChainId(212);
    let temp_root = unique_temp_dir("test12_low_quorum");
    let kp1 = Ed25519Keypair::generate();
    let kp2 = Ed25519Keypair::generate();
    let kp3 = Ed25519Keypair::generate();

    let v1_id = ValidatorId(Address([0x01; 20]));
    let v2_id = ValidatorId(Address([0x02; 20]));
    let v3_id = ValidatorId(Address([0x03; 20]));

    let val1 = ValidatorInfo {
        validator_id: v1_id,
        consensus_pubkey: kp1.public_key(),
        voting_power: 100,
        commission_bp: 0,
    };
    let val2 = ValidatorInfo {
        validator_id: v2_id,
        consensus_pubkey: kp2.public_key(),
        voting_power: 100,
        commission_bp: 0,
    };
    let val3 = ValidatorInfo {
        validator_id: v3_id,
        consensus_pubkey: kp3.public_key(),
        voting_power: 100,
        commission_bp: 0,
    };

    // Total power = 300. Quorum threshold = 2 * 300 / 3 + 1 = 201.
    let committee = ValidatorSet {
        epoch: Epoch(0),
        validators: vec![val1.clone(), val2, val3],
        total_voting_power: 300,
    };

    let dir = temp_root.join("node");
    std::fs::create_dir_all(&dir).unwrap();
    let state_db = PersistentStateDB::open(dir.join("state")).unwrap();

    let genesis_cfg = GenesisConfig {
        chain_id,
        genesis_time: 1770000000,
        accounts: vec![],
        initial_validators: vec![val1],
    };

    let cfg = NodeConfig {
        chain_id,
        data_dir: dir,
        is_validator: true,
        validator_keypair: Some(kp1.clone()),
        operator_address: Some(Address([0x01; 20])),
        p2p_port: 39691,
        rpc_port: 8695,
        genesis: genesis_cfg,
    };

    let node = TolaNode::new(cfg, state_db, committee).unwrap();

    let block_hash = Hash([0xAA; 32]);
    let vote_hash = tola_consensus::qc::compute_vote_hash(
        VoteType::Precommit,
        BlockHeight(1),
        Round(0),
        &block_hash,
    );
    let sig1 = kp1.sign(&vote_hash.0);
    let sig1_bytes = match sig1 {
        Signature::Ed25519(s) => s.to_vec(),
        _ => unreachable!(),
    };

    // Only validator 1 voted (power 100 < 201)
    let low_qc = QuorumCertificate {
        height: BlockHeight(1),
        round: Round(0),
        block_hash,
        vote_type: VoteType::Precommit,
        validator_bitfield: vec![0b00000001],
        aggregated_signature: sig1_bytes,
    };

    let res = node.process_qc(&low_qc);
    assert_eq!(res, Err(ConsensusError::InvalidQuorum));
    assert_eq!(node.current_height(), BlockHeight(0));

    let _ = std::fs::remove_dir_all(&temp_root);
}

/// 13. QC verification: a QC containing mismatched block identity or height is rejected.
#[test]
fn test_13_qc_mismatched_block_identity_or_height_rejected() {
    let chain_id = ChainId(213);
    let (node_a, node_b, wallet) = setup_cluster(chain_id, "test13_mismatched_id", vec![]);

    let recipient = Address([0x33; 20]);
    let tx = build_native_tx(&wallet, chain_id, recipient, Amount(500_000_000), 0);
    {
        let mut mp = node_a.mempool.write().unwrap();
        let state = node_a.state_db.read().unwrap();
        mp.insert(tx, &*state, BlockHeight(0)).unwrap();
    }

    let block = node_a.propose_block(None, Round(0), 1000).unwrap();
    let vote = node_b.process_proposal(&block).unwrap().unwrap();
    let qc = node_b.process_vote(vote).unwrap().unwrap();

    // Case A: QC specifies height 2, but block header is height 1
    let mut bad_h_qc = qc.clone();
    bad_h_qc.height = BlockHeight(2);
    let res_h = node_b.process_qc(&bad_h_qc);
    assert!(res_h.is_err(), "Mismatched height must be rejected");
    assert_eq!(node_b.current_height(), BlockHeight(0));

    // Case B: QC specifies different block hash
    let mut bad_hash_qc = qc.clone();
    bad_hash_qc.block_hash = Hash([0x88; 32]);
    let res_hash = node_b.process_qc(&bad_hash_qc);
    assert!(res_hash.is_err(), "Mismatched block hash must be rejected");
    assert_eq!(node_b.current_height(), BlockHeight(0));
}

/// 14. QC verification: a QC for an unknown or nonexistent proposal cannot commit arbitrary state.
#[test]
fn test_14_qc_unknown_proposal_rejected() {
    let chain_id = ChainId(214);
    let (_node_a, node_b, _wallet) = setup_cluster(chain_id, "test14_unknown_qc", vec![]);

    // Form a QC for an unknown block hash
    let unknown_hash = Hash([0xEE; 32]);
    let val_kp = node_b.config.validator_keypair.clone().unwrap();
    let vote_hash = tola_consensus::qc::compute_vote_hash(
        VoteType::Precommit,
        BlockHeight(1),
        Round(0),
        &unknown_hash,
    );
    let sig = val_kp.sign(&vote_hash.0);
    let sig_bytes = match sig {
        Signature::Ed25519(s) => s.to_vec(),
        _ => unreachable!(),
    };

    let qc = QuorumCertificate {
        height: BlockHeight(1),
        round: Round(0),
        block_hash: unknown_hash,
        vote_type: VoteType::Precommit,
        validator_bitfield: vec![0b00000001],
        aggregated_signature: sig_bytes,
    };

    let res = node_b.process_qc(&qc);
    assert!(res.is_err(), "QC for unknown proposal must be rejected");
    assert_eq!(node_b.current_height(), BlockHeight(0));
}

/// 15. Replay safety: reprocessing the same QC cannot double-apply transactions or alter state.
#[test]
fn test_15_reprocessing_qc_idempotent_no_double_apply() {
    let chain_id = ChainId(215);
    let (node_a, node_b, wallet) = setup_cluster(chain_id, "test15_replay_qc", vec![]);

    let recipient = Address([0x44; 20]);
    let transfer_amount = Amount(500_000_000);
    let tx = build_native_tx(&wallet, chain_id, recipient, transfer_amount, 0);
    {
        let mut mp = node_a.mempool.write().unwrap();
        let state = node_a.state_db.read().unwrap();
        mp.insert(tx, &*state, BlockHeight(0)).unwrap();
    }

    let block = node_a.propose_block(None, Round(0), 1000).unwrap();
    let vote = node_b.process_proposal(&block).unwrap().unwrap();
    let qc = node_b.process_vote(vote).unwrap().unwrap();

    // First ingestion: commits Block 1
    node_b
        .process_qc(&qc)
        .expect("First QC processing succeeds");
    assert_eq!(node_b.current_height(), BlockHeight(1));
    {
        let state_b = node_b.state_db.read().unwrap();
        let acct = state_b.get_account(&recipient).unwrap().unwrap();
        assert_eq!(acct.balance, transfer_amount);
    }

    // Second ingestion: replay must be idempotent
    let res_replay = node_b.process_qc(&qc);
    assert!(
        res_replay.is_ok(),
        "Replay must be cleanly accepted as no-op"
    );
    assert_eq!(
        node_b.current_height(),
        BlockHeight(1),
        "Height must remain 1"
    );

    {
        let state_b = node_b.state_db.read().unwrap();
        let acct = state_b.get_account(&recipient).unwrap().unwrap();
        assert_eq!(
            acct.balance, transfer_amount,
            "Balance must NOT double-credit"
        );
    }
}

/// 16. Rejected QC leaves canonical state, storage, and indexes untouched.
#[test]
fn test_16_rejected_qc_leaves_canonical_state_unchanged() {
    let chain_id = ChainId(216);
    let (node_a, node_b, wallet) = setup_cluster(chain_id, "test16_rejected_qc_clean", vec![]);

    let initial_balance = {
        let state_b = node_b.state_db.read().unwrap();
        state_b
            .get_account(&wallet.address())
            .unwrap()
            .unwrap()
            .balance
    };

    let recipient = Address([0x66; 20]);
    let tx = build_native_tx(&wallet, chain_id, recipient, Amount(500_000_000), 0);
    {
        let mut mp = node_a.mempool.write().unwrap();
        let state = node_a.state_db.read().unwrap();
        mp.insert(tx, &*state, BlockHeight(0)).unwrap();
    }

    let block = node_a.propose_block(None, Round(0), 1000).unwrap();
    let _vote = node_b.process_proposal(&block).unwrap().unwrap();

    // Create a QC with invalid signature
    let val_kp = Ed25519Keypair::generate(); // Unbonded keypair
    let vote_hash = tola_consensus::qc::compute_vote_hash(
        VoteType::Precommit,
        BlockHeight(1),
        Round(0),
        &block.header.parent_hash,
    );
    let sig = val_kp.sign(&vote_hash.0);
    let sig_bytes = match sig {
        Signature::Ed25519(s) => s.to_vec(),
        _ => unreachable!(),
    };
    let bad_qc = QuorumCertificate {
        height: BlockHeight(1),
        round: Round(0),
        block_hash: tola_consensus::PipelinedBftEngine::compute_block_hash(&block),
        vote_type: VoteType::Precommit,
        validator_bitfield: vec![0b00000001],
        aggregated_signature: sig_bytes,
    };

    let res = node_b.process_qc(&bad_qc);
    assert!(res.is_err());

    // Canonical state must be 100% unchanged
    assert_eq!(node_b.current_height(), BlockHeight(0));
    let state_b = node_b.state_db.read().unwrap();
    assert_eq!(
        state_b
            .get_account(&wallet.address())
            .unwrap()
            .unwrap()
            .balance,
        initial_balance
    );
    assert!(state_b.get_account(&recipient).unwrap().is_none());
}

/// 17. Sequential chain enforcement: a QC that attempts to skip height (non-sequential)
///     is rejected, preserving canonical chain order.
#[test]
fn test_17_qc_non_sequential_height_rejected() {
    let chain_id = ChainId(217);
    let (_node_a, node_b, _wallet) = setup_cluster(chain_id, "test17_non_seq", vec![]);

    let val_kp = node_b.config.validator_keypair.clone().unwrap();
    let dummy_hash = Hash([0x55; 32]);
    // Skip to Height 3 when state is at Height 0
    let vote_hash = tola_consensus::qc::compute_vote_hash(
        VoteType::Precommit,
        BlockHeight(3),
        Round(0),
        &dummy_hash,
    );
    let sig = val_kp.sign(&vote_hash.0);
    let sig_bytes = match sig {
        Signature::Ed25519(s) => s.to_vec(),
        _ => unreachable!(),
    };

    let non_seq_qc = QuorumCertificate {
        height: BlockHeight(3),
        round: Round(0),
        block_hash: dummy_hash,
        vote_type: VoteType::Precommit,
        validator_bitfield: vec![0b00000001],
        aggregated_signature: sig_bytes,
    };

    let res = node_b.process_qc(&non_seq_qc);
    assert!(res.is_err());
    assert_eq!(node_b.current_height(), BlockHeight(0));
}

/// 18. Jolteon 2-chain commit rule verification:
///     - Block 1 achieves QC1 -> state commits Block 1, but last_finalized remains 0.
///     - Block 2 achieves QC2 extending Block 1 -> state commits Block 2, last_finalized advances to 1.
///     - Block 3 achieves QC3 extending Block 2 -> state commits Block 3, last_finalized advances to 2.
#[test]
fn test_18_jolteon_2chain_commit_and_finality_rule_tracking() {
    let chain_id = ChainId(218);
    let (node_a, node_b, wallet) = setup_cluster(chain_id, "test18_2chain_tracking", vec![]);

    let recipient = Address([0x77; 20]);

    // View 1 (Height 1): Block 1
    let tx1 = build_native_tx(&wallet, chain_id, recipient, Amount(100_000_000), 0);
    {
        let mut mp = node_a.mempool.write().unwrap();
        let state = node_a.state_db.read().unwrap();
        mp.insert(tx1, &*state, BlockHeight(0)).unwrap();
    }
    let block1 = node_a.propose_block(None, Round(0), 1000).unwrap();
    let vote1 = node_b.process_proposal(&block1).unwrap().unwrap();
    let qc1 = node_b.process_vote(vote1).unwrap().unwrap();
    node_b.process_qc(&qc1).unwrap();

    assert_eq!(node_b.current_height(), BlockHeight(1));
    assert_eq!(
        node_b.last_finalized_height(),
        BlockHeight(0),
        "Block 1 is certified by QC1, but not yet 2-chain finalized"
    );

    // View 2 (Height 2): Block 2 extending Block 1 with QC1
    let tx2 = build_native_tx(&wallet, chain_id, recipient, Amount(100_000_000), 1);
    {
        let mut mp = node_a.mempool.write().unwrap();
        let state = node_a.state_db.read().unwrap();
        mp.insert(tx2, &*state, BlockHeight(1)).unwrap();
    }
    let block2 = node_a.propose_block(Some(qc1), Round(0), 2000).unwrap();
    let vote2 = node_b.process_proposal(&block2).unwrap().unwrap();
    let qc2 = node_b.process_vote(vote2).unwrap().unwrap();
    node_b.process_qc(&qc2).unwrap();

    assert_eq!(node_b.current_height(), BlockHeight(2));
    assert_eq!(
        node_b.last_finalized_height(),
        BlockHeight(1),
        "Block 1 is now 2-chain finalized upon QC2 formation"
    );

    // View 3 (Height 3): Block 3 extending Block 2 with QC2
    let tx3 = build_native_tx(&wallet, chain_id, recipient, Amount(100_000_000), 2);
    {
        let mut mp = node_a.mempool.write().unwrap();
        let state = node_a.state_db.read().unwrap();
        mp.insert(tx3, &*state, BlockHeight(2)).unwrap();
    }
    let block3 = node_a.propose_block(Some(qc2), Round(0), 3000).unwrap();
    let vote3 = node_b.process_proposal(&block3).unwrap().unwrap();
    let qc3 = node_b.process_vote(vote3).unwrap().unwrap();
    node_b.process_qc(&qc3).unwrap();

    assert_eq!(node_b.current_height(), BlockHeight(3));
    assert_eq!(
        node_b.last_finalized_height(),
        BlockHeight(2),
        "Block 2 is now 2-chain finalized upon QC3 formation"
    );
}

#[test]
fn test_19_qc_block_hash_commits_to_txs_root_and_receipts_root() {
    let chain_id = ChainId(219);
    let (node_a, node_b, wallet) = setup_cluster(chain_id, "test19_block_hash_commitments", vec![]);
    let recipient = Address([0x44; 20]);

    // Submit transaction to mempool
    let tx1 = build_native_tx(&wallet, chain_id, recipient, Amount(500_000_000), 0);
    {
        let mut mp = node_a.mempool.write().unwrap();
        let state = node_a.state_db.read().unwrap();
        mp.insert(tx1, &*state, BlockHeight(0)).unwrap();
    }

    let valid_block = node_a.propose_block(None, Round(0), 1000).unwrap();
    let valid_hash = tola_consensus::PipelinedBftEngine::compute_block_hash(&valid_block);

    // Create block with forged txs_root (identical parent, height, round, state_root)
    let mut tampered_txs_block = valid_block.clone();
    tampered_txs_block.header.txs_root = Hash([0x99; 32]);
    let tampered_txs_hash =
        tola_consensus::PipelinedBftEngine::compute_block_hash(&tampered_txs_block);

    // Cryptographic block identity MUST differ
    assert_ne!(
        valid_hash, tampered_txs_hash,
        "Block hash must cryptographically bind txs_root"
    );

    // Create block with forged receipts_root (identical parent, height, round, state_root)
    let mut tampered_receipts_block = valid_block.clone();
    tampered_receipts_block.header.receipts_root = Hash([0x88; 32]);
    let tampered_receipts_hash =
        tola_consensus::PipelinedBftEngine::compute_block_hash(&tampered_receipts_block);

    // Cryptographic block identity MUST differ
    assert_ne!(
        valid_hash, tampered_receipts_hash,
        "Block hash must cryptographically bind receipts_root"
    );

    // A QC certifying valid_hash cannot certify tampered_txs_block
    let vote_valid = node_b.process_proposal(&valid_block).unwrap().unwrap();
    assert_eq!(vote_valid.block_hash, valid_hash);
    let qc_valid = node_b.process_vote(vote_valid).unwrap().unwrap();

    // Directly attempt to commit tampered block under valid QC: must fail block hash mismatch
    node_b
        .pending_proposals
        .write()
        .unwrap()
        .insert(qc_valid.block_hash, tampered_txs_block);
    let commit_res = node_b.process_qc(&qc_valid);
    assert!(
        commit_res.is_err(),
        "QC cannot commit proposal with tampered txs_root under valid block hash"
    );

    // Persistent state remains completely unmutated
    assert_eq!(node_b.current_height(), BlockHeight(0));
}
