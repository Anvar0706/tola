//! Production Cryptographic Primitives for TOLA Network.
//!
//! Provides deterministic hashing (BLAKE3, Keccak-256, SHA-256),
//! digital signatures (Ed25519 for BFT consensus, Secp256k1 with public key recovery for user transactions),
//! domain separation, and zeroization memory guards.

#![allow(clippy::manual_div_ceil, clippy::manual_is_multiple_of)]

use tola_types::{
    Address, CoordinationBlock, CrossShardReceipt, ExternalAssetType, Hash, Log, PublicKey,
    Receipt, ShardId, Signature, DOMAIN_COORD_BLOCK, DOMAIN_RECEIPT_HASH, DOMAIN_RECEIPT_NULLIFIER,
};
use zeroize::{Zeroize, ZeroizeOnDrop};

// ============================================================================
// 1. Registered Domain Separation Tags
// ============================================================================
pub const DOMAIN_CONSENSUS_PREVOTE: &[u8; 24] = b"TOLA-CONSENSUS-PREVOTE-1";
pub const DOMAIN_CONSENSUS_PRECOMMIT: &[u8; 24] = b"TOLA-CONSENSUS-PRECOMMIT";
pub const DOMAIN_TRANSACTION_PAYLOAD: &[u8; 24] = b"TOLA-TX-ENVELOPE-v1.0.0-";
pub const DOMAIN_CONNECT_SESSION: &[u8; 24] = b"TOLA-CONNECT-SESSION-v1-";

#[derive(Debug, PartialEq, Eq)]
pub enum CryptoError {
    InvalidKeyLength,
    InvalidSignatureLength,
    InvalidRecoveryId,
    SignatureVerificationFailed,
    MalleableSignatureHighS,
    RecoveryFailed,
    InvalidPublicKeyEncoding,
    AttestationExpired,
    SignerMismatch,
    InvalidMnemonic(String),
    DerivationFailed(String),
}

pub mod mnemonic;
pub use mnemonic::{
    canonical_path_for_network, format_bip44_path, Mnemonic, MnemonicCount, Seed,
    CANONICAL_DERIVATION_PATH, DEFAULT_DEVNET_PATH, DEFAULT_MAINNET_PATH, DEFAULT_TESTNET_PATH,
    LEGACY_TESTNET_PROVISIONAL_PATH,
};

// ============================================================================
// 2. Cryptographic Hash Functions
// ============================================================================

/// Compute 32-byte BLAKE3 digest (primary internal tree and block digest).
pub fn blake3_hash(data: &[u8]) -> Hash {
    let digest = blake3::hash(data);
    Hash(*digest.as_bytes())
}

/// Compute 32-byte Keccak-256 digest (EVM opcodes and address derivation).
pub fn keccak256_hash(data: &[u8]) -> Hash {
    use sha3::{Digest, Keccak256};
    let mut hasher = Keccak256::new();
    hasher.update(data);
    let result = hasher.finalize();
    let mut h = [0u8; 32];
    h.copy_from_slice(&result);
    Hash(h)
}

/// Compute 32-byte SHA-256 digest (NIST FIPS 180-4 standard).
pub fn sha256_hash(data: &[u8]) -> Hash {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(data);
    let result = hasher.finalize();
    let mut h = [0u8; 32];
    h.copy_from_slice(&result);
    Hash(h)
}

/// Construct a domain-separated signing hash pre-image.
/// Format: BLAKE3( DomainTag (24B) || ChainID (8B LE) || Epoch (8B LE) || PayloadHash (32B) )
pub fn domain_separated_hash(
    domain_tag: &[u8; 24],
    chain_id: u64,
    epoch: u64,
    payload_hash: &Hash,
) -> Hash {
    let mut pre_image = Vec::with_capacity(24 + 8 + 8 + 32);
    pre_image.extend_from_slice(domain_tag);
    pre_image.extend_from_slice(&chain_id.to_le_bytes());
    pre_image.extend_from_slice(&epoch.to_le_bytes());
    pre_image.extend_from_slice(payload_hash.as_bytes());
    blake3_hash(&pre_image)
}

// ============================================================================
// 3. Ed25519 Signatures (RFC 8032) — Consensus & Peer Identity
// ============================================================================

/// Secret key for Ed25519 with automatic zeroization on drop.
#[derive(Zeroize, ZeroizeOnDrop)]
pub struct Ed25519SecretKey([u8; 32]);

#[derive(Clone)]
pub struct Ed25519Keypair {
    signing_key: ed25519_dalek::SigningKey,
}

impl Ed25519Keypair {
    /// Generate a new random Ed25519 keypair from system CSPRNG.
    pub fn generate() -> Self {
        use rand_core::OsRng;
        let signing_key = ed25519_dalek::SigningKey::generate(&mut OsRng);
        Self { signing_key }
    }

    /// Instantiate keypair from 32-byte seed.
    pub fn from_seed(seed: &[u8; 32]) -> Self {
        let signing_key = ed25519_dalek::SigningKey::from_bytes(seed);
        Self { signing_key }
    }

    /// Return 32-byte compressed public key.
    pub fn public_key_bytes(&self) -> [u8; 32] {
        self.signing_key.verifying_key().to_bytes()
    }

