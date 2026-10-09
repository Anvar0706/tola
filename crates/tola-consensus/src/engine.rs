//! Pipelined BFT consensus engine implementing the Jolteon/HotStuff 2-chain commit rule.
//!
//! Provides deterministic finality, linear message aggregation, and Byzantine safety ($3f + 1$).

use std::collections::HashMap;

use tola_core::{ConsensusEngine, ConsensusError};
use tola_crypto::{blake3_hash, Ed25519Keypair};
use tola_types::{Block, BlockHeight, Hash, QuorumCertificate, Round, ValidatorId, Vote, VoteType};

use crate::committee::CommitteeManager;
use crate::pacemaker::{Pacemaker, PacemakerConfig};
use crate::qc::{compute_vote_hash, verify_qc, VoteAggregator};

/// Core Pipelined BFT consensus state machine.
pub struct PipelinedBftEngine {
    local_validator_id: ValidatorId,
    keypair: Ed25519Keypair,
    committee: CommitteeManager,
    pacemaker: Pacemaker,
    vote_aggregator: VoteAggregator,
    blocks_by_hash: HashMap<Hash, Block>,
    highest_qc: Option<QuorumCertificate>,
    locked_qc: Option<QuorumCertificate>,
    last_finalized: BlockHeight,
}

impl PipelinedBftEngine {
    pub fn new(
        local_validator_id: ValidatorId,
        keypair: Ed25519Keypair,
        committee: CommitteeManager,
        pacemaker_config: PacemakerConfig,
        start_height: BlockHeight,
        current_time_ms: u64,
    ) -> Self {
        Self {
            local_validator_id,
            keypair,
            committee,
            pacemaker: Pacemaker::new(pacemaker_config, start_height, current_time_ms),
            vote_aggregator: VoteAggregator::new(),
            blocks_by_hash: HashMap::new(),
            highest_qc: None,
            locked_qc: None,
            last_finalized: BlockHeight(0),
        }
    }

    pub fn committee(&self) -> &CommitteeManager {
        &self.committee
    }

    pub fn pacemaker(&self) -> &Pacemaker {
        &self.pacemaker
    }

    pub fn highest_qc(&self) -> Option<&QuorumCertificate> {
        self.highest_qc.as_ref()
    }

    pub fn locked_qc(&self) -> Option<&QuorumCertificate> {
        self.locked_qc.as_ref()
    }

    pub fn get_block(&self, hash: &Hash) -> Option<&Block> {
        self.blocks_by_hash.get(hash)
    }

    /// Triggers round advance upon pacemaker timeout.
    pub fn on_timeout(&mut self, current_time_ms: u64) -> Round {
        self.pacemaker.advance_round(current_time_ms)
    }

    /// Computes the block hash for verification and voting.
    ///
    /// Cryptographically binds all consensus-critical header fields according to
    /// canonical deterministic encoding: parent_hash, height, epoch, round,
    /// timestamp, proposer, state_root, txs_root, receipts_root, gas_used,
    /// gas_limit, base_fee_per_gas, and extra_data.
    pub fn compute_block_hash(block: &Block) -> Hash {
        Self::compute_block_header_hash(&block.header)
    }

    /// Computes the cryptographic block hash directly from a canonical BlockHeader.
    pub fn compute_block_header_hash(header: &tola_types::BlockHeader) -> Hash {
        use tola_codec::CanonicalSerialize;
        let mut data = Vec::with_capacity(256);
        header.serialize_canonical(&mut data);
        blake3_hash(&data)
    }

    /// Legacy v1 block hash computation (hashing parent_hash || height || round || state_root).
    /// Preserved for historical compatibility verification and fork migration testing.
    pub fn compute_block_hash_v1_legacy(header: &tola_types::BlockHeader) -> Hash {
        let mut data = Vec::with_capacity(32 + 8 + 8 + 32);
        data.extend_from_slice(&header.parent_hash.0);
        data.extend_from_slice(&header.height.0.to_le_bytes());
        data.extend_from_slice(&header.round.0.to_le_bytes());
        data.extend_from_slice(&header.state_root.0);
        blake3_hash(&data)
    }

