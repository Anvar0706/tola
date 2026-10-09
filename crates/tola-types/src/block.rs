//! Block header, block body, and execution receipt structures.

use crate::address::Address;
use crate::consensus::QuorumCertificate;
use crate::primitives::{Amount, BlockHeight, Epoch, Gas, Hash, Round, Timestamp};
use crate::transaction::{Transaction, TransactionId};

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
    pub base_fee_per_gas: Amount,
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
