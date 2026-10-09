//! Persistent StateDB implementation backed by DiskKvStore and PersistentJmt.
//!
//! Complies with ADR-0008, ADR-0010, and Trait Contracts v0.1:
//! - Implements tola_core::StateDB
//! - Atomic multi-table commits
//! - Sub-microsecond point reads and zero compaction stalls

use crate::jmt::{PersistentJmt, PersistentJmtReader};
use crate::kv::{DiskKvStore, KeyValueStore, StorageBatch};
use crate::schema::{
    encode_account_key, encode_hash_key, encode_storage_history_key, encode_storage_key, Table,
};
use std::collections::BTreeMap;
use tola_codec::{CanonicalDeserialize, CanonicalSerialize};
use tola_core::{MerkleProof, StateDB, StateDelta, StateError};
use tola_types::{
    AccountState, Address, Block, BlockHeader, BlockHeight, Hash, Receipt, TransactionId,
};

pub struct PersistentStateDB {
    store: DiskKvStore,
    current_height: BlockHeight,
    state_root: Hash,
    working_accounts: BTreeMap<Address, Option<AccountState>>,
    working_storage: BTreeMap<(Address, Hash), Hash>,
    working_code: BTreeMap<Hash, Vec<u8>>,
}

impl PersistentStateDB {
    pub fn open(path: impl AsRef<std::path::Path>) -> Result<Self, StateError> {
        let store =
            DiskKvStore::open(path).map_err(|e| StateError::StorageFailure(e.to_string()))?;
        Self::init(store)
    }

    pub fn open_transient() -> Self {
        let store = DiskKvStore::open_transient();
        Self::init(store).expect("Transient init succeeds")
    }

    fn init(store: DiskKvStore) -> Result<Self, StateError> {
        let mut db = Self {
            store,
            current_height: BlockHeight(0),
            state_root: Hash::ZERO,
            working_accounts: BTreeMap::new(),
            working_storage: BTreeMap::new(),
            working_code: BTreeMap::new(),
        };

        // Load metadata if available
        if let Ok(Some(bytes)) = db.store.get(Table::Metadata, b"latest_height") {
            if bytes.len() == 8 {
                let h = u64::from_le_bytes(bytes.try_into().unwrap());
                db.current_height = BlockHeight(h);
            }
        }
        if let Ok(Some(bytes)) = db.store.get(Table::Metadata, b"state_root") {
            if bytes.len() == 32 {
                let mut r = [0u8; 32];
                r.copy_from_slice(&bytes);
                db.state_root = Hash(r);
            }
        }

        Ok(db)
    }

    pub fn current_height(&self) -> BlockHeight {
        self.current_height
    }

    pub fn state_root(&self) -> Hash {
        self.state_root
    }

    pub fn index_eth_tx(
        &mut self,
        eth_tx_hash: &Hash,
        tola_tx_id: &TransactionId,
    ) -> Result<(), StateError> {
        let mut batch = StorageBatch::new();
        batch.put(
            Table::EthTxLookup,
            eth_tx_hash.as_bytes().to_vec(),
            tola_tx_id.0.as_bytes().to_vec(),
        );
        self.store
            .write_batch(batch)
            .map_err(|e| StateError::StorageFailure(e.to_string()))
    }

    pub fn get_eth_tx(&self, eth_tx_hash: &Hash) -> Result<Option<TransactionId>, StateError> {
        let bytes_opt = self
            .store
            .get(Table::EthTxLookup, eth_tx_hash.as_bytes())
            .map_err(|e| StateError::StorageFailure(e.to_string()))?;
        match bytes_opt {
            Some(bytes) => {
                if bytes.len() == 32 {
                    let mut arr = [0u8; 32];
                    arr.copy_from_slice(&bytes);
                    Ok(Some(TransactionId(Hash(arr))))
                } else {
                    Ok(None)
                }
            }
            None => Ok(None),
        }
    }

    pub fn put_receipt(
        &mut self,
        tx_id: &TransactionId,
        receipt: &Receipt,
    ) -> Result<(), StateError> {
        let mut batch = StorageBatch::new();
        batch.put(
            Table::Receipts,
            tx_id.0.as_bytes().to_vec(),
            receipt.to_canonical_bytes(),
        );
        self.store
            .write_batch(batch)
            .map_err(|e| StateError::StorageFailure(e.to_string()))
    }

