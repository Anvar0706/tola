//! Production BIP-39 Mnemonic and Hierarchical Deterministic (HD) Key Derivation.
//!
//! Provides cryptographically secure 12- and 24-word mnemonic generation, checksum validation,
//! PBKDF2-HMAC-SHA512 seed derivation (2048 iterations), and BIP-44 Secp256k1 child key derivation.
//!
//! All secret buffers (entropy, mnemonics, seeds) implement zeroization on drop and redact
//! Debug output to prevent accidental memory or log exposure.

use core::fmt;
use core::str::FromStr;
use tola_types::Network;
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::{CryptoError, Secp256k1Keypair};

// ============================================================================
// 1. Derivation Path Constants (BIP-44)
// ============================================================================

/// Canonical TOLA Wallet Derivation Path v1.0 (BIP-44 Coin Type 9999)
///
/// Used uniformly across Mainnet, Testnet, and Devnet to preserve cryptographic
/// identity across network environments. Replay protection is enforced via Chain ID
/// in the transaction payload digest, not via path mutation.
pub const CANONICAL_DERIVATION_PATH: &str = "m/44'/9999'/0'/0/0";

/// Canonical path aliases (all point to the unified v1.0 standard path)
pub const DEFAULT_MAINNET_PATH: &str = CANONICAL_DERIVATION_PATH;
pub const DEFAULT_TESTNET_PATH: &str = CANONICAL_DERIVATION_PATH;
pub const DEFAULT_DEVNET_PATH: &str = CANONICAL_DERIVATION_PATH;

/// Deprecated provisional testnet path from early prototyping (ADR-0013 draft).
/// Retained strictly for legacy migration verification.
pub const LEGACY_TESTNET_PROVISIONAL_PATH: &str = "m/44'/102'/0'/0/0";

/// Return canonical derivation path string for target network.
///
/// Under TOLA Wallet Derivation Standard v1.0, this returns `m/44'/9999'/0'/0/0`
/// for all networks (Mainnet, Testnet, Devnet), ensuring consistent account identity.
pub fn canonical_path_for_network(_network: &Network) -> &'static str {
    CANONICAL_DERIVATION_PATH
}

/// Format a BIP-44 derivation path string: m/44'/{coin_type}'/{account}'/{change}/{index}
pub fn format_bip44_path(coin_type: u32, account: u32, change: u32, index: u32) -> String {
    format!("m/44'/{coin_type}'/{account}'/{change}/{index}")
}

// ============================================================================
// 2. Mnemonic Count & Secret Mnemonic Container
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MnemonicCount {
    Words12,
    Words24,
}

impl MnemonicCount {
    pub fn word_count(&self) -> usize {
        match self {
            MnemonicCount::Words12 => 12,
            MnemonicCount::Words24 => 24,
        }
    }

    pub fn entropy_bytes_len(&self) -> usize {
        match self {
            MnemonicCount::Words12 => 16, // 128 bits
            MnemonicCount::Words24 => 32, // 256 bits
        }
    }
}

/// A BIP-39 mnemonic phrase guarded with zeroization and redacted Debug output.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct Mnemonic {
    phrase: String,
}

impl fmt::Debug for Mnemonic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Mnemonic([REDACTED])")
    }
}

impl Mnemonic {
    /// Generate a fresh random mnemonic from system CSPRNG.
    pub fn generate(count: MnemonicCount) -> Result<Self, CryptoError> {
        use rand_core::{OsRng, RngCore};
        let mut entropy = vec![0u8; count.entropy_bytes_len()];
        OsRng.fill_bytes(&mut entropy);

        let bip39_m = bip39::Mnemonic::from_entropy_in(bip39::Language::English, &entropy)
            .map_err(|e| CryptoError::InvalidMnemonic(e.to_string()))?;

        // Zeroize raw entropy buffer immediately
        entropy.zeroize();

        Ok(Self {
            phrase: bip39_m.to_string(),
        })
    }

    /// Construct a mnemonic from an existing phrase string, validating wordlist and checksum.
    pub fn from_phrase(phrase: &str) -> Result<Self, CryptoError> {
        let normalized = phrase
            .trim()
            .to_lowercase()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        let bip39_m = bip39::Mnemonic::parse_in_normalized(bip39::Language::English, &normalized)
            .map_err(|e| CryptoError::InvalidMnemonic(e.to_string()))?;

        let word_count = normalized.split_whitespace().count();
        if word_count != 12 && word_count != 24 {
            return Err(CryptoError::InvalidMnemonic(format!(
                "Unsupported word count: {word_count}. Expected 12 or 24 words."
            )));
        }

        Ok(Self {
            phrase: bip39_m.to_string(),
        })
    }

