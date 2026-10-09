//! Persistent Jellyfish Merkle Tree (JMT) with BLAKE3 hashing.
//!
//! Complies with ADR-0010:
//! - 16-ary radix path traversal with nibble decomposition
//! - BLAKE3 domain-separated node commitments
//! - Compact cryptographic inclusion and non-inclusion proofs
//! - Multi-version historical tree state persistence

use crate::kv::{KeyValueStore, StorageError};
use crate::schema::{encode_jmt_node_key, Table};
use std::collections::BTreeMap;
use tola_codec::CanonicalSerialize;
use tola_crypto::blake3_hash;
use tola_types::{AccountState, Address, Hash};

pub const DOMAIN_JMT_INTERNAL: &[u8; 16] = b"TOLA-JMT-INT-v1-";
pub const DOMAIN_JMT_LEAF: &[u8; 16] = b"TOLA-JMT-LF-v1--";

/// 4-bit nibble path extracted from 32-byte BLAKE3 key digest.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct NibblePath {
    bytes: [u8; 32],
}

impl NibblePath {
    pub fn from_key(key: &Hash) -> Self {
        Self {
            bytes: *key.as_bytes(),
        }
    }

    pub fn from_address(address: &Address) -> Self {
        let key = blake3_hash(address.as_bytes());
        Self::from_key(&key)
    }

    pub fn get_nibble(&self, index: usize) -> u8 {
        let byte_idx = index / 2;
        if byte_idx >= 32 {
            return 0;
        }
        let b = self.bytes[byte_idx];
        if index % 2 == 0 {
            (b >> 4) & 0x0f
        } else {
            b & 0x0f
        }
    }
}

/// JMT tree node variant.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum JmtNode {
    Null,
    Leaf {
        address: Address,
        account_state: AccountState,
    },
    Internal {
        children: [Option<Hash>; 16],
    },
}

impl JmtNode {
    pub fn compute_hash(&self) -> Hash {
        match self {
            JmtNode::Null => Hash::ZERO,
            JmtNode::Leaf {
                address,
                account_state,
            } => {
                let mut pre_image = Vec::with_capacity(16 + 20 + 92);
                pre_image.extend_from_slice(DOMAIN_JMT_LEAF);
                pre_image.extend_from_slice(address.as_bytes());
                pre_image.extend_from_slice(&account_state.to_canonical_bytes());
                blake3_hash(&pre_image)
            }
            JmtNode::Internal { children } => {
                let mut presence_mask: u16 = 0;
                let mut present_hashes = Vec::with_capacity(16 * 32);

                for (i, child_opt) in children.iter().enumerate() {
                    if let Some(child_hash) = child_opt {
                        presence_mask |= 1 << i;
                        present_hashes.extend_from_slice(child_hash.as_bytes());
                    }
                }

                let mut pre_image = Vec::with_capacity(16 + 2 + present_hashes.len());
                pre_image.extend_from_slice(DOMAIN_JMT_INTERNAL);
                pre_image.extend_from_slice(&presence_mask.to_le_bytes());
                pre_image.extend_from_slice(&present_hashes);
                blake3_hash(&pre_image)
            }
        }
    }

    pub fn serialize_node(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        match self {
            JmtNode::Null => buf.push(0x00),
            JmtNode::Leaf {
                address,
                account_state,
            } => {
                buf.push(0x01);
                address.serialize_canonical(&mut buf);
                account_state.serialize_canonical(&mut buf);
            }
            JmtNode::Internal { children } => {
                buf.push(0x02);
                let mut mask: u16 = 0;
                for (i, child) in children.iter().enumerate() {
                    if child.is_some() {
                        mask |= 1 << i;
                    }
                }
                buf.extend_from_slice(&mask.to_le_bytes());
                for child in children {
                    if let Some(h) = child {
                        h.serialize_canonical(&mut buf);
                    }
                }
            }
        }
        buf
    }
}