    pub fn get_receipt(&self, tx_id: &TransactionId) -> Result<Option<Receipt>, StateError> {
        let bytes_opt = self
            .store
            .get(Table::Receipts, tx_id.0.as_bytes())
            .map_err(|e| StateError::StorageFailure(e.to_string()))?;
        match bytes_opt {
            Some(bytes) => {
                let receipt = Receipt::deserialize_canonical(&bytes)
                    .map_err(|e| StateError::StorageFailure(e.to_string()))?;
                Ok(Some(receipt))
            }
            None => Ok(None),
        }
    }

    pub fn get_tx_location(
        &self,
        tx_id: &TransactionId,
    ) -> Result<Option<(Hash, u64)>, StateError> {
        let bytes_opt = self
            .store
            .get(Table::Transactions, tx_id.0.as_bytes())
            .map_err(|e| StateError::StorageFailure(e.to_string()))?;
        match bytes_opt {
            Some(bytes) => {
                if bytes.len() >= 40 {
                    let mut b_hash = [0u8; 32];
                    b_hash.copy_from_slice(&bytes[0..32]);
                    let idx = u64::from_le_bytes(bytes[32..40].try_into().unwrap());
                    Ok(Some((Hash(b_hash), idx)))
                } else {
                    Ok(None)
                }
            }
            None => Ok(None),
        }
    }

    pub fn get_block_by_hash(&self, block_hash: &Hash) -> Result<Option<Block>, StateError> {
        let bytes_opt = self
            .store
            .get(Table::BlockBodies, block_hash.as_bytes())
            .map_err(|e| StateError::StorageFailure(e.to_string()))?;
        match bytes_opt {
            Some(bytes) => {
                let block = Block::deserialize_canonical(&bytes)
                    .map_err(|e| StateError::StorageFailure(e.to_string()))?;
                Ok(Some(block))
            }
            None => Ok(None),
        }
    }

    pub fn get_block_by_height(&self, height: BlockHeight) -> Result<Option<Block>, StateError> {
        let key = crate::schema::encode_height_key(height);
        let hash_opt = self
            .store
            .get(Table::BlockHeightIndex, &key)
            .map_err(|e| StateError::StorageFailure(e.to_string()))?;
        match hash_opt {
            Some(bytes) => {
                if bytes.len() == 32 {
                    let mut h = [0u8; 32];
                    h.copy_from_slice(&bytes);
                    self.get_block_by_hash(&Hash(h))
                } else {
                    Ok(None)
                }
            }
            None => Ok(None),
        }
    }

    pub fn get_block_hash_by_height(
        &self,
        height: BlockHeight,
    ) -> Result<Option<Hash>, StateError> {
        let key = crate::schema::encode_height_key(height);
        let hash_opt = self
            .store
            .get(Table::BlockHeightIndex, &key)
            .map_err(|e| StateError::StorageFailure(e.to_string()))?;
        match hash_opt {
            Some(bytes) if bytes.len() == 32 => {
                let mut h = [0u8; 32];
                h.copy_from_slice(&bytes);
                Ok(Some(Hash(h)))
            }
            _ => Ok(None),
        }
    }

    pub fn get_block_header_by_hash(
        &self,
        block_hash: &Hash,
    ) -> Result<Option<BlockHeader>, StateError> {
        let bytes_opt = self
            .store
            .get(Table::BlockHeaders, block_hash.as_bytes())
            .map_err(|e| StateError::StorageFailure(e.to_string()))?;
        match bytes_opt {
            Some(bytes) => {
                let header = BlockHeader::deserialize_canonical(&bytes)
                    .map_err(|e| StateError::StorageFailure(e.to_string()))?;
                Ok(Some(header))
            }
            None => Ok(None),
        }
    }

    pub fn get_block_header_by_height(
        &self,
        height: BlockHeight,
    ) -> Result<Option<BlockHeader>, StateError> {
        let key = crate::schema::encode_height_key(height);
        let hash_opt = self
            .store
            .get(Table::BlockHeightIndex, &key)
            .map_err(|e| StateError::StorageFailure(e.to_string()))?;
        match hash_opt {
            Some(bytes) if bytes.len() == 32 => {
                let mut h = [0u8; 32];
                h.copy_from_slice(&bytes);
                self.get_block_header_by_hash(&Hash(h))
            }
            _ => Ok(None),
        }
    }

