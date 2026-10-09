//! Production TOLA node daemon orchestrating consensus, parallel execution, storage, and RPC.

use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::{Arc, RwLock};

use tola_consensus::{CommitteeManager, PacemakerConfig, PipelinedBftEngine};
use tola_core::{BlockEngine, ConsensusEngine, ConsensusError, StateDB, StateDelta};
use tola_crypto::{blake3_hash, Ed25519Keypair};
use tola_execution::{ParallelBlockEngine, ParallelEngineConfig};
use tola_mempool::{BlockAssembler, Mempool, MempoolConfig};
use tola_rpc::{ChainIndexer, IndexerStatus, RpcDispatcher};
use tola_storage::PersistentStateDB;
use tola_types::*;

use crate::config::NodeConfig;

pub fn compute_tx_digest(tx: &Transaction) -> Hash {
    use tola_codec::CanonicalSerialize;
    let b = tx.to_canonical_bytes();
    blake3_hash(&b)
}

#[derive(Clone)]
pub struct TolaNode {
    pub config: NodeConfig,
    pub state_db: Arc<RwLock<PersistentStateDB>>,
    pub mempool: Arc<RwLock<Mempool>>,
    pub block_engine: Arc<ParallelBlockEngine>,
    pub consensus: Arc<RwLock<PipelinedBftEngine>>,
    pub rpc: Arc<RpcDispatcher<PersistentStateDB>>,
    pub governance: Arc<RwLock<tola_consensus::governance::GovernanceEngine>>,
    pub indexer: Arc<ChainIndexer<PersistentStateDB>>,
    pub highest_block: Arc<RwLock<Option<Block>>>,
    pub pending_proposals: Arc<RwLock<HashMap<Hash, Block>>>,
}