    /// Return reference to normalized mnemonic phrase string.
    /// CAUTION: Only expose this for explicit user-facing offline backup verification!
    pub fn phrase(&self) -> &str {
        &self.phrase
    }

    /// Return word count of the mnemonic.
    pub fn word_count(&self) -> usize {
        self.phrase.split_whitespace().count()
    }

    /// Derive 64-byte master seed using PBKDF2-HMAC-SHA512 (2048 iterations).
    pub fn to_seed(&self, passphrase: &str) -> Seed {
        let bip39_m = bip39::Mnemonic::parse_in_normalized(bip39::Language::English, &self.phrase)
            .expect("Mnemonic phrase invariant holds");
        let seed_bytes = bip39_m.to_seed(passphrase);
        Seed::from_bytes(seed_bytes)
    }
}

// ============================================================================
// 3. Cryptographic Master Seed Container
// ============================================================================

/// 512-bit (64-byte) cryptographic master seed with zeroization on drop.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct Seed {
    bytes: [u8; 64],
}

impl fmt::Debug for Seed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Seed([REDACTED])")
    }
}

impl Seed {
    pub fn from_bytes(bytes: [u8; 64]) -> Self {
        Self { bytes }
    }

    pub fn as_bytes(&self) -> &[u8; 64] {
        &self.bytes
    }

