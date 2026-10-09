//! System Smart Contracts for TOLA Network.
//!
//! Provides deterministic, state-persistent execution for Tokenized External Assets (TLS-721 / ERC-721)
//! complying with:
//! - Complete decoupling doctrine (TOLA != External Platform)
//! - Multi-platform authorized verifier registry
//! - Anti-replay and duplicate mint protection
//! - Standard ERC-721 events and transfer semantics

use tola_core::StateDB;
use tola_crypto::{
    blake3_hash, compute_external_asset_attestation_digest, compute_external_asset_canonical_id,
    verify_external_asset_attestation,
};
use tola_types::*;

/// Canonical event topic hashes (Keccak-256)
pub const TOPIC_TRANSFER: Hash = Hash([
    0xdd, 0xf2, 0x52, 0xad, 0x1b, 0xe2, 0xc8, 0x9b, 0x69, 0xc2, 0xb0, 0x68, 0xfc, 0x37, 0x8d, 0xaa,
    0x95, 0x2b, 0xa7, 0xf1, 0x63, 0xc4, 0xa1, 0x16, 0x28, 0xf5, 0x5a, 0x4d, 0xf5, 0x23, 0xb3, 0xef,
]);

pub const TOPIC_APPROVAL: Hash = Hash([
    0x8c, 0x5b, 0xe1, 0xe5, 0xeb, 0xec, 0x7d, 0x5b, 0xd1, 0x4f, 0x71, 0x42, 0x7d, 0x1e, 0x84, 0xf3,
    0xdd, 0x03, 0x14, 0xc0, 0xf7, 0xb2, 0x29, 0x1e, 0x5b, 0x20, 0x0a, 0xc8, 0xc7, 0xc3, 0xb9, 0x25,
]);

pub const TOPIC_APPROVAL_FOR_ALL: Hash = Hash([
    0x17, 0x30, 0x7e, 0xab, 0x39, 0x29, 0x77, 0x07, 0x42, 0xf3, 0x9c, 0x02, 0x24, 0x62, 0x91, 0xca,
    0x58, 0x9f, 0x93, 0xe0, 0xb0, 0x9f, 0x45, 0xd4, 0xf7, 0x4f, 0x99, 0xf8, 0x47, 0xb9, 0x63, 0x5c,
]);

pub const TOPIC_EXTERNAL_ASSET_MINTED: Hash = Hash([
    0x9e, 0x41, 0xa3, 0x64, 0x12, 0x88, 0x34, 0x18, 0x01, 0x90, 0xb6, 0x0e, 0x82, 0xb1, 0x33, 0x22,
    0x54, 0x77, 0x11, 0x88, 0xa9, 0x22, 0x09, 0x33, 0x71, 0xaa, 0x55, 0xbb, 0x44, 0xcc, 0x11, 0x00,
]);

// ----------------------------------------------------------------------------
// Deterministic Storage Slot Calculators
// ----------------------------------------------------------------------------

pub fn slot_next_token_id() -> Hash {
    blake3_hash(b"TOLA_SLOT_NEXT_TOKEN_ID_V1")
}

pub fn slot_token_record(token_id: u64) -> Hash {
    let mut b = Vec::with_capacity(32);
    b.extend_from_slice(b"TOLA_SLOT_RECORD_V1:");
    b.extend_from_slice(&token_id.to_be_bytes());
    blake3_hash(&b)
}

pub fn slot_token_record_chunk(token_id: u64, chunk_idx: u32) -> Hash {
    let mut b = Vec::with_capacity(36);
    b.extend_from_slice(b"TOLA_SLOT_RECORD_CHUNK_V1:");
    b.extend_from_slice(&token_id.to_be_bytes());
    b.extend_from_slice(&chunk_idx.to_be_bytes());
    blake3_hash(&b)
}

pub fn slot_token_owner(token_id: u64) -> Hash {
    let mut b = Vec::with_capacity(32);
    b.extend_from_slice(b"TOLA_SLOT_OWNER_V1:");
    b.extend_from_slice(&token_id.to_be_bytes());
    blake3_hash(&b)
}

pub fn slot_token_approval(token_id: u64) -> Hash {
    let mut b = Vec::with_capacity(32);
    b.extend_from_slice(b"TOLA_SLOT_APPROVAL_V1:");
    b.extend_from_slice(&token_id.to_be_bytes());
    blake3_hash(&b)
}

pub fn slot_operator_approval(owner: &Address, operator: &Address) -> Hash {
    let mut b = Vec::with_capacity(60);
    b.extend_from_slice(b"TOLA_SLOT_OPERATOR_V1:");
    b.extend_from_slice(owner.as_bytes());
    b.extend_from_slice(operator.as_bytes());
    blake3_hash(&b)
}

pub fn slot_canonical_token(canonical_id: &[u8; 32]) -> Hash {
    let mut b = Vec::with_capacity(60);
    b.extend_from_slice(b"TOLA_SLOT_CANONICAL_V1:");
    b.extend_from_slice(canonical_id);
    blake3_hash(&b)
}

pub fn slot_used_attestation(digest: &[u8; 32]) -> Hash {
    let mut b = Vec::with_capacity(60);
    b.extend_from_slice(b"TOLA_SLOT_USED_DIGEST_V1:");
    b.extend_from_slice(digest);
    blake3_hash(&b)
}

