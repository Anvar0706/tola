//! Core trait contracts defining subsystem boundaries for TOLA Network.

use tola_types::*;

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum StateError {
    NotFound,
    CorruptedData,
    StorageFailure(String),
}

#[derive(Debug, PartialEq, Eq, Clone)]
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
    FeeBelowBaseFee,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum ConsensusError {
    InvalidQuorum,
    StaleRound,
    EquivocationDetected(ValidatorId),
    LeaderMismatch,
    InvalidSignature,
    UnknownValidator(ValidatorId),
    LockedBlockMismatch,
    InvalidParentQC,
    VotingPowerInsufficient,
    InvalidRound,
    DuplicateVote,
    InvalidTxsRoot,
    InvalidReceiptsRoot,
    InvalidGasUsed,
    GasLimitExceeded,
    InvalidStateRoot,
    InternalError(String),
}

impl core::fmt::Display for ConsensusError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            ConsensusError::InvalidQuorum => write!(f, "Invalid quorum"),
            ConsensusError::StaleRound => write!(f, "Stale round"),
            ConsensusError::EquivocationDetected(v) => {
                write!(f, "Equivocation detected by validator {:?}", v)
            }
            ConsensusError::LeaderMismatch => write!(f, "Leader mismatch for view"),
            ConsensusError::InvalidSignature => write!(f, "Invalid consensus signature"),
            ConsensusError::UnknownValidator(v) => write!(f, "Unknown validator {:?}", v),
            ConsensusError::LockedBlockMismatch => write!(f, "Locked block mismatch"),
            ConsensusError::InvalidParentQC => write!(f, "Invalid parent QC"),
            ConsensusError::VotingPowerInsufficient => write!(f, "Voting power insufficient"),
            ConsensusError::InvalidRound => write!(f, "Invalid round"),
            ConsensusError::DuplicateVote => write!(f, "Duplicate vote"),
            ConsensusError::InvalidTxsRoot => write!(f, "Invalid block header transactions root"),
            ConsensusError::InvalidReceiptsRoot => write!(f, "Invalid block header receipts root"),
            ConsensusError::InvalidGasUsed => write!(f, "Invalid block header gas used"),
            ConsensusError::GasLimitExceeded => write!(f, "Block gas used exceeds gas limit"),
            ConsensusError::InvalidStateRoot => write!(f, "Invalid block header state root"),
            ConsensusError::InternalError(s) => write!(f, "Internal consensus error: {s}"),
        }
    }
}

