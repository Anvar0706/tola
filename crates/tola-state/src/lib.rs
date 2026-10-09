//! In-memory state journal, MVCC speculative delta, and cryptographic state root accumulator.
//!
//! Complies with ADR-0008, ADR-0010 (Jellyfish Merkle Tree with BLAKE3),
//! and State Transition Model & Invariants v0.1:
//! - Multi-level rollback checkpoints
//! - Lexicographically sorted deterministic Merkle root
//! - Cryptographic inclusion proof generation and verification
//! - Total supply conservation tracking

use std::collections::{BTreeMap, HashMap};
use tola_codec::CanonicalSerialize;
use tola_core::{MerkleProof, StateDB, StateDelta, StateError};
use tola_crypto::blake3_hash;
use tola_types::*;

const DOMAIN_JMT_LEAF: &[u8; 16] = b"TOLA-JMT-LEAF-v1";
const DOMAIN_JMT_NODE: &[u8; 16] = b"TOLA-JMT-NODE-v1";

/// Rollback frame tracking original values before mutations occurred.
#[derive(Default)]
struct CheckpointFrame {
    account_originals: HashMap<Address, Option<AccountState>>,
    storage_originals: HashMap<(Address, Hash), Option<Hash>>,
    code_originals: HashMap<Hash, Option<Vec<u8>>>,
}

/// In-memory state journal with multi-level checkpoints and Merkle root calculation.
pub struct MemoryStateJournal {
    accounts: BTreeMap<Address, AccountState>,
    storage: BTreeMap<(Address, Hash), Hash>,
    code: BTreeMap<Hash, Vec<u8>>,
    blocks_by_height: BTreeMap<BlockHeight, Hash>,
    blocks_by_hash: BTreeMap<Hash, Block>,
    checkpoints: Vec<CheckpointFrame>,
    total_burned: Amount,
    account_history: BTreeMap<(Address, u64), AccountState>,
    storage_history: BTreeMap<(Address, Hash, u64), Hash>,
}

impl MemoryStateJournal {
    pub fn new() -> Self {
        Self {
            accounts: BTreeMap::new(),
            storage: BTreeMap::new(),
            code: BTreeMap::new(),
            blocks_by_height: BTreeMap::new(),
            blocks_by_hash: BTreeMap::new(),
            checkpoints: Vec::new(),
            total_burned: Amount::ZERO,
            account_history: BTreeMap::new(),
            storage_history: BTreeMap::new(),
        }
    }

    pub fn new_with_genesis(accounts: Vec<(Address, AccountState)>) -> Self {
        let mut journal = Self::new();
        for (addr, state) in accounts {
            journal.accounts.insert(addr, state);
        }
        journal
    }

    /// Total number of accounts currently active in state.
    pub fn account_count(&self) -> usize {
        self.accounts.len()
    }

    /// Calculate circulating supply: sum of all account balances.
    pub fn circulating_supply(&self) -> Amount {
        let mut total: u128 = 0;
        for acct in self.accounts.values() {
            total = total
                .checked_add(acct.balance.as_u128())
                .expect("Circulating supply overflow");
        }
        Amount(total)
    }

    pub fn total_burned(&self) -> Amount {
        self.total_burned
    }

    pub fn record_burn(&mut self, amount: Amount) {
        self.total_burned = self.total_burned.saturating_add(amount);
    }

    pub fn record_block(&mut self, block: &Block, block_hash: Hash) {
        let h = block.header.height.0;
        self.blocks_by_height
            .insert(block.header.height, block_hash);
        self.blocks_by_hash.insert(block_hash, block.clone());
        for (addr, state) in &self.accounts {
            self.account_history.insert((*addr, h), state.clone());
        }
        for ((addr, slot), val) in &self.storage {
            self.storage_history.insert((*addr, *slot, h), *val);
        }
    }

    pub fn record_block_hash(&mut self, height: BlockHeight, block_hash: Hash) {
        self.blocks_by_height.insert(height, block_hash);
    }