pub fn slot_balance(owner: &Address) -> Hash {
    let mut b = Vec::with_capacity(50);
    b.extend_from_slice(b"TOLA_SLOT_BALANCE_V1:");
    b.extend_from_slice(owner.as_bytes());
    blake3_hash(&b)
}

// ----------------------------------------------------------------------------
// Execution Result
// ----------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContractExecutionResult {
    pub success: bool,
    pub gas_consumed: Gas,
    pub storage_deltas: Vec<((Address, Hash), Hash)>,
    pub logs: Vec<Log>,
    pub return_data: Vec<u8>,
}

// ----------------------------------------------------------------------------
// Action Types
// ----------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExternalAssetCall {
    MintWithAttestation {
        attestation: ExternalAssetAttestation,
        metadata_uri: String,
    },
    TransferFrom {
        from: Address,
        to: Address,
        token_id: u64,
    },
    SafeTransferFrom {
        from: Address,
        to: Address,
        token_id: u64,
        data: Vec<u8>,
    },
    Approve {
        to: Address,
        token_id: u64,
    },
    SetApprovalForAll {
        operator: Address,
        approved: bool,
    },
    Burn {
        token_id: u64,
    },
}

impl ExternalAssetCall {
    /// Action discriminant tags
    pub const TAG_MINT: u8 = 1;
    pub const TAG_TRANSFER_FROM: u8 = 2;
    pub const TAG_SAFE_TRANSFER_FROM: u8 = 3;
    pub const TAG_APPROVE: u8 = 4;
    pub const TAG_SET_APPROVAL_FOR_ALL: u8 = 5;
    pub const TAG_BURN: u8 = 6;

    /// Decode canonical call from transaction calldata
    pub fn decode(data: &[u8]) -> Result<Self, &'static str> {
        if data.is_empty() {
            return Err("Empty calldata");
        }

        // Support both 1-byte action tag and standard 4-byte ABI selector
        let tag = data[0];
        let payload = &data[1..];