impl std::error::Error for ConsensusError {}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum StakingError {
    ValidatorNotFound(ValidatorId),
    AlreadyRegistered,
    InsufficientSelfStake,
    InvalidCommissionRate,
    ValidatorJailed,
    ValidatorInactive,
    InsufficientShares,
    UnbondingNotMatured,
    InvalidSlashingEvidence(String),
    MathOverflow,
    InsufficientBalance,
    DelegationDisabledInV1,
    AlreadySettled,
    StorageError,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum VMError {
    OutOfGas,
    Revert(Vec<u8>),
    InvalidOpcode,
    StackOverflow,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum MempoolError {
    PoolFull,
    InvalidNonce,
    Underpriced,
    AlreadyKnown,
    InsufficientBalance,
    InvalidSignature,
    ExceedsMaxFutureGap,
    TransactionExpired,
    SpamFeeBelowMinimum,
    CrossTypeConflict,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum NetworkError {
    PeerDisconnected,
    GossipRejected(String),
    RateLimitExceeded,
    MalformedMessage(String),
    DuplicateMessage,
    ScoreTooLow,
    InvalidMagic,
    NetworkMismatch,
    GenesisMismatch,
    MessageTooLarge,
    ChecksumMismatch,
    PeerBanned,
    SubnetLimitExceeded,
    SerializationError(String),
    PeerNotFound,
}

pub trait P2PService: Send + Sync {
    fn broadcast_transaction(&self, tx: &Transaction) -> Result<(), NetworkError>;
    fn broadcast_block(&self, block: &Block) -> Result<(), NetworkError>;
    fn broadcast_vote(&self, vote: &Vote) -> Result<(), NetworkError>;
    fn peer_count(&self) -> usize;
}

pub struct MerkleProof {
    pub leaf_index: u64,
    pub sibling_hashes: Vec<Hash>,
}

#[derive(Default, Clone, Debug, PartialEq, Eq)]
pub struct StateDelta {
    pub accounts: Vec<(Address, Option<AccountState>)>,
    pub storage: Vec<((Address, Hash), Hash)>,
    pub code: Vec<(Hash, Vec<u8>)>,
}

impl StateDelta {
    pub fn new(
        accounts: Vec<(Address, Option<AccountState>)>,
        storage: Vec<((Address, Hash), Hash)>,
    ) -> Self {
        Self {
            accounts,
            storage,
            code: Vec::new(),
        }
    }
}

pub trait StateDB: Send + Sync {
    fn get_account(&self, address: &Address) -> Result<Option<AccountState>, StateError>;
    fn get_storage(&self, address: &Address, slot: &Hash) -> Result<Hash, StateError>;
    fn get_code(&self, code_hash: &Hash) -> Result<Option<Vec<u8>>, StateError> {
        let _ = code_hash;
        Ok(None)
    }
    fn apply_delta(&mut self, delta: StateDelta) -> Result<(), StateError>;
    fn compute_state_root(&self) -> Result<Hash, StateError>;
    fn commit(&mut self, height: BlockHeight) -> Result<Hash, StateError>;
    fn rollback(&mut self) -> Result<(), StateError>;
    fn generate_account_proof(&self, address: &Address) -> Result<MerkleProof, StateError>;
    fn index_eth_tx(
        &mut self,
        eth_tx_hash: &Hash,
        tola_tx_id: &TransactionId,
    ) -> Result<(), StateError> {
        let _ = (eth_tx_hash, tola_tx_id);
        Ok(())
    }
    fn get_eth_tx(&self, eth_tx_hash: &Hash) -> Result<Option<TransactionId>, StateError> {
        let _ = eth_tx_hash;
        Ok(None)
    }
    fn get_receipt(&self, tx_id: &TransactionId) -> Result<Option<Receipt>, StateError> {
        let _ = tx_id;
        Ok(None)
    }
    fn get_tx_location(&self, tx_id: &TransactionId) -> Result<Option<(Hash, u64)>, StateError> {
        let _ = tx_id;
        Ok(None)
    }
    fn get_block_by_hash(&self, block_hash: &Hash) -> Result<Option<Block>, StateError> {
        let _ = block_hash;
        Ok(None)
    }
    fn get_block_by_height(&self, height: BlockHeight) -> Result<Option<Block>, StateError> {
        let _ = height;
        Ok(None)
    }
    fn get_block_hash_by_height(&self, height: BlockHeight) -> Result<Option<Hash>, StateError> {
        let _ = height;
        Ok(None)
    }
    fn get_historical_account(
        &self,
        height: BlockHeight,
        address: &Address,
    ) -> Result<Option<AccountState>, StateError> {
        let _ = height;
        self.get_account(address)
    }
    fn get_historical_storage(
        &self,
        height: BlockHeight,
        address: &Address,
        slot: &Hash,
    ) -> Result<Hash, StateError> {
        let _ = height;
        self.get_storage(address, slot)
    }
    fn get_block_header_by_height(
        &self,
        height: BlockHeight,
    ) -> Result<Option<BlockHeader>, StateError> {
        let _ = height;
        Ok(None)
    }
    fn get_block_header_by_hash(
        &self,
        block_hash: &Hash,
    ) -> Result<Option<BlockHeader>, StateError> {
        let _ = block_hash;
        Ok(None)
    }
    fn current_height(&self) -> BlockHeight {
        BlockHeight(0)
    }
}

pub trait BlockEngine: Send + Sync {
    fn validate_block(
        &self,
        block: &Block,
        state: &dyn StateDB,
    ) -> Result<(), BlockValidationError>;
    fn execute_block(
        &self,
        block: &Block,
        state: &mut dyn StateDB,
    ) -> Result<Vec<Receipt>, VMError>;
}

pub trait ConsensusEngine: Send + Sync {
    fn handle_proposal(&mut self, block: &Block) -> Result<Option<Vote>, ConsensusError>;
    fn handle_vote(&mut self, vote: Vote) -> Result<Option<QuorumCertificate>, ConsensusError>;
    fn get_last_finalized_height(&self) -> BlockHeight;
}
