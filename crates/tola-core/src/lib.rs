//! Core trait contracts defining subsystem boundaries for TOLA Network.

use tola_types::*;

#[derive(Debug)]
pub enum StateError {
    NotFound,
    CorruptedData,
    StorageFailure(String),
}

#[derive(Debug)]
pub enum BlockValidationError {
    InvalidParentHash,
    InvalidHeight,
    InvalidStateRoot,
    InvalidTransactionsRoot,
    InvalidSignature,
    NonceMismatch,
    InsufficientBalance,
    GasLimitExceeded,
    ExpiredTransaction,
}

#[derive(Debug)]
pub enum ConsensusError {
    InvalidQuorum,
    StaleRound,
    EquivocationDetected(ValidatorId),
    LeaderMismatch,
}

#[derive(Debug)]
pub enum VMError {
    OutOfGas,
    Revert(Vec<u8>),
    InvalidOpcode,
    StackOverflow,
}

#[derive(Debug)]
pub enum MempoolError {
    PoolFull,
    InvalidNonce,
    Underpriced,
    AlreadyKnown,
}

#[derive(Debug)]
pub enum NetworkError {
    PeerDisconnected,
    GossipRejected(String),
    RateLimitExceeded,
}

pub struct MerkleProof {
    pub leaf_index: u64,
    pub sibling_hashes: Vec<Hash>,
}

pub struct StateDelta {
    pub accounts: Vec<(Address, Option<AccountState>)>,
    pub storage: Vec<((Address, Hash), Hash)>,
}

pub trait StateDB: Send + Sync {
    fn get_account(&self, address: &Address) -> Result<Option<AccountState>, StateError>;
    fn get_storage(&self, address: &Address, slot: &Hash) -> Result<Hash, StateError>;
    fn apply_delta(&mut self, delta: StateDelta) -> Result<(), StateError>;
    fn compute_state_root(&self) -> Result<Hash, StateError>;
    fn commit(&mut self, height: BlockHeight) -> Result<Hash, StateError>;
    fn rollback(&mut self) -> Result<(), StateError>;
    fn generate_account_proof(&self, address: &Address) -> Result<MerkleProof, StateError>;
}

pub trait BlockEngine: Send + Sync {
    fn validate_block(&self, block: &Block, state: &dyn StateDB) -> Result<(), BlockValidationError>;
    fn execute_block(&self, block: &Block, state: &mut dyn StateDB) -> Result<Vec<Receipt>, VMError>;
}

pub trait ConsensusEngine: Send + Sync {
    fn handle_proposal(&mut self, block: &Block) -> Result<Option<Vote>, ConsensusError>;
    fn handle_vote(&mut self, vote: Vote) -> Result<Option<QuorumCertificate>, ConsensusError>;
    fn get_last_finalized_height(&self) -> BlockHeight;
}