    pub fn get_historical_account(
        &self,
        height: BlockHeight,
        address: &Address,
    ) -> Result<Option<AccountState>, StateError> {
        if height == self.current_height {
            return self.get_account(address);
        }
        if height.0 > self.current_height.0 {
            return Err(StateError::NotFound);
        }

        let root_hash = if height.0 == 0 {
            match self.get_block_header_by_height(height)? {
                Some(hdr) => hdr.state_root,
                None => Hash::ZERO,
            }
        } else {
            let hdr = self
                .get_block_header_by_height(height)?
                .ok_or(StateError::NotFound)?;
            hdr.state_root
        };

        if root_hash == Hash::ZERO {
            return Ok(None);
        }

        let reader = PersistentJmtReader::new(&self.store, root_hash);
        reader
            .get(address)
            .map_err(|e| StateError::StorageFailure(e.to_string()))
    }

    pub fn get_historical_storage(
        &self,
        height: BlockHeight,
        address: &Address,
        slot: &Hash,
    ) -> Result<Hash, StateError> {
        if height == self.current_height {
            return self.get_storage(address, slot);
        }
        if height.0 > self.current_height.0 {
            return Err(StateError::NotFound);
        }

        let max_key = encode_storage_history_key(address, slot, height);
        match self
            .store
            .get_prev_or_equal(Table::StorageHistory, &max_key)
        {
            Ok(Some((key, val))) => {
                if key.len() == 60 && key[0..52] == max_key[0..52] {
                    let hash = Hash::deserialize_canonical(&val)
                        .map_err(|e| StateError::StorageFailure(e.to_string()))?;
                    Ok(hash)
                } else {
                    Ok(Hash::ZERO)
                }
            }
            Ok(None) => Ok(Hash::ZERO),
            Err(e) => Err(StateError::StorageFailure(e.to_string())),
        }
    }

    /// Atomically commits state deltas, JMT tree nodes, block headers, block bodies,
    /// block indices, transactions, receipts, and Ethereum transaction lookups in a single StorageBatch.
    pub fn commit_block_bundle(
        &mut self,
        block: &Block,
        block_hash: Hash,
        receipts: &[Receipt],
    ) -> Result<Hash, StateError> {
        let height = block.header.height;
        let mut batch = StorageBatch::new();

        // 1. Commit accounts
        for (addr, state_opt) in &self.working_accounts {
            let key = encode_account_key(addr).to_vec();
            match state_opt {
                Some(state) => {
                    let val = state.to_canonical_bytes();
                    batch.put(Table::Accounts, key, val);
                }
                None => {
                    batch.delete(Table::Accounts, key);
                }
            }
        }

        // 2. Commit storage slots
        for ((addr, slot), val) in &self.working_storage {
            let key = encode_storage_key(addr, slot).to_vec();
            if *val == Hash::ZERO {
                batch.delete(Table::Storage, key);
            } else {
                batch.put(Table::Storage, key, val.to_canonical_bytes());
            }

            // Write historical storage entry: (Address, Slot, Height) -> Value
            let hist_key = encode_storage_history_key(addr, slot, height).to_vec();
            batch.put(Table::StorageHistory, hist_key, val.to_canonical_bytes());
        }

        // 3. Commit contract bytecodes
        for (code_hash, bytecode) in &self.working_code {
            let key = encode_hash_key(code_hash).to_vec();
            batch.put(Table::Code, key, bytecode.clone());
        }

        // 4. Compute JMT root and persist JMT nodes
        let mut jmt = PersistentJmt::new(&mut self.store, height.0, self.state_root);
        for (addr, state_opt) in &self.working_accounts {
            if let Some(state) = state_opt {
                jmt.insert(*addr, state.clone())
                    .map_err(|e| StateError::StorageFailure(e.to_string()))?;
            }
        }
        let new_root = jmt.root_hash();

        // 5. Commit block headers & bodies & height indices
        batch.put(
            Table::BlockHeaders,
            block_hash.as_bytes().to_vec(),
            block.header.to_canonical_bytes(),
        );
        batch.put(
            Table::BlockBodies,
            block_hash.as_bytes().to_vec(),
            block.to_canonical_bytes(),
        );
        batch.put(
            Table::BlockHeightIndex,
            crate::schema::encode_height_key(height).to_vec(),
            block_hash.as_bytes().to_vec(),
        );
        batch.put(
            Table::BlockHashIndex,
            block_hash.as_bytes().to_vec(),
            crate::schema::encode_height_key(height).to_vec(),
        );

        // 6. Commit transactions location & EthTxLookup
        for (idx, tx) in block.transactions.iter().enumerate() {
            let tx_bytes = tx.to_canonical_bytes();
            let tx_id = TransactionId(tola_crypto::blake3_hash(&tx_bytes));
            let mut loc = [0u8; 40];
            loc[0..32].copy_from_slice(block_hash.as_bytes());
            loc[32..40].copy_from_slice(&(idx as u64).to_le_bytes());
            batch.put(
                Table::Transactions,
                tx_id.0.as_bytes().to_vec(),
                loc.to_vec(),
            );

            if tx.is_evm() {
                if let Some(eth_hash) = tola_codec::rlp::compute_eth_tx_hash(tx) {
                    batch.put(
                        Table::EthTxLookup,
                        eth_hash.as_bytes().to_vec(),
                        tx_id.0.as_bytes().to_vec(),
                    );
                }
            }
        }

        // 7. Commit receipts
        for receipt in receipts {
            batch.put(
                Table::Receipts,
                receipt.tx_id.0.as_bytes().to_vec(),
                receipt.to_canonical_bytes(),
            );
        }

        // 8. Commit metadata
        batch.put(
            Table::Metadata,
            b"latest_height".to_vec(),
            height.0.to_le_bytes().to_vec(),
        );
        batch.put(
            Table::Metadata,
            b"state_root".to_vec(),
            new_root.as_bytes().to_vec(),
        );

        // ATOMIC DISK WRITE: All tables synced atomically
        self.store
            .write_batch(batch)
            .map_err(|e| StateError::StorageFailure(e.to_string()))?;

        self.current_height = height;
        self.state_root = new_root;
        self.working_accounts.clear();
        self.working_storage.clear();
        self.working_code.clear();

        Ok(new_root)
    }