        match tag {
            Self::TAG_MINT => Self::decode_mint(payload),
            Self::TAG_TRANSFER_FROM => Self::decode_transfer(payload),
            Self::TAG_SAFE_TRANSFER_FROM => Self::decode_safe_transfer(payload),
            Self::TAG_APPROVE => Self::decode_approve(payload),
            Self::TAG_SET_APPROVAL_FOR_ALL => Self::decode_approval_for_all(payload),
            Self::TAG_BURN => Self::decode_burn(payload),
            _ => {
                // Check standard 4-byte function selectors
                if data.len() >= 4 {
                    match &data[0..4] {
                        [0x23, 0xb8, 0x72, 0xdd] => Self::decode_transfer(&data[4..]),
                        [0x42, 0x84, 0x2e, 0x0e] => Self::decode_safe_transfer(&data[4..]),
                        [0x09, 0x5e, 0xa7, 0xb3] => Self::decode_approve(&data[4..]),
                        [0xa2, 0x2c, 0xb4, 0x65] => Self::decode_approval_for_all(&data[4..]),
                        [0x42, 0x96, 0x6c, 0x68] => Self::decode_burn(&data[4..]),
                        _ => Err("Unknown function selector or action tag"),
                    }
                } else {
                    Err("Invalid action tag")
                }
            }
        }
    }

    /// Encode canonical call to binary bytes
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        match self {
            Self::MintWithAttestation {
                attestation,
                metadata_uri,
            } => {
                buf.push(Self::TAG_MINT);
                buf.extend_from_slice(&attestation.chain_id.to_be_bytes());
                buf.extend_from_slice(&attestation.nonce.to_be_bytes());
                buf.extend_from_slice(attestation.destination_wallet.as_bytes());
                buf.extend_from_slice(&attestation.expires_at.to_be_bytes());
                buf.extend_from_slice(&(attestation.source_platform.len() as u32).to_be_bytes());
                buf.extend_from_slice(attestation.source_platform.as_bytes());
                buf.push(attestation.asset_type.to_u8());
                buf.extend_from_slice(&(attestation.external_asset_id.len() as u32).to_be_bytes());
                buf.extend_from_slice(attestation.external_asset_id.as_bytes());
                buf.extend_from_slice(attestation.verifier_address.as_bytes());
                buf.extend_from_slice(&attestation.signature);
                buf.extend_from_slice(&(metadata_uri.len() as u32).to_be_bytes());
                buf.extend_from_slice(metadata_uri.as_bytes());
            }
            Self::TransferFrom { from, to, token_id } => {
                buf.push(Self::TAG_TRANSFER_FROM);
                buf.extend_from_slice(from.as_bytes());
                buf.extend_from_slice(to.as_bytes());
                buf.extend_from_slice(&token_id.to_be_bytes());
            }
            Self::SafeTransferFrom {
                from,
                to,
                token_id,
                data,
            } => {
                buf.push(Self::TAG_SAFE_TRANSFER_FROM);
                buf.extend_from_slice(from.as_bytes());
                buf.extend_from_slice(to.as_bytes());
                buf.extend_from_slice(&token_id.to_be_bytes());
                buf.extend_from_slice(&(data.len() as u32).to_be_bytes());
                buf.extend_from_slice(data);
            }
            Self::Approve { to, token_id } => {
                buf.push(Self::TAG_APPROVE);
                buf.extend_from_slice(to.as_bytes());
                buf.extend_from_slice(&token_id.to_be_bytes());
            }
            Self::SetApprovalForAll { operator, approved } => {
                buf.push(Self::TAG_SET_APPROVAL_FOR_ALL);
                buf.extend_from_slice(operator.as_bytes());
                buf.push(if *approved { 1 } else { 0 });
            }
            Self::Burn { token_id } => {
                buf.push(Self::TAG_BURN);
                buf.extend_from_slice(&token_id.to_be_bytes());
            }
        }
        buf
    }

    fn decode_mint(p: &[u8]) -> Result<Self, &'static str> {
        let mut offset = 0;
        if p.len() < 8 + 8 + 20 + 8 + 4 {
            return Err("Mint payload too short");
        }

        let chain_id = u64::from_be_bytes(p[offset..offset + 8].try_into().unwrap());
        offset += 8;
        let nonce = u64::from_be_bytes(p[offset..offset + 8].try_into().unwrap());
        offset += 8;
        let dest = Address(p[offset..offset + 20].try_into().unwrap());
        offset += 20;
        let expires_at = u64::from_be_bytes(p[offset..offset + 8].try_into().unwrap());
        offset += 8;

        let src_len = u32::from_be_bytes(p[offset..offset + 4].try_into().unwrap()) as usize;
        offset += 4;
        if offset + src_len > p.len() {
            return Err("Invalid source_platform length");
        }
        let source_platform =
            String::from_utf8(p[offset..offset + src_len].to_vec()).map_err(|_| "Invalid UTF-8")?;
        offset += src_len;

        if offset >= p.len() {
            return Err("Missing asset_type");
        }
        let asset_type =
            ExternalAssetType::from_u8(p[offset]).ok_or("Invalid asset_type integer")?;
        offset += 1;

        if offset + 4 > p.len() {
            return Err("Missing external_asset_id length");
        }
        let id_len = u32::from_be_bytes(p[offset..offset + 4].try_into().unwrap()) as usize;
        offset += 4;
        if offset + id_len > p.len() {
            return Err("Invalid external_asset_id length");
        }
        let external_asset_id =
            String::from_utf8(p[offset..offset + id_len].to_vec()).map_err(|_| "Invalid UTF-8")?;
        offset += id_len;

        if offset + 20 > p.len() {
            return Err("Missing verifier_address");
        }
        let verifier_address = Address(p[offset..offset + 20].try_into().unwrap());
        offset += 20;

        if offset + 65 > p.len() {
            return Err("Missing signature");
        }
        let signature: [u8; 65] = p[offset..offset + 65].try_into().unwrap();
        offset += 65;

        let meta_len = if offset + 4 <= p.len() {
            let l = u32::from_be_bytes(p[offset..offset + 4].try_into().unwrap()) as usize;
            offset += 4;
            l
        } else {
            0
        };

        let metadata_uri = if meta_len > 0 && offset + meta_len <= p.len() {
            String::from_utf8(p[offset..offset + meta_len].to_vec()).unwrap_or_default()
        } else {
            String::new()
        };

        Ok(Self::MintWithAttestation {
            attestation: ExternalAssetAttestation {
                chain_id,
                nonce,
                destination_wallet: dest,
                expires_at,
                source_platform,
                asset_type,
                external_asset_id,
                authorizer_address: verifier_address,
                verifier_address,
                signature,
            },
            metadata_uri,
        })
    }

    fn decode_transfer(p: &[u8]) -> Result<Self, &'static str> {
        if p.len() < 20 + 20 + 8 {
            return Err("Transfer payload too short");
        }
        let from = Address(p[0..20].try_into().unwrap());
        let to = Address(p[20..40].try_into().unwrap());
        let token_id = u64::from_be_bytes(p[40..48].try_into().unwrap());
        Ok(Self::TransferFrom { from, to, token_id })
    }

    fn decode_safe_transfer(p: &[u8]) -> Result<Self, &'static str> {
        if p.len() < 20 + 20 + 8 {
            return Err("SafeTransfer payload too short");
        }
        let from = Address(p[0..20].try_into().unwrap());
        let to = Address(p[20..40].try_into().unwrap());
        let token_id = u64::from_be_bytes(p[40..48].try_into().unwrap());
        let data = if p.len() > 52 {
            let len = u32::from_be_bytes(p[48..52].try_into().unwrap()) as usize;
            if 52 + len <= p.len() {
                p[52..52 + len].to_vec()
            } else {
                p[52..].to_vec()
            }
        } else {
            vec![]
        };
        Ok(Self::SafeTransferFrom {
            from,
            to,
            token_id,
            data,
        })
    }

    fn decode_approve(p: &[u8]) -> Result<Self, &'static str> {
        if p.len() < 20 + 8 {
            return Err("Approve payload too short");
        }
        let to = Address(p[0..20].try_into().unwrap());
        let token_id = u64::from_be_bytes(p[20..28].try_into().unwrap());
        Ok(Self::Approve { to, token_id })
    }

    fn decode_approval_for_all(p: &[u8]) -> Result<Self, &'static str> {
        if p.len() < 21 {
            return Err("ApprovalForAll payload too short");
        }
        let operator = Address(p[0..20].try_into().unwrap());
        let approved = p[20] != 0;
        Ok(Self::SetApprovalForAll { operator, approved })
    }

    fn decode_burn(p: &[u8]) -> Result<Self, &'static str> {
        if p.len() < 8 {
            return Err("Burn payload too short");
        }
        let token_id = u64::from_be_bytes(p[0..8].try_into().unwrap());
        Ok(Self::Burn { token_id })
    }
}