    pub fn public_key(&self) -> PublicKey {
        PublicKey::Ed25519(self.public_key_bytes())
    }

    /// Sign a message using pure Ed25519 (RFC 8032).
    pub fn sign(&self, message: &[u8]) -> Signature {
        use ed25519_dalek::Signer;
        let sig = self.signing_key.sign(message);
        Signature::Ed25519(sig.to_bytes())
    }
}

/// Verify an Ed25519 signature against a public key.
pub fn verify_ed25519(
    pubkey: &[u8; 32],
    message: &[u8],
    signature: &[u8; 64],
) -> Result<(), CryptoError> {
    use ed25519_dalek::Verifier;
    let vk = ed25519_dalek::VerifyingKey::from_bytes(pubkey)
        .map_err(|_| CryptoError::InvalidKeyLength)?;
    let sig = ed25519_dalek::Signature::from_bytes(signature);
    vk.verify(message, &sig)
        .map_err(|_| CryptoError::SignatureVerificationFailed)
}

/// Batch verify multiple Ed25519 signatures simultaneously.
pub fn verify_ed25519_batch(
    messages: &[&[u8]],
    signatures: &[[u8; 64]],
    public_keys: &[[u8; 32]],
) -> Result<(), CryptoError> {
    if messages.len() != signatures.len() || signatures.len() != public_keys.len() {
        return Err(CryptoError::InvalidSignatureLength);
    }
    use ed25519_dalek::Verifier;
    for i in 0..messages.len() {
        let vk = ed25519_dalek::VerifyingKey::from_bytes(&public_keys[i])
            .map_err(|_| CryptoError::InvalidKeyLength)?;
        let sig = ed25519_dalek::Signature::from_bytes(&signatures[i]);
        vk.verify(messages[i], &sig)
            .map_err(|_| CryptoError::SignatureVerificationFailed)?;
    }
    Ok(())
}

// ============================================================================
// 4. Secp256k1 Signatures (RFC 6979) — User Transactions & Public Key Recovery
// ============================================================================

#[derive(Clone)]
pub struct Secp256k1Keypair {
    signing_key: k256::ecdsa::SigningKey,
}

impl Secp256k1Keypair {
    /// Generate a new random Secp256k1 keypair.
    pub fn generate() -> Self {
        use rand_core::OsRng;
        let signing_key = k256::ecdsa::SigningKey::random(&mut OsRng);
        Self { signing_key }
    }

    /// Instantiate keypair from 32-byte private key scalar.
    pub fn from_private_key_bytes(bytes: &[u8; 32]) -> Result<Self, CryptoError> {
        let signing_key = k256::ecdsa::SigningKey::from_bytes(bytes.into())
            .map_err(|_| CryptoError::InvalidKeyLength)?;
        Ok(Self { signing_key })
    }

    /// Get 32-byte private key scalar.
    pub fn private_key_bytes(&self) -> [u8; 32] {
        let bytes = self.signing_key.to_bytes();
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        arr
    }

    /// Get 33-byte compressed public key.
    pub fn public_key_bytes(&self) -> [u8; 33] {
        let vk = self.signing_key.verifying_key();
        let pt = vk.to_encoded_point(true);
        let mut arr = [0u8; 33];
        arr.copy_from_slice(pt.as_bytes());
        arr
    }

    /// Get 65-byte uncompressed public key (0x04 || X || Y).
    pub fn uncompressed_public_key_bytes(&self) -> [u8; 65] {
        let vk = self.signing_key.verifying_key();
        let pt = vk.to_encoded_point(false);
        let mut arr = [0u8; 65];
        arr.copy_from_slice(pt.as_bytes());
        arr
    }

    /// Derive 20-byte address: Keccak256(UncompressedPubKey[1..65])[12..32].
    pub fn address(&self) -> Address {
        let vk = self.signing_key.verifying_key();
        secp256k1_verifying_key_to_address(vk)
    }

    /// Sign digest deterministically (RFC 6979) with recovery ID: returns 65-byte [r(32) || s(32) || v(1)].
    pub fn sign_recoverable(&self, digest: &[u8; 32]) -> Result<[u8; 65], CryptoError> {
        let (sig, recid) = self
            .signing_key
            .sign_prehash_recoverable(digest)
            .map_err(|_| CryptoError::SignatureVerificationFailed)?;

        let mut out = [0u8; 65];
        let (r, s) = sig.split_bytes();
        out[0..32].copy_from_slice(&r);
        out[32..64].copy_from_slice(&s);
        out[64] = recid.to_byte();
        Ok(out)
    }

    /// Sign an arbitrary message with standard prefix:
    /// `\x19TOLA Signed Message:\n<length><message>`
    pub fn sign_message(&self, message: &[u8]) -> Result<[u8; 65], CryptoError> {
        let digest = compute_message_digest(message);
        self.sign_recoverable(&digest)
    }
}

impl core::fmt::Debug for Secp256k1Keypair {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "Secp256k1Keypair([REDACTED: {}])",
            self.address().to_hex()
        )
    }
}

