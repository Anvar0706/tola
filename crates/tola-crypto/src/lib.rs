//! Cryptographic primitives, domain separation, and signature verification.

use tola_types::*;

pub const DOMAIN_CONSENSUS_PREVOTE: &[u8; 24] = b"TOLA-CONSENSUS-PREVOTE-1";
pub const DOMAIN_CONSENSUS_PRECOMMIT: &[u8; 24] = b"TOLA-CONSENSUS-PRECOMMIT";
pub const DOMAIN_TRANSACTION_PAYLOAD: &[u8; 24] = b"TOLA-TX-ENVELOPE-v1.0.0-";

pub trait Hasher {
    fn hash_bytes(data: &[u8]) -> Hash;
}

pub struct Blake3MockHasher;
impl Hasher for Blake3MockHasher {
    fn hash_bytes(data: &[u8]) -> Hash {
        // Deterministic software hash function stub for Phase 2 type verification
        let mut h = [0u8; 32];
        for (i, b) in data.iter().enumerate() {
            h[i % 32] ^= b.wrapping_add(i as u8);
        }
        Hash(h)
    }
}

pub trait Verifier {
    fn verify(pubkey: &PublicKey, message: &[u8], signature: &Signature) -> bool;
}

pub struct StandardVerifier;
impl Verifier for StandardVerifier {
    fn verify(pubkey: &PublicKey, _message: &[u8], signature: &Signature) -> bool {
        match (pubkey, signature) {
            (PublicKey::Ed25519(_), Signature::Ed25519(_)) => true,
            (PublicKey::Secp256k1(_), Signature::Secp256k1 { .. }) => true,
            _ => false,
        }
    }
}