// ----------------------------------------------------------------------------
// Deterministic StateDB Slot Reading / Writing Helpers
// ----------------------------------------------------------------------------

pub struct ExternalAssetStorage;

impl ExternalAssetStorage {
    /// Read serialized bytes from chunked storage slots
    pub fn read_bytes(state: &dyn StateDB, base_slot: Hash) -> Option<Vec<u8>> {
        let meta_hash = state
            .get_storage(&EXTERNAL_ASSET_CONTRACT_ADDRESS, &base_slot)
            .ok()?;
        if meta_hash == Hash::ZERO {
            return None;
        }

        let total_len = u32::from_be_bytes(meta_hash.0[0..4].try_into().unwrap()) as usize;
        if total_len == 0 {
            return Some(Vec::new());
        }

        let num_chunks = (total_len + 31) / 32;
        let mut out = Vec::with_capacity(num_chunks * 32);

        for i in 0..num_chunks {
            let mut chunk_key = Vec::with_capacity(36);
            chunk_key.extend_from_slice(base_slot.as_bytes());
            chunk_key.extend_from_slice(&(i as u32).to_be_bytes());
            let chunk_slot = blake3_hash(&chunk_key);
            let chunk_val = state
                .get_storage(&EXTERNAL_ASSET_CONTRACT_ADDRESS, &chunk_slot)
                .ok()
                .unwrap_or(Hash::ZERO);
            out.extend_from_slice(chunk_val.as_bytes());
        }

        out.truncate(total_len);
        Some(out)
    }

    /// Write serialized bytes into chunked storage slots, returning state deltas
    pub fn write_bytes(base_slot: Hash, data: &[u8]) -> Vec<((Address, Hash), Hash)> {
        let mut deltas = Vec::new();
        let total_len = data.len();

        let mut meta_val = [0u8; 32];
        meta_val[0..4].copy_from_slice(&(total_len as u32).to_be_bytes());
        deltas.push(((EXTERNAL_ASSET_CONTRACT_ADDRESS, base_slot), Hash(meta_val)));

        let num_chunks = (total_len + 31) / 32;
        for i in 0..num_chunks {
            let mut chunk_key = Vec::with_capacity(36);
            chunk_key.extend_from_slice(base_slot.as_bytes());
            chunk_key.extend_from_slice(&(i as u32).to_be_bytes());
            let chunk_slot = blake3_hash(&chunk_key);

            let start = i * 32;
            let end = (start + 32).min(total_len);
            let mut chunk_val = [0u8; 32];
            chunk_val[0..end - start].copy_from_slice(&data[start..end]);

            deltas.push((
                (EXTERNAL_ASSET_CONTRACT_ADDRESS, chunk_slot),
                Hash(chunk_val),
            ));
        }

        deltas
    }

    /// Read an ExternalAsset record directly from persistent StateDB
    pub fn read_asset(state: &dyn StateDB, token_id: u64) -> Option<ExternalAsset> {
        let base_slot = slot_token_record(token_id);
        let bytes = Self::read_bytes(state, base_slot)?;
        Self::deserialize_asset(&bytes)
    }

    /// Read owner of a token directly from persistent StateDB
    pub fn read_owner(state: &dyn StateDB, token_id: u64) -> Option<Address> {
        let slot = slot_token_owner(token_id);
        let val = state
            .get_storage(&EXTERNAL_ASSET_CONTRACT_ADDRESS, &slot)
            .ok()?;
        if val == Hash::ZERO {
            None
        } else {
            let mut addr = [0u8; 20];
            addr.copy_from_slice(&val.0[12..32]);
            Some(Address(addr))
        }
    }

    /// Read token ID by canonical ID
    pub fn read_token_by_canonical_id(state: &dyn StateDB, canonical_id: &[u8; 32]) -> Option<u64> {
        let slot = slot_canonical_token(canonical_id);
        let val = state
            .get_storage(&EXTERNAL_ASSET_CONTRACT_ADDRESS, &slot)
            .ok()?;
        if val == Hash::ZERO {
            None
        } else {
            let mut b = [0u8; 8];
            b.copy_from_slice(&val.0[24..32]);
            Some(u64::from_be_bytes(b))
        }
    }

    /// Check if an attestation digest was already used
    pub fn is_attestation_used(state: &dyn StateDB, digest: &[u8; 32]) -> bool {
        let slot = slot_used_attestation(digest);
        state
            .get_storage(&EXTERNAL_ASSET_CONTRACT_ADDRESS, &slot)
            .ok()
            .map(|v| v != Hash::ZERO)
            .unwrap_or(false)
    }

    /// Read token balance of an owner
    pub fn read_balance(state: &dyn StateDB, owner: &Address) -> u64 {
        let slot = slot_balance(owner);
        state
            .get_storage(&EXTERNAL_ASSET_CONTRACT_ADDRESS, &slot)
            .ok()
            .map(|v| {
                let mut b = [0u8; 8];
                b.copy_from_slice(&v.0[24..32]);
                u64::from_be_bytes(b)
            })
            .unwrap_or(0)
    }