    /// Derive a child Secp256k1 keypair along a BIP-32 / BIP-44 path string.
    pub fn derive_secp256k1_keypair(
        &self,
        path_str: &str,
    ) -> Result<Secp256k1Keypair, CryptoError> {
        let path = bip32::DerivationPath::from_str(path_str)
            .map_err(|e| CryptoError::DerivationFailed(format!("Invalid derivation path: {e}")))?;

        let xprv = bip32::XPrv::derive_from_path(self.bytes, &path)
            .map_err(|e| CryptoError::DerivationFailed(format!("Key derivation error: {e}")))?;

        let mut scalar_bytes = [0u8; 32];
        scalar_bytes.copy_from_slice(&xprv.private_key().to_bytes());
        Secp256k1Keypair::from_private_key_bytes(&scalar_bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tola_types::Address;

    fn hex_encode(bytes: &[u8]) -> String {
        let mut s = String::with_capacity(bytes.len() * 2);
        for b in bytes {
            s.push_str(&format!("{b:02x}"));
        }
        s
    }

    #[test]
    fn test_bip39_official_test_vector_1_seed() {
        let phrase = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
        let mnemonic = Mnemonic::from_phrase(phrase).expect("Valid phrase");

        // Passphrase "TREZOR"
        let seed_trezor = mnemonic.to_seed("TREZOR");
        assert_eq!(
            hex_encode(seed_trezor.as_bytes()),
            "c55257c360c07c72029aebc1b53c05ed0362ada38ead3e3e9efa3708e53495531f09a6987599d18264c1e1c92f2cf141630c7a3c4ab7c81b2f001698e7463b04"
        );

        // Empty passphrase ""
        let seed_empty = mnemonic.to_seed("");
        assert_eq!(
            hex_encode(seed_empty.as_bytes()),
            "5eb00bbddcf069084889a8ab9155568165f5c453ccb85e70811aaed6f6da5fc19a5ac40b389cd370d086206dec8aa6c43daea6690f20ad3d8d48b2d2ce9e38e4"
        );
    }

    #[test]
    fn test_mnemonic_generation_12_and_24_words() {
        let m12 = Mnemonic::generate(MnemonicCount::Words12).expect("Generate 12 words");
        assert_eq!(m12.word_count(), 12);
        // Validating round-trip
        let re_parsed12 = Mnemonic::from_phrase(m12.phrase()).expect("Parse generated 12 words");
        assert_eq!(re_parsed12.phrase(), m12.phrase());

        let m24 = Mnemonic::generate(MnemonicCount::Words24).expect("Generate 24 words");
        assert_eq!(m24.word_count(), 24);
        let re_parsed24 = Mnemonic::from_phrase(m24.phrase()).expect("Parse generated 24 words");
        assert_eq!(re_parsed24.phrase(), m24.phrase());
    }

    #[test]
    fn test_mnemonic_validation_and_rejection() {
        // Valid
        let valid = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
        assert!(Mnemonic::from_phrase(valid).is_ok());

        // Normalization: leading, trailing, extra whitespace, mixed case
        let messy = "  ABANDON   abandon   abandon abandon abandon abandon abandon abandon abandon abandon abandon about  ";
        let parsed = Mnemonic::from_phrase(messy).expect("Should normalize cleanly");
        assert_eq!(parsed.phrase(), valid);

        // Invalid checksum (last word altered to 'abandon' which fails checksum for 12 words)
        let invalid_checksum = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon";
        let err = Mnemonic::from_phrase(invalid_checksum).unwrap_err();
        match err {
            CryptoError::InvalidMnemonic(_) => {}
            other => panic!("Expected InvalidMnemonic error, got: {:?}", other),
        }

        // Invalid word not in wordlist
        let invalid_word = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon xyzfoobar";
        assert!(Mnemonic::from_phrase(invalid_word).is_err());

        // Wrong word count (e.g. 11 words)
        let count_11 = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon";
        assert!(Mnemonic::from_phrase(count_11).is_err());

        // 15 words
        let count_15 = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
        assert!(Mnemonic::from_phrase(count_15).is_err());
    }

    #[test]
    fn test_canonical_derivation_determinism() {
        let phrase = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
        let m1 = Mnemonic::from_phrase(phrase).unwrap();
        let m2 = Mnemonic::from_phrase(phrase).unwrap();

        let s1 = m1.to_seed("");
        let s2 = m2.to_seed("");
        assert_eq!(s1.as_bytes(), s2.as_bytes());

        // Derivation under Canonical v1.0 Path: m/44'/9999'/0'/0/0
        let kp1 = s1
            .derive_secp256k1_keypair(CANONICAL_DERIVATION_PATH)
            .unwrap();
        let kp2 = s2
            .derive_secp256k1_keypair(CANONICAL_DERIVATION_PATH)
            .unwrap();
        assert_eq!(kp1.address(), kp2.address());
        assert_eq!(kp1.public_key_bytes(), kp2.public_key_bytes());
        assert_eq!(kp1.private_key_bytes(), kp2.private_key_bytes());
        assert_ne!(kp1.address(), Address::ZERO);

        // Model A: All networks map to canonical path
        assert_eq!(
            canonical_path_for_network(&Network::Mainnet),
            CANONICAL_DERIVATION_PATH
        );
        assert_eq!(
            canonical_path_for_network(&Network::Testnet),
            CANONICAL_DERIVATION_PATH
        );
        assert_eq!(
            canonical_path_for_network(&Network::Devnet),
            CANONICAL_DERIVATION_PATH
        );

        // Explicit legacy path verification
        let kp_legacy = s1
            .derive_secp256k1_keypair(LEGACY_TESTNET_PROVISIONAL_PATH)
            .unwrap();
        assert_ne!(kp1.address(), kp_legacy.address());
    }

    #[test]
    fn test_different_mnemonics_yield_different_wallets() {
        let m1 = Mnemonic::generate(MnemonicCount::Words12).unwrap();
        let m2 = Mnemonic::generate(MnemonicCount::Words12).unwrap();
        assert_ne!(m1.phrase(), m2.phrase());

        let kp1 = m1
            .to_seed("")
            .derive_secp256k1_keypair(CANONICAL_DERIVATION_PATH)
            .unwrap();
        let kp2 = m2
            .to_seed("")
            .derive_secp256k1_keypair(CANONICAL_DERIVATION_PATH)
            .unwrap();
        assert_ne!(kp1.address(), kp2.address());
    }

    #[test]
    fn test_secret_redaction_and_memory_safety() {
        let m = Mnemonic::from_phrase("abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about").unwrap();
        let debug_m = format!("{:?}", m);
        assert_eq!(debug_m, "Mnemonic([REDACTED])");
        assert!(!debug_m.contains("abandon"));

        let s = m.to_seed("");
        let debug_s = format!("{:?}", s);
        assert_eq!(debug_s, "Seed([REDACTED])");
    }

    #[test]
    fn test_canonical_test_vector_outputs() {
        let phrase = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
        let mnemonic = Mnemonic::from_phrase(phrase).unwrap();
        let seed = mnemonic.to_seed("");

        // Canonical Path v1.0: m/44'/9999'/0'/0/0
        let kp = seed
            .derive_secp256k1_keypair(CANONICAL_DERIVATION_PATH)
            .unwrap();
        let addr = kp.address();
        let pubkey = kp.public_key_bytes();
        let uncomp_pubkey = kp.uncompressed_public_key_bytes();
        let privkey = kp.private_key_bytes();
        let bech32_mainnet = addr.to_bech32m(&Network::Mainnet);
        let bech32_testnet = addr.to_bech32m(&Network::Testnet);
        let bech32_devnet = addr.to_bech32m(&Network::Devnet);

        // Compute fingerprint: SHA-256 of private key bytes (first 4 bytes hex)
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(privkey);
        let privkey_sha256 = hasher.finalize();
        let privkey_fp = hex_encode(&privkey_sha256[..4]);

        assert_eq!(privkey_fp, "0cc22062");
        assert_eq!(
            hex_encode(&privkey),
            "f77999a4d727d792213ae012f18621208ea91a7246e5bf69a4317942abeca750"
        );
        assert_eq!(
            hex_encode(&pubkey),
            "03e95745eb5f028b4e23fb4c5af97fb49426d522c4228fe6dd922f2585e6b1bf96"
        );
        assert_eq!(
            hex_encode(&uncomp_pubkey),
            "04e95745eb5f028b4e23fb4c5af97fb49426d522c4228fe6dd922f2585e6b1bf9689629921a91231749eaf52b0198c172ea8a2cce8ee2c9f62f8a6c2fed81c217f"
        );
        assert_eq!(addr.to_hex(), "0x3b45664e195dbacd0035046b511491e09bd3ca8a");
        assert_eq!(
            bech32_mainnet,
            "tola18dzkvnsetkav6qp4q344z9y3uzda8j5263ez6j"
        );
        assert_eq!(
            bech32_testnet,
            "ttola18dzkvnsetkav6qp4q344z9y3uzda8j527xgjrh"
        );
        assert_eq!(
            bech32_devnet,
            "dtola18dzkvnsetkav6qp4q344z9y3uzda8j52x0hhl4"
        );

        // Legacy Provisional Path: m/44'/102'/0'/0/0 (Strictly for backward verification)
        let kp_legacy = seed
            .derive_secp256k1_keypair(LEGACY_TESTNET_PROVISIONAL_PATH)
            .unwrap();
        assert_eq!(
            kp_legacy.address().to_hex(),
            "0x8e3719da26b9408f27fa9602c4362d9fbc49dc73"
        );
        assert_eq!(
            kp_legacy.address().to_bech32m(&Network::Testnet),
            "ttola13cm3nk3xh9qg7fl6jcpvgd3dn77ynhrnukw78v"
        );
        assert_eq!(
            hex_encode(&kp_legacy.public_key_bytes()),
            "02b74bcb92d52c5cfe20ece37e6cc6ae0b12dd58fc328c509e8f904dfc62977403"
        );

        // Canonical Vector 2 (24 words)
        let phrase24 = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon art";
        let m24 = Mnemonic::from_phrase(phrase24).expect("Valid 24-word phrase");
        let s24 = m24.to_seed("");
        assert_eq!(
            hex_encode(s24.as_bytes()),
            "408b285c123836004f4b8842c89324c1f01382450c0d439af345ba7fc49acf705489c6fc77dbd4e3dc1dd8cc6bc9f043db8ada1e243c4a0eafb290d399480840"
        );
        let kp24_canonical = s24
            .derive_secp256k1_keypair(CANONICAL_DERIVATION_PATH)
            .unwrap();
        assert_eq!(
            kp24_canonical.address().to_hex(),
            "0x37061e043e1b89972ed615c46ff3e5090514c909"
        );
        assert_eq!(
            kp24_canonical.address().to_bech32m(&Network::Mainnet),
            "tola1xurpupp7rwyewtkkzhzxlul9pyz3fjgf4wludn"
        );
        assert_eq!(
            hex_encode(&kp24_canonical.public_key_bytes()),
            "03d20a0a3b709e063b279a5810d91b922c50b5b62ba09ef16b6c3916f1e5d66e3f"
        );

        let kp24_legacy = s24
            .derive_secp256k1_keypair(LEGACY_TESTNET_PROVISIONAL_PATH)
            .unwrap();
        assert_eq!(
            kp24_legacy.address().to_hex(),
            "0x1073f501701b4a14dd4bc6a1f1746fca1fb8de64"
        );
    }
}