/// Compute canonical message signing digest:
/// `Keccak-256("\x19TOLA Signed Message:\n" + len + message)`
pub fn compute_message_digest(message: &[u8]) -> [u8; 32] {
    let prefix = format!("\x19TOLA Signed Message:\n{}", message.len());
    let mut pre_image = Vec::with_capacity(prefix.len() + message.len());
    pre_image.extend_from_slice(prefix.as_bytes());
    pre_image.extend_from_slice(message);
    let hash = keccak256_hash(&pre_image);
    *hash.as_bytes()
}

/// Recover 20-byte address from arbitrary message and 65-byte signature.
pub fn recover_message_address(
    message: &[u8],
    signature_65: &[u8; 65],
) -> Result<Address, CryptoError> {
    let digest = compute_message_digest(message);
    recover_secp256k1_address(&digest, signature_65)
}

/// Convert uncompressed Secp256k1 public key to 20-byte canonical address.
pub fn secp256k1_verifying_key_to_address(vk: &k256::ecdsa::VerifyingKey) -> Address {
    let pt = vk.to_encoded_point(false); // Uncompressed: 65 bytes (0x04 || X || Y)
    let uncompressed_bytes = pt.as_bytes();
    let hash = keccak256_hash(&uncompressed_bytes[1..65]);
    let mut addr = [0u8; 20];
    addr.copy_from_slice(&hash.as_bytes()[12..32]);
    Address(addr)
}

/// Recover 20-byte sender address from 32-byte digest and 65-byte signature [r(32) || s(32) || v(1)].
pub fn recover_secp256k1_address(
    digest: &[u8; 32],
    signature_65: &[u8; 65],
) -> Result<Address, CryptoError> {
    // 1. Strict Low-S check to prevent signature malleability
    if !verify_secp256k1_low_s(signature_65) {
        return Err(CryptoError::MalleableSignatureHighS);
    }

    let recid_byte = signature_65[64];
    let recid =
        k256::ecdsa::RecoveryId::from_byte(recid_byte).ok_or(CryptoError::InvalidRecoveryId)?;

    let sig = k256::ecdsa::Signature::from_slice(&signature_65[0..64])
        .map_err(|_| CryptoError::SignatureVerificationFailed)?;

    let vk = k256::ecdsa::VerifyingKey::recover_from_prehash(digest, &sig, recid)
        .map_err(|_| CryptoError::RecoveryFailed)?;

    Ok(secp256k1_verifying_key_to_address(&vk))
}

/// Verify that Secp256k1 signature S-value is in canonical lower half of curve order: s <= n/2.
pub fn verify_secp256k1_low_s(signature_65: &[u8; 65]) -> bool {
    let s_bytes = &signature_65[32..64];
    // Curve order n/2 for Secp256k1
    // n = 0xFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFEBAAEDCE6AF48A03BBFD25E8CD0364141
    // n/2 = 0x7FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF5D576E7357A4501DDFE92F46681B20A0
    const HALF_N: [u8; 32] = [
        0x7f, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
        0xff, 0x5d, 0x57, 0x6e, 0x73, 0x57, 0xa4, 0x50, 0x1d, 0xdf, 0xe9, 0x2f, 0x46, 0x68, 0x1b,
        0x20, 0xa0,
    ];
    s_bytes <= &HALF_N
}

// ============================================================================
// 5. Ecosystem Cryptographic Bindings (External Assets)
// ============================================================================

/// Compute canonical Keccak-256 digest for a generic external asset mint attestation.
/// Format: Keccak256("TOLA_EXTERNAL_ASSET_ATTESTATION_V1:" || chain_id (8B BE) || nonce (8B BE) || destination (20B) || expires_at (8B BE) || source_len (4B BE) || source || asset_type (1B) || external_id_len (4B BE) || external_id)
pub fn compute_external_asset_attestation_digest(
    chain_id: u64,
    nonce: u64,
    destination_wallet: &Address,
    expires_at: u64,
    source_platform: &str,
    asset_type: ExternalAssetType,
    external_asset_id: &str,
) -> [u8; 32] {
    let prefix = b"TOLA_EXTERNAL_ASSET_ATTESTATION_V1:";
    let mut pre_image = Vec::with_capacity(
        prefix.len() + 8 + 8 + 20 + 8 + 4 + source_platform.len() + 1 + 4 + external_asset_id.len(),
    );
    pre_image.extend_from_slice(prefix);
    pre_image.extend_from_slice(&chain_id.to_be_bytes());
    pre_image.extend_from_slice(&nonce.to_be_bytes());
    pre_image.extend_from_slice(&destination_wallet.0);
    pre_image.extend_from_slice(&expires_at.to_be_bytes());
    pre_image.extend_from_slice(&(source_platform.len() as u32).to_be_bytes());
    pre_image.extend_from_slice(source_platform.as_bytes());
    pre_image.push(asset_type.to_u8());
    pre_image.extend_from_slice(&(external_asset_id.len() as u32).to_be_bytes());
    pre_image.extend_from_slice(external_asset_id.as_bytes());
    keccak256_hash(&pre_image).0
}

