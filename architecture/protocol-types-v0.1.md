# TOLA Network: Protocol Types Specification v0.1

## 1. Primitive Foundation Types

```rust
/// 32-byte cryptographic hash digest (BLAKE3 or Keccak-256).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Hash(pub [u8; 32]);

/// 20-byte canonical address derived from public key.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Address(pub [u8; 20]);

/// Public key representation (Ed25519: 32 bytes; Secp256k1: 33 bytes compressed).
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum PublicKey {
    Ed25519([u8; 32]),
    Secp256k1([u8; 33]),
}

/// Cryptographic signature payload.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Signature {
    Ed25519([u8; 64]),
    Secp256k1 { r: [u8; 32], s: [u8; 32], v: u8 },
}

/// Native currency value expressed in nano-tola (10^-18 TOLA).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Debug)]
pub struct Amount(pub u128);

/// Account nonce to prevent transaction replay.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Debug)]
pub struct Nonce(pub u64);

/// Gas counter for computational execution.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Debug)]
pub struct Gas(pub u64);

/// Network identifier (Mainnet = 9999, Testnet = 9998, Devnet = 9997).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ChainId(pub u64);

/// Monotonically increasing block height.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Debug)]
pub struct BlockHeight(pub u64);

/// Consensus epoch identifier (rotates validator committees).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Debug)]
pub struct Epoch(pub u64);

/// Consensus round/view within a specific block height.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Debug)]
pub struct Round(pub u32);

/// UTC Unix timestamp in milliseconds.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Debug)]
pub struct Timestamp(pub u64);
```

---

## 2. Core Block & Transaction Structures

```rust
/// Canonical transaction representation.
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

/// 32-byte transaction identifier: BLAKE3(Borsh(TransactionWithoutSig)).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct TransactionId(pub Hash);

/// Canonical block header.
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

/// Complete block entity.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Block {
    pub header: BlockHeader,
    pub transactions: Vec<Transaction>,
    pub last_commit_qc: Option<QuorumCertificate>,
}

/// Deterministic transaction execution receipt.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Receipt {
    pub tx_id: TransactionId,
    pub success: bool,
    pub gas_used: Gas,
    pub cumulative_gas_used: Gas,
    pub logs: Vec<Log>,
    pub return_data: Vec<u8>,
}

/// Event log entry emitted during contract execution.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Log {
    pub address: Address,
    pub topics: Vec<Hash>,
    pub data: Vec<u8>,
}
```

---

## 3. Consensus & Validator Types

```rust
/// Unique identifier for an active validator.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ValidatorId(pub Address);

/// Consensus vote message.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Vote {
    pub vote_type: VoteType,
    pub height: BlockHeight,
    pub round: Round,
    pub block_hash: Hash,
    pub validator: ValidatorId,
    pub signature: Signature,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VoteType {
    Prevote,
    Precommit,
}

/// Quorum Certificate (QC) aggregating >= 2/3 + 1 validator signatures.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct QuorumCertificate {
    pub height: BlockHeight,
    pub round: Round,
    pub block_hash: Hash,
    pub vote_type: VoteType,
    pub validator_bitfield: Vec<u8>,
    pub aggregated_signature: Vec<u8>,
}

/// Active validator metadata.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ValidatorInfo {
    pub validator_id: ValidatorId,
    pub consensus_pubkey: PublicKey,
    pub voting_power: u64,
    pub commission_bp: u16,
}

/// Set of validators authorized for a consensus epoch.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ValidatorSet {
    pub epoch: Epoch,
    pub validators: Vec<ValidatorInfo>,
    pub total_voting_power: u64,
}
```