    /// Begin a new speculative checkpoint frame. Returns checkpoint index.
    pub fn begin_checkpoint(&mut self) -> usize {
        self.checkpoints.push(CheckpointFrame::default());
        self.checkpoints.len() - 1
    }

    /// Commit the top checkpoint frame, collapsing mutations into parent frame if exists.
    pub fn commit_checkpoint(&mut self) -> Result<(), StateError> {
        if self.checkpoints.is_empty() {
            return Err(StateError::StorageFailure(
                "No active checkpoint to commit".into(),
            ));
        }

        let child = self.checkpoints.pop().unwrap();
        if let Some(parent) = self.checkpoints.last_mut() {
            // Propagate original values to parent only if parent doesn't already have them
            for (addr, orig) in child.account_originals {
                parent.account_originals.entry(addr).or_insert(orig);
            }
            for (key, orig) in child.storage_originals {
                parent.storage_originals.entry(key).or_insert(orig);
            }
            for (hash, orig) in child.code_originals {
                parent.code_originals.entry(hash).or_insert(orig);
            }
        }
        Ok(())
    }

    /// Rollback the state to a specific checkpoint index, reverting all mutations since.
    pub fn revert_to_checkpoint(&mut self, checkpoint_id: usize) -> Result<(), StateError> {
        if checkpoint_id >= self.checkpoints.len() {
            return Err(StateError::StorageFailure("Invalid checkpoint ID".into()));
        }

        while self.checkpoints.len() > checkpoint_id {
            let frame = self.checkpoints.pop().unwrap();
            for (addr, orig) in frame.account_originals {
                match orig {
                    Some(state) => self.accounts.insert(addr, state),
                    None => self.accounts.remove(&addr),
                };
            }
            for (key, orig) in frame.storage_originals {
                match orig {
                    Some(val) => self.storage.insert(key, val),
                    None => self.storage.remove(&key),
                };
            }
            for (hash, orig) in frame.code_originals {
                match orig {
                    Some(bytes) => self.code.insert(hash, bytes),
                    None => self.code.remove(&hash),
                };
            }
        }
        Ok(())
    }

    /// Mutate an account state while recording original value for active checkpoints.
    pub fn set_account(&mut self, address: Address, state: AccountState) {
        if let Some(frame) = self.checkpoints.last_mut() {
            frame
                .account_originals
                .entry(address)
                .or_insert_with(|| self.accounts.get(&address).cloned());
        }
        self.accounts.insert(address, state);
    }

    /// Mutate contract bytecode while recording original value for active checkpoints.
    pub fn set_code(&mut self, code_hash: Hash, bytecode: Vec<u8>) {
        if let Some(frame) = self.checkpoints.last_mut() {
            frame
                .code_originals
                .entry(code_hash)
                .or_insert_with(|| self.code.get(&code_hash).cloned());
        }
        self.code.insert(code_hash, bytecode);
    }

    /// Delete an account while recording original value for active checkpoints.
    pub fn remove_account(&mut self, address: &Address) {
        if let Some(frame) = self.checkpoints.last_mut() {
            frame
                .account_originals
                .entry(*address)
                .or_insert_with(|| self.accounts.get(address).cloned());
        }
        self.accounts.remove(address);
    }

    /// Mutate storage slot while recording original value for active checkpoints.
    pub fn set_storage(&mut self, address: Address, slot: Hash, value: Hash) {
        let key = (address, slot);
        if let Some(frame) = self.checkpoints.last_mut() {
            frame
                .storage_originals
                .entry(key)
                .or_insert_with(|| self.storage.get(&key).copied());
        }
        if value == Hash::ZERO {
            self.storage.remove(&key);
        } else {
            self.storage.insert(key, value);
        }
    }

    /// Compute cryptographic leaf hash for an account.
    pub fn compute_leaf_hash(address: &Address, state: &AccountState) -> Hash {
        let mut pre_image = Vec::with_capacity(16 + 20 + 92);
        pre_image.extend_from_slice(DOMAIN_JMT_LEAF);
        pre_image.extend_from_slice(address.as_bytes());
        pre_image.extend_from_slice(&state.to_canonical_bytes());
        blake3_hash(&pre_image)
    }

