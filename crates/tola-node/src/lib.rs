#![allow(clippy::all)]
pub mod config;
pub mod economics;
pub mod engine;
pub mod genesis;
pub mod governance;
pub mod metrics;
pub mod network;
pub mod security;
pub mod upgrade;

pub use config::{GenesisAccount, GenesisConfig, NodeConfig};
pub use economics::{
    BlockReward, EconomicEngine, EconomicError, EconomicParameters, BLOCKS_PER_YEAR_2S,
    MAINNET_GENESIS_SUPPLY, TOLA_BASE_UNITS,
};
pub use engine::{compute_receipts_root, compute_tx_digest, compute_txs_root, TolaNode};
pub use genesis::{compute_genesis_state_root, parse_genesis_json, serialize_genesis_json};
pub use governance::{
    ActiveProtocolParams, GovernanceConfig, GovernanceEngine, GovernanceError, ParamChange,
    Proposal, ProposalStatus, VoteOption,
};
pub use metrics::NodeMetrics;
pub use network::LocalMultiNodeNetwork;
pub use security::{InvariantViolation, SecurityAuditEngine};
pub use upgrade::{ProtocolVersion, UpgradeError, UpgradePlan, UpgradeSignalingManager};

#[cfg(test)]
mod tests {
    use tola_core::{BlockEngine, StateDB};
    use tola_execution::SequentialBlockEngine;
    use tola_state::MemoryStateJournal;
    use tola_types::*;

    const TEST_CHAIN_ID: ChainId = ChainId(9999);

    #[test]
    fn test_valid_transaction_state_transition() {
        let mut state = MemoryStateJournal::new();
        let alice = Address([1u8; 20]);
        let bob = Address([2u8; 20]);

        state.set_account(
            alice,
            AccountState {
                nonce: Nonce(0),
                balance: Amount(100_000_000),
                code_hash: Hash::ZERO,
                storage_root: Hash::ZERO,
                flags: 0,
            },
        );

        let tx = Transaction {
            chain_id: TEST_CHAIN_ID,
            nonce: Nonce(0),
            from: alice,
            to: Some(bob),
            value: Amount(40_000_000),
            gas_limit: Gas(21_000),
            max_fee_per_gas: Amount(1),
            max_priority_fee_per_gas: Amount(1),
            valid_until_block: BlockHeight(100),
            data: vec![],
            memo: None,
            signature: Signature::Ed25519([0u8; 64]),
            tx_type: 0,
            access_list: vec![],
        };

        let block = Block {
            header: BlockHeader {
                parent_hash: Hash::ZERO,
                height: BlockHeight(1),
                epoch: Epoch(0),
                round: Round(0),
                timestamp: Timestamp(1000),
                proposer: alice,
                state_root: Hash::ZERO,
                txs_root: Hash::ZERO,
                receipts_root: Hash::ZERO,
                gas_used: Gas(21_000),
                gas_limit: Gas(30_000_000),
                base_fee_per_gas: Amount::ZERO,
                extra_data: vec![],
            },
            transactions: vec![tx],
            last_commit_qc: None,
        };

        let engine = SequentialBlockEngine::new(TEST_CHAIN_ID);
        assert!(engine.validate_block(&block, &state).is_ok());
        let receipts = engine.execute_block(&block, &mut state).unwrap();
        assert_eq!(receipts.len(), 1);
        // Alice was deducted 40,000,000 value + 21,000 gas per D01/D04
        assert_eq!(
            state.get_account(&alice).unwrap().unwrap().balance.0,
            59_979_000
        );
        assert_eq!(
            state.get_account(&bob).unwrap().unwrap().balance.0,
            40_000_000
        );
        assert_eq!(
            state
                .get_account(&Address::VALIDATOR_REWARD_POOL)
                .unwrap()
                .unwrap()
                .balance
                .0,
            15_750
        );
        assert_eq!(
            state
                .get_account(&Address::PROTOCOL_TREASURY)
                .unwrap()
                .unwrap()
                .balance
                .0,
            5_250
        );
    }

