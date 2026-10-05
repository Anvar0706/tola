//! Canonical data types and foundation structures for TOLA Network.

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Debug)]
pub struct Hash(pub [u8; 32]);

impl Hash {
    pub const ZERO: Self = Self([0u8; 32]);
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Debug)]
pub struct Address(pub [u8; 20]);

impl Address {
    pub const ZERO: Self = Self([0u8; 20]);
    pub fn as_bytes(&self) -> &[u8; 20] {
        &self.0
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Debug)]
pub struct Amount(pub u128);

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Debug)]
pub struct Nonce(pub u64);

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Debug)]
pub struct Gas(pub u64);

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ChainId(pub u64);

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Debug)]
pub struct BlockHeight(pub u64);

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Debug)]
pub struct Epoch(pub u64);

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Debug)]
pub struct Round(pub u32);

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Debug)]
pub struct Timestamp(pub u64);

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum PublicKey {
    Ed25519([u8; 32]),
    Secp256k1([u8; 33]),
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Signature {
    Ed25519([u8; 64]),
    Secp256k1 { r: [u8; 32], s: [u8; 32], v: u8 },
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Transaction {
    pub chain_id: ChainId,
    pub nonce: Nonce,
    pub from: Address,
    pub to: Option<Address>,
    pub value: Amount,
    pub gas_limit: Gas,
    pub max_fee_per_gas: Amount,
    pub max_priority_fee_per_gas: Amount,
    pub valid_until_block: BlockHeight,
    pub data: Vec<u8>,
    pub signature: Signature,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct TransactionId(pub Hash);

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct BlockHeader {
    pub parent_hash: Hash,
    pub height: BlockHeight,
    pub epoch: Epoch,
    pub round: Round,
    pub timestamp: Timestamp,
    pub proposer: Address,
    pub state_root: Hash,
    pub txs_root: Hash,
    pub receipts_root: Hash,
    pub gas_used: Gas,
    pub gas_limit: Gas,
    pub extra_data: Vec<u8>,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Block {
    pub header: BlockHeader,
    pub transactions: Vec<Transaction>,
    pub last_commit_qc: Option<QuorumCertificate>,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Receipt {
    pub tx_id: TransactionId,
    pub success: bool,
    pub gas_used: Gas,
    pub cumulative_gas_used: Gas,
    pub logs: Vec<Log>,
    pub return_data: Vec<u8>,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Log {
    pub address: Address,
    pub topics: Vec<Hash>,
    pub data: Vec<u8>,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ValidatorId(pub Address);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VoteType {
    Prevote,
    Precommit,
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
pub struct AccountState {
    pub nonce: Nonce,
    pub balance: Amount,
    pub code_hash: Hash,
    pub storage_root: Hash,
    pub flags: u32,
}