    pub fn serialize_asset(asset: &ExternalAsset) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(&asset.token_id.to_be_bytes());
        b.extend_from_slice(&asset.canonical_id);
        b.extend_from_slice(&(asset.source_platform.len() as u32).to_be_bytes());
        b.extend_from_slice(asset.source_platform.as_bytes());
        b.push(asset.asset_type.to_u8());
        b.extend_from_slice(&(asset.external_asset_id.len() as u32).to_be_bytes());
        b.extend_from_slice(asset.external_asset_id.as_bytes());
        b.extend_from_slice(&(asset.external_reference.len() as u32).to_be_bytes());
        b.extend_from_slice(asset.external_reference.as_bytes());
        b.extend_from_slice(&(asset.metadata_uri.len() as u32).to_be_bytes());
        b.extend_from_slice(asset.metadata_uri.as_bytes());
        b.extend_from_slice(asset.owner.as_bytes());
        b.extend_from_slice(asset.authorizer.as_bytes());
        b.extend_from_slice(asset.verifier_address.as_bytes());
        b.extend_from_slice(&asset.minted_at.to_be_bytes());
        b
    }

    pub fn deserialize_asset(p: &[u8]) -> Option<ExternalAsset> {
        let mut offset = 0;
        if p.len() < 8 + 32 + 4 {
            return None;
        }

        let token_id = u64::from_be_bytes(p[offset..offset + 8].try_into().ok()?);
        offset += 8;
        let mut canonical_id = [0u8; 32];
        canonical_id.copy_from_slice(&p[offset..offset + 32]);
        offset += 32;

        let src_len = u32::from_be_bytes(p[offset..offset + 4].try_into().ok()?) as usize;
        offset += 4;
        let source_platform = String::from_utf8(p[offset..offset + src_len].to_vec()).ok()?;
        offset += src_len;

        let asset_type = ExternalAssetType::from_u8(p[offset])?;
        offset += 1;

        let id_len = u32::from_be_bytes(p[offset..offset + 4].try_into().ok()?) as usize;
        offset += 4;
        let external_asset_id = String::from_utf8(p[offset..offset + id_len].to_vec()).ok()?;
        offset += id_len;

        let ref_len = u32::from_be_bytes(p[offset..offset + 4].try_into().ok()?) as usize;
        offset += 4;
        let external_reference = String::from_utf8(p[offset..offset + ref_len].to_vec()).ok()?;
        offset += ref_len;

        let meta_len = u32::from_be_bytes(p[offset..offset + 4].try_into().ok()?) as usize;
        offset += 4;
        let metadata_uri = String::from_utf8(p[offset..offset + meta_len].to_vec()).ok()?;
        offset += meta_len;

        let owner = Address(p[offset..offset + 20].try_into().ok()?);
        offset += 20;

        let authorizer = Address(p[offset..offset + 20].try_into().ok()?);
        offset += 20;

        let (verifier_address, minted_at) = if offset + 20 + 8 <= p.len() {
            let v = Address(p[offset..offset + 20].try_into().ok()?);
            offset += 20;
            let m = u64::from_be_bytes(p[offset..offset + 8].try_into().ok()?);
            (v, m)
        } else if offset + 8 <= p.len() {
            let m = u64::from_be_bytes(p[offset..offset + 8].try_into().ok()?);
            (authorizer, m)
        } else {
            return None;
        };

        Some(ExternalAsset {
            token_id,
            canonical_id,
            source_platform,
            asset_type,
            external_asset_id,
            external_reference,
            metadata_uri,
            owner,
            authorizer,
            verifier_address,
            minted_at,
        })
    }
}

// ----------------------------------------------------------------------------
// Execution Engine Dispatcher
// ----------------------------------------------------------------------------

pub struct ExternalAssetContractExecutor;

impl ExternalAssetContractExecutor {
    /// Execute a contract call against StateDB
    pub fn execute(
        tx: &Transaction,
        _block_height: BlockHeight,
        block_timestamp: Timestamp,
        state: &dyn StateDB,
    ) -> ContractExecutionResult {
        let call = match ExternalAssetCall::decode(&tx.data) {
            Ok(c) => c,
            Err(reason) => {
                return ContractExecutionResult {
                    success: false,
                    gas_consumed: Gas(21_000),
                    storage_deltas: vec![],
                    logs: vec![],
                    return_data: reason.as_bytes().to_vec(),
                };
            }
        };

        match call {
            ExternalAssetCall::MintWithAttestation {
                attestation,
                metadata_uri,
            } => Self::execute_mint(tx, attestation, metadata_uri, block_timestamp.0, state),
            ExternalAssetCall::TransferFrom { from, to, token_id }
            | ExternalAssetCall::SafeTransferFrom {
                from, to, token_id, ..
            } => Self::execute_transfer(tx.from, from, to, token_id, state),
            ExternalAssetCall::Approve { to, token_id } => {
                Self::execute_approve(tx.from, to, token_id, state)
            }
            ExternalAssetCall::SetApprovalForAll { operator, approved } => {
                Self::execute_set_approval_for_all(tx.from, operator, approved, state)
            }
            ExternalAssetCall::Burn { token_id } => Self::execute_burn(tx.from, token_id, state),
        }
    }

