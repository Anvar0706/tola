//! Automated equivocation detection, cryptographic proof verification, and slashing penalties.
//!
//! Enforces Byzantine accountability: any validator that signs conflicting votes
//! for the same height, round, and vote phase suffers an immediate 10% stake burn
//! and jailing for 30 days.

use tola_core::StakingError;
use tola_crypto::verify_ed25519;
use tola_types::{BlockHeight, PublicKey, Signature, SlashReceipt, SlashingEvidence};

use crate::qc::compute_vote_hash;
use crate::registry::ValidatorRegistry;

/// Default slashing penalty for Byzantine equivocation: 500 basis points (5.00%) per Decisions D02/D06.
pub const DEFAULT_EQUIVOCATION_PENALTY_BP: u16 = 500;

#[derive(Clone, Debug)]
pub struct SlashingEngineConfig {
    pub equivocation_penalty_bp: u16,
}

impl Default for SlashingEngineConfig {
    fn default() -> Self {
        Self {
            equivocation_penalty_bp: DEFAULT_EQUIVOCATION_PENALTY_BP,
        }
    }
}

pub struct SlashingEngine {
    config: SlashingEngineConfig,
    slashed_offenses: std::sync::Mutex<
        std::collections::HashSet<(tola_types::ValidatorId, BlockHeight, tola_types::Round)>,
    >,
}

impl SlashingEngine {
    pub fn new(config: SlashingEngineConfig) -> Self {
        Self {
            config,
            slashed_offenses: std::sync::Mutex::new(std::collections::HashSet::new()),
        }
    }

    /// Verifies the cryptographic validity of equivocation evidence.
    ///
    /// Validates:
    /// 1. Both votes are attributed to the exact same validator.
    /// 2. Both votes target the identical block height and consensus round.
    /// 3. Both votes share the identical vote type (Prevote or Precommit).
    /// 4. Both votes reference distinct, conflicting block hashes.
    /// 5. The validator exists in the registry.
    /// 6. Both cryptographic signatures are valid for their respective vote message hashes under the validator's registered consensus public key.
    pub fn verify_equivocation_evidence(
        &self,
        evidence: &SlashingEvidence,
        registry: &ValidatorRegistry,
    ) -> Result<(), StakingError> {
        // 1. Same validator
        if evidence.vote_a.validator != evidence.vote_b.validator {
            return Err(StakingError::InvalidSlashingEvidence(
                "Conflicting votes must originate from the same validator".into(),
            ));
        }

        // 2. Matching height and round
        if evidence.vote_a.height != evidence.height || evidence.vote_b.height != evidence.height {
            return Err(StakingError::InvalidSlashingEvidence(
                "Vote height does not match evidence height".into(),
            ));
        }

        if evidence.vote_a.round != evidence.round || evidence.vote_b.round != evidence.round {
            return Err(StakingError::InvalidSlashingEvidence(
                "Vote round does not match evidence round".into(),
            ));
        }

        // 3. Same vote type
        if evidence.vote_a.vote_type != evidence.vote_b.vote_type {
            return Err(StakingError::InvalidSlashingEvidence(
                "Conflicting votes must be of the same vote type".into(),
            ));
        }

        // 4. Distinct block hashes (must prove equivocation)
        if evidence.vote_a.block_hash == evidence.vote_b.block_hash {
            return Err(StakingError::InvalidSlashingEvidence(
                "Block hashes are identical; evidence does not demonstrate equivocation".into(),
            ));
        }

        // 5. Look up validator record
        let val_record = registry
            .get_validator(&evidence.vote_a.validator)
            .ok_or(StakingError::ValidatorNotFound(evidence.vote_a.validator))?;

        // 6. Verify cryptographic signatures
        let hash_a = compute_vote_hash(
            evidence.vote_a.vote_type,
            evidence.vote_a.height,
            evidence.vote_a.round,
            &evidence.vote_a.block_hash,
        );
        let hash_b = compute_vote_hash(
            evidence.vote_b.vote_type,
            evidence.vote_b.height,
            evidence.vote_b.round,
            &evidence.vote_b.block_hash,
        );

        match (
            &val_record.consensus_pubkey,
            &evidence.vote_a.signature,
            &evidence.vote_b.signature,
        ) {
            (PublicKey::Ed25519(pk), Signature::Ed25519(sig_a), Signature::Ed25519(sig_b)) => {
                if verify_ed25519(pk, &hash_a.0, sig_a).is_err() {
                    return Err(StakingError::InvalidSlashingEvidence(
                        "Vote A signature verification failed".into(),
                    ));
                }
                if verify_ed25519(pk, &hash_b.0, sig_b).is_err() {
                    return Err(StakingError::InvalidSlashingEvidence(
                        "Vote B signature verification failed".into(),
                    ));
                }
            }
            _ => {
                return Err(StakingError::InvalidSlashingEvidence(
                    "Unsupported cryptographic key or signature format".into(),
                ));
            }
        }

        Ok(())
    }