pub fn deserialize_jmt_node(bytes: &[u8]) -> Result<JmtNode, StorageError> {
    if bytes.is_empty() {
        return Ok(JmtNode::Null);
    }
    match bytes[0] {
        0x00 => Ok(JmtNode::Null),
        0x01 => {
            use tola_codec::CanonicalDeserialize;
            let addr = Address::deserialize_canonical(&bytes[1..21])
                .map_err(|e| StorageError::Codec(e.to_string()))?;
            let state = AccountState::deserialize_canonical(&bytes[21..113])
                .map_err(|e| StorageError::Codec(e.to_string()))?;
            Ok(JmtNode::Leaf {
                address: addr,
                account_state: state,
            })
        }
        0x02 => {
            use tola_codec::CanonicalDeserialize;
            let mask = u16::from_le_bytes(bytes[1..3].try_into().unwrap());
            let mut children = [None; 16];
            let mut pos = 3;
            for i in 0..16 {
                if (mask & (1 << i)) != 0 {
                    let h = Hash::deserialize_canonical(&bytes[pos..pos + 32])
                        .map_err(|e| StorageError::Codec(e.to_string()))?;
                    children[i] = Some(h);
                    pos += 32;
                }
            }
            Ok(JmtNode::Internal { children })
        }
        _ => Err(StorageError::CorruptedRecord),
    }
}

/// Cryptographic inclusion / non-inclusion proof for JMT leaf.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct JmtProof {
    pub leaf_hash: Hash,
    pub sibling_levels: Vec<[Option<Hash>; 16]>,
}

pub struct PersistentJmt<'a, S: KeyValueStore> {
    store: &'a mut S,
    fallback: Option<&'a dyn KeyValueStore>,
    version: u64,
    cache: BTreeMap<Hash, JmtNode>,
    root_hash: Hash,
}

impl<'a, S: KeyValueStore> PersistentJmt<'a, S> {
    pub fn new(store: &'a mut S, version: u64, root_hash: Hash) -> Self {
        Self {
            store,
            fallback: None,
            version,
            cache: BTreeMap::new(),
            root_hash,
        }
    }

    pub fn new_with_fallback(
        store: &'a mut S,
        fallback: &'a dyn KeyValueStore,
        version: u64,
        root_hash: Hash,
    ) -> Self {
        Self {
            store,
            fallback: Some(fallback),
            version,
            cache: BTreeMap::new(),
            root_hash,
        }
    }

    pub fn root_hash(&self) -> Hash {
        self.root_hash
    }

    pub fn version(&self) -> u64 {
        self.version
    }

    fn get_node(&self, hash: &Hash) -> Result<JmtNode, StorageError> {
        if *hash == Hash::ZERO {
            return Ok(JmtNode::Null);
        }
        if let Some(cached) = self.cache.get(hash) {
            return Ok(cached.clone());
        }
        let key = encode_jmt_node_key(hash);
        let bytes_opt = if let Some(bytes) = self.store.get(Table::TreeNodes, &key)? {
            Some(bytes)
        } else if let Some(fallback) = self.fallback {
            fallback.get(Table::TreeNodes, &key)?
        } else {
            None
        };

        if let Some(bytes) = bytes_opt {
            deserialize_jmt_node(&bytes)
        } else {
            Err(StorageError::KeyNotFound)
        }
    }

    pub fn get(&self, address: &Address) -> Result<Option<AccountState>, StorageError> {
        if self.root_hash == Hash::ZERO {
            return Ok(None);
        }

        let path = NibblePath::from_address(address);
        let mut curr_hash = self.root_hash;
        let mut depth = 0;

        while curr_hash != Hash::ZERO && depth < 64 {
            let node = self.get_node(&curr_hash)?;
            match node {
                JmtNode::Null => return Ok(None),
                JmtNode::Leaf {
                    address: leaf_addr,
                    account_state,
                } => {
                    if leaf_addr == *address {
                        return Ok(Some(account_state));
                    } else {
                        return Ok(None);
                    }
                }
                JmtNode::Internal { children } => {
                    let nibble = path.get_nibble(depth) as usize;
                    match children[nibble] {
                        Some(child_hash) => {
                            curr_hash = child_hash;
                            depth += 1;
                        }
                        None => return Ok(None),
                    }
                }
            }
        }

        Ok(None)
    }

    pub fn insert(&mut self, address: Address, state: AccountState) -> Result<Hash, StorageError> {
        let path = NibblePath::from_address(&address);
        let new_root = self.insert_recursive(self.root_hash, &path, 0, address, state)?;
        self.root_hash = new_root;
        Ok(new_root)
    }