    fn execute_mint(
        tx: &Transaction,
        attestation: ExternalAssetAttestation,
        metadata_uri: String,
        current_time: u64,
        state: &dyn StateDB,
    ) -> ContractExecutionResult {
        // 1. Expiry check
        if current_time > attestation.expires_at {
            return Self::revert("Attestation expired", 35_000);
        }

        // 2. Chain ID match
        if attestation.chain_id != tx.chain_id.0 {
            return Self::revert("Chain ID mismatch", 35_000);
        }

        // 3. Destination wallet match
        if attestation.destination_wallet != tx.from {
            return Self::revert("Destination wallet mismatch", 35_000);
        }

        // 4. Replay protection (attestation digest already used?)
        let digest = compute_external_asset_attestation_digest(
            attestation.chain_id,
            attestation.nonce,
            &attestation.destination_wallet,
            attestation.expires_at,
            &attestation.source_platform,
            attestation.asset_type,
            &attestation.external_asset_id,
        );
        if ExternalAssetStorage::is_attestation_used(state, &digest) {
            return Self::revert("Attestation already used", 40_000);
        }

        // 5. Canonical ID duplicate check
        let canonical_id = compute_external_asset_canonical_id(
            &attestation.source_platform,
            attestation.asset_type,
            &attestation.external_asset_id,
        );
        if ExternalAssetStorage::read_token_by_canonical_id(state, &canonical_id).is_some() {
            return Self::revert("Asset already minted with this canonical ID", 40_000);
        }

        // 6. Cryptographic signature verification
        let is_valid_sig = match verify_external_asset_attestation(
            attestation.chain_id,
            attestation.nonce,
            &attestation.destination_wallet,
            attestation.expires_at,
            &attestation.source_platform,
            attestation.asset_type,
            &attestation.external_asset_id,
            &attestation.verifier_address,
            &attestation.signature,
            current_time,
        ) {
            Ok(v) => v,
            Err(_) => false,
        };
        if !is_valid_sig {
            return Self::revert("Invalid verifier signature", 55_000);
        }

        // 7. Next Token ID allocation
        let next_id_slot = slot_next_token_id();
        let next_id_val = state
            .get_storage(&EXTERNAL_ASSET_CONTRACT_ADDRESS, &next_id_slot)
            .ok()
            .unwrap_or(Hash::ZERO);
        let token_id = if next_id_val == Hash::ZERO {
            1u64
        } else {
            let mut b = [0u8; 8];
            b.copy_from_slice(&next_id_val.0[24..32]);
            u64::from_be_bytes(b)
        };

        let new_next_id = token_id + 1;
        let mut new_next_val = [0u8; 32];
        new_next_val[24..32].copy_from_slice(&new_next_id.to_be_bytes());

        // 8. Assemble storage mutations
        let mut deltas = Vec::new();

        // Increment next token ID
        deltas.push((
            (EXTERNAL_ASSET_CONTRACT_ADDRESS, next_id_slot),
            Hash(new_next_val),
        ));

        // Mark attestation digest as used
        let used_slot = slot_used_attestation(&digest);
        deltas.push((
            (EXTERNAL_ASSET_CONTRACT_ADDRESS, used_slot),
            Hash([1u8; 32]),
        ));

        // Record canonical ID -> Token ID mapping
        let canon_slot = slot_canonical_token(&canonical_id);
        let mut tid_val = [0u8; 32];
        tid_val[24..32].copy_from_slice(&token_id.to_be_bytes());
        deltas.push(((EXTERNAL_ASSET_CONTRACT_ADDRESS, canon_slot), Hash(tid_val)));

        // Record owner
        let owner_slot = slot_token_owner(token_id);
        let mut owner_val = [0u8; 32];
        owner_val[12..32].copy_from_slice(attestation.destination_wallet.as_bytes());
        deltas.push((
            (EXTERNAL_ASSET_CONTRACT_ADDRESS, owner_slot),
            Hash(owner_val),
        ));

        // Update recipient balance
        let bal_slot = slot_balance(&attestation.destination_wallet);
        let current_bal =
            ExternalAssetStorage::read_balance(state, &attestation.destination_wallet);
        let mut bal_val = [0u8; 32];
        bal_val[24..32].copy_from_slice(&(current_bal + 1).to_be_bytes());
        deltas.push(((EXTERNAL_ASSET_CONTRACT_ADDRESS, bal_slot), Hash(bal_val)));

        // Record full ExternalAsset struct
        let asset = ExternalAsset {
            token_id,
            canonical_id,
            source_platform: attestation.source_platform.clone(),
            asset_type: attestation.asset_type,
            external_asset_id: attestation.external_asset_id.clone(),
            external_reference: format!("ref_{}", attestation.external_asset_id),
            metadata_uri: metadata_uri.clone(),
            owner: attestation.destination_wallet,
            authorizer: attestation.authorizer_address,
            verifier_address: attestation.verifier_address,
            minted_at: current_time,
        };
        let asset_bytes = ExternalAssetStorage::serialize_asset(&asset);
        let record_slot = slot_token_record(token_id);
        deltas.extend(ExternalAssetStorage::write_bytes(record_slot, &asset_bytes));

        // 10. Generate Event Logs
        let mut tid_topic = [0u8; 32];
        tid_topic[24..32].copy_from_slice(&token_id.to_be_bytes());

        let mut dest_topic = [0u8; 32];
        dest_topic[12..32].copy_from_slice(attestation.destination_wallet.as_bytes());

        let logs = vec![
            Log {
                address: EXTERNAL_ASSET_CONTRACT_ADDRESS,
                topics: vec![
                    TOPIC_EXTERNAL_ASSET_MINTED,
                    Hash(tid_topic),
                    Hash(canonical_id),
                    Hash(dest_topic),
                ],
                data: asset_bytes,
            },
            Log {
                address: EXTERNAL_ASSET_CONTRACT_ADDRESS,
                topics: vec![
                    TOPIC_TRANSFER,
                    Hash::ZERO,
                    Hash(dest_topic),
                    Hash(tid_topic),
                ],
                data: vec![],
            },
        ];

        ContractExecutionResult {
            success: true,
            gas_consumed: Gas(85_000),
            storage_deltas: deltas,
            logs,
            return_data: token_id.to_be_bytes().to_vec(),
        }
    }

