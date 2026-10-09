//! Active validator committee, quorum thresholds, and deterministic leader election.
//!
//! Enforces the BFT Byzantine quorum invariant (Q >= 2f + 1) where total voting power W
//! safely tolerates up to f < W / 3 faulty or malicious voting power.

use tola_core::ConsensusError;
use tola_crypto::blake3_hash;
use tola_types::{BlockHeight, Round, ValidatorId, ValidatorInfo, ValidatorSet};

/// Calculates the minimal voting power required to achieve a BFT 2/3 + 1 quorum.
/// Formula: Q = floor(2 * W / 3) + 1
pub fn quorum_threshold(total_power: u64) -> u64 {
    (total_power * 2) / 3 + 1
}

/// Evaluates if a given accumulated voting power meets or exceeds the 2/3 + 1 quorum.
pub fn is_quorum(accumulated_power: u64, total_power: u64) -> bool {
    accumulated_power >= quorum_threshold(total_power)
}

/// Committee manager handling voting power lookups and deterministic leader scheduling.
#[derive(Clone, Debug)]
pub struct CommitteeManager {
    validator_set: ValidatorSet,
}

impl CommitteeManager {
    pub fn new(validator_set: ValidatorSet) -> Result<Self, ConsensusError> {
        if validator_set.validators.is_empty() {
            return Err(ConsensusError::InvalidQuorum);
        }
        if validator_set.total_voting_power == 0 {
            return Err(ConsensusError::VotingPowerInsufficient);
        }

        // Verify total voting power matches sum of individual validator weights
        let calculated_total: u64 = validator_set
            .validators
            .iter()
            .map(|v| v.voting_power)
            .sum();
        if calculated_total != validator_set.total_voting_power {
            return Err(ConsensusError::InternalError(format!(
                "Total voting power mismatch: expected {}, calculated {}",
                validator_set.total_voting_power, calculated_total
            )));
        }

        Ok(Self { validator_set })
    }

    pub fn validator_set(&self) -> &ValidatorSet {
        &self.validator_set
    }

    pub fn total_voting_power(&self) -> u64 {
        self.validator_set.total_voting_power
    }

    pub fn quorum_threshold(&self) -> u64 {
        quorum_threshold(self.validator_set.total_voting_power)
    }

    pub fn find_validator(&self, id: &ValidatorId) -> Option<&ValidatorInfo> {
        self.validator_set
            .validators
            .iter()
            .find(|v| v.validator_id == *id)
    }

    pub fn get_validator_index(&self, id: &ValidatorId) -> Option<usize> {
        self.validator_set
            .validators
            .iter()
            .position(|v| v.validator_id == *id)
    }

    /// Selects the deterministic leader for a given height and round.
    ///
    /// Computes a deterministic pseudorandom seed:
    /// `Seed = BLAKE3(b"TOLA-LEADER-ELECTION" || height || round)`
    /// and maps the value across the cumulative stake distribution of active validators.
    pub fn leader_for_view(&self, height: BlockHeight, round: Round) -> ValidatorId {
        let mut seed_data = Vec::with_capacity(36);
        seed_data.extend_from_slice(b"TOLA-LEADER-ELECTION");
        seed_data.extend_from_slice(&height.0.to_le_bytes());
        seed_data.extend_from_slice(&round.0.to_le_bytes());

        let hash = blake3_hash(&seed_data);
        let sample = u64::from_le_bytes([
            hash.0[0], hash.0[1], hash.0[2], hash.0[3], hash.0[4], hash.0[5], hash.0[6], hash.0[7],
        ]);

        let target_weight = sample % self.validator_set.total_voting_power;

        let mut cumulative = 0u64;
        for val in &self.validator_set.validators {
            cumulative += val.voting_power;
            if cumulative > target_weight {
                return val.validator_id;
            }
        }

        // Fallback to last validator
        self.validator_set.validators.last().unwrap().validator_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tola_types::*;

    fn make_test_committee() -> CommitteeManager {
        let v1 = ValidatorInfo {
            validator_id: ValidatorId(Address([0x01; 20])),
            consensus_pubkey: PublicKey::Ed25519([0x11; 32]),
            voting_power: 100,
            commission_bp: 500,
        };
        let v2 = ValidatorInfo {
            validator_id: ValidatorId(Address([0x02; 20])),
            consensus_pubkey: PublicKey::Ed25519([0x22; 32]),
            voting_power: 200,
            commission_bp: 500,
        };
        let v3 = ValidatorInfo {
            validator_id: ValidatorId(Address([0x03; 20])),
            consensus_pubkey: PublicKey::Ed25519([0x33; 32]),
            voting_power: 300,
            commission_bp: 500,
        };

        let set = ValidatorSet {
            epoch: Epoch(1),
            validators: vec![v1, v2, v3],
            total_voting_power: 600,
        };

        CommitteeManager::new(set).expect("Valid committee")
    }

    #[test]
    fn test_quorum_calculation() {
        // Total = 600. 2 * 600 / 3 = 400. Quorum = 401.
        assert_eq!(quorum_threshold(600), 401);
        assert!(!is_quorum(400, 600));
        assert!(is_quorum(401, 600));
        assert!(is_quorum(600, 600));

        // Total = 4. 2 * 4 / 3 = 2. Quorum = 3. (3f + 1 where f = 1 -> 3 * 1 + 1 = 4)
        assert_eq!(quorum_threshold(4), 3);
        assert!(!is_quorum(2, 4));
        assert!(is_quorum(3, 4));
    }

    #[test]
    fn test_deterministic_leader_election() {
        let cm = make_test_committee();

        let l1 = cm.leader_for_view(BlockHeight(1), Round(0));
        let l1_repeat = cm.leader_for_view(BlockHeight(1), Round(0));
        assert_eq!(
            l1, l1_repeat,
            "Leader election must be completely deterministic"
        );

        let l2 = cm.leader_for_view(BlockHeight(1), Round(1));
        // Verify leaders are valid members of the committee
        assert!(cm.find_validator(&l1).is_some());
        assert!(cm.find_validator(&l2).is_some());
    }
}