    #[test]
    fn test_invalid_nonce_rejected() {
        let mut state = MemoryStateJournal::new();
        let alice = Address([1u8; 20]);
        state.set_account(
            alice,
            AccountState {
                nonce: Nonce(5),
                balance: Amount(100_000),
                code_hash: Hash::ZERO,
                storage_root: Hash::ZERO,
                flags: 0,
            },
        );

        let tx = Transaction {
            chain_id: TEST_CHAIN_ID,
            nonce: Nonce(0), // Out of order!
            from: alice,
            to: None,
            value: Amount(10),
            gas_limit: Gas(21_000),
            max_fee_per_gas: Amount(1),
            max_priority_fee_per_gas: Amount(1),
            valid_until_block: BlockHeight(100),
            data: vec![],
            memo: None,
            signature: Signature::Ed25519([0u8; 64]),
            tx_type: 0,
            access_list: vec![],
        };

        let block = Block {
            header: BlockHeader {
                parent_hash: Hash::ZERO,
                height: BlockHeight(1),
                epoch: Epoch(0),
                round: Round(0),
                timestamp: Timestamp(1000),
                proposer: alice,
                state_root: Hash::ZERO,
                txs_root: Hash::ZERO,
                receipts_root: Hash::ZERO,
                gas_used: Gas(21_000),
                gas_limit: Gas(30_000_000),
                base_fee_per_gas: Amount::ZERO,
                extra_data: vec![],
            },
            transactions: vec![tx],
            last_commit_qc: None,
        };

        let engine = SequentialBlockEngine::new(TEST_CHAIN_ID);
        assert!(engine.validate_block(&block, &state).is_err());
    }

    #[test]
    fn test_insufficient_balance_rejected() {
        let mut state = MemoryStateJournal::new();
        let alice = Address([1u8; 20]);
        state.set_account(
            alice,
            AccountState {
                nonce: Nonce(0),
                balance: Amount(500),
                code_hash: Hash::ZERO,
                storage_root: Hash::ZERO,
                flags: 0,
            },
        );

        let tx = Transaction {
            chain_id: TEST_CHAIN_ID,
            nonce: Nonce(0),
            from: alice,
            to: None,
            value: Amount(10_000), // Exceeds balance!
            gas_limit: Gas(21_000),
            max_fee_per_gas: Amount(1),
            max_priority_fee_per_gas: Amount(1),
            valid_until_block: BlockHeight(100),
            data: vec![],
            memo: None,
            signature: Signature::Ed25519([0u8; 64]),
            tx_type: 0,
            access_list: vec![],
        };

        let block = Block {
            header: BlockHeader {
                parent_hash: Hash::ZERO,
                height: BlockHeight(1),
                epoch: Epoch(0),
                round: Round(0),
                timestamp: Timestamp(1000),
                proposer: alice,
                state_root: Hash::ZERO,
                txs_root: Hash::ZERO,
                receipts_root: Hash::ZERO,
                gas_used: Gas(21_000),
                gas_limit: Gas(30_000_000),
                base_fee_per_gas: Amount::ZERO,
                extra_data: vec![],
            },
            transactions: vec![tx],
            last_commit_qc: None,
        };

        let engine = SequentialBlockEngine::new(TEST_CHAIN_ID);
        assert!(engine.validate_block(&block, &state).is_err());
    }

