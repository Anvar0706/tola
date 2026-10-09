//! Consensus validator definitions, voting envelopes, and quorum certificates.

use crate::address::Address;
use crate::primitives::{Amount, BlockHeight, Epoch, Hash, PublicKey, Round, Signature};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ValidatorId(pub Address);

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum VoteType {
    Prevote,
    Precommit,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ValidatorStatus {
    Candidate,
    Active,
    Inactive,
    Jailed,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Vote {
    pub vote_type: VoteType,
    pub height: BlockHeight,
    pub round: Round,
    pub block_hash: Hash,
    pub validator: ValidatorId,
    pub signature: Signature,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct QuorumCertificate {
    pub height: BlockHeight,
    pub round: Round,
    pub block_hash: Hash,
    pub vote_type: VoteType,
    pub validator_bitfield: Vec<u8>,
    pub aggregated_signature: Vec<u8>,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ValidatorInfo {
    pub validator_id: ValidatorId,
    pub consensus_pubkey: PublicKey,
    pub voting_power: u64,
    pub commission_bp: u16,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ValidatorSet {
    pub epoch: Epoch,
    pub validators: Vec<ValidatorInfo>,
    pub total_voting_power: u64,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SlashingEvidence {
    pub height: BlockHeight,
    pub round: Round,
    pub vote_a: Vote,
    pub vote_b: Vote,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SlashReceipt {
    pub validator_id: ValidatorId,
    pub slashed_amount: Amount,
    pub jailing_height: BlockHeight,
    pub reason: String,
}