/// Compute canonical unique 32-byte identity for an external asset to enforce duplicate mint protection.
/// Format: Keccak256("TOLA_EXTERNAL_ASSET_CANONICAL_V1:" || source_len (4B BE) || source || asset_type (1B) || external_id_len (4B BE) || external_id)
pub fn compute_external_asset_canonical_id(
    source_platform: &str,
    asset_type: ExternalAssetType,
    external_asset_id: &str,
) -> [u8; 32] {
    let prefix = b"TOLA_EXTERNAL_ASSET_CANONICAL_V1:";
    let mut pre_image = Vec::with_capacity(
        prefix.len() + 4 + source_platform.len() + 1 + 4 + external_asset_id.len(),
    );
    pre_image.extend_from_slice(prefix);
    pre_image.extend_from_slice(&(source_platform.len() as u32).to_be_bytes());
    pre_image.extend_from_slice(source_platform.as_bytes());
    pre_image.push(asset_type.to_u8());
    pre_image.extend_from_slice(&(external_asset_id.len() as u32).to_be_bytes());
    pre_image.extend_from_slice(external_asset_id.as_bytes());
    keccak256_hash(&pre_image).0
}

/// Verify a generic external asset mint attestation against an expected authorized verifier key.
#[allow(clippy::too_many_arguments)]
pub fn verify_external_asset_attestation(
    chain_id: u64,
    nonce: u64,
    destination_wallet: &Address,
    expires_at: u64,
    source_platform: &str,
    asset_type: ExternalAssetType,
    external_asset_id: &str,
    expected_verifier: &Address,
    signature_65: &[u8; 65],
    current_time: u64,
) -> Result<bool, CryptoError> {
    if current_time > expires_at {
        return Err(CryptoError::AttestationExpired);
    }
    let digest = compute_external_asset_attestation_digest(
        chain_id,
        nonce,
        destination_wallet,
        expires_at,
        source_platform,
        asset_type,
        external_asset_id,
    );
    let recovered_address = recover_secp256k1_address(&digest, signature_65)?;
    if recovered_address != *expected_verifier {
        return Err(CryptoError::SignerMismatch);
    }
    Ok(true)
}

// ============================================================================
// 6. Cross-Shard & State Proof Primitives (ADR-0014)
// ============================================================================

/// Compute canonical BLAKE3 hash of a cross-shard receipt.
pub fn compute_receipt_hash(receipt: &CrossShardReceipt) -> Hash {
    let mut hasher = blake3::Hasher::new();
    hasher.update(DOMAIN_RECEIPT_HASH);
    hasher.update(&receipt.source_shard.0.to_le_bytes());
    hasher.update(&receipt.target_shard.0.to_le_bytes());
    hasher.update(&receipt.receipt_nonce.to_le_bytes());
    hasher.update(&receipt.sender.0);
    hasher.update(&receipt.recipient.0);
    hasher.update(&receipt.value.as_u128().to_le_bytes());
    hasher.update(&(receipt.data.len() as u32).to_le_bytes());
    hasher.update(&receipt.data);
    hasher.update(&receipt.timeout_height.0.to_le_bytes());
    Hash(*hasher.finalize().as_bytes())
}

/// Compute unique 32-byte nullifier for cross-shard replay immunity.
pub fn compute_receipt_nullifier(
    source_shard: ShardId,
    target_shard: ShardId,
    receipt_nonce: u64,
    sender: &Address,
) -> Hash {
    let mut hasher = blake3::Hasher::new();
    hasher.update(DOMAIN_RECEIPT_NULLIFIER);
    hasher.update(&source_shard.0.to_le_bytes());
    hasher.update(&target_shard.0.to_le_bytes());
    hasher.update(&receipt_nonce.to_le_bytes());
    hasher.update(&sender.0);
    Hash(*hasher.finalize().as_bytes())
}

/// Compute canonical header hash of a Coordination Layer masterchain block.
pub fn compute_coordination_block_hash(coord: &CoordinationBlock) -> Hash {
    let mut hasher = blake3::Hasher::new();
    hasher.update(DOMAIN_COORD_BLOCK);
    hasher.update(&coord.height.0.to_le_bytes());
    hasher.update(&coord.parent_hash.0);
    hasher.update(&coord.epoch.0.to_le_bytes());
    hasher.update(&coord.vrf_seed.0);
    for entry in &coord.shard_roots {
        hasher.update(&entry.shard_id.0.to_le_bytes());
        hasher.update(&entry.shard_height.0.to_le_bytes());
        hasher.update(&entry.state_root.0);
        hasher.update(&entry.receipts_root.0);
    }
    Hash(*hasher.finalize().as_bytes())
}

/// Computes the binary Merkle root from a sequence of leaf hashes using BLAKE3.
pub fn compute_merkle_root(leaves: &[Hash]) -> Hash {
    if leaves.is_empty() {
        return blake3_hash(b"EMPTY_MERKLE_TREE");
    }
    let mut current = leaves.to_vec();
    while current.len() > 1 {
        let mut next = Vec::with_capacity((current.len() + 1) / 2);
        for chunk in current.chunks(2) {
            if chunk.len() == 2 {
                let mut concat = [0u8; 64];
                concat[0..32].copy_from_slice(&chunk[0].0);
                concat[32..64].copy_from_slice(&chunk[1].0);
                next.push(blake3_hash(&concat));
            } else {
                let mut concat = [0u8; 64];
                concat[0..32].copy_from_slice(&chunk[0].0);
                concat[32..64].copy_from_slice(&chunk[0].0);
                next.push(blake3_hash(&concat));
            }
        }
        current = next;
    }
    current[0]
}

