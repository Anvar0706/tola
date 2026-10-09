//! Quorum Certificate (QC) aggregation, threshold bitfield accounting, and cryptographic verification.
//!
//! Enforces that any valid QC represents >= 2/3 + 1 of bonded validator voting power
//! signed by authorized committee members for a specific height, round, and block hash.

use std::collections::HashMap;
use tola_core::ConsensusError;
use tola_crypto::{blake3_hash, verify_ed25519};
use tola_types::{
    BlockHeight, Hash, PublicKey, QuorumCertificate, Round, Signature, ValidatorId, Vote, VoteType,
};

use crate::committee::CommitteeManager;

/// Computes the domain-separated message hash that validators sign when voting for a block.
pub fn compute_vote_hash(
    vote_type: VoteType,
    height: BlockHeight,
    round: Round,
    block_hash: &Hash,
) -> Hash {
    let mut data = Vec::with_capacity(49);
    data.extend_from_slice(b"TOLA-BFT-VOTE-v1--");
    data.push(match vote_type {
        VoteType::Prevote => 0x00,
        VoteType::Precommit => 0x01,
    });
    data.extend_from_slice(&height.0.to_le_bytes());
    data.extend_from_slice(&round.0.to_le_bytes());
    data.extend_from_slice(&block_hash.0);

    blake3_hash(&data)
}

/// Key identifying a specific consensus vote target.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct VoteTarget {
    pub height: BlockHeight,
    pub round: Round,
    pub block_hash: Hash,
    pub vote_type: VoteType,
}

/// Accumulator collecting individual validator votes and assembling threshold Quorum Certificates.
pub struct VoteAggregator {
    /// Maps validator_id -> (height, round, block_hash) for equivocation detection.
    cast_votes: HashMap<ValidatorId, (BlockHeight, Round, Hash)>,
    /// Accumulated votes per target: VoteTarget -> HashMap<ValidatorId, (u64, Signature)>.
    buckets: HashMap<VoteTarget, HashMap<ValidatorId, (u64, Signature)>>,
}

impl VoteAggregator {
    pub fn new() -> Self {
        Self {
            cast_votes: HashMap::new(),
            buckets: HashMap::new(),
        }
    }

    /// Adds a validated vote to the aggregator.
    ///
    /// If the validator has already cast a vote for a DIFFERENT block hash at the same
    /// height and round, emits `ConsensusError::EquivocationDetected`.
    pub fn add_vote(
        &mut self,
        vote: &Vote,
        committee: &CommitteeManager,
    ) -> Result<Option<QuorumCertificate>, ConsensusError> {
        // 1. Verify validator is in the active committee
        let val_info = committee
            .find_validator(&vote.validator)
            .ok_or(ConsensusError::UnknownValidator(vote.validator))?;

        // 2. Check for equivocation (double-voting at same height & round)
        if let Some((prev_h, prev_r, prev_hash)) = self.cast_votes.get(&vote.validator) {
            if *prev_h == vote.height && *prev_r == vote.round && *prev_hash != vote.block_hash {
                return Err(ConsensusError::EquivocationDetected(vote.validator));
            }
        }

        // 3. Verify cryptographic signature
        let vote_msg = compute_vote_hash(vote.vote_type, vote.height, vote.round, &vote.block_hash);
        match (&val_info.consensus_pubkey, &vote.signature) {
            (PublicKey::Ed25519(pk_bytes), Signature::Ed25519(sig_bytes)) => {
                if verify_ed25519(pk_bytes, &vote_msg.0, sig_bytes).is_err() {
                    return Err(ConsensusError::InvalidSignature);
                }
            }
            _ => return Err(ConsensusError::InvalidSignature),
        }

        // 4. Record cast vote
        self.cast_votes
            .insert(vote.validator, (vote.height, vote.round, vote.block_hash));

        // 5. Add to voting target bucket
        let target = VoteTarget {
            height: vote.height,
            round: vote.round,
            block_hash: vote.block_hash,
            vote_type: vote.vote_type,
        };

        let bucket = self.buckets.entry(target).or_default();
        bucket.insert(
            vote.validator,
            (val_info.voting_power, vote.signature.clone()),
        );

        // 6. Check if quorum is reached
        let accumulated_power: u64 = bucket.values().map(|(power, _)| *power).sum();

        if committee.quorum_threshold() <= accumulated_power {
            // Assemble Quorum Certificate
            let num_validators = committee.validator_set().validators.len();
            let bitfield_bytes = num_validators.div_ceil(8);
            let mut bitfield = vec![0u8; bitfield_bytes];
            let mut signatures_concat = Vec::new();

            for (idx, val_info) in committee.validator_set().validators.iter().enumerate() {
                if let Some((_, sig)) = bucket.get(&val_info.validator_id) {
                    bitfield[idx / 8] |= 1 << (idx % 8);
                    if let Signature::Ed25519(s) = sig {
                        signatures_concat.extend_from_slice(s);
                    }
                }
            }

            let qc = QuorumCertificate {
                height: vote.height,
                round: vote.round,
                block_hash: vote.block_hash,
                vote_type: vote.vote_type,
                validator_bitfield: bitfield,
                aggregated_signature: signatures_concat,
            };

            Ok(Some(qc))
        } else {
            Ok(None)
        }
    }