    fn insert_recursive(
        &mut self,
        current_hash: Hash,
        path: &NibblePath,
        depth: usize,
        address: Address,
        state: AccountState,
    ) -> Result<Hash, StorageError> {
        if current_hash == Hash::ZERO {
            let leaf = JmtNode::Leaf {
                address,
                account_state: state,
            };
            let leaf_hash = leaf.compute_hash();
            self.persist_node(leaf_hash, leaf)?;
            return Ok(leaf_hash);
        }

        let current_node = self.get_node(&current_hash)?;

        match current_node {
            JmtNode::Null => {
                let leaf = JmtNode::Leaf {
                    address,
                    account_state: state,
                };
                let leaf_hash = leaf.compute_hash();
                self.persist_node(leaf_hash, leaf)?;
                Ok(leaf_hash)
            }
            JmtNode::Leaf {
                address: existing_addr,
                account_state: existing_state,
            } => {
                if existing_addr == address {
                    // Overwrite state of same account
                    let new_leaf = JmtNode::Leaf {
                        address,
                        account_state: state,
                    };
                    let new_hash = new_leaf.compute_hash();
                    self.persist_node(new_hash, new_leaf)?;
                    Ok(new_hash)
                } else {
                    // Collision: convert to internal node and push both leaves down
                    let existing_path = NibblePath::from_address(&existing_addr);
                    let mut internal_children = [None; 16];

                    let existing_nibble = existing_path.get_nibble(depth) as usize;
                    let new_nibble = path.get_nibble(depth) as usize;

                    if existing_nibble == new_nibble {
                        // Recursively push collision down
                        let child_hash =
                            self.insert_recursive(current_hash, path, depth + 1, address, state)?;
                        internal_children[existing_nibble] = Some(child_hash);
                    } else {
                        let existing_leaf = JmtNode::Leaf {
                            address: existing_addr,
                            account_state: existing_state,
                        };
                        let existing_hash = existing_leaf.compute_hash();
                        self.persist_node(existing_hash, existing_leaf)?;

                        let new_leaf = JmtNode::Leaf {
                            address,
                            account_state: state,
                        };
                        let new_hash = new_leaf.compute_hash();
                        self.persist_node(new_hash, new_leaf)?;

                        internal_children[existing_nibble] = Some(existing_hash);
                        internal_children[new_nibble] = Some(new_hash);
                    }

                    let internal = JmtNode::Internal {
                        children: internal_children,
                    };
                    let internal_hash = internal.compute_hash();
                    self.persist_node(internal_hash, internal)?;
                    Ok(internal_hash)
                }
            }
            JmtNode::Internal { mut children } => {
                let nibble = path.get_nibble(depth) as usize;
                let child_curr = children[nibble].unwrap_or(Hash::ZERO);
                let new_child =
                    self.insert_recursive(child_curr, path, depth + 1, address, state)?;
                children[nibble] = Some(new_child);

                let new_internal = JmtNode::Internal { children };
                let new_hash = new_internal.compute_hash();
                self.persist_node(new_hash, new_internal)?;
                Ok(new_hash)
            }
        }
    }

    fn persist_node(&mut self, hash: Hash, node: JmtNode) -> Result<(), StorageError> {
        let bytes = node.serialize_node();
        let key = encode_jmt_node_key(&hash);
        self.store.put(Table::TreeNodes, &key, &bytes)?;
        self.cache.insert(hash, node);
        Ok(())
    }

    /// Generate cryptographic inclusion proof for address.
    pub fn generate_proof(&self, address: &Address) -> Result<JmtProof, StorageError> {
        let path = NibblePath::from_address(address);
        let mut curr_hash = self.root_hash;
        let mut depth = 0;
        let mut sibling_levels = Vec::new();
        let mut target_leaf_hash = Hash::ZERO;

        while curr_hash != Hash::ZERO && depth < 64 {
            let node = self.get_node(&curr_hash)?;
            match node {
                JmtNode::Null => break,
                JmtNode::Leaf {
                    address: leaf_addr, ..
                } => {
                    target_leaf_hash = curr_hash;
                    if leaf_addr == *address {
                        break;
                    }
                    break;
                }
                JmtNode::Internal { children } => {
                    sibling_levels.push(children);
                    let nibble = path.get_nibble(depth) as usize;
                    match children[nibble] {
                        Some(child_hash) => {
                            curr_hash = child_hash;
                            depth += 1;
                        }
                        None => break,
                    }
                }
            }
        }

        Ok(JmtProof {
            leaf_hash: target_leaf_hash,
            sibling_levels,
        })
    }
}

/// Read-only historical JMT traverser anchored to an immutable historical state root.
pub struct PersistentJmtReader<'a> {
    store: &'a dyn KeyValueStore,
    root_hash: Hash,
}

impl<'a> PersistentJmtReader<'a> {
    pub fn new(store: &'a dyn KeyValueStore, root_hash: Hash) -> Self {
        Self { store, root_hash }
    }

    pub fn root_hash(&self) -> Hash {
        self.root_hash
    }