    /// Combine two sibling hashes into a parent Merkle node hash.
    pub fn combine_merkle_nodes(left: &Hash, right: &Hash) -> Hash {
        let mut pre_image = Vec::with_capacity(16 + 32 + 32);
        pre_image.extend_from_slice(DOMAIN_JMT_NODE);
        pre_image.extend_from_slice(left.as_bytes());
        pre_image.extend_from_slice(right.as_bytes());
        blake3_hash(&pre_image)
    }
}

impl Default for MemoryStateJournal {
    fn default() -> Self {
        Self::new()
    }
}

impl StateDB for MemoryStateJournal {
    fn get_account(&self, address: &Address) -> Result<Option<AccountState>, StateError> {
        Ok(self.accounts.get(address).cloned())
    }

    fn get_storage(&self, address: &Address, slot: &Hash) -> Result<Hash, StateError> {
        Ok(self
            .storage
            .get(&(*address, *slot))
            .copied()
            .unwrap_or(Hash::ZERO))
    }

    fn get_code(&self, code_hash: &Hash) -> Result<Option<Vec<u8>>, StateError> {
        Ok(self.code.get(code_hash).cloned())
    }

    fn get_block_by_hash(&self, block_hash: &Hash) -> Result<Option<Block>, StateError> {
        Ok(self.blocks_by_hash.get(block_hash).cloned())
    }

    fn get_block_by_height(&self, height: BlockHeight) -> Result<Option<Block>, StateError> {
        if let Some(hash) = self.blocks_by_height.get(&height) {
            Ok(self.blocks_by_hash.get(hash).cloned())
        } else {
            Ok(None)
        }
    }

    fn get_block_hash_by_height(&self, height: BlockHeight) -> Result<Option<Hash>, StateError> {
        Ok(self.blocks_by_height.get(&height).copied())
    }

    fn apply_delta(&mut self, delta: StateDelta) -> Result<(), StateError> {
        for (addr, state_opt) in delta.accounts {
            match state_opt {
                Some(state) => self.set_account(addr, state),
                None => self.remove_account(&addr),
            }
        }
        for ((addr, slot), val) in delta.storage {
            self.set_storage(addr, slot, val);
        }
        for (code_hash, bytecode) in delta.code {
            self.set_code(code_hash, bytecode);
        }
        Ok(())
    }

    /// Compute 32-byte cryptographic state root using sorted account leaves.
    fn compute_state_root(&self) -> Result<Hash, StateError> {
        if self.accounts.is_empty() {
            return Ok(Hash::ZERO);
        }

        // 1. Gather sorted leaf hashes (BTreeMap is inherently ordered by Address)
        let mut current_level: Vec<Hash> = self
            .accounts
            .iter()
            .map(|(addr, state)| Self::compute_leaf_hash(addr, state))
            .collect();

        // 2. Pairwise reduce until root
        while current_level.len() > 1 {
            let mut next_level = Vec::with_capacity(current_level.len().div_ceil(2));
            for chunk in current_level.chunks(2) {
                if chunk.len() == 2 {
                    next_level.push(Self::combine_merkle_nodes(&chunk[0], &chunk[1]));
                } else {
                    // Odd element promoted with itself to form balanced parent
                    next_level.push(Self::combine_merkle_nodes(&chunk[0], &chunk[0]));
                }
            }
            current_level = next_level;
        }

        Ok(current_level[0])
    }

    fn commit(&mut self, height: BlockHeight) -> Result<Hash, StateError> {
        self.checkpoints.clear();
        for (addr, state) in &self.accounts {
            self.account_history
                .insert((*addr, height.0), state.clone());
        }
        for ((addr, slot), val) in &self.storage {
            self.storage_history.insert((*addr, *slot, height.0), *val);
        }
        self.compute_state_root()
    }