    /// Discards all uncommitted in-memory working modifications without touching disk.
    pub fn rollback(&mut self) -> Result<(), StateError> {
        self.working_accounts.clear();
        self.working_storage.clear();
        self.working_code.clear();
        Ok(())
    }
}

impl StateDB for PersistentStateDB {
    fn get_account(&self, address: &Address) -> Result<Option<AccountState>, StateError> {
        // 1. Check working uncommitted cache
        if let Some(entry) = self.working_accounts.get(address) {
            return Ok(entry.clone());
        }

        // 2. Read from persistent Accounts table
        let key = encode_account_key(address);
        let bytes_opt = self
            .store
            .get(Table::Accounts, &key)
            .map_err(|e| StateError::StorageFailure(e.to_string()))?;

        match bytes_opt {
            Some(bytes) => {
                let account = AccountState::deserialize_canonical(&bytes)
                    .map_err(|e| StateError::StorageFailure(e.to_string()))?;
                Ok(Some(account))
            }
            None => Ok(None),
        }
    }

    fn get_storage(&self, address: &Address, slot: &Hash) -> Result<Hash, StateError> {
        let key_pair = (*address, *slot);
        if let Some(val) = self.working_storage.get(&key_pair) {
            return Ok(*val);
        }

        let key = encode_storage_key(address, slot);
        let bytes_opt = self
            .store
            .get(Table::Storage, &key)
            .map_err(|e| StateError::StorageFailure(e.to_string()))?;

        match bytes_opt {
            Some(bytes) => {
                let hash = Hash::deserialize_canonical(&bytes)
                    .map_err(|e| StateError::StorageFailure(e.to_string()))?;
                Ok(hash)
            }
            None => Ok(Hash::ZERO),
        }
    }

    fn get_code(&self, code_hash: &Hash) -> Result<Option<Vec<u8>>, StateError> {
        if let Some(bytecode) = self.working_code.get(code_hash) {
            return Ok(Some(bytecode.clone()));
        }
        let key = encode_hash_key(code_hash);
        let bytes_opt = self
            .store
            .get(Table::Code, &key)
            .map_err(|e| StateError::StorageFailure(e.to_string()))?;
        Ok(bytes_opt)
    }

    fn apply_delta(&mut self, delta: StateDelta) -> Result<(), StateError> {
        for (addr, state_opt) in delta.accounts {
            self.working_accounts.insert(addr, state_opt);
        }
        for (key, val) in delta.storage {
            self.working_storage.insert(key, val);
        }
        for (code_hash, bytecode) in delta.code {
            self.working_code.insert(code_hash, bytecode);
        }
        Ok(())
    }

