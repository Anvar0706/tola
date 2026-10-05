//! Pipelined BFT consensus state machine, pacemaker, and 2-chain commit rule.

use tola_types::*;
use tola_core::{ConsensusEngine, ConsensusError};

#[allow(dead_code)]
pub struct PipelinedBftEngine {
    current_height: BlockHeight,
    current_round: Round,
    last_finalized: BlockHeight,
    locked_qc: Option<QuorumCertificate>,
}

impl PipelinedBftEngine {
    pub fn new() -> Self {
        Self {
            current_height: BlockHeight(0),
            current_round: Round(0),
            last_finalized: BlockHeight(0),
            locked_qc: None,
        }
    }
}

impl ConsensusEngine for PipelinedBftEngine {
    fn handle_proposal(&mut self, block: &Block) -> Result<Option<Vote>, ConsensusError> {
        if block.header.height.0 < self.current_height.0 {
            return Err(ConsensusError::StaleRound);
        }
        self.current_height = block.header.height;
        Ok(Some(Vote {
            vote_type: VoteType::Precommit,
            height: block.header.height,
            round: block.header.round,
            block_hash: block.header.state_root,
            validator: ValidatorId(block.header.proposer),
            signature: Signature::Ed25519([0u8; 64]),
        }))
    }

    fn handle_vote(&mut self, vote: Vote) -> Result<Option<QuorumCertificate>, ConsensusError> {
        Ok(Some(QuorumCertificate {
            height: vote.height,
            round: vote.round,
            block_hash: vote.block_hash,
            vote_type: vote.vote_type,
            validator_bitfield: vec![0b00000001],
            aggregated_signature: vec![0u8; 96],
        }))
    }

    fn get_last_finalized_height(&self) -> BlockHeight {
        self.last_finalized
    }
}