    /// Verifies equivocation evidence and executes the slashing penalty on the validator registry.
    pub fn process_slashing(
        &self,
        evidence: &SlashingEvidence,
        registry: &mut ValidatorRegistry,
        current_height: BlockHeight,
    ) -> Result<SlashReceipt, StakingError> {
        self.verify_equivocation_evidence(evidence, registry)?;

        let offense_key = (evidence.vote_a.validator, evidence.height, evidence.round);
        {
            let mut slashed = self
                .slashed_offenses
                .lock()
                .map_err(|_| StakingError::InvalidSlashingEvidence("Lock poisoned".into()))?;
            if slashed.contains(&offense_key) {
                return Err(StakingError::InvalidSlashingEvidence(
                    "Offense has already been slashed".into(),
                ));
            }
            slashed.insert(offense_key);
        }

        let reason = format!(
            "Equivocation at height {} round {}",
            evidence.height.0, evidence.round.0
        );

        registry.slash_validator(
            &evidence.vote_a.validator,
            self.config.equivocation_penalty_bp,
            current_height,
            reason,
        )
    }
}

impl Default for SlashingEngine {
    fn default() -> Self {
        Self::new(SlashingEngineConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::RegistryConfig;
    use tola_crypto::Ed25519Keypair;
    use tola_types::{Address, Amount, Hash, Round, ValidatorId, ValidatorStatus, Vote, VoteType};

    fn setup_test_registry_with_validator() -> (ValidatorRegistry, Ed25519Keypair, ValidatorId) {
        let config = RegistryConfig {
            min_self_stake: Amount(10_000),
            max_active_validators: 4,
            unbonding_delay_blocks: 100,
            jailing_duration_blocks: 500,
        };
        let mut reg = ValidatorRegistry::new(config);
        let kp = Ed25519Keypair::generate();
        let op = Address([0x42; 20]);
        let val_id = ValidatorId(op);

        reg.register_validator(op, kp.public_key(), Amount(50_000), 500)
            .unwrap();
        (reg, kp, val_id)
    }

    #[test]
    fn test_valid_equivocation_evidence_slashes_and_jails_validator() {
        let (mut reg, kp, val_id) = setup_test_registry_with_validator();
        let engine = SlashingEngine::default();

        let height = BlockHeight(42);
        let round = Round(1);
        let hash_a = Hash([0x11; 32]);
        let hash_b = Hash([0x22; 32]);

        let msg_a = compute_vote_hash(VoteType::Prevote, height, round, &hash_a);
        let msg_b = compute_vote_hash(VoteType::Prevote, height, round, &hash_b);

        let vote_a = Vote {
            vote_type: VoteType::Prevote,
            height,
            round,
            block_hash: hash_a,
            validator: val_id,
            signature: kp.sign(&msg_a.0),
        };

        let vote_b = Vote {
            vote_type: VoteType::Prevote,
            height,
            round,
            block_hash: hash_b,
            validator: val_id,
            signature: kp.sign(&msg_b.0),
        };

        let evidence = SlashingEvidence {
            height,
            round,
            vote_a,
            vote_b,
        };

        let receipt = engine
            .process_slashing(&evidence, &mut reg, BlockHeight(50))
            .unwrap();

        // 5% of 50,000 = 2,500 slashed per Decision D02/D06
        assert_eq!(receipt.slashed_amount, Amount(2_500));
        assert_eq!(receipt.validator_id, val_id);
        assert_eq!(receipt.jailing_height, BlockHeight(50));

        let val = reg.get_validator(&val_id).unwrap();
        assert_eq!(val.status, ValidatorStatus::Jailed);
        assert_eq!(val.total_bonded_stake, Amount(47_500));
        assert_eq!(val.unjail_allowed_height, Some(BlockHeight(550)));

        // Duplicate submission of the exact same offense must be rejected
        let dup_err = engine
            .process_slashing(&evidence, &mut reg, BlockHeight(51))
            .unwrap_err();
        match dup_err {
            StakingError::InvalidSlashingEvidence(msg) => {
                assert!(msg.contains("already been slashed"));
            }
            _ => panic!("Expected duplicate slashing rejection"),
        }
    }

    #[test]
    fn test_identical_hashes_rejected_as_non_equivocation() {
        let (mut reg, kp, val_id) = setup_test_registry_with_validator();
        let engine = SlashingEngine::default();

        let height = BlockHeight(10);
        let round = Round(0);
        let hash = Hash([0x99; 32]);

        let msg = compute_vote_hash(VoteType::Precommit, height, round, &hash);
        let sig = kp.sign(&msg.0);

        let vote_a = Vote {
            vote_type: VoteType::Precommit,
            height,
            round,
            block_hash: hash,
            validator: val_id,
            signature: sig.clone(),
        };

        let vote_b = Vote {
            vote_type: VoteType::Precommit,
            height,
            round,
            block_hash: hash,
            validator: val_id,
            signature: sig,
        };

        let evidence = SlashingEvidence {
            height,
            round,
            vote_a,
            vote_b,
        };

        let err = engine
            .process_slashing(&evidence, &mut reg, BlockHeight(11))
            .unwrap_err();
        match err {
            StakingError::InvalidSlashingEvidence(msg) => {
                assert!(msg.contains("identical"));
            }
            _ => panic!("Expected InvalidSlashingEvidence, got {:?}", err),
        }
    }

    #[test]
    fn test_forged_signature_rejected() {
        let (mut reg, kp, val_id) = setup_test_registry_with_validator();
        let other_kp = Ed25519Keypair::generate();
        let engine = SlashingEngine::default();

        let height = BlockHeight(10);
        let round = Round(0);
        let hash_a = Hash([0x01; 32]);
        let hash_b = Hash([0x02; 32]);

        let msg_a = compute_vote_hash(VoteType::Prevote, height, round, &hash_a);
        let msg_b = compute_vote_hash(VoteType::Prevote, height, round, &hash_b);

        // Vote A signed by the actual validator
        let vote_a = Vote {
            vote_type: VoteType::Prevote,
            height,
            round,
            block_hash: hash_a,
            validator: val_id,
            signature: kp.sign(&msg_a.0),
        };

        // Vote B forged by an attacker with a different key
        let vote_b = Vote {
            vote_type: VoteType::Prevote,
            height,
            round,
            block_hash: hash_b,
            validator: val_id,
            signature: other_kp.sign(&msg_b.0),
        };

        let evidence = SlashingEvidence {
            height,
            round,
            vote_a,
            vote_b,
        };

        let err = engine
            .process_slashing(&evidence, &mut reg, BlockHeight(11))
            .unwrap_err();
        match err {
            StakingError::InvalidSlashingEvidence(msg) => {
                assert!(msg.contains("signature verification failed"));
            }
            _ => panic!("Expected InvalidSlashingEvidence, got {:?}", err),
        }
    }

    #[test]
    fn test_mismatched_validator_ids_rejected() {
        let (mut reg, kp, val_id1) = setup_test_registry_with_validator();
        let val_id2 = ValidatorId(Address([0x99; 20]));
        let engine = SlashingEngine::default();

        let height = BlockHeight(5);
        let round = Round(0);
        let hash_a = Hash([0x10; 32]);
        let hash_b = Hash([0x20; 32]);

        let msg_a = compute_vote_hash(VoteType::Prevote, height, round, &hash_a);
        let msg_b = compute_vote_hash(VoteType::Prevote, height, round, &hash_b);

        let vote_a = Vote {
            vote_type: VoteType::Prevote,
            height,
            round,
            block_hash: hash_a,
            validator: val_id1,
            signature: kp.sign(&msg_a.0),
        };

        let vote_b = Vote {
            vote_type: VoteType::Prevote,
            height,
            round,
            block_hash: hash_b,
            validator: val_id2,
            signature: kp.sign(&msg_b.0),
        };

        let evidence = SlashingEvidence {
            height,
            round,
            vote_a,
            vote_b,
        };

        let err = engine
            .process_slashing(&evidence, &mut reg, BlockHeight(6))
            .unwrap_err();
        match err {
            StakingError::InvalidSlashingEvidence(msg) => {
                assert!(msg.contains("same validator"));
            }
            _ => panic!("Expected InvalidSlashingEvidence, got {:?}", err),
        }
    }
}