    fn execute_transfer(
        caller: Address,
        from: Address,
        to: Address,
        token_id: u64,
        state: &dyn StateDB,
    ) -> ContractExecutionResult {
        if to == Address::ZERO {
            return Self::revert("Zero address transfer", 25_000);
        }

        let current_owner = match ExternalAssetStorage::read_owner(state, token_id) {
            Some(o) => o,
            None => return Self::revert("Token does not exist", 25_000),
        };

        if current_owner != from {
            return Self::revert("From is not owner", 25_000);
        }

        // Authorization check
        let is_owner = caller == current_owner;
        let is_approved = if !is_owner {
            let app_slot = slot_token_approval(token_id);
            let app_val = state
                .get_storage(&EXTERNAL_ASSET_CONTRACT_ADDRESS, &app_slot)
                .ok()
                .unwrap_or(Hash::ZERO);
            let mut app_addr = [0u8; 20];
            app_addr.copy_from_slice(&app_val.0[12..32]);
            Address(app_addr) == caller
        } else {
            false
        };

        let is_operator = if !is_owner && !is_approved {
            let op_slot = slot_operator_approval(&current_owner, &caller);
            state
                .get_storage(&EXTERNAL_ASSET_CONTRACT_ADDRESS, &op_slot)
                .ok()
                .map(|v| v != Hash::ZERO)
                .unwrap_or(false)
        } else {
            false
        };

        if !is_owner && !is_approved && !is_operator {
            return Self::revert("Not owner or approved", 25_000);
        }

        let mut deltas = Vec::new();

        // Clear approval
        let app_slot = slot_token_approval(token_id);
        deltas.push(((EXTERNAL_ASSET_CONTRACT_ADDRESS, app_slot), Hash::ZERO));

        // Update owner
        let owner_slot = slot_token_owner(token_id);
        let mut owner_val = [0u8; 32];
        owner_val[12..32].copy_from_slice(to.as_bytes());
        deltas.push((
            (EXTERNAL_ASSET_CONTRACT_ADDRESS, owner_slot),
            Hash(owner_val),
        ));

        // Update balances
        let from_bal = ExternalAssetStorage::read_balance(state, &from);
        if from_bal > 0 {
            let mut f_val = [0u8; 32];
            f_val[24..32].copy_from_slice(&(from_bal - 1).to_be_bytes());
            deltas.push((
                (EXTERNAL_ASSET_CONTRACT_ADDRESS, slot_balance(&from)),
                Hash(f_val),
            ));
        }
        let to_bal = ExternalAssetStorage::read_balance(state, &to);
        let mut t_val = [0u8; 32];
        t_val[24..32].copy_from_slice(&(to_bal + 1).to_be_bytes());
        deltas.push((
            (EXTERNAL_ASSET_CONTRACT_ADDRESS, slot_balance(&to)),
            Hash(t_val),
        ));

        // Update owner inside the stored asset record if it exists
        if let Some(mut asset) = ExternalAssetStorage::read_asset(state, token_id) {
            asset.owner = to;
            let bytes = ExternalAssetStorage::serialize_asset(&asset);
            deltas.extend(ExternalAssetStorage::write_bytes(
                slot_token_record(token_id),
                &bytes,
            ));
        }

        // Event logs
        let mut from_topic = [0u8; 32];
        from_topic[12..32].copy_from_slice(from.as_bytes());
        let mut to_topic = [0u8; 32];
        to_topic[12..32].copy_from_slice(to.as_bytes());
        let mut tid_topic = [0u8; 32];
        tid_topic[24..32].copy_from_slice(&token_id.to_be_bytes());

        let logs = vec![Log {
            address: EXTERNAL_ASSET_CONTRACT_ADDRESS,
            topics: vec![
                TOPIC_TRANSFER,
                Hash(from_topic),
                Hash(to_topic),
                Hash(tid_topic),
            ],
            data: vec![],
        }];

        ContractExecutionResult {
            success: true,
            gas_consumed: Gas(45_000),
            storage_deltas: deltas,
            logs,
            return_data: vec![],
        }
    }