    #[test]
    fn test_persistent_block_execution_and_crash_recovery() {
        use tola_codec::CanonicalSerialize;
        use tola_storage::{PersistentStateDB, SegmentStorage, RECORD_BLOCK_BODY};

        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let temp_dir = std::env::temp_dir().join(format!("tola_node_persistent_{nanos}"));

        let alice = Address([0x11; 20]);
        let bob = Address([0x22; 20]);
        let proposer = Address([0x33; 20]);

        let committed_root: Hash;

        // --------------------------------------------------------------------
        // Phase 1: Genesis commit and Block 1 execution on disk
        // --------------------------------------------------------------------
        {
            let mut state = PersistentStateDB::open(&temp_dir).expect("Open persistent state");
            let mut segments = SegmentStorage::open(temp_dir.join("segments"), 64 * 1024 * 1024)
                .expect("Open segment storage");

            // Genesis accounts
            state
                .apply_delta(tola_core::StateDelta {
                    accounts: vec![
                        (
                            alice,
                            Some(AccountState::new_with_balance(Amount::from_u128(
                                500_000_000,
                            ))),
                        ),
                        (
                            bob,
                            Some(AccountState::new_with_balance(Amount::from_u128(
                                100_000_000,
                            ))),
                        ),
                    ],
                    storage: vec![],
                    code: vec![],
                })
                .unwrap();
            state.commit(BlockHeight(0)).unwrap();

            // Block 1 Transaction
            let tx = Transaction {
                chain_id: TEST_CHAIN_ID,
                nonce: Nonce(0),
                from: alice,
                to: Some(bob),
                value: Amount::from_u128(150_000_000),
                gas_limit: Gas(21_000),
                max_fee_per_gas: Amount::from_u128(10),
                max_priority_fee_per_gas: Amount::from_u128(2),
                valid_until_block: BlockHeight(100),
                data: vec![],
                memo: None,
                signature: Signature::Ed25519([0u8; 64]),
                tx_type: 0,
                access_list: vec![],
            };

            let block = Block {
                header: BlockHeader {
                    parent_hash: Hash::ZERO,
                    height: BlockHeight(1),
                    epoch: Epoch(0),
                    round: Round(0),
                    timestamp: Timestamp(2000),
                    proposer,
                    state_root: Hash::ZERO,
                    txs_root: Hash::ZERO,
                    receipts_root: Hash::ZERO,
                    gas_used: Gas(21_000),
                    gas_limit: Gas(30_000_000),
                    base_fee_per_gas: Amount::from_u128(5),
                    extra_data: vec![],
                },
                transactions: vec![tx],
                last_commit_qc: None,
            };

            let engine = SequentialBlockEngine::new(TEST_CHAIN_ID);
            assert!(engine.validate_block(&block, &state).is_ok());

            let receipts = engine
                .execute_block(&block, &mut state)
                .expect("Block execution succeeds");
            assert_eq!(receipts.len(), 1);

            // Persist block body into segment storage
            let block_bytes = block.header.to_canonical_bytes();
            let saved_loc = segments.append(RECORD_BLOCK_BODY, &block_bytes).unwrap();
            segments.sync().unwrap();

            // Commit block 1 state to disk
            committed_root = state.commit(BlockHeight(1)).expect("Commit block 1 state");
            assert_ne!(committed_root, Hash::ZERO);

            // Verify reading back from segment storage within Phase 1
            assert_eq!(segments.read(&saved_loc).unwrap(), block_bytes);

            // Store index bytes to simulate block index lookup across restart
            let index_bytes = saved_loc.to_bytes();

            // Phase 2: verify recovery across simulated node restart
            let state_reopened =
                PersistentStateDB::open(&temp_dir).expect("Reopen persistent state");
            let segments_reopened =
                SegmentStorage::open(temp_dir.join("segments"), 64 * 1024 * 1024)
                    .expect("Reopen segment storage");

            // Verify state recovered from disk
            assert_eq!(state_reopened.current_height(), BlockHeight(1));
            assert_eq!(state_reopened.state_root(), committed_root);

            let alice_acct = state_reopened
                .get_account(&alice)
                .unwrap()
                .expect("Alice exists");
            let bob_acct = state_reopened
                .get_account(&bob)
                .unwrap()
                .expect("Bob exists");
            let pool_acct = state_reopened
                .get_account(&Address::VALIDATOR_REWARD_POOL)
                .expect("Get validator pool")
                .expect("Validator pool exists");

            // Alice was deducted 150,000,000 + (21,000 * 7 = 147,000 gas)
            assert_eq!(alice_acct.nonce, Nonce(1));
            assert_eq!(
                alice_acct.balance,
                Amount::from_u128(500_000_000 - 150_000_000 - 147_000)
            );

            // Bob received 150,000,000
            assert_eq!(
                bob_acct.balance,
                Amount::from_u128(100_000_000 + 150_000_000)
            );

            // 75% of 147,000 gas fee = 110,250 to Validator Reward Pool per D01/D04
            assert_eq!(pool_acct.balance, Amount::from_u128(110_250));

            // Proposer receives zero direct fee credits
            assert_eq!(
                state_reopened
                    .get_account(&proposer)
                    .unwrap()
                    .unwrap_or(AccountState::EMPTY)
                    .balance,
                Amount::ZERO
            );

            let treasury_acct = state_reopened
                .get_account(&Address::PROTOCOL_TREASURY)
                .expect("Get treasury")
                .expect("Treasury exists");
            assert_eq!(treasury_acct.balance, Amount::from_u128(36_750));

            // Read segment using deserialized index pointer
            let loc_from_index = tola_storage::SegmentLocation::from_bytes(&index_bytes);
            let read_block_bytes = segments_reopened
                .read(&loc_from_index)
                .expect("Read from segment");
            assert_eq!(read_block_bytes, block_bytes);
        }

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_multi_node_3_validator_consensus_and_state_replication() {
        use crate::network::LocalMultiNodeNetwork;

        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let temp_dir = std::env::temp_dir().join(format!("tola_multinode_test_{nanos}"));

        let alice = Address([0xAA; 20]);
        let bob = Address([0xBB; 20]);

        let mut cluster = LocalMultiNodeNetwork::new_3_validators(&temp_dir, TEST_CHAIN_ID)
            .expect("Cluster initialization succeeds");

        // --------------------------------------------------------------------
        // Genesis state verification across all 3 independent nodes
        // --------------------------------------------------------------------
        assert_eq!(cluster.nodes.len(), 3);
        let genesis_root = cluster.assert_state_consensus();
        assert_ne!(genesis_root, Hash::ZERO);
        cluster.assert_account_balance(&alice, Amount::from_u128(1_000_000_000));
        cluster.assert_account_balance(&bob, Amount::from_u128(500_000_000));

        // --------------------------------------------------------------------
        // Round 1: Alice transfers 100,000,000 TOLA to Bob
        // --------------------------------------------------------------------
        let tx1 = Transaction {
            chain_id: TEST_CHAIN_ID,
            nonce: Nonce(0),
            from: alice,
            to: Some(bob),
            value: Amount::from_u128(100_000_000),
            gas_limit: Gas(21_000),
            max_fee_per_gas: Amount::from_u128(10),
            max_priority_fee_per_gas: Amount::from_u128(2),
            valid_until_block: BlockHeight(100),
            data: vec![],
            memo: None,
            signature: Signature::Ed25519([0u8; 64]),
            tx_type: 0,
            access_list: vec![],
        };

        cluster.submit_transaction(tx1).expect("Submit tx1");
        let (block_1, qc_1) = cluster
            .step_round(1770000000 * 1000 + 2000)
            .expect("Round 1 step");

        assert_eq!(block_1.header.height, BlockHeight(1));
        assert_eq!(qc_1.height, BlockHeight(1));
        assert_eq!(qc_1.round, Round(0));
        assert_eq!(block_1.transactions.len(), 1);

        // State consensus across all 3 nodes after Round 1
        let root_1 = cluster.assert_state_consensus();
        assert_ne!(root_1, genesis_root);
        cluster.assert_account_balance(&bob, Amount::from_u128(600_000_000));
        cluster.assert_account_balance(
            &alice,
            Amount::from_u128(1_000_000_000 - 100_000_000 - 210_000),
        );

        // Jolteon 2-chain rule: Block 1 is not finalized yet (last_finalized = 0)
        for node in &cluster.nodes {
            assert_eq!(node.last_finalized_height(), BlockHeight(0));
        }

        // --------------------------------------------------------------------
        // Round 2: Bob transfers 50,000,000 TOLA back to Alice
        // --------------------------------------------------------------------
        let tx2 = Transaction {
            chain_id: TEST_CHAIN_ID,
            nonce: Nonce(0),
            from: bob,
            to: Some(alice),
            value: Amount::from_u128(50_000_000),
            gas_limit: Gas(21_000),
            max_fee_per_gas: Amount::from_u128(10),
            max_priority_fee_per_gas: Amount::from_u128(2),
            valid_until_block: BlockHeight(100),
            data: vec![],
            memo: None,
            signature: Signature::Ed25519([0u8; 64]),
            tx_type: 0,
            access_list: vec![],
        };

        cluster.submit_transaction(tx2).expect("Submit tx2");
        let (block_2, qc_2) = cluster
            .step_round(1770000000 * 1000 + 4000)
            .expect("Round 2 step");

        assert_eq!(block_2.header.height, BlockHeight(2));
        assert_eq!(qc_2.height, BlockHeight(2));
        assert_eq!(block_2.last_commit_qc, Some(qc_1));
        assert_eq!(block_2.transactions.len(), 1);

        // State consensus across all 3 nodes after Round 2
        let root_2 = cluster.assert_state_consensus();
        assert_ne!(root_2, root_1);
        cluster.assert_account_balance(&bob, Amount::from_u128(600_000_000 - 50_000_000 - 210_000));

        // Jolteon 2-chain rule: Block 2's QC commits Block 1!
        for (i, node) in cluster.nodes.iter().enumerate() {
            assert_eq!(
                node.last_finalized_height(),
                BlockHeight(1),
                "Node {} should have finalized Block 1",
                i
            );
        }

        // --------------------------------------------------------------------
        // Round 3: Block 3 proposed (empty block)
        // --------------------------------------------------------------------
        let (block_3, qc_3) = cluster
            .step_round(1770000000 * 1000 + 6000)
            .expect("Round 3 step");

        assert_eq!(block_3.header.height, BlockHeight(3));
        assert_eq!(qc_3.height, BlockHeight(3));
        assert_eq!(block_3.last_commit_qc, Some(qc_2));

        // Jolteon 2-chain rule: Block 3's QC commits Block 2!
        for (i, node) in cluster.nodes.iter().enumerate() {
            assert_eq!(
                node.last_finalized_height(),
                BlockHeight(2),
                "Node {} should have finalized Block 2",
                i
            );
        }

        // --------------------------------------------------------------------
        // JSON-RPC interface verification on Node 0 and Node 1
        // --------------------------------------------------------------------
        let rpc_node0 = &cluster.nodes[0].rpc;
        let block_num_res = rpc_node0
            .dispatch_json(r#"{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}"#);
        assert!(
            block_num_res.contains(r#""result":"0x3""#),
            "eth_blockNumber response: {}",
            block_num_res
        );

        let bob_balance_req = format!(
            r#"{{"jsonrpc":"2.0","method":"eth_getBalance","params":["{}"],"id":2}}"#,
            bob.to_hex()
        );
        let balance_res = rpc_node0.dispatch_json(&bob_balance_req);
        assert!(
            balance_res.contains(r#""result":"0x"#),
            "eth_getBalance response: {}",
            balance_res
        );

        let qc_req =
            r#"{"jsonrpc":"2.0","method":"tola_getQuorumCertificate","params":[2],"id":3}"#;
        let qc_res = cluster.nodes[1].rpc.dispatch_json(qc_req);
        assert!(
            qc_res.contains(r#""block_hash""#),
            "tola_getQuorumCertificate response: {}",
            qc_res
        );

        let val_req = r#"{"jsonrpc":"2.0","method":"tola_getValidators","params":[],"id":4}"#;
        let val_res = cluster.nodes[2].rpc.dispatch_json(val_req);
        assert!(
            val_res.contains(r#""voting_power":100000"#),
            "tola_getValidators response: {}",
            val_res
        );

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_multi_node_byzantine_fault_tolerance_with_offline_validator() {
        use crate::config::GenesisAccount;
        use crate::network::LocalMultiNodeNetwork;

        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let temp_dir = std::env::temp_dir().join(format!("tola_bft_offline_test_{nanos}"));

        let genesis_accounts = vec![GenesisAccount {
            address: Address([0x11; 20]),
            balance: Amount::from_u128(1_000_000_000),
        }];

        // 4 validators: Total voting power = 400,000. Quorum threshold = (400,000 * 2) / 3 + 1 = 266,667.
        // Tolerates f = 1 faulty/offline validator (3f + 1 = 4).
        let mut cluster = LocalMultiNodeNetwork::new(4, &temp_dir, TEST_CHAIN_ID, genesis_accounts)
            .expect("4-validator cluster initialization succeeds");

        assert_eq!(cluster.nodes.len(), 4);

        // Case 1: Validator 3 is completely offline. Participating: [0, 1, 2]
        // Accumulated voting power = 300,000 >= 266,667 quorum threshold!
        let participating = vec![0, 1, 2];
        let (block_1, maybe_qc) = cluster
            .step_round_with_participating(&participating, 1770000000 * 1000 + 2000)
            .expect("Round step succeeds with 3 of 4 validators");

        assert!(
            maybe_qc.is_some(),
            "Quorum Certificate must assemble with 3 of 4 validators"
        );
        let qc_1 = maybe_qc.unwrap();
        assert_eq!(block_1.header.height, BlockHeight(1));
        assert_eq!(qc_1.height, BlockHeight(1));

        // Participating nodes have converged to consensual state root at height 1
        let root_0 = cluster.nodes[0].state_root();
        let root_1 = cluster.nodes[1].state_root();
        let root_2 = cluster.nodes[2].state_root();
        assert_eq!(root_0, root_1);
        assert_eq!(root_1, root_2);

        // Case 2: Only 2 validators participate: [0, 1]
        // Accumulated power = 200,000 < 266,667 quorum threshold!
        // Quorum CANNOT be reached!
        let insufficient = vec![0, 1];
        let (_block_2, insufficient_qc) = cluster
            .step_round_with_participating(&insufficient, 1770000000 * 1000 + 4000)
            .expect("Step completes");

        assert!(
            insufficient_qc.is_none(),
            "Quorum must fail when accumulated voting power is below 2/3 + 1"
        );

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