impl TolaNode {
    pub fn new(
        config: NodeConfig,
        mut state_db: PersistentStateDB,
        committee_set: ValidatorSet,
    ) -> Result<Self, String> {
        let chain_id = config.chain_id;

        // Initialize genesis accounts if state is fresh
        if state_db.compute_state_root().unwrap_or(Hash::ZERO) == Hash::ZERO {
            let mut deltas = Vec::new();
            for acct in &config.genesis.accounts {
                deltas.push((
                    acct.address,
                    Some(AccountState::new_with_balance(acct.balance)),
                ));
            }
            if !deltas.is_empty() {
                state_db
                    .apply_delta(StateDelta {
                        accounts: deltas,
                        storage: vec![],
                        code: vec![],
                    })
                    .map_err(|e| format!("Failed to apply genesis accounts: {:?}", e))?;
                let _ = state_db.commit(BlockHeight(0));
            }
        }

        let state_arc = Arc::new(RwLock::new(state_db));
        let mempool_arc = Arc::new(RwLock::new(Mempool::new(MempoolConfig::new(chain_id))));
        let block_engine = Arc::new(ParallelBlockEngine::new(
            chain_id,
            ParallelEngineConfig::default(),
        ));

        // Consensus initialization
        let val_id = ValidatorId(config.operator_address.unwrap_or(Address::ZERO));
        let keypair = config
            .validator_keypair
            .clone()
            .unwrap_or_else(Ed25519Keypair::generate);
        let comm_mgr = CommitteeManager::new(committee_set.clone())
            .map_err(|e| format!("Invalid committee set: {:?}", e))?;

        let initial_h = state_arc.read().unwrap().current_height();
        let consensus = Arc::new(RwLock::new(PipelinedBftEngine::new(
            val_id,
            keypair,
            comm_mgr,
            PacemakerConfig::default(),
            initial_h,
            config.genesis.genesis_time * 1000,
        )));

        // RPC dispatcher
        let rpc = Arc::new(RpcDispatcher::new(
            chain_id,
            "TOLA/v0.1.0-alpha".into(),
            state_arc.clone(),
            mempool_arc.clone(),
        ));
        rpc.current_height.store(initial_h.0, Ordering::SeqCst);
        *rpc.active_committee.write().unwrap() = Some(committee_set);

        let indexer = Arc::new(ChainIndexer::new(rpc.clone()));
        let governance = rpc.governance.clone();

        Ok(Self {
            config,
            state_db: state_arc,
            mempool: mempool_arc,
            block_engine,
            consensus,
            rpc,
            governance,
            indexer,
            highest_block: Arc::new(RwLock::new(None)),
            pending_proposals: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Assembles, executes, and proposes a new block candidate if elected leader.
    pub fn propose_block(
        &self,
        parent_qc: Option<QuorumCertificate>,
        round: Round,
        current_time_ms: u64,
    ) -> Result<Block, String> {
        let (height, parent_hash) = match &parent_qc {
            Some(qc) => (BlockHeight(qc.height.0 + 1), qc.block_hash),
            None => {
                let cur = self.current_height();
                let parent = self
                    .highest_block
                    .read()
                    .unwrap()
                    .as_ref()
                    .map(|b| tola_consensus::PipelinedBftEngine::compute_block_hash(b))
                    .unwrap_or(Hash::ZERO);
                (BlockHeight(cur.0 + 1), parent)
            }
        };
        let proposer = self.config.operator_address.unwrap_or(Address::ZERO);

        let base_fee = Amount::from_u128(1_000_000_000); // 1 Gwei
        let assembler = BlockAssembler::new(self.config.chain_id);
        let mut candidate_block = {
            let mp = self.mempool.read().unwrap();
            assembler.assemble_block(
                &*mp,
                100,
                Gas(30_000_000),
                base_fee,
                proposer,
                parent_hash,
                height,
                Timestamp(current_time_ms / 1000),
            )
        };
        candidate_block.header.round = round;
        candidate_block.header.epoch = Epoch(height.0 / 21600);
        candidate_block.last_commit_qc = parent_qc;

        // 3. Parallel Block-STM execution against StateDB
        let receipts = {
            let mut state = self.state_db.write().unwrap();
            self.block_engine
                .execute_block(&candidate_block, &mut *state)
                .map_err(|e| format!("Block execution failed: {:?}", e))?
        };

        // 4. Compute state root and finalize block roots
        let state_root = {
            let state = self.state_db.read().unwrap();
            state
                .compute_state_root()
                .map_err(|e| format!("Failed to compute block state root: {:?}", e))?
        };

        let cumulative_gas = receipts
            .last()
            .map(|r| r.cumulative_gas_used)
            .unwrap_or(Gas::ZERO);

        // 5. Finalize block roots
        candidate_block.header.state_root = state_root;
        candidate_block.header.gas_used = cumulative_gas;
        candidate_block.header.txs_root = compute_txs_root(&candidate_block.transactions);
        candidate_block.header.receipts_root = compute_receipts_root(&receipts);

        let block_hash = tola_consensus::PipelinedBftEngine::compute_block_hash(&candidate_block);

        let is_solo = self.is_standalone();

        if is_solo {
            // Standalone solo node: commit immediately because single validator is 100% quorum
            {
                let mut state = self.state_db.write().unwrap();
                state
                    .commit_block_bundle(&candidate_block, block_hash, &receipts)
                    .map_err(|e| format!("Failed to commit block state bundle: {:?}", e))?;
            }

            // Check and execute matured governance proposals upon canonical block commit
            self.governance
                .write()
                .unwrap()
                .check_and_execute_matured_proposals(candidate_block.header.height);

            // Remove committed transactions from mempool
            self.mempool
                .write()
                .unwrap()
                .remove_committed(&candidate_block.transactions);

            // Record into indexer / RPC caches and node state
            self.indexer.index_block(candidate_block.clone(), receipts);
            *self.highest_block.write().unwrap() = Some(candidate_block.clone());
        } else {
            // Multi-validator committee: roll back speculative working cache so persistence waits for QC commit
            {
                let mut state = self.state_db.write().unwrap();
                let _ = state.rollback();
            }
            self.pending_proposals
                .write()
                .unwrap()
                .insert(block_hash, candidate_block.clone());
        }

        Ok(candidate_block)
    }

    /// Convenience helper to propose next block using the node's highest known Quorum Certificate.
    pub fn propose_next_block(&self, round: Round, current_time_ms: u64) -> Result<Block, String> {
        let parent_qc = self.highest_qc();
        self.propose_block(parent_qc, round, current_time_ms)
    }

    /// Handles an incoming block proposal, verifying consensus preconditions,
    /// executing state if not yet committed, and producing a precommit vote if valid.
    pub fn process_proposal(&self, block: &Block) -> Result<Option<Vote>, ConsensusError> {
        let height = block.header.height;

        // 1. Verify consensus preconditions before state execution
        {
            let c_guard = self.consensus.read().unwrap();
            if height.0 < c_guard.pacemaker().current_height().0 {
                return Err(ConsensusError::StaleRound);
            }
            let expected_leader = c_guard
                .committee()
                .leader_for_view(height, block.header.round);
            if ValidatorId(block.header.proposer) != expected_leader {
                return Err(ConsensusError::LeaderMismatch);
            }
            if height.0 > 1 {
                let parent_qc = block
                    .last_commit_qc
                    .as_ref()
                    .ok_or(ConsensusError::InvalidParentQC)?;
                tola_consensus::verify_qc(parent_qc, c_guard.committee())?;
                if parent_qc.block_hash != block.header.parent_hash {
                    return Err(ConsensusError::InvalidParentQC);
                }
                if let Some(locked) = c_guard.locked_qc() {
                    if parent_qc.round.0 < locked.round.0 {
                        return Err(ConsensusError::LockedBlockMismatch);
                    }
                }
            }
        }

        // 2. Pre-execution block commitment and limit validation
        if block.header.gas_used > block.header.gas_limit {
            return Err(ConsensusError::GasLimitExceeded);
        }

        let computed_txs_root = compute_txs_root(&block.transactions);
        if block.header.txs_root != computed_txs_root {
            return Err(ConsensusError::InvalidTxsRoot);
        }

        // 3. Speculative execution against StateDB if not yet applied
        {
            let mut state = self.state_db.write().unwrap();
            if state.current_height().0 < height.0 {
                let receipts = match self.block_engine.execute_block(block, &mut *state) {
                    Ok(r) => r,
                    Err(e) => {
                        let _ = state.rollback();
                        return Err(ConsensusError::InternalError(format!(
                            "Block execution failed: {:?}",
                            e
                        )));
                    }
                };

                let computed_receipts_root = compute_receipts_root(&receipts);
                if block.header.receipts_root != computed_receipts_root {
                    let _ = state.rollback();
                    return Err(ConsensusError::InvalidReceiptsRoot);
                }

                let derived_gas_used = receipts
                    .last()
                    .map(|r| r.cumulative_gas_used)
                    .unwrap_or(Gas::ZERO);
                if block.header.gas_used != derived_gas_used {
                    let _ = state.rollback();
                    return Err(ConsensusError::InvalidGasUsed);
                }

                let computed_state_root = match state.compute_state_root() {
                    Ok(r) => r,
                    Err(e) => {
                        let _ = state.rollback();
                        return Err(ConsensusError::InternalError(format!(
                            "Failed to compute state root: {:?}",
                            e
                        )));
                    }
                };

                if block.header.state_root != computed_state_root {
                    let _ = state.rollback();
                    return Err(ConsensusError::InvalidStateRoot);
                }

                // Speculative execution verified. Roll back working state so persistent storage
                // is NOT modified prior to QC commit boundary.
                let _ = state.rollback();

                let block_hash = tola_consensus::PipelinedBftEngine::compute_block_hash(block);
                self.pending_proposals
                    .write()
                    .unwrap()
                    .insert(block_hash, block.clone());
            }
        }

        // 4. Delegate to consensus engine for HotStuff safety rules, storage, and precommit signing
        let mut c_guard = self.consensus.write().unwrap();
        c_guard.handle_proposal(block)
    }

    /// Handles an incoming vote, returning an assembled Quorum Certificate if threshold is reached.
    pub fn process_vote(&self, vote: Vote) -> Result<Option<QuorumCertificate>, ConsensusError> {
        let mut c_guard = self.consensus.write().unwrap();
        c_guard.handle_vote(vote)
    }

    /// Ingests an assembled Quorum Certificate, updating highest/locked QCs,
    /// evaluating the 2-chain commit finality rule, advancing pacemaker height,
    /// and atomically committing the certified block bundle to persistent storage.
    pub fn process_qc(&self, qc: &QuorumCertificate) -> Result<(), ConsensusError> {
        let block_opt = {
            let mut c_guard = self.consensus.write().unwrap();
            c_guard.handle_qc(qc)?;
            c_guard.get_block(&qc.block_hash).cloned()
        };

        let mut state = self.state_db.write().unwrap();
        let cur_h = state.current_height();

        // 1. Replay / Stale check: if already at or past this QC's height, idempotent return Ok
        if qc.height.0 <= cur_h.0 {
            return Ok(());
        }

        // 2. Sequential height check: QC must commit the immediate next height
        if qc.height.0 != cur_h.0 + 1 {
            return Err(ConsensusError::InternalError(format!(
                "Non-sequential QC height: canonical height is {}, QC specifies height {}",
                cur_h.0, qc.height.0
            )));
        }

        // 3. Locate candidate block from pending proposals or consensus block store
        let block = if let Some(b) = self
            .pending_proposals
            .write()
            .unwrap()
            .remove(&qc.block_hash)
        {
            b
        } else if let Some(b) = block_opt {
            b
        } else {
            return Err(ConsensusError::InternalError(format!(
                "Block not found for QC: {:?}",
                qc.block_hash
            )));
        };

        // 4. Verify block identity against QC commitments
        if block.header.height != qc.height {
            return Err(ConsensusError::InternalError(format!(
                "Block height mismatch: block has height {}, QC specifies height {}",
                block.header.height.0, qc.height.0
            )));
        }

        if block.header.round != qc.round {
            return Err(ConsensusError::InternalError(format!(
                "Block round mismatch: block has round {}, QC specifies round {}",
                block.header.round.0, qc.round.0
            )));
        }

        let computed_hash = tola_consensus::PipelinedBftEngine::compute_block_hash(&block);
        if computed_hash != qc.block_hash {
            return Err(ConsensusError::InternalError(format!(
                "Block hash mismatch: computed {:?}, QC specifies {:?}",
                computed_hash, qc.block_hash
            )));
        }

        // 5. Verify canonical parent link
        if cur_h.0 == 0 {
            if block.header.parent_hash != Hash::ZERO {
                return Err(ConsensusError::InternalError(format!(
                    "Invalid genesis parent hash: expected Hash::ZERO, found {:?}",
                    block.header.parent_hash
                )));
            }
        } else {
            let parent_hash = state
                .get_block_hash_by_height(cur_h)
                .map_err(|e| ConsensusError::InternalError(format!("Storage error: {:?}", e)))?
                .ok_or_else(|| {
                    ConsensusError::InternalError(format!(
                        "Missing parent block hash at canonical height {}",
                        cur_h.0
                    ))
                })?;
            if block.header.parent_hash != parent_hash {
                return Err(ConsensusError::InternalError(format!(
                    "Parent hash mismatch: block points to {:?}, but canonical tip is {:?}",
                    block.header.parent_hash, parent_hash
                )));
            }
        }

        // 6. Verify transaction root commitment
        let computed_txs_root = compute_txs_root(&block.transactions);
        if block.header.txs_root != computed_txs_root {
            return Err(ConsensusError::InvalidTxsRoot);
        }

        // 7. Execute block against state and verify post-execution commitments
        let receipts = self
            .block_engine
            .execute_block(&block, &mut *state)
            .map_err(|e| {
                let _ = state.rollback();
                ConsensusError::InternalError(format!("Block execution failed on commit: {:?}", e))
            })?;

        let computed_receipts_root = compute_receipts_root(&receipts);
        if block.header.receipts_root != computed_receipts_root {
            let _ = state.rollback();
            return Err(ConsensusError::InvalidReceiptsRoot);
        }

        let derived_gas_used = receipts
            .last()
            .map(|r| r.cumulative_gas_used)
            .unwrap_or(Gas::ZERO);
        if block.header.gas_used != derived_gas_used {
            let _ = state.rollback();
            return Err(ConsensusError::InvalidGasUsed);
        }

        let computed_state_root = match state.compute_state_root() {
            Ok(r) => r,
            Err(e) => {
                let _ = state.rollback();
                return Err(ConsensusError::InternalError(format!(
                    "Failed to compute state root: {:?}",
                    e
                )));
            }
        };

        if block.header.state_root != computed_state_root {
            let _ = state.rollback();
            return Err(ConsensusError::InvalidStateRoot);
        }

        // 8. Atomically commit block bundle to persistent storage
        state
            .commit_block_bundle(&block, qc.block_hash, &receipts)
            .map_err(|e| {
                let _ = state.rollback();
                ConsensusError::InternalError(format!("State commit failed: {:?}", e))
            })?;

        // Check and execute matured governance proposals upon canonical block commit
        self.governance
            .write()
            .unwrap()
            .check_and_execute_matured_proposals(block.header.height);

        // 9. Clean up mempool, update indexer / RPC caches, update highest block
        self.mempool
            .write()
            .unwrap()
            .remove_committed(&block.transactions);

        self.indexer.index_block(block.clone(), receipts);
        *self.highest_block.write().unwrap() = Some(block);

        // 10. Prune stale pending proposals
        self.pending_proposals
            .write()
            .unwrap()
            .retain(|_, b| b.header.height.0 > qc.height.0);

        Ok(())
    }

    /// Returns the node's highest observed Quorum Certificate.
    pub fn highest_qc(&self) -> Option<QuorumCertificate> {
        self.consensus.read().unwrap().highest_qc().cloned()
    }

    /// Returns the latest finalized height committed under Jolteon 2-chain rules.
    pub fn last_finalized_height(&self) -> BlockHeight {
        self.consensus.read().unwrap().get_last_finalized_height()
    }

    /// Returns the current state height.
    pub fn current_height(&self) -> BlockHeight {
        self.state_db.read().unwrap().current_height()
    }

    /// Returns the current state root.
    pub fn state_root(&self) -> Hash {
        self.state_db.read().unwrap().state_root()
    }

    /// Returns the observable status and lag of the decoupled chain indexer per Decision D13.
    pub fn indexer_status(&self) -> IndexerStatus {
        let cur_h = self.current_height().0;
        self.indexer.set_tip_height(cur_h);
        self.indexer.status()
    }

    /// Returns true if this node operates in solo/standalone mode (committee size <= 1).
    pub fn is_standalone(&self) -> bool {
        let c = self.consensus.read().unwrap();
        c.committee().validator_set().validators.len() <= 1
    }
}

pub fn compute_txs_root(txs: &[Transaction]) -> Hash {
    if txs.is_empty() {
        return Hash::ZERO;
    }
    let mut concat = Vec::with_capacity(txs.len() * 32);
    for tx in txs {
        concat.extend_from_slice(&compute_tx_digest(tx).0);
    }
    blake3_hash(&concat)
}

pub fn compute_receipts_root(receipts: &[Receipt]) -> Hash {
    if receipts.is_empty() {
        return Hash::ZERO;
    }
    let mut concat = Vec::with_capacity(receipts.len() * 32);
    for r in receipts {
        concat.extend_from_slice(&r.tx_id.0 .0);
    }
    blake3_hash(&concat)
}