    fn rollback(&mut self) -> Result<(), StateError> {
        if !self.checkpoints.is_empty() {
            self.revert_to_checkpoint(self.checkpoints.len() - 1)
        } else {
            Ok(())
        }
    }

    fn get_historical_account(
        &self,
        height: BlockHeight,
        address: &Address,
    ) -> Result<Option<AccountState>, StateError> {
        if self.account_history.is_empty() {
            return self.get_account(address);
        }
        let max_key = (*address, height.0);
        if let Some(((addr, _h), state)) = self.account_history.range(..=max_key).next_back() {
            if addr == address {
                return Ok(Some(state.clone()));
            }
        }
        Ok(None)
    }

    fn get_historical_storage(
        &self,
        height: BlockHeight,
        address: &Address,
        slot: &Hash,
    ) -> Result<Hash, StateError> {
        let max_key = (*address, *slot, height.0);
        if let Some(((addr, s, _h), val)) = self.storage_history.range(..=max_key).next_back() {
            if addr == address && s == slot {
                return Ok(*val);
            }
        }
        Ok(Hash::ZERO)
    }

    fn get_block_header_by_height(
        &self,
        height: BlockHeight,
    ) -> Result<Option<BlockHeader>, StateError> {
        if let Some(bh) = self.blocks_by_height.get(&height) {
            if let Some(b) = self.blocks_by_hash.get(bh) {
                return Ok(Some(b.header.clone()));
            }
        }
        Ok(None)
    }

    fn get_block_header_by_hash(
        &self,
        block_hash: &Hash,
    ) -> Result<Option<BlockHeader>, StateError> {
        if let Some(b) = self.blocks_by_hash.get(block_hash) {
            return Ok(Some(b.header.clone()));
        }
        Ok(None)
    }

    fn current_height(&self) -> BlockHeight {
        BlockHeight(
            self.blocks_by_height
                .keys()
                .max()
                .copied()
                .unwrap_or(BlockHeight(0))
                .0,
        )
    }

    /// Generate cryptographic Merkle inclusion proof for account.
    fn generate_account_proof(&self, address: &Address) -> Result<MerkleProof, StateError> {
        let (target_idx, _) = self
            .accounts
            .iter()
            .enumerate()
            .find(|(_, (a, _))| *a == address)
            .ok_or(StateError::NotFound)?;

        let mut current_level: Vec<Hash> = self
            .accounts
            .iter()
            .map(|(addr, state)| Self::compute_leaf_hash(addr, state))
            .collect();

        let mut sibling_hashes = Vec::new();
        let mut idx = target_idx;

        while current_level.len() > 1 {
            let sibling_idx = if idx % 2 == 0 {
                if idx + 1 < current_level.len() {
                    idx + 1
                } else {
                    idx
                }
            } else {
                idx - 1
            };
            sibling_hashes.push(current_level[sibling_idx]);

            let mut next_level = Vec::with_capacity(current_level.len().div_ceil(2));
            for chunk in current_level.chunks(2) {
                if chunk.len() == 2 {
                    next_level.push(Self::combine_merkle_nodes(&chunk[0], &chunk[1]));
                } else {
                    next_level.push(Self::combine_merkle_nodes(&chunk[0], &chunk[0]));
                }
            }
            current_level = next_level;
            idx /= 2;
        }

        Ok(MerkleProof {
            leaf_index: target_idx as u64,
            sibling_hashes,
        })
    }
}