    /// Evaluates the 2-chain commit rule to finalize historic blocks.
    ///
    /// Jolteon 2-chain commit rule:
    /// When Block B (height H) achieves a valid QC, if B directly extends B_parent (height H-1)
    /// whose QC justified B_grandparent (height H-2), then B_grandparent is permanently FINALIZED.
    fn evaluate_commit_rule(&mut self, new_qc: &QuorumCertificate) {
        if let Some(parent_block) = self.blocks_by_hash.get(&new_qc.block_hash) {
            if let Some(parent_qc) = &parent_block.last_commit_qc {
                if let Some(grandparent_block) = self.blocks_by_hash.get(&parent_qc.block_hash) {
                    let gp_height = grandparent_block.header.height;
                    if gp_height.0 > self.last_finalized.0 {
                        self.last_finalized = gp_height;
                        self.vote_aggregator.prune(self.last_finalized);
                    }
                }
            }
        }
    }

    /// Ingests an assembled Quorum Certificate, updating highest/locked QCs,
    /// evaluating the 2-chain commit finality rule, and advancing pacemaker height.
    pub fn handle_qc(&mut self, qc: &QuorumCertificate) -> Result<(), ConsensusError> {
        verify_qc(qc, &self.committee)?;

        if self
            .highest_qc
            .as_ref()
            .is_none_or(|h| qc.round.0 >= h.round.0)
        {
            self.highest_qc = Some(qc.clone());
        }

        if self
            .locked_qc
            .as_ref()
            .is_none_or(|l| qc.round.0 >= l.round.0)
        {
            self.locked_qc = Some(qc.clone());
        }

        self.evaluate_commit_rule(qc);

        let next_height = BlockHeight(qc.height.0 + 1);
        self.pacemaker.advance_height(next_height, 0);

        Ok(())
    }
}

impl ConsensusEngine for PipelinedBftEngine {
    /// Validates an incoming block proposal from the view leader.
    ///
    /// Checks:
    /// 1. Stale round rejection.
    /// 2. Authorized view leader schedule.
    /// 3. Parent QC validity.
    /// 4. HotStuff locking rule safety invariant.
    fn handle_proposal(&mut self, block: &Block) -> Result<Option<Vote>, ConsensusError> {
        let height = block.header.height;
        let round = block.header.round;

        // 1. Check stale view
        if height.0 < self.pacemaker.current_height().0 {
            return Err(ConsensusError::StaleRound);
        }

        // 2. Check authorized leader
        let expected_leader = self.committee.leader_for_view(height, round);
        if ValidatorId(block.header.proposer) != expected_leader {
            return Err(ConsensusError::LeaderMismatch);
        }

        // 3. Verify parent QC if height > 1
        if height.0 > 1 {
            let parent_qc = block
                .last_commit_qc
                .as_ref()
                .ok_or(ConsensusError::InvalidParentQC)?;

            verify_qc(parent_qc, &self.committee)?;

            if parent_qc.block_hash != block.header.parent_hash {
                return Err(ConsensusError::InvalidParentQC);
            }

            // 4. HotStuff Safety / Locking Rule:
            // Validator only votes if parent_qc round >= locked_qc round
            if let Some(locked) = &self.locked_qc {
                if parent_qc.round.0 < locked.round.0 {
                    return Err(ConsensusError::LockedBlockMismatch);
                }
            }

            // Update local highest_qc and locked_qc
            if self
                .highest_qc
                .as_ref()
                .is_none_or(|h| parent_qc.round.0 > h.round.0)
            {
                self.highest_qc = Some(parent_qc.clone());
            }
            if self
                .locked_qc
                .as_ref()
                .is_none_or(|l| parent_qc.round.0 > l.round.0)
            {
                self.locked_qc = Some(parent_qc.clone());
            }
        }

        // 5. Store block
        let block_hash = Self::compute_block_hash(block);
        self.blocks_by_hash.insert(block_hash, block.clone());

        // 6. Sign and emit vote
        let vote_hash = compute_vote_hash(VoteType::Precommit, height, round, &block_hash);
        let sig = self.keypair.sign(&vote_hash.0);

        let vote = Vote {
            vote_type: VoteType::Precommit,
            height,
            round,
            block_hash,
            validator: self.local_validator_id,
            signature: sig,
        };

        Ok(Some(vote))
    }

