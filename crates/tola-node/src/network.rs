//! Local multi-node network orchestration for cluster simulation, consensus, and state replication.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU16, Ordering};
use tola_core::StateDB;
use tola_crypto::Ed25519Keypair;
use tola_storage::PersistentStateDB;
use tola_types::*;

static NEXT_NETWORK_PORT: AtomicU16 = AtomicU16::new(31000);

use crate::config::{GenesisAccount, GenesisConfig, NodeConfig};
use crate::engine::TolaNode;

pub struct LocalMultiNodeNetwork {
    pub nodes: Vec<TolaNode>,
    pub committee_set: ValidatorSet,
    pub keypairs: Vec<Ed25519Keypair>,
    pub validator_ids: Vec<ValidatorId>,
    pub base_dir: PathBuf,
}

impl LocalMultiNodeNetwork {
    /// Creates a 3-validator local network cluster with default funded genesis accounts.
    pub fn new_3_validators(temp_dir: &Path, chain_id: ChainId) -> Result<Self, String> {
        let genesis_accounts = vec![
            GenesisAccount {
                address: Address([0xAA; 20]),
                balance: Amount::from_u128(1_000_000_000),
            },
            GenesisAccount {
                address: Address([0xBB; 20]),
                balance: Amount::from_u128(500_000_000),
            },
        ];
        Self::new(3, temp_dir, chain_id, genesis_accounts)
    }

    /// Creates an N-validator local network cluster ($N \ge 3$).
    pub fn new(
        num_validators: usize,
        temp_dir: &Path,
        chain_id: ChainId,
        genesis_accounts: Vec<GenesisAccount>,
    ) -> Result<Self, String> {
        assert!(
            num_validators >= 3,
            "Network must have at least 3 validators for BFT"
        );

        let mut keypairs = Vec::with_capacity(num_validators);
        let mut validator_ids = Vec::with_capacity(num_validators);
        let mut validator_infos = Vec::with_capacity(num_validators);

        for i in 0..num_validators {
            let kp = Ed25519Keypair::generate();
            let op_addr = Address([i as u8 + 1; 20]);
            let val_id = ValidatorId(op_addr);

            validator_infos.push(ValidatorInfo {
                validator_id: val_id,
                consensus_pubkey: kp.public_key(),
                voting_power: 100_000,
                commission_bp: 500, // 5%
            });

            keypairs.push(kp);
            validator_ids.push(val_id);
        }

        let total_power: u64 = validator_infos.iter().map(|v| v.voting_power).sum();
        let committee_set = ValidatorSet {
            epoch: Epoch(0),
            validators: validator_infos.clone(),
            total_voting_power: total_power,
        };

        let genesis = GenesisConfig {
            chain_id,
            genesis_time: 1770000000,
            accounts: genesis_accounts,
            initial_validators: validator_infos,
        };

        let mut nodes = Vec::with_capacity(num_validators);

        let base_port = NEXT_NETWORK_PORT.fetch_add(50, Ordering::SeqCst);

        for i in 0..num_validators {
            let node_dir = temp_dir.join(format!("validator_{}", i));
            std::fs::create_dir_all(&node_dir).map_err(|e| e.to_string())?;

            let state_db = PersistentStateDB::open(&node_dir)
                .map_err(|e| format!("Failed to open state DB for validator {}: {:?}", i, e))?;

            let config = NodeConfig {
                chain_id,
                data_dir: node_dir,
                is_validator: true,
                validator_keypair: Some(keypairs[i].clone()),
                operator_address: Some(validator_ids[i].0),
                p2p_port: base_port + i as u16,
                rpc_port: base_port + 20 + i as u16,
                genesis: genesis.clone(),
            };

            let node = TolaNode::new(config, state_db, committee_set.clone())?;
            nodes.push(node);
        }

        Ok(Self {
            nodes,
            committee_set,
            keypairs,
            validator_ids,
            base_dir: temp_dir.to_path_buf(),
        })
    }

    /// Determines which validator index is the deterministic leader for the view.
    pub fn leader_index_for_view(&self, height: BlockHeight, round: Round) -> usize {
        let leader_id = self.nodes[0]
            .consensus
            .read()
            .unwrap()
            .committee()
            .leader_for_view(height, round);

        self.nodes
            .iter()
            .position(|n| n.config.operator_address == Some(leader_id.0))
            .unwrap_or(0)
    }