/// Verify cryptographic Merkle proof against root hash.
pub fn verify_account_proof(
    root: Hash,
    address: &Address,
    state: &AccountState,
    proof: &MerkleProof,
) -> bool {
    let mut current = MemoryStateJournal::compute_leaf_hash(address, state);
    let mut idx = proof.leaf_index as usize;

    for sibling in &proof.sibling_hashes {
        if idx.is_multiple_of(2) {
            current = MemoryStateJournal::combine_merkle_nodes(&current, sibling);
        } else {
            current = MemoryStateJournal::combine_merkle_nodes(sibling, &current);
        }
        idx /= 2;
    }

    current == root
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_checkpoint_revert() {
        let mut journal = MemoryStateJournal::new();
        let addr1 = Address([1u8; 20]);
        let addr2 = Address([2u8; 20]);

        journal.set_account(
            addr1,
            AccountState::new_with_balance(Amount::from_u128(100)),
        );

        let cp0 = journal.begin_checkpoint();
        journal.set_account(
            addr1,
            AccountState::new_with_balance(Amount::from_u128(200)),
        );
        journal.set_account(
            addr2,
            AccountState::new_with_balance(Amount::from_u128(500)),
        );

        assert_eq!(
            journal.get_account(&addr1).unwrap().unwrap().balance,
            Amount::from_u128(200)
        );
        assert_eq!(
            journal.get_account(&addr2).unwrap().unwrap().balance,
            Amount::from_u128(500)
        );

        // Revert to cp0
        journal.revert_to_checkpoint(cp0).unwrap();

        assert_eq!(
            journal.get_account(&addr1).unwrap().unwrap().balance,
            Amount::from_u128(100)
        );
        assert!(journal.get_account(&addr2).unwrap().is_none());
    }

    #[test]
    fn test_checkpoint_commit() {
        let mut journal = MemoryStateJournal::new();
        let addr = Address([3u8; 20]);

        let _cp = journal.begin_checkpoint();
        journal.set_account(addr, AccountState::new_with_balance(Amount::from_u128(300)));
        journal.commit_checkpoint().unwrap();

        // After commit, account remains present
        assert_eq!(
            journal.get_account(&addr).unwrap().unwrap().balance,
            Amount::from_u128(300)
        );
    }

    #[test]
    fn test_deterministic_state_root() {
        let addr1 = Address([1u8; 20]);
        let addr2 = Address([2u8; 20]);
        let addr3 = Address([3u8; 20]);

        let state1 = AccountState::new_with_balance(Amount::from_u128(100));
        let state2 = AccountState::new_with_balance(Amount::from_u128(200));
        let state3 = AccountState::new_with_balance(Amount::from_u128(300));

        // Insert in order 1, 2, 3
        let mut j1 = MemoryStateJournal::new();
        j1.set_account(addr1, state1.clone());
        j1.set_account(addr2, state2.clone());
        j1.set_account(addr3, state3.clone());
        let root1 = j1.compute_state_root().unwrap();

        // Insert in order 3, 1, 2
        let mut j2 = MemoryStateJournal::new();
        j2.set_account(addr3, state3.clone());
        j2.set_account(addr1, state1.clone());
        j2.set_account(addr2, state2.clone());
        let root2 = j2.compute_state_root().unwrap();

        assert_ne!(root1, Hash::ZERO);
        assert_eq!(
            root1, root2,
            "State root must be deterministic regardless of insertion order"
        );
    }

    #[test]
    fn test_merkle_proof_verification() {
        let mut journal = MemoryStateJournal::new();
        let addr1 = Address([0x10; 20]);
        let addr2 = Address([0x20; 20]);
        let addr3 = Address([0x30; 20]);
        let addr4 = Address([0x40; 20]);

        let state1 = AccountState::new_with_balance(Amount::from_u128(1_000));
        let state2 = AccountState::new_with_balance(Amount::from_u128(2_000));
        let state3 = AccountState::new_with_balance(Amount::from_u128(3_000));
        let state4 = AccountState::new_with_balance(Amount::from_u128(4_000));

        journal.set_account(addr1, state1.clone());
        journal.set_account(addr2, state2.clone());
        journal.set_account(addr3, state3.clone());
        journal.set_account(addr4, state4.clone());

        let root = journal.compute_state_root().unwrap();

        // Generate proof for addr2
        let proof = journal.generate_account_proof(&addr2).unwrap();
        assert!(verify_account_proof(root, &addr2, &state2, &proof));

        // Verification fails if state or address is modified
        let fake_state = AccountState::new_with_balance(Amount::from_u128(9_999));
        assert!(!verify_account_proof(root, &addr2, &fake_state, &proof));
    }
}