    /// Prunes votes older than the given finalized height.
    pub fn prune(&mut self, finalized_height: BlockHeight) {
        self.buckets
            .retain(|target, _| target.height.0 > finalized_height.0);
        self.cast_votes
            .retain(|_, (h, _, _)| h.0 > finalized_height.0);
    }
}

impl Default for VoteAggregator {
    fn default() -> Self {
        Self::new()
    }
}

/// Cryptographically validates a Quorum Certificate against an active validator committee.
/// Returns the total voting power verified.
pub fn verify_qc(
    qc: &QuorumCertificate,
    committee: &CommitteeManager,
) -> Result<u64, ConsensusError> {
    let validators = &committee.validator_set().validators;
    let num_validators = validators.len();
    let expected_bitfield_len = num_validators.div_ceil(8);

    if qc.validator_bitfield.len() != expected_bitfield_len {
        return Err(ConsensusError::InvalidQuorum);
    }

    let vote_msg = compute_vote_hash(qc.vote_type, qc.height, qc.round, &qc.block_hash);

    let mut accumulated_power = 0u64;
    let mut sig_offset = 0usize;

    for (idx, val_info) in validators.iter().enumerate() {
        let is_set = (qc.validator_bitfield[idx / 8] & (1 << (idx % 8))) != 0;
        if is_set {
            accumulated_power += val_info.voting_power;

            // Verify individual 64-byte Ed25519 signature
            if sig_offset + 64 > qc.aggregated_signature.len() {
                return Err(ConsensusError::InvalidSignature);
            }

            let mut sig_bytes = [0u8; 64];
            sig_bytes.copy_from_slice(&qc.aggregated_signature[sig_offset..sig_offset + 64]);
            sig_offset += 64;

            match &val_info.consensus_pubkey {
                PublicKey::Ed25519(pk_bytes) => {
                    if verify_ed25519(pk_bytes, &vote_msg.0, &sig_bytes).is_err() {
                        return Err(ConsensusError::InvalidSignature);
                    }
                }
                _ => return Err(ConsensusError::InvalidSignature),
            }
        }
    }

    for bit_idx in num_validators..(expected_bitfield_len * 8) {
        if (qc.validator_bitfield[bit_idx / 8] & (1 << (bit_idx % 8))) != 0 {
            return Err(ConsensusError::InvalidQuorum);
        }
    }

    if sig_offset != qc.aggregated_signature.len() {
        return Err(ConsensusError::InvalidSignature);
    }

    if !crate::committee::is_quorum(accumulated_power, committee.total_voting_power()) {
        return Err(ConsensusError::InvalidQuorum);
    }

    Ok(accumulated_power)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tola_crypto::Ed25519Keypair;
    use tola_types::*;

    #[test]
    fn test_qc_threshold_aggregation_and_verification() {
        let kp1 = Ed25519Keypair::generate();
        let kp2 = Ed25519Keypair::generate();
        let kp3 = Ed25519Keypair::generate();

        let v1_id = ValidatorId(Address([0x01; 20]));
        let v2_id = ValidatorId(Address([0x02; 20]));
        let v3_id = ValidatorId(Address([0x03; 20]));

        let v1 = ValidatorInfo {
            validator_id: v1_id,
            consensus_pubkey: kp1.public_key(),
            voting_power: 100,
            commission_bp: 0,
        };
        let v2 = ValidatorInfo {
            validator_id: v2_id,
            consensus_pubkey: kp2.public_key(),
            voting_power: 200,
            commission_bp: 0,
        };
        let v3 = ValidatorInfo {
            validator_id: v3_id,
            consensus_pubkey: kp3.public_key(),
            voting_power: 100,
            commission_bp: 0,
        };

        // Total = 400. Quorum threshold = 2 * 400 / 3 + 1 = 267 + 1 = 268.
        let set = ValidatorSet {
            epoch: Epoch(1),
            validators: vec![v1, v2, v3],
            total_voting_power: 400,
        };
        let committee = CommitteeManager::new(set).unwrap();

        let mut aggregator = VoteAggregator::new();
        let block_hash = Hash([0xaa; 32]);
        let vote_msg =
            compute_vote_hash(VoteType::Precommit, BlockHeight(1), Round(0), &block_hash);

        // Vote 1 (Power 100) -> Not enough for quorum (100 < 268)
        let sig1 = kp1.sign(&vote_msg.0);
        let vote1 = Vote {
            vote_type: VoteType::Precommit,
            height: BlockHeight(1),
            round: Round(0),
            block_hash,
            validator: v1_id,
            signature: sig1,
        };
        let res1 = aggregator.add_vote(&vote1, &committee).unwrap();
        assert!(res1.is_none());

        // Vote 2 (Power 200) -> Total 300 >= 268! Quorum Certificate generated!
        let sig2 = kp2.sign(&vote_msg.0);
        let vote2 = Vote {
            vote_type: VoteType::Precommit,
            height: BlockHeight(1),
            round: Round(0),
            block_hash,
            validator: v2_id,
            signature: sig2,
        };
        let res2 = aggregator.add_vote(&vote2, &committee).unwrap();
        assert!(res2.is_some());

        let qc = res2.unwrap();
        assert_eq!(qc.height, BlockHeight(1));
        assert_eq!(qc.block_hash, block_hash);

        // Verify the generated QC cryptographically
        let power_verified = verify_qc(&qc, &committee).expect("Valid QC");
        assert_eq!(power_verified, 300);
    }

    #[test]
    fn test_equivocation_detected_on_conflicting_vote() {
        let kp1 = Ed25519Keypair::generate();
        let v1_id = ValidatorId(Address([0x01; 20]));
        let v1 = ValidatorInfo {
            validator_id: v1_id,
            consensus_pubkey: kp1.public_key(),
            voting_power: 100,
            commission_bp: 0,
        };
        let set = ValidatorSet {
            epoch: Epoch(1),
            validators: vec![v1],
            total_voting_power: 100,
        };
        let committee = CommitteeManager::new(set).unwrap();

        let mut aggregator = VoteAggregator::new();
        let hash_a = Hash([0xaa; 32]);
        let hash_b = Hash([0xbb; 32]);

        // First vote for Block A
        let msg_a = compute_vote_hash(VoteType::Precommit, BlockHeight(5), Round(1), &hash_a);
        let sig_a = kp1.sign(&msg_a.0);
        let vote_a = Vote {
            vote_type: VoteType::Precommit,
            height: BlockHeight(5),
            round: Round(1),
            block_hash: hash_a,
            validator: v1_id,
            signature: sig_a,
        };
        assert!(aggregator.add_vote(&vote_a, &committee).is_ok());

        // Conflicting vote for Block B at same height and round -> EQUIVOCATION!
        let msg_b = compute_vote_hash(VoteType::Precommit, BlockHeight(5), Round(1), &hash_b);
        let sig_b = kp1.sign(&msg_b.0);
        let vote_b = Vote {
            vote_type: VoteType::Precommit,
            height: BlockHeight(5),
            round: Round(1),
            block_hash: hash_b,
            validator: v1_id,
            signature: sig_b,
        };
        let err = aggregator.add_vote(&vote_b, &committee).unwrap_err();
        assert_eq!(err, ConsensusError::EquivocationDetected(v1_id));
    }
}