    /// Submits a transaction to all nodes' mempools (simulating P2P gossip).
    pub fn submit_transaction(&self, tx: Transaction) -> Result<TransactionId, String> {
        let mut last_id = None;
        for (i, node) in self.nodes.iter().enumerate() {
            let state = node.state_db.read().unwrap();
            let height = node.current_height();
            let mut mp = node.mempool.write().unwrap();
            let tx_id = mp
                .insert(tx.clone(), &*state, height)
                .map_err(|e| format!("Node {} rejected tx: {:?}", i, e))?;
            last_id = Some(tx_id);
        }
        last_id.ok_or_else(|| "No nodes in network".to_string())
    }

    /// Executes one consensus round where all validators participate:
    /// 1. Identifies the view leader.
    /// 2. Leader proposes block candidate from pending transactions.
    /// 3. All validators process the proposal, execute transactions, and produce precommit votes.
    /// 4. All validators process votes and assemble threshold Quorum Certificate.
    /// 5. Evaluates 2-chain commit finality.
    pub fn step_round(
        &mut self,
        current_time_ms: u64,
    ) -> Result<(Block, QuorumCertificate), String> {
        let participating: Vec<usize> = (0..self.nodes.len()).collect();
        let (block, maybe_qc) =
            self.step_round_with_participating(&participating, current_time_ms)?;
        let qc =
            maybe_qc.ok_or_else(|| "Failed to reach quorum with all validators".to_string())?;
        Ok((block, qc))
    }

    /// Executes one consensus round with a specific subset of participating validator indices.
    pub fn step_round_with_participating(
        &mut self,
        participating: &[usize],
        current_time_ms: u64,
    ) -> Result<(Block, Option<QuorumCertificate>), String> {
        // 1. Determine current height from highest known QC across nodes
        let highest_h = self
            .nodes
            .iter()
            .filter_map(|n| n.highest_qc().map(|qc| qc.height.0))
            .max()
            .unwrap_or(0);
        let height = BlockHeight(highest_h + 1);
        let round = Round(0);

        // 2. Identify view leader
        let leader_idx = self.leader_index_for_view(height, round);
        let leader = &self.nodes[leader_idx];

        // 3. Leader proposes block
        let parent_qc = leader.highest_qc();
        let block = leader.propose_block(parent_qc, round, current_time_ms)?;

        // 4. Participating validators process proposal and cast precommit votes
        let mut votes = Vec::new();
        for &idx in participating {
            let node = &self.nodes[idx];
            if let Some(vote) = node
                .process_proposal(&block)
                .map_err(|e| format!("Validator {} rejected proposal: {:?}", idx, e))?
            {
                votes.push(vote);
            }
        }

        // 5. Broadcast votes to participating validators
        let mut assembled_qc = None;
        for vote in votes {
            for &idx in participating {
                let node = &self.nodes[idx];
                if let Some(qc) = node
                    .process_vote(vote.clone())
                    .map_err(|e| format!("Validator {} failed processing vote: {:?}", idx, e))?
                {
                    assembled_qc = Some(qc);
                }
            }
        }

        if let Some(ref qc) = assembled_qc {
            for &idx in participating {
                let node = &self.nodes[idx];
                let _ = node.process_qc(qc);
            }
        }

        Ok((block, assembled_qc))
    }

    /// Asserts that all nodes have converged to the exact same state height,
    /// identical state root, and identical finalized height.
    pub fn assert_state_consensus(&self) -> Hash {
        let first_height = self.nodes[0].current_height();
        let first_root = self.nodes[0].state_root();
        let first_finalized = self.nodes[0].last_finalized_height();

        for (i, node) in self.nodes.iter().enumerate() {
            assert_eq!(
                node.current_height(),
                first_height,
                "Node {} height mismatch: expected {:?}, got {:?}",
                i,
                first_height,
                node.current_height()
            );
            assert_eq!(
                node.state_root(),
                first_root,
                "Node {} state root mismatch: expected {:?}, got {:?}",
                i,
                first_root,
                node.state_root()
            );
            assert_eq!(
                node.last_finalized_height(),
                first_finalized,
                "Node {} finalized height mismatch: expected {:?}, got {:?}",
                i,
                first_finalized,
                node.last_finalized_height()
            );
        }

        first_root
    }

    /// Asserts that all nodes report identical balance for an account.
    pub fn assert_account_balance(&self, address: &Address, expected: Amount) {
        for (i, node) in self.nodes.iter().enumerate() {
            let acct = node
                .state_db
                .read()
                .unwrap()
                .get_account(address)
                .expect("State read error")
                .expect("Account not found");
            assert_eq!(
                acct.balance, expected,
                "Node {} balance mismatch for {:?}: expected {}, got {}",
                i, address, expected.0, acct.balance.0
            );
        }
    }
}