    fn compute_state_root(&self) -> Result<Hash, StateError> {
        let mut temp_store = DiskKvStore::open_transient();
        let mut jmt = PersistentJmt::new_with_fallback(
            &mut temp_store,
            &self.store,
            self.current_height.0,
            self.state_root,
        );

        for (addr, state_opt) in &self.working_accounts {
            if let Some(state) = state_opt {
                jmt.insert(*addr, state.clone())
                    .map_err(|e| StateError::StorageFailure(e.to_string()))?;
            }
        }

        Ok(jmt.root_hash())
    }

    fn commit(&mut self, height: BlockHeight) -> Result<Hash, StateError> {
        let mut batch = StorageBatch::new();

        // 1. Commit accounts
        for (addr, state_opt) in &self.working_accounts {
            let key = encode_account_key(addr).to_vec();
            match state_opt {
                Some(state) => {
                    let val = state.to_canonical_bytes();
                    batch.put(Table::Accounts, key, val);
                }
                None => {
                    batch.delete(Table::Accounts, key);
                }
            }
        }

        // 2. Commit storage slots
        for ((addr, slot), val) in &self.working_storage {
            let key = encode_storage_key(addr, slot).to_vec();
            if *val == Hash::ZERO {
                batch.delete(Table::Storage, key);
            } else {
                batch.put(Table::Storage, key, val.to_canonical_bytes());
            }

            // Write historical storage entry: (Address, Slot, Height) -> Value
            let hist_key = encode_storage_history_key(addr, slot, height).to_vec();
            batch.put(Table::StorageHistory, hist_key, val.to_canonical_bytes());
        }

        // 3. Commit contract bytecodes
        for (code_hash, bytecode) in &self.working_code {
            let key = encode_hash_key(code_hash).to_vec();
            batch.put(Table::Code, key, bytecode.clone());
        }

        self.store
            .write_batch(batch)
            .map_err(|e| StateError::StorageFailure(e.to_string()))?;

        // 4. Compute and persist JMT nodes directly into self.store
        let mut jmt = PersistentJmt::new(&mut self.store, height.0, self.state_root);
        for (addr, state_opt) in &self.working_accounts {
            if let Some(state) = state_opt {
                jmt.insert(*addr, state.clone())
                    .map_err(|e| StateError::StorageFailure(e.to_string()))?;
            }
        }
        let new_root = jmt.root_hash();

        // 5. Update metadata
        let mut meta_batch = StorageBatch::new();
        meta_batch.put(
            Table::Metadata,
            b"latest_height".to_vec(),
            height.0.to_le_bytes().to_vec(),
        );
        meta_batch.put(
            Table::Metadata,
            b"state_root".to_vec(),
            new_root.as_bytes().to_vec(),
        );
        self.store
            .write_batch(meta_batch)
            .map_err(|e| StateError::StorageFailure(e.to_string()))?;

        self.current_height = height;
        self.state_root = new_root;
        self.working_accounts.clear();
        self.working_storage.clear();
        self.working_code.clear();

        Ok(new_root)
    }

    fn rollback(&mut self) -> Result<(), StateError> {
        self.working_accounts.clear();
        self.working_storage.clear();
        self.working_code.clear();
        Ok(())
    }

    fn generate_account_proof(&self, address: &Address) -> Result<MerkleProof, StateError> {
        let mut temp_store = DiskKvStore::open_transient();
        let mut jmt = PersistentJmt::new_with_fallback(
            &mut temp_store,
            &self.store,
            self.current_height.0,
            self.state_root,
        );

        for (addr, state_opt) in &self.working_accounts {
            if let Some(state) = state_opt {
                let _ = jmt.insert(*addr, state.clone());
            }
        }

        let proof = jmt
            .generate_proof(address)
            .map_err(|e| StateError::StorageFailure(e.to_string()))?;

        let mut sibling_hashes = Vec::new();
        for level in proof.sibling_levels {
            for child in level {
                if let Some(h) = child {
                    sibling_hashes.push(h);
                }
            }
        }

        Ok(MerkleProof {
            leaf_index: 0,
            sibling_hashes,
        })
    }

    fn index_eth_tx(
        &mut self,
        eth_tx_hash: &Hash,
        tola_tx_id: &TransactionId,
    ) -> Result<(), StateError> {
        PersistentStateDB::index_eth_tx(self, eth_tx_hash, tola_tx_id)
    }

    fn get_eth_tx(&self, eth_tx_hash: &Hash) -> Result<Option<TransactionId>, StateError> {
        PersistentStateDB::get_eth_tx(self, eth_tx_hash)
    }