/// Generate a Merkle inclusion proof for the leaf at `index`.
pub fn generate_merkle_proof(leaves: &[Hash], index: usize) -> Option<Vec<Hash>> {
    if leaves.is_empty() || index >= leaves.len() {
        return None;
    }
    let mut proof = Vec::new();
    let mut current_idx = index;
    let mut current = leaves.to_vec();

    while current.len() > 1 {
        let sibling_idx = if current_idx % 2 == 0 {
            if current_idx + 1 < current.len() {
                current_idx + 1
            } else {
                current_idx
            }
        } else {
            current_idx - 1
        };
        proof.push(current[sibling_idx]);

        let mut next = Vec::with_capacity((current.len() + 1) / 2);
        for chunk in current.chunks(2) {
            if chunk.len() == 2 {
                let mut concat = [0u8; 64];
                concat[0..32].copy_from_slice(&chunk[0].0);
                concat[32..64].copy_from_slice(&chunk[1].0);
                next.push(blake3_hash(&concat));
            } else {
                let mut concat = [0u8; 64];
                concat[0..32].copy_from_slice(&chunk[0].0);
                concat[32..64].copy_from_slice(&chunk[0].0);
                next.push(blake3_hash(&concat));
            }
        }
        current = next;
        current_idx /= 2;
    }

    Some(proof)
}

/// Verify a binary Merkle inclusion proof against a root.
pub fn verify_merkle_inclusion_proof(
    root: &Hash,
    leaf: &Hash,
    proof: &[Hash],
    index: usize,
) -> bool {
    let mut current = *leaf;
    let mut current_idx = index;

    for sibling in proof {
        let mut concat = [0u8; 64];
        if current_idx % 2 == 0 {
            concat[0..32].copy_from_slice(&current.0);
            concat[32..64].copy_from_slice(&sibling.0);
        } else {
            concat[0..32].copy_from_slice(&sibling.0);
            concat[32..64].copy_from_slice(&current.0);
        }
        current = blake3_hash(&concat);
        current_idx /= 2;
    }

    current == *root
}

// ============================================================================
// 6b. Ethereum 2048-bit LogsBloom Filter (Yellow Paper Section 4.3.1)
// ============================================================================

/// Adds data slice to a 256-byte Ethereum logs bloom filter.
pub fn bloom_add(bloom: &mut [u8; 256], data: &[u8]) {
    let hash = keccak256_hash(data);
    for i in [0, 2, 4] {
        let bit = (((hash.0[i] as usize) << 8) | (hash.0[i + 1] as usize)) & 0x07ff;
        let byte_idx = 255 - (bit / 8);
        let bit_idx = bit % 8;
        bloom[byte_idx] |= 1 << bit_idx;
    }
}

/// Computes the 256-byte Ethereum logsBloom filter for a list of logs.
pub fn compute_logs_bloom(logs: &[Log]) -> [u8; 256] {
    let mut bloom = [0u8; 256];
    for log in logs {
        bloom_add(&mut bloom, log.address.as_bytes());
        for topic in &log.topics {
            bloom_add(&mut bloom, topic.as_bytes());
        }
    }
    bloom
}

/// Computes the block-level logsBloom filter by bitwise OR of all receipt blooms.
pub fn compute_block_logs_bloom(receipts: &[Receipt]) -> [u8; 256] {
    let mut block_bloom = [0u8; 256];
    for receipt in receipts {
        let receipt_bloom = compute_logs_bloom(&receipt.logs);
        for i in 0..256 {
            block_bloom[i] |= receipt_bloom[i];
        }
    }
    block_bloom
}