    fn execute_approve(
        caller: Address,
        to: Address,
        token_id: u64,
        state: &dyn StateDB,
    ) -> ContractExecutionResult {
        let current_owner = match ExternalAssetStorage::read_owner(state, token_id) {
            Some(o) => o,
            None => return Self::revert("Token does not exist", 25_000),
        };

        let is_owner = caller == current_owner;
        let is_operator = if !is_owner {
            let op_slot = slot_operator_approval(&current_owner, &caller);
            state
                .get_storage(&EXTERNAL_ASSET_CONTRACT_ADDRESS, &op_slot)
                .ok()
                .map(|v| v != Hash::ZERO)
                .unwrap_or(false)
        } else {
            false
        };

        if !is_owner && !is_operator {
            return Self::revert("Not owner or operator", 25_000);
        }

        let app_slot = slot_token_approval(token_id);
        let mut app_val = [0u8; 32];
        app_val[12..32].copy_from_slice(to.as_bytes());

        let mut owner_topic = [0u8; 32];
        owner_topic[12..32].copy_from_slice(current_owner.as_bytes());
        let mut to_topic = [0u8; 32];
        to_topic[12..32].copy_from_slice(to.as_bytes());
        let mut tid_topic = [0u8; 32];
        tid_topic[24..32].copy_from_slice(&token_id.to_be_bytes());

        let logs = vec![Log {
            address: EXTERNAL_ASSET_CONTRACT_ADDRESS,
            topics: vec![
                TOPIC_APPROVAL,
                Hash(owner_topic),
                Hash(to_topic),
                Hash(tid_topic),
            ],
            data: vec![],
        }];

        ContractExecutionResult {
            success: true,
            gas_consumed: Gas(35_000),
            storage_deltas: vec![((EXTERNAL_ASSET_CONTRACT_ADDRESS, app_slot), Hash(app_val))],
            logs,
            return_data: vec![],
        }
    }

    fn execute_set_approval_for_all(
        caller: Address,
        operator: Address,
        approved: bool,
        _state: &dyn StateDB,
    ) -> ContractExecutionResult {
        if operator == Address::ZERO {
            return Self::revert("Zero address operator", 25_000);
        }

        let op_slot = slot_operator_approval(&caller, &operator);
        let val = if approved {
            Hash([1u8; 32])
        } else {
            Hash::ZERO
        };

        let mut owner_topic = [0u8; 32];
        owner_topic[12..32].copy_from_slice(caller.as_bytes());
        let mut op_topic = [0u8; 32];
        op_topic[12..32].copy_from_slice(operator.as_bytes());

        let logs = vec![Log {
            address: EXTERNAL_ASSET_CONTRACT_ADDRESS,
            topics: vec![TOPIC_APPROVAL_FOR_ALL, Hash(owner_topic), Hash(op_topic)],
            data: vec![if approved { 1 } else { 0 }],
        }];

        ContractExecutionResult {
            success: true,
            gas_consumed: Gas(35_000),
            storage_deltas: vec![((EXTERNAL_ASSET_CONTRACT_ADDRESS, op_slot), val)],
            logs,
            return_data: vec![],
        }
    }

    fn execute_burn(
        caller: Address,
        token_id: u64,
        state: &dyn StateDB,
    ) -> ContractExecutionResult {
        let current_owner = match ExternalAssetStorage::read_owner(state, token_id) {
            Some(o) => o,
            None => return Self::revert("Token does not exist", 25_000),
        };

        if caller != current_owner {
            return Self::revert("Only owner can burn token", 25_000);
        }

        let mut deltas = Vec::new();

        // Clear owner
        deltas.push((
            (EXTERNAL_ASSET_CONTRACT_ADDRESS, slot_token_owner(token_id)),
            Hash::ZERO,
        ));
        // Clear approval
        deltas.push((
            (
                EXTERNAL_ASSET_CONTRACT_ADDRESS,
                slot_token_approval(token_id),
            ),
            Hash::ZERO,
        ));

        // Decrement balance
        let bal = ExternalAssetStorage::read_balance(state, &current_owner);
        if bal > 0 {
            let mut b_val = [0u8; 32];
            b_val[24..32].copy_from_slice(&(bal - 1).to_be_bytes());
            deltas.push((
                (
                    EXTERNAL_ASSET_CONTRACT_ADDRESS,
                    slot_balance(&current_owner),
                ),
                Hash(b_val),
            ));
        }

        let mut from_topic = [0u8; 32];
        from_topic[12..32].copy_from_slice(current_owner.as_bytes());
        let mut tid_topic = [0u8; 32];
        tid_topic[24..32].copy_from_slice(&token_id.to_be_bytes());

        let logs = vec![Log {
            address: EXTERNAL_ASSET_CONTRACT_ADDRESS,
            topics: vec![
                TOPIC_TRANSFER,
                Hash(from_topic),
                Hash::ZERO,
                Hash(tid_topic),
            ],
            data: vec![],
        }];

        ContractExecutionResult {
            success: true,
            gas_consumed: Gas(35_000),
            storage_deltas: deltas,
            logs,
            return_data: vec![],
        }
    }

    fn revert(reason: &'static str, gas: u64) -> ContractExecutionResult {
        ContractExecutionResult {
            success: false,
            gas_consumed: Gas(gas),
            storage_deltas: vec![],
            logs: vec![],
            return_data: reason.as_bytes().to_vec(),
        }
    }
}