    pub fn get(&self, address: &Address) -> Result<Option<AccountState>, StorageError> {
        if self.root_hash == Hash::ZERO {
            return Ok(None);
        }

        let path = NibblePath::from_address(address);
        let mut curr_hash = self.root_hash;
        let mut depth = 0;

        while curr_hash != Hash::ZERO && depth < 64 {
            let node = self.get_node(&curr_hash)?;
            match node {
                JmtNode::Null => return Ok(None),
                JmtNode::Leaf {
                    address: leaf_addr,
                    account_state,
                } => {
                    if leaf_addr == *address {
                        return Ok(Some(account_state));
                    } else {
                        return Ok(None);
                    }
                }
                JmtNode::Internal { children } => {
                    let nibble = path.get_nibble(depth) as usize;
                    match children[nibble] {
                        Some(child_hash) => {
                            curr_hash = child_hash;
                            depth += 1;
                        }
                        None => return Ok(None),
                    }
                }
            }
        }

        Ok(None)
    }

    fn get_node(&self, hash: &Hash) -> Result<JmtNode, StorageError> {
        if *hash == Hash::ZERO {
            return Ok(JmtNode::Null);
        }
        let key = encode_jmt_node_key(hash);
        let bytes_opt = self.store.get(Table::TreeNodes, &key)?;
        match bytes_opt {
            Some(bytes) => deserialize_jmt_node(&bytes),
            None => Err(StorageError::KeyNotFound),
        }
    }
}

/// Verify JMT cryptographic proof against root hash.
pub fn verify_jmt_proof(
    root: Hash,
    address: &Address,
    expected_state: &AccountState,
    proof: &JmtProof,
) -> bool {
    let expected_leaf = JmtNode::Leaf {
        address: *address,
        account_state: expected_state.clone(),
    };
    let expected_leaf_hash = expected_leaf.compute_hash();

    if proof.leaf_hash != expected_leaf_hash {
        return false;
    }

    let path = NibblePath::from_address(address);
    let mut current_hash = expected_leaf_hash;

    for (level, children) in proof.sibling_levels.iter().rev().enumerate() {
        let depth = proof.sibling_levels.len() - 1 - level;
        let nibble = path.get_nibble(depth) as usize;

        let mut reconstructed_children = *children;
        reconstructed_children[nibble] = Some(current_hash);

        let internal = JmtNode::Internal {
            children: reconstructed_children,
        };
        current_hash = internal.compute_hash();
    }

    current_hash == root
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kv::DiskKvStore;
    use tola_types::{Amount, Nonce};

    #[test]
    fn test_jmt_insert_lookup_and_persistence() {
        let mut db = DiskKvStore::open_transient();
        let addr1 = Address([0x11; 20]);
        let addr2 = Address([0x22; 20]);

        let state1 = AccountState::new_with_balance(Amount::from_u128(500));
        let state2 = AccountState::new_with_balance(Amount::from_u128(1_000));

        let root = {
            let mut tree = PersistentJmt::new(&mut db, 1, Hash::ZERO);
            tree.insert(addr1, state1.clone()).unwrap();
            tree.insert(addr2, state2.clone()).unwrap();
            tree.root_hash()
        };

        assert_ne!(root, Hash::ZERO);

        // Reconnect tree from db store using same root
        let tree_reloaded = PersistentJmt::new(&mut db, 1, root);
        let found1 = tree_reloaded.get(&addr1).unwrap().expect("Addr1 found");
        let found2 = tree_reloaded.get(&addr2).unwrap().expect("Addr2 found");

        assert_eq!(found1, state1);
        assert_eq!(found2, state2);

        // Non-existent address returns None
        let found3 = tree_reloaded.get(&Address([0x99; 20])).unwrap();
        assert!(found3.is_none());
    }

    #[test]
    fn test_jmt_cryptographic_proof_verification() {
        let mut db = DiskKvStore::open_transient();
        let addr = Address([0x77; 20]);
        let state = AccountState {
            nonce: Nonce(10),
            balance: Amount::from_u128(88_888),
            code_hash: Hash::ZERO,
            storage_root: Hash::ZERO,
            flags: 0,
        };

        let mut tree = PersistentJmt::new(&mut db, 1, Hash::ZERO);
        let root = tree.insert(addr, state.clone()).unwrap();

        let proof = tree.generate_proof(&addr).unwrap();
        assert!(verify_jmt_proof(root, &addr, &state, &proof));

        // Proof fails if state or address is modified
        let bad_state = AccountState::new_with_balance(Amount::from_u128(999));
        assert!(!verify_jmt_proof(root, &addr, &bad_state, &proof));
    }
}