// ============================================================================
// 7. Unit Tests & RFC Test Vectors
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_blake3_known_vector() {
        // BLAKE3("") = af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262
        let h = blake3_hash(b"");
        let expected = [
            0xaf, 0x13, 0x49, 0xb9, 0xf5, 0xf9, 0xa1, 0xa6, 0xa0, 0x40, 0x4d, 0xea, 0x36, 0xdc,
            0xc9, 0x49, 0x9b, 0xcb, 0x25, 0xc9, 0xad, 0xc1, 0x12, 0xb7, 0xcc, 0x9a, 0x93, 0xca,
            0xe4, 0x1f, 0x32, 0x62,
        ];
        assert_eq!(h.0, expected);
    }

    #[test]
    fn test_sha256_known_vector() {
        // SHA-256("abc") = ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad
        let h = sha256_hash(b"abc");
        let expected = [
            0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae,
            0x22, 0x23, 0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61,
            0xf2, 0x00, 0x15, 0xad,
        ];
        assert_eq!(h.0, expected);
    }

    #[test]
    fn test_keccak256_known_vector() {
        // Keccak-256("") = c5d2460186f7233c927e7db2dcc703c0e500b653ca82273b7bfad8045d85a470
        let h = keccak256_hash(b"");
        let expected = [
            0xc5, 0xd2, 0x46, 0x01, 0x86, 0xf7, 0x23, 0x3c, 0x92, 0x7e, 0x7d, 0xb2, 0xdc, 0xc7,
            0x03, 0xc0, 0xe5, 0x00, 0xb6, 0x53, 0xca, 0x82, 0x27, 0x3b, 0x7b, 0xfa, 0xd8, 0x04,
            0x5d, 0x85, 0xa4, 0x70,
        ];
        assert_eq!(h.0, expected);
    }

    #[test]
    fn test_domain_separation_uniqueness() {
        let payload = Hash([7u8; 32]);
        let h1 = domain_separated_hash(DOMAIN_CONSENSUS_PREVOTE, 9999, 1, &payload);
        let h2 = domain_separated_hash(DOMAIN_CONSENSUS_PRECOMMIT, 9999, 1, &payload);
        let h3 = domain_separated_hash(DOMAIN_CONSENSUS_PREVOTE, 9998, 1, &payload); // Different chain
        assert_ne!(h1, h2);
        assert_ne!(h1, h3);
    }

    #[test]
    fn test_ed25519_sign_and_verify_roundtrip() {
        let keypair = Ed25519Keypair::generate();
        let message = b"TOLA Network BFT Prevote Height 400";
        let sig = keypair.sign(message);

        let pubkey_bytes = keypair.public_key_bytes();
        let sig_bytes = match sig {
            Signature::Ed25519(s) => s,
            _ => panic!("Wrong signature type"),
        };

        assert!(verify_ed25519(&pubkey_bytes, message, &sig_bytes).is_ok());

        // Tampered message must fail
        assert!(verify_ed25519(&pubkey_bytes, b"Tampered message", &sig_bytes).is_err());
    }

    #[test]
    fn test_ed25519_rfc8032_vector_1() {
        // RFC 8032 Section 7.1 Test Vector 1
        let secret = [
            0x9d, 0x61, 0xb1, 0x9d, 0xef, 0xfd, 0x5a, 0x60, 0xba, 0x84, 0x4a, 0xf4, 0x92, 0xec,
            0x2c, 0xc4, 0x44, 0x49, 0xc5, 0x69, 0x7b, 0x32, 0x69, 0x19, 0x70, 0x3b, 0xac, 0x03,
            0x1c, 0xae, 0x7f, 0x60,
        ];

        let keypair = Ed25519Keypair::from_seed(&secret);
        let expected_pk = [
            0xd7, 0x5a, 0x98, 0x01, 0x82, 0xb1, 0x0a, 0xb7, 0xd5, 0x4b, 0xfe, 0xd3, 0xc9, 0x64,
            0x07, 0x3a, 0x0e, 0xe1, 0x72, 0xf3, 0xda, 0xa6, 0x23, 0x25, 0xaf, 0x02, 0x1a, 0x68,
            0xf7, 0x07, 0x51, 0x1a,
        ];
        assert_eq!(keypair.public_key_bytes(), expected_pk);

        let sig = keypair.sign(b"");
        let sig_bytes = match sig {
            Signature::Ed25519(s) => s,
            _ => panic!(),
        };
        let expected_sig = [
            0xe5, 0x56, 0x43, 0x00, 0xc3, 0x60, 0xac, 0x72, 0x90, 0x86, 0xe2, 0xcc, 0x80, 0x6e,
            0x82, 0x8a, 0x84, 0x87, 0x7f, 0x1e, 0xb8, 0xe5, 0xd9, 0x74, 0xd8, 0x73, 0xe0, 0x65,
            0x22, 0x49, 0x01, 0x55, 0x5f, 0xb8, 0x82, 0x15, 0x90, 0xa3, 0x3b, 0xac, 0xc6, 0x1e,
            0x39, 0x70, 0x1c, 0xf9, 0xb4, 0x6b, 0xd2, 0x5b, 0xf5, 0xf0, 0x59, 0x5b, 0xbe, 0x24,
            0x65, 0x51, 0x41, 0x43, 0x8e, 0x7a, 0x10, 0x0b,
        ];
        assert_eq!(sig_bytes, expected_sig);
        assert!(verify_ed25519(&expected_pk, b"", &expected_sig).is_ok());
    }

    #[test]
    fn test_secp256k1_sign_and_recover_address() {
        let keypair = Secp256k1Keypair::generate();
        let expected_address = keypair.address();

        let digest = blake3_hash(b"TOLA Native Transfer: Alice -> Bob 500 nano-tola");
        let sig_65 = keypair.sign_recoverable(digest.as_bytes()).unwrap();

        // 1. Low-S check
        assert!(verify_secp256k1_low_s(&sig_65));

        // 2. Recover sender address
        let recovered_address = recover_secp256k1_address(digest.as_bytes(), &sig_65).unwrap();
        assert_eq!(recovered_address, expected_address);

        // 3. Altered digest fails recovery
        let altered_digest = blake3_hash(b"Tampered payload");
        let wrong_address = recover_secp256k1_address(altered_digest.as_bytes(), &sig_65).unwrap();
        assert_ne!(wrong_address, expected_address);
    }
    #[test]
    fn test_message_signing_and_address_recovery() {
        let keypair = Secp256k1Keypair::generate();
        let message = b"Hello TOLA Network Genesis 2026";
        let sig_65 = keypair.sign_message(message).expect("Sign message");

        let recovered = recover_message_address(message, &sig_65).expect("Recover address");
        assert_eq!(recovered, keypair.address());

        // Tampered message fails to recover same address
        let tampered = b"Hello TOLA Network Genesis 2027";
        let wrong = recover_message_address(tampered, &sig_65).expect("Recover address");
        assert_ne!(wrong, keypair.address());
    }

    #[test]
    fn test_secp256k1_debug_redaction() {
        let keypair = Secp256k1Keypair::generate();
        let debug_str = format!("{:?}", keypair);
        assert!(debug_str.contains("REDACTED"));
        let priv_hex = keypair
            .private_key_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        assert!(!debug_str.contains(&priv_hex));
    }

    #[test]
    fn test_cross_shard_receipt_hash_and_nullifier() {
        let sender = Address([0x11; 20]);
        let recipient = Address([0x22; 20]);

        let receipt1 = CrossShardReceipt {
            source_shard: ShardId(0),
            target_shard: ShardId(1),
            receipt_nonce: 100,
            sender,
            recipient,
            value: tola_types::Amount::from_u128(5000),
            data: vec![1, 2, 3],
            timeout_height: tola_types::BlockHeight(1000),
        };

        let hash1 = compute_receipt_hash(&receipt1);
        let nullifier1 = compute_receipt_nullifier(
            receipt1.source_shard,
            receipt1.target_shard,
            receipt1.receipt_nonce,
            &receipt1.sender,
        );

        assert_ne!(hash1, Hash([0; 32]));
        assert_ne!(nullifier1, Hash([0; 32]));

        // Mutating value changes hash, but nullifier stays the same
        let mut receipt2 = receipt1.clone();
        receipt2.value = tola_types::Amount::from_u128(9999);
        assert_ne!(compute_receipt_hash(&receipt2), hash1);
        assert_eq!(
            compute_receipt_nullifier(
                receipt2.source_shard,
                receipt2.target_shard,
                receipt2.receipt_nonce,
                &receipt2.sender
            ),
            nullifier1
        );
    }

    #[test]
    fn test_coordination_block_hash() {
        let coord = CoordinationBlock {
            height: tola_types::BlockHeight(50),
            parent_hash: Hash([0x11; 32]),
            epoch: tola_types::Epoch(2),
            vrf_seed: Hash([0x99; 32]),
            shard_roots: vec![
                tola_types::ShardRootEntry {
                    shard_id: ShardId(0),
                    shard_height: tola_types::BlockHeight(500),
                    state_root: Hash([0xaa; 32]),
                    receipts_root: Hash([0xbb; 32]),
                },
                tola_types::ShardRootEntry {
                    shard_id: ShardId(1),
                    shard_height: tola_types::BlockHeight(502),
                    state_root: Hash([0xcc; 32]),
                    receipts_root: Hash([0xdd; 32]),
                },
            ],
        };

        let hash = compute_coordination_block_hash(&coord);
        assert_ne!(hash, Hash([0; 32]));
    }

    #[test]
    fn test_merkle_tree_proof_generation_and_verification() {
        let leaves: Vec<Hash> = (0..8).map(|i| blake3_hash(&[i as u8])).collect();

        let root = compute_merkle_root(&leaves);

        for i in 0..leaves.len() {
            let proof = generate_merkle_proof(&leaves, i).expect("Proof exists");
            let valid = verify_merkle_inclusion_proof(&root, &leaves[i], &proof, i);
            assert!(valid, "Proof must verify for leaf {}", i);

            // Corrupted leaf fails verification
            let bad_leaf = blake3_hash(b"tampered");
            assert!(!verify_merkle_inclusion_proof(&root, &bad_leaf, &proof, i));

            // Wrong index fails verification
            let wrong_idx = (i + 1) % leaves.len();
            assert!(!verify_merkle_inclusion_proof(
                &root, &leaves[i], &proof, wrong_idx
            ));
        }
    }

    #[test]
    fn test_external_asset_attestation_and_canonical_id_security() {
        let chain_id = 9999u64;
        let nonce = 1u64;
        let dest_wallet = Address([0x11; 20]);
        let verifier_kp = Secp256k1Keypair::generate();
        let verifier_addr = verifier_kp.address();
        let expires_at = 2000000000u64;
        let now = 1700000000u64;

        let source = "PaxtaGram";
        let asset_type = ExternalAssetType::MessengerUsername;
        let ext_id = "@anvar";

        // 1. Canonical ID determinism
        let canon_id1 = compute_external_asset_canonical_id(source, asset_type, ext_id);
        let canon_id2 = compute_external_asset_canonical_id(source, asset_type, ext_id);
        assert_eq!(canon_id1, canon_id2);

        // Different asset type or platform gives different canonical ID
        let canon_id3 = compute_external_asset_canonical_id("Telegram", asset_type, ext_id);
        assert_ne!(canon_id1, canon_id3);

        // 2. Digest & signing
        let digest = compute_external_asset_attestation_digest(
            chain_id,
            nonce,
            &dest_wallet,
            expires_at,
            source,
            asset_type,
            ext_id,
        );
        let sig = verifier_kp.sign_recoverable(&digest).unwrap();

        // 3. Valid verification
        assert!(verify_external_asset_attestation(
            chain_id,
            nonce,
            &dest_wallet,
            expires_at,
            source,
            asset_type,
            ext_id,
            &verifier_addr,
            &sig,
            now,
        )
        .is_ok());

        // 4. Expired attestation
        assert_eq!(
            verify_external_asset_attestation(
                chain_id,
                nonce,
                &dest_wallet,
                expires_at,
                source,
                asset_type,
                ext_id,
                &verifier_addr,
                &sig,
                expires_at + 10,
            ),
            Err(CryptoError::AttestationExpired)
        );

        // 5. Cross-chain replay attempt (chain_id 102 instead of 9999)
        assert_eq!(
            verify_external_asset_attestation(
                102,
                nonce,
                &dest_wallet,
                expires_at,
                source,
                asset_type,
                ext_id,
                &verifier_addr,
                &sig,
                now,
            ),
            Err(CryptoError::SignerMismatch)
        );

        // 6. Cross-wallet replay attempt (different destination)
        let other_dest = Address([0x22; 20]);
        assert_eq!(
            verify_external_asset_attestation(
                chain_id,
                nonce,
                &other_dest,
                expires_at,
                source,
                asset_type,
                ext_id,
                &verifier_addr,
                &sig,
                now,
            ),
            Err(CryptoError::SignerMismatch)
        );

        // 7. Forged verifier (different expected verifier)
        let other_verifier = Address([0x33; 20]);
        assert_eq!(
            verify_external_asset_attestation(
                chain_id,
                nonce,
                &dest_wallet,
                expires_at,
                source,
                asset_type,
                ext_id,
                &other_verifier,
                &sig,
                now,
            ),
            Err(CryptoError::SignerMismatch)
        );
    }

    #[test]
    fn test_receipt_logs_bloom_matches_ethereum_reference() {
        let addr = Address([
            0x0f, 0x57, 0x2e, 0x52, 0x95, 0xc5, 0x7f, 0x15, 0x88, 0x6f, 0x9b, 0x26, 0x3e, 0x2f,
            0x6d, 0x2d, 0x6c, 0x7b, 0x5e, 0xc6,
        ]);
        let t0 = Hash([
            0xdd, 0xf2, 0x52, 0xad, 0x1b, 0xe2, 0xc8, 0x9b, 0x69, 0xc2, 0xb0, 0x68, 0xfc, 0x37,
            0x8d, 0xaa, 0x95, 0x2b, 0xa7, 0xf1, 0x63, 0xc4, 0xa1, 0x16, 0x28, 0xf5, 0x5a, 0x4d,
            0xf5, 0x23, 0xb3, 0xef,
        ]);
        let t1 = Hash([
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xa1, 0x08,
            0x7d, 0x32, 0xa7, 0x7b, 0x57, 0x83, 0x0b, 0x30, 0x80, 0xd0, 0x45, 0x96, 0xd3, 0x33,
            0x70, 0x7f, 0x71, 0x7f,
        ]);
        let t2 = Hash([
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x51, 0xe3,
            0xa4, 0x78, 0x17, 0x08, 0x8b, 0x9d, 0xd6, 0x32, 0xd4, 0xe8, 0xc6, 0xbe, 0xab, 0x96,
            0x4e, 0xbc, 0x45, 0x33,
        ]);

        let log = Log {
            address: addr,
            topics: vec![t0, t1, t2],
            data: vec![],
        };

        let calculated_bloom = compute_logs_bloom(&[log]);
        let calculated_hex: String = calculated_bloom
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect();

        let expected_golden_bloom = "00000000000000000000000000000040000000000000000000002000000000000000000000000000000000000000000000000000000000000000000000010000000000000000000000000008000000100000000000000000000008800000000000000000000000000000000000000000000000000000000000000010000000000000000000000000000000000000000000000000000000000000000000000000000000000000040000000000000000000000000000000000000000000000040000000002000000000000800000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000";

        assert_eq!(calculated_hex, expected_golden_bloom);
    }

    #[test]
    fn test_block_logs_bloom_matches_union_of_receipt_blooms() {
        let addr1 = Address([0x11; 20]);
        let addr2 = Address([0x22; 20]);
        let log1 = Log {
            address: addr1,
            topics: vec![Hash([0x33; 32])],
            data: vec![],
        };
        let log2 = Log {
            address: addr2,
            topics: vec![Hash([0x44; 32])],
            data: vec![],
        };

        let receipt1 = Receipt {
            tx_id: tola_types::TransactionId(Hash([0x01; 32])),
            success: true,
            gas_used: tola_types::Gas(21_000),
            cumulative_gas_used: tola_types::Gas(21_000),
            logs: vec![log1],
            return_data: vec![],
        };
        let receipt2 = Receipt {
            tx_id: tola_types::TransactionId(Hash([0x02; 32])),
            success: true,
            gas_used: tola_types::Gas(21_000),
            cumulative_gas_used: tola_types::Gas(42_000),
            logs: vec![log2],
            return_data: vec![],
        };

        let receipts = vec![receipt1.clone(), receipt2.clone()];
        let block_bloom = compute_block_logs_bloom(&receipts);
        let b1 = compute_logs_bloom(&receipt1.logs);
        let b2 = compute_logs_bloom(&receipt2.logs);

        for i in 0..256 {
            assert_eq!(block_bloom[i], b1[i] | b2[i]);
        }
    }
}