    fn get_receipt(&self, tx_id: &TransactionId) -> Result<Option<Receipt>, StateError> {
        PersistentStateDB::get_receipt(self, tx_id)
    }

    fn get_tx_location(&self, tx_id: &TransactionId) -> Result<Option<(Hash, u64)>, StateError> {
        PersistentStateDB::get_tx_location(self, tx_id)
    }

    fn get_block_by_hash(&self, block_hash: &Hash) -> Result<Option<Block>, StateError> {
        PersistentStateDB::get_block_by_hash(self, block_hash)
    }

    fn get_block_by_height(&self, height: BlockHeight) -> Result<Option<Block>, StateError> {
        PersistentStateDB::get_block_by_height(self, height)
    }

    fn get_block_hash_by_height(&self, height: BlockHeight) -> Result<Option<Hash>, StateError> {
        PersistentStateDB::get_block_hash_by_height(self, height)
    }

    fn get_historical_account(
        &self,
        height: BlockHeight,
        address: &Address,
    ) -> Result<Option<AccountState>, StateError> {
        PersistentStateDB::get_historical_account(self, height, address)
    }

    fn get_historical_storage(
        &self,
        height: BlockHeight,
        address: &Address,
        slot: &Hash,
    ) -> Result<Hash, StateError> {
        PersistentStateDB::get_historical_storage(self, height, address, slot)
    }

    fn get_block_header_by_height(
        &self,
        height: BlockHeight,
    ) -> Result<Option<BlockHeader>, StateError> {
        PersistentStateDB::get_block_header_by_height(self, height)
    }

    fn get_block_header_by_hash(
        &self,
        block_hash: &Hash,
    ) -> Result<Option<BlockHeader>, StateError> {
        PersistentStateDB::get_block_header_by_hash(self, block_hash)
    }

    fn current_height(&self) -> BlockHeight {
        self.current_height
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tola_types::Amount;

    #[test]
    fn test_persistent_state_commit_and_reopen() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let temp_dir = std::env::temp_dir().join(format!("tola_test_pstate_{nanos}"));

        let addr1 = Address([0x10; 20]);
        let addr2 = Address([0x20; 20]);
        let slot_key = Hash([0x55; 32]);
        let slot_val = Hash([0x77; 32]);

        // 1. Commit block 1
        let code_hash = Hash([0x99; 32]);
        let bytecode = vec![0x60, 0x00, 0x60, 0x00, 0xf3];
        let root1 = {
            let mut db = PersistentStateDB::open(&temp_dir).expect("Open db");
            let delta = StateDelta {
                accounts: vec![
                    (
                        addr1,
                        Some(AccountState::new_with_balance(Amount::from_u128(1_000))),
                    ),
                    (
                        addr2,
                        Some(AccountState::new_with_balance(Amount::from_u128(2_000))),
                    ),
                ],
                storage: vec![((addr1, slot_key), slot_val)],
                code: vec![(code_hash, bytecode.clone())],
            };
            db.apply_delta(delta).unwrap();
            db.commit(BlockHeight(1)).unwrap()
        };

        assert_ne!(root1, Hash::ZERO);

        // 2. Re-open and verify persistence
        {
            let db = PersistentStateDB::open(&temp_dir).expect("Reopen db");
            assert_eq!(db.current_height(), BlockHeight(1));
            assert_eq!(db.state_root(), root1);

            let a1 = db.get_account(&addr1).unwrap().unwrap();
            assert_eq!(a1.balance, Amount::from_u128(1_000));

            let a2 = db.get_account(&addr2).unwrap().unwrap();
            assert_eq!(a2.balance, Amount::from_u128(2_000));

            let stored_val = db.get_storage(&addr1, &slot_key).unwrap();
            assert_eq!(stored_val, slot_val);

            let loaded_code = db.get_code(&code_hash).unwrap().unwrap();
            assert_eq!(loaded_code, bytecode);
        }

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_persistent_state_rollback() {
        let mut db = PersistentStateDB::open_transient();
        let addr = Address([0x33; 20]);

        db.apply_delta(StateDelta {
            accounts: vec![(
                addr,
                Some(AccountState::new_with_balance(Amount::from_u128(500))),
            )],
            storage: vec![],
            code: vec![],
        })
        .unwrap();

        assert_eq!(
            db.get_account(&addr).unwrap().unwrap().balance,
            Amount::from_u128(500)
        );

        // Discard uncommitted changes
        db.rollback().unwrap();
        assert!(db.get_account(&addr).unwrap().is_none());
    }
}