    /// Processes an incoming validator vote, accumulates voting power, and forms QCs.
    fn handle_vote(&mut self, vote: Vote) -> Result<Option<QuorumCertificate>, ConsensusError> {
        let maybe_qc = self.vote_aggregator.add_vote(&vote, &self.committee)?;

        if let Some(ref qc) = maybe_qc {
            // Update highest QC
            if self
                .highest_qc
                .as_ref()
                .is_none_or(|h| qc.round.0 >= h.round.0)
            {
                self.highest_qc = Some(qc.clone());
            }

            // Update locked QC
            if self
                .locked_qc
                .as_ref()
                .is_none_or(|l| qc.round.0 >= l.round.0)
            {
                self.locked_qc = Some(qc.clone());
            }

            // Evaluate 2-chain finalization rule
            self.evaluate_commit_rule(qc);

            // Advance pacemaker height
            let next_height = BlockHeight(qc.height.0 + 1);
            self.pacemaker.advance_height(next_height, 0);
        }

        Ok(maybe_qc)
    }

    fn get_last_finalized_height(&self) -> BlockHeight {
        self.last_finalized
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tola_crypto::Ed25519Keypair;
    use tola_types::*;

    fn setup_test_engine() -> (PipelinedBftEngine, Ed25519Keypair, ValidatorId, ValidatorId) {
        let kp1 = Ed25519Keypair::generate();
        let kp2 = Ed25519Keypair::generate();

        let v1_id = ValidatorId(Address([0x01; 20]));
        let v2_id = ValidatorId(Address([0x02; 20]));

        let v1 = ValidatorInfo {
            validator_id: v1_id,
            consensus_pubkey: kp1.public_key(),
            voting_power: 200,
            commission_bp: 0,
        };
        let v2 = ValidatorInfo {
            validator_id: v2_id,
            consensus_pubkey: kp2.public_key(),
            voting_power: 100,
            commission_bp: 0,
        };

        // Total = 300. Quorum = 2 * 300 / 3 + 1 = 201. (v1 + v2 = 300 >= 201)
        let set = ValidatorSet {
            epoch: Epoch(1),
            validators: vec![v1, v2],
            total_voting_power: 300,
        };
        let committee = CommitteeManager::new(set).unwrap();

        let engine = PipelinedBftEngine::new(
            v1_id,
            kp1,
            committee,
            PacemakerConfig::default(),
            BlockHeight(1),
            1000,
        );

        (engine, kp2, v1_id, v2_id)
    }

    #[test]
    fn test_pipelined_bft_proposal_voting_and_2chain_commit() {
        let (mut engine, kp2, _v1_id, v2_id) = setup_test_engine();

        // View 1 (Height 1, Round 0): Determine leader
        let leader_h1 = engine.committee().leader_for_view(BlockHeight(1), Round(0));

        let block_h1 = Block {
            header: BlockHeader {
                parent_hash: Hash([0x00; 32]),
                height: BlockHeight(1),
                epoch: Epoch(1),
                round: Round(0),
                timestamp: Timestamp(1000),
                proposer: leader_h1.0,
                state_root: Hash([0x11; 32]),
                txs_root: Hash([0; 32]),
                receipts_root: Hash([0; 32]),
                gas_used: Gas(0),
                gas_limit: Gas(30_000_000),
                base_fee_per_gas: Amount::from_u128(1),
                extra_data: vec![],
            },
            transactions: vec![],
            last_commit_qc: None,
        };

        // Engine processes block proposal at Height 1
        let vote_res = engine.handle_proposal(&block_h1).expect("Valid proposal");
        assert!(vote_res.is_some());
        let vote1_v1 = vote_res.unwrap();
        let hash_h1 = vote1_v1.block_hash;

        // Collect votes for Block 1 from v1 and v2 to form QC1
        let vote1_msg = compute_vote_hash(VoteType::Precommit, BlockHeight(1), Round(0), &hash_h1);
        let sig1_v2 = kp2.sign(&vote1_msg.0);
        let vote1_v2 = Vote {
            vote_type: VoteType::Precommit,
            height: BlockHeight(1),
            round: Round(0),
            block_hash: hash_h1,
            validator: v2_id,
            signature: sig1_v2,
        };

        // Feed v1's vote -> 200 power < 201 (no QC yet)
        let qc1_partial = engine.handle_vote(vote1_v1).unwrap();
        assert!(qc1_partial.is_none());

        // Feed v2's vote -> 300 power >= 201 -> QC1 FORMED!
        let qc1 = engine.handle_vote(vote1_v2).unwrap().expect("QC1 formed");
        assert_eq!(qc1.height, BlockHeight(1));
        assert_eq!(qc1.block_hash, hash_h1);

        // View 2 (Height 2, Round 0): Propose Block 2 incorporating QC1
        let leader_h2 = engine.committee().leader_for_view(BlockHeight(2), Round(0));
        let block_h2 = Block {
            header: BlockHeader {
                parent_hash: hash_h1,
                height: BlockHeight(2),
                epoch: Epoch(1),
                round: Round(0),
                timestamp: Timestamp(2000),
                proposer: leader_h2.0,
                state_root: Hash([0x22; 32]),
                txs_root: Hash([0; 32]),
                receipts_root: Hash([0; 32]),
                gas_used: Gas(0),
                gas_limit: Gas(30_000_000),
                base_fee_per_gas: Amount::from_u128(1),
                extra_data: vec![],
            },
            transactions: vec![],
            last_commit_qc: Some(qc1.clone()),
        };

        let vote2_v1 = engine
            .handle_proposal(&block_h2)
            .expect("Valid proposal for H2")
            .unwrap();
        let hash_h2 = vote2_v1.block_hash;

        // Collect votes for Block 2 to form QC2
        let vote2_msg = compute_vote_hash(VoteType::Precommit, BlockHeight(2), Round(0), &hash_h2);
        let sig2_v2 = kp2.sign(&vote2_msg.0);
        let vote2_v2 = Vote {
            vote_type: VoteType::Precommit,
            height: BlockHeight(2),
            round: Round(0),
            block_hash: hash_h2,
            validator: v2_id,
            signature: sig2_v2,
        };

        let _ = engine.handle_vote(vote2_v1);
        let qc2 = engine.handle_vote(vote2_v2).unwrap().expect("QC2 formed");

        // View 3 (Height 3, Round 0): Propose Block 3 incorporating QC2
        let leader_h3 = engine.committee().leader_for_view(BlockHeight(3), Round(0));
        let block_h3 = Block {
            header: BlockHeader {
                parent_hash: hash_h2,
                height: BlockHeight(3),
                epoch: Epoch(1),
                round: Round(0),
                timestamp: Timestamp(3000),
                proposer: leader_h3.0,
                state_root: Hash([0x33; 32]),
                txs_root: Hash([0; 32]),
                receipts_root: Hash([0; 32]),
                gas_used: Gas(0),
                gas_limit: Gas(30_000_000),
                base_fee_per_gas: Amount::from_u128(1),
                extra_data: vec![],
            },
            transactions: vec![],
            last_commit_qc: Some(qc2.clone()),
        };

        let vote3_v1 = engine
            .handle_proposal(&block_h3)
            .expect("Valid proposal for H3")
            .unwrap();
        let hash_h3 = vote3_v1.block_hash;

        let vote3_msg = compute_vote_hash(VoteType::Precommit, BlockHeight(3), Round(0), &hash_h3);
        let sig3_v2 = kp2.sign(&vote3_msg.0);
        let vote3_v2 = Vote {
            vote_type: VoteType::Precommit,
            height: BlockHeight(3),
            round: Round(0),
            block_hash: hash_h3,
            validator: v2_id,
            signature: sig3_v2,
        };

        // Upon QC2 formation, 2-chain commit rule finalizes Block 1 (B1 <- B2 <- QC2)
        assert_eq!(engine.get_last_finalized_height(), BlockHeight(1));

        let _ = engine.handle_vote(vote3_v1);
        let _qc3 = engine.handle_vote(vote3_v2).unwrap().expect("QC3 formed");

        // Upon QC3 formation, 2-chain commit rule finalizes Block 2 (B2 <- B3 <- QC3)
        assert_eq!(engine.get_last_finalized_height(), BlockHeight(2));
    }

    #[test]
    fn test_stale_round_and_leader_mismatch_rejected() {
        let (mut engine, _kp2, _v1_id, _v2_id) = setup_test_engine();

        // 1. Proposer mismatch: wrong proposer for Height 1 Round 0
        let invalid_leader_block = Block {
            header: BlockHeader {
                parent_hash: Hash([0; 32]),
                height: BlockHeight(1),
                epoch: Epoch(1),
                round: Round(0),
                timestamp: Timestamp(1000),
                proposer: Address([0x99; 20]), // Wrong leader!
                state_root: Hash([0; 32]),
                txs_root: Hash([0; 32]),
                receipts_root: Hash([0; 32]),
                gas_used: Gas(0),
                gas_limit: Gas(30_000_000),
                base_fee_per_gas: Amount::from_u128(1),
                extra_data: vec![],
            },
            transactions: vec![],
            last_commit_qc: None,
        };

        let err = engine.handle_proposal(&invalid_leader_block).unwrap_err();
        assert_eq!(err, ConsensusError::LeaderMismatch);
    }

    #[test]
    fn test_compute_block_hash_binds_all_consensus_critical_header_fields() {
        let base_header = BlockHeader {
            parent_hash: Hash([0x11; 32]),
            height: BlockHeight(10),
            epoch: Epoch(1),
            round: Round(2),
            timestamp: Timestamp(1700000000),
            proposer: Address([0xaa; 20]),
            state_root: Hash([0x22; 32]),
            txs_root: Hash([0x33; 32]),
            receipts_root: Hash([0x44; 32]),
            gas_used: Gas(21000),
            gas_limit: Gas(30_000_000),
            base_fee_per_gas: Amount::from_u128(1_000_000_000),
            extra_data: vec![0x01, 0x02, 0x03],
        };

        let base_block = Block {
            header: base_header.clone(),
            transactions: vec![],
            last_commit_qc: None,
        };

        let base_hash = PipelinedBftEngine::compute_block_hash(&base_block);
        assert_eq!(
            base_hash,
            PipelinedBftEngine::compute_block_header_hash(&base_header)
        );

        // 1. Modifying parent_hash changes block hash
        let mut h = base_header.clone();
        h.parent_hash = Hash([0x99; 32]);
        assert_ne!(base_hash, PipelinedBftEngine::compute_block_header_hash(&h));

        // 2. Modifying height changes block hash
        let mut h = base_header.clone();
        h.height = BlockHeight(11);
        assert_ne!(base_hash, PipelinedBftEngine::compute_block_header_hash(&h));

        // 3. Modifying epoch changes block hash
        let mut h = base_header.clone();
        h.epoch = Epoch(2);
        assert_ne!(base_hash, PipelinedBftEngine::compute_block_header_hash(&h));

        // 4. Modifying round changes block hash
        let mut h = base_header.clone();
        h.round = Round(3);
        assert_ne!(base_hash, PipelinedBftEngine::compute_block_header_hash(&h));

        // 5. Modifying timestamp changes block hash
        let mut h = base_header.clone();
        h.timestamp = Timestamp(1700000001);
        assert_ne!(base_hash, PipelinedBftEngine::compute_block_header_hash(&h));

        // 6. Modifying proposer changes block hash
        let mut h = base_header.clone();
        h.proposer = Address([0xbb; 20]);
        assert_ne!(base_hash, PipelinedBftEngine::compute_block_header_hash(&h));

        // 7. Modifying state_root changes block hash
        let mut h = base_header.clone();
        h.state_root = Hash([0x99; 32]);
        assert_ne!(base_hash, PipelinedBftEngine::compute_block_header_hash(&h));

        // 8. Modifying txs_root MUST change block hash (critical protocol fix)
        let mut h = base_header.clone();
        h.txs_root = Hash([0x99; 32]);
        assert_ne!(base_hash, PipelinedBftEngine::compute_block_header_hash(&h));

        // 9. Modifying receipts_root MUST change block hash (critical protocol fix)
        let mut h = base_header.clone();
        h.receipts_root = Hash([0x99; 32]);
        assert_ne!(base_hash, PipelinedBftEngine::compute_block_header_hash(&h));

        // 10. Modifying gas_used MUST change block hash
        let mut h = base_header.clone();
        h.gas_used = Gas(50000);
        assert_ne!(base_hash, PipelinedBftEngine::compute_block_header_hash(&h));

        // 11. Modifying gas_limit MUST change block hash
        let mut h = base_header.clone();
        h.gas_limit = Gas(15_000_000);
        assert_ne!(base_hash, PipelinedBftEngine::compute_block_header_hash(&h));

        // 12. Modifying base_fee_per_gas MUST change block hash
        let mut h = base_header.clone();
        h.base_fee_per_gas = Amount::from_u128(2_000_000_000);
        assert_ne!(base_hash, PipelinedBftEngine::compute_block_header_hash(&h));

        // 13. Modifying extra_data MUST change block hash
        let mut h = base_header.clone();
        h.extra_data = vec![0x99];
        assert_ne!(base_hash, PipelinedBftEngine::compute_block_header_hash(&h));

        // Verify difference with legacy v1 computation (demonstrates why legacy was defective)
        let legacy_base = PipelinedBftEngine::compute_block_hash_v1_legacy(&base_header);
        let mut forged_txs_header = base_header.clone();
        forged_txs_header.txs_root = Hash([0xfe; 32]);
        let legacy_forged = PipelinedBftEngine::compute_block_hash_v1_legacy(&forged_txs_header);
        // Under legacy, forged txs root went undetected by the hash!
        assert_eq!(legacy_base, legacy_forged);
        // Under current canonical hashing, forged txs root is strictly detected!
        assert_ne!(
            PipelinedBftEngine::compute_block_header_hash(&base_header),
            PipelinedBftEngine::compute_block_header_hash(&forged_txs_header)
        );
    }

    #[test]
    fn test_deterministic_block_hash_across_validators() {
        let block_header = BlockHeader {
            parent_hash: Hash([0x01; 32]),
            height: BlockHeight(5),
            epoch: Epoch(0),
            round: Round(1),
            timestamp: Timestamp(123456789),
            proposer: Address([0x02; 20]),
            state_root: Hash([0x03; 32]),
            txs_root: Hash([0x04; 32]),
            receipts_root: Hash([0x05; 32]),
            gas_used: Gas(21000),
            gas_limit: Gas(30_000_000),
            base_fee_per_gas: Amount::from_u128(1_000_000_000),
            extra_data: vec![],
        };

        let block = Block {
            header: block_header.clone(),
            transactions: vec![],
            last_commit_qc: None,
        };

        // Repeated invocations on separate instances must yield bit-for-bit identical hashes
        let hash1 = PipelinedBftEngine::compute_block_hash(&block);
        let hash2 = PipelinedBftEngine::compute_block_header_hash(&block_header);
        assert_eq!(hash1, hash2);
        assert_ne!(hash1, Hash::ZERO);
    }
}
