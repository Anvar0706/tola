use tola_crypto::{
    canonical_path_for_network, Mnemonic, MnemonicCount, Secp256k1Keypair, Seed,
    CANONICAL_DERIVATION_PATH, LEGACY_TESTNET_PROVISIONAL_PATH,
};
use tola_types::{Address, Hash, Network, PublicKey, Signature, Transaction};

#[derive(Clone)]
pub struct TolaWallet {
    keypair: Secp256k1Keypair,
}

impl TolaWallet {
    /// Generate a fresh random wallet using system entropy.
    pub fn generate() -> Self {
        Self {
            keypair: Secp256k1Keypair::generate(),
        }
    }

    /// Generate a fresh random wallet accompanied by its recovery mnemonic phrase (BIP-39 -> m/44'/9999'/0'/0/0).
    pub fn generate_with_mnemonic(count: MnemonicCount) -> Result<(Self, Mnemonic), String> {
        let mnemonic = Mnemonic::generate(count)
            .map_err(|e| format!("Mnemonic generation failed: {:?}", e))?;
        let seed = mnemonic.to_seed("");
        let keypair = seed
            .derive_secp256k1_keypair(CANONICAL_DERIVATION_PATH)
            .map_err(|e| format!("Key derivation failed: {:?}", e))?;
        Ok((Self { keypair }, mnemonic))
    }

    /// Construct a wallet from a BIP-39 mnemonic phrase using the canonical v1 derivation path.
    ///
    /// Under TOLA Derivation Standard v1.0, the same key is derived across all networks (Mainnet, Testnet, Devnet).
    pub fn from_mnemonic(
        phrase: &str,
        passphrase: &str,
        network: &Network,
    ) -> Result<Self, String> {
        let path = canonical_path_for_network(network);
        Self::from_mnemonic_with_path(phrase, passphrase, path)
    }

    /// Construct a wallet from a BIP-39 mnemonic using the canonical v1 path: `m/44'/9999'/0'/0/0`.
    pub fn from_mnemonic_canonical(phrase: &str, passphrase: &str) -> Result<Self, String> {
        Self::from_mnemonic_with_path(phrase, passphrase, CANONICAL_DERIVATION_PATH)
    }

    /// Recover a wallet from a BIP-39 mnemonic phrase using canonical path `m/44'/9999'/0'/0/0`.
    pub fn recover(phrase: &str, passphrase: &str) -> Result<Self, String> {
        Self::from_mnemonic_canonical(phrase, passphrase)
    }

    /// Construct a wallet from a BIP-39 mnemonic phrase using an explicit derivation path.
    pub fn from_mnemonic_with_path(
        phrase: &str,
        passphrase: &str,
        path: &str,
    ) -> Result<Self, String> {
        let mnemonic =
            Mnemonic::from_phrase(phrase).map_err(|e| format!("Invalid mnemonic: {:?}", e))?;
        let seed = mnemonic.to_seed(passphrase);
        Self::from_seed(&seed, path)
    }

    /// Construct a wallet using the deprecated legacy testnet path (`m/44'/102'/0'/0/0`) for migration verification.
    pub fn from_mnemonic_legacy_testnet(phrase: &str, passphrase: &str) -> Result<Self, String> {
        Self::from_mnemonic_with_path(phrase, passphrase, LEGACY_TESTNET_PROVISIONAL_PATH)
    }

    /// Construct a wallet from a pre-computed master seed and explicit derivation path.
    pub fn from_seed(seed: &Seed, path: &str) -> Result<Self, String> {
        let keypair = seed
            .derive_secp256k1_keypair(path)
            .map_err(|e| format!("Key derivation failed: {:?}", e))?;
        Ok(Self { keypair })
    }

    /// Construct a wallet from a 32-byte private key scalar.
    pub fn from_private_key_bytes(bytes: &[u8; 32]) -> Result<Self, String> {
        let keypair = Secp256k1Keypair::from_private_key_bytes(bytes)
            .map_err(|e| format!("Invalid private key: {:?}", e))?;
        Ok(Self { keypair })
    }

    /// Construct a wallet from a 32-byte private key scalar (alias).
    pub fn from_private_key(bytes: &[u8; 32]) -> Result<Self, String> {
        Self::from_private_key_bytes(bytes)
    }

    /// Return 32-byte private key scalar.
    pub fn private_key_bytes(&self) -> [u8; 32] {
        self.keypair.private_key_bytes()
    }

    /// Return 32-byte private key scalar (alias).
    pub fn private_key(&self) -> [u8; 32] {
        self.private_key_bytes()
    }

    /// Return reference to underlying Secp256k1Keypair.
    pub fn keypair(&self) -> &Secp256k1Keypair {
        &self.keypair
    }

    /// Return 65-byte uncompressed Secp256k1 public key (0x04 || X || Y).
    pub fn uncompressed_public_key_bytes(&self) -> [u8; 65] {
        self.keypair.uncompressed_public_key_bytes()
    }

    /// Construct a wallet from a hex string (with or without '0x' prefix).
    pub fn from_hex_private_key(hex_str: &str) -> Result<Self, String> {
        let clean = hex_str
            .strip_prefix("0x")
            .or_else(|| hex_str.strip_prefix("0X"))
            .unwrap_or(hex_str);
        if clean.len() != 64 {
            return Err("Private key hex must be 64 characters long".into());
        }
        let mut bytes = [0u8; 32];
        for i in 0..32 {
            bytes[i] = u8::from_str_radix(&clean[i * 2..i * 2 + 2], 16)
                .map_err(|e| format!("Hex parse error: {}", e))?;
        }
        Self::from_private_key_bytes(&bytes)
    }

    /// Return the 20-byte canonical address.
    pub fn address(&self) -> Address {
        self.keypair.address()
    }

    /// Return the address in Bech32m checksummed format (e.g. `ttola1...` or `tola1...`).
    pub fn address_bech32m(&self, network: &Network) -> String {
        self.address().to_bech32m(network)
    }

    /// Return 33-byte compressed Secp256k1 public key.
    pub fn public_key_bytes(&self) -> [u8; 33] {
        self.keypair.public_key_bytes()
    }

    /// Return public key as canonical PublicKey enum.
    pub fn public_key(&self) -> PublicKey {
        PublicKey::Secp256k1(self.public_key_bytes())
    }

    /// Sign a 32-byte digest returning canonical `Signature::Secp256k1`.
    pub fn sign_digest(&self, digest: &[u8; 32]) -> Result<Signature, String> {
        let sig_65 = self
            .keypair
            .sign_recoverable(digest)
            .map_err(|e| format!("Signing error: {:?}", e))?;

        let mut r = [0u8; 32];
        let mut s = [0u8; 32];
        r.copy_from_slice(&sig_65[0..32]);
        s.copy_from_slice(&sig_65[32..64]);
        let v = sig_65[64];

        Ok(Signature::Secp256k1 { r, s, v })
    }

    /// Signs an in-place transaction payload with the wallet's private key.
    pub fn sign_transaction(&self, tx: &mut Transaction) -> Result<(), String> {
        tx.from = self.address();
        let digest = compute_transaction_digest(tx);
        let sig = self.sign_digest(digest.as_bytes())?;
        tx.signature = sig;
        Ok(())
    }

    /// Sign an arbitrary text or binary message with standard prefix:
    /// `\x19TOLA Signed Message:\n<length><message>`
    pub fn sign_message(&self, message: &[u8]) -> Result<[u8; 65], String> {
        self.keypair
            .sign_message(message)
            .map_err(|e| format!("Message signing error: {:?}", e))
    }

    /// Sign an arbitrary message returning 0x-prefixed 130-hex-character string.
    pub fn sign_message_hex(&self, message: &[u8]) -> Result<String, String> {
        let sig = self.sign_message(message)?;
        let mut s = String::with_capacity(130 + 2);
        s.push_str("0x");
        for b in sig {
            s.push_str(&format!("{b:02x}"));
        }
        Ok(s)
    }

    /// Recover signer 20-byte address from message and 65-byte signature.
    pub fn recover_message_address(
        message: &[u8],
        signature_65: &[u8; 65],
    ) -> Result<Address, String> {
        tola_crypto::recover_message_address(message, signature_65)
            .map_err(|e| format!("Recovery error: {:?}", e))
    }
}

impl core::fmt::Debug for TolaWallet {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "TolaWallet([REDACTED: {}])", self.address().to_hex())
    }
}

/// Compute canonical transaction signing digest matching protocol mempool verification.
pub fn compute_transaction_digest(tx: &Transaction) -> Hash {
    let mut unsigned_bytes = Vec::new();
    tola_codec::serialize_unsigned_transaction(tx, &mut unsigned_bytes);
    let payload_hash = tola_crypto::blake3_hash(&unsigned_bytes);
    tola_crypto::domain_separated_hash(
        tola_crypto::DOMAIN_TRANSACTION_PAYLOAD,
        tx.chain_id.0,
        0,
        &payload_hash,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tola_codec::CanonicalSerialize;
    use tola_crypto::recover_secp256k1_address;
    use tola_types::*;

    #[test]
    fn test_wallet_generation_and_address_derivation() {
        let wallet = TolaWallet::generate();
        let addr = wallet.address();
        assert_ne!(addr, Address::ZERO);

        let bech32_testnet = wallet.address_bech32m(&Network::Testnet);
        assert!(bech32_testnet.starts_with("ttola1"));

        let (net, parsed_addr) = Address::from_bech32m(&bech32_testnet).expect("Parse bech32m");
        assert_eq!(net, Network::Testnet);
        assert_eq!(parsed_addr, addr);
    }

    #[test]
    fn test_wallet_generate_with_mnemonic_roundtrip() {
        // 12 Words
        let (wallet12, mnemonic12) =
            TolaWallet::generate_with_mnemonic(MnemonicCount::Words12).unwrap();
        assert_eq!(mnemonic12.word_count(), 12);
        let restored12 =
            TolaWallet::from_mnemonic(mnemonic12.phrase(), "", &Network::Mainnet).unwrap();
        assert_eq!(wallet12.address(), restored12.address());
        assert_eq!(wallet12.public_key_bytes(), restored12.public_key_bytes());

        // 24 Words
        let (wallet24, mnemonic24) =
            TolaWallet::generate_with_mnemonic(MnemonicCount::Words24).unwrap();
        assert_eq!(mnemonic24.word_count(), 24);
        let restored24 =
            TolaWallet::from_mnemonic(mnemonic24.phrase(), "", &Network::Mainnet).unwrap();
        assert_eq!(wallet24.address(), restored24.address());
        assert_eq!(wallet24.public_key_bytes(), restored24.public_key_bytes());
    }

    #[test]
    fn test_wallet_mnemonic_sign_and_verify_transaction() {
        let phrase = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
        let wallet = TolaWallet::from_mnemonic(phrase, "", &Network::Mainnet).unwrap();
        let addr = wallet.address();

        let mut tx = Transaction {
            chain_id: ChainId(9999),
            nonce: Nonce(42),
            from: addr,
            to: Some(Address([0x88; 20])),
            value: Amount(500_000_000_000_000_000), // 0.5 TOLA
            gas_limit: Gas(21_000),
            max_fee_per_gas: Amount(1_000_000_000),
            max_priority_fee_per_gas: Amount(1_000_000_000),
            valid_until_block: BlockHeight(200),
            data: vec![],
            memo: None,
            signature: Signature::Secp256k1 {
                r: [0u8; 32],
                s: [0u8; 32],
                v: 0,
            },
            tx_type: 0,
            access_list: vec![],
        };

        wallet.sign_transaction(&mut tx).expect("Sign transaction");

        // Verify that signature recovers back to wallet.address()
        let digest = compute_transaction_digest(&tx);
        match tx.signature {
            Signature::Secp256k1 { r, s, v } => {
                let mut sig_65 = [0u8; 65];
                sig_65[0..32].copy_from_slice(&r);
                sig_65[32..64].copy_from_slice(&s);
                sig_65[64] = v;
                let recovered = recover_secp256k1_address(digest.as_bytes(), &sig_65)
                    .expect("Address recovery succeeds");
                assert_eq!(recovered, addr);
                assert_eq!(recovered, tx.from);
            }
            _ => panic!("Expected Secp256k1 signature"),
        }
    }

    #[test]
    fn test_wallet_hex_import_roundtrip() {
        let _original = TolaWallet::generate();
        let priv_hex = hex_encode(&[0x42u8; 32]);
        let imported = TolaWallet::from_hex_private_key(&priv_hex).expect("Import hex");
        assert_ne!(imported.address(), Address::ZERO);
    }

    #[test]
    fn test_canonical_vector_values() {
        let priv_bytes = [0x42u8; 32];
        let wallet = TolaWallet::from_private_key_bytes(&priv_bytes).unwrap();
        let addr = wallet.address();
        assert_eq!(
            hex_encode(&wallet.public_key_bytes()),
            "0324653eac434488002cc06bbfb7f10fe18991e35f9fe4302dbea6d2353dc0ab1c"
        );
        assert_eq!(addr.to_hex(), "0x17c5185167401ed00cf5f5b2fc97d9bbfdb7d025");
        assert_eq!(
            wallet.address_bech32m(&Network::Mainnet),
            "tola1zlz3s5t8gq0dqr847ke0e97eh07m05p9t9nfc9"
        );
        assert_eq!(
            wallet.address_bech32m(&Network::Testnet),
            "ttola1zlz3s5t8gq0dqr847ke0e97eh07m05p90jzepq"
        );

        let mut tx = Transaction {
            chain_id: ChainId(102),
            nonce: Nonce(1),
            from: addr,
            to: Some(addr),
            value: Amount(1_000_000_000_000_000_000),
            gas_limit: Gas(21_000),
            max_fee_per_gas: Amount(1_000_000_000),
            max_priority_fee_per_gas: Amount(1_000_000_000),
            valid_until_block: BlockHeight(100),
            data: vec![0xaa, 0xbb],
            memo: None,
            signature: Signature::Secp256k1 {
                r: [0u8; 32],
                s: [0u8; 32],
                v: 0,
            },
            tx_type: 0,
            access_list: vec![],
        };
        let mut unsigned_bytes = Vec::new();
        tola_codec::serialize_unsigned_transaction(&tx, &mut unsigned_bytes);
        assert_eq!(hex_encode(&unsigned_bytes), "6600000000000000010000000000000017c5185167401ed00cf5f5b2fc97d9bbfdb7d0250117c5185167401ed00cf5f5b2fc97d9bbfdb7d025000064a7b3b6e00d0000000000000000085200000000000000ca9a3b00000000000000000000000000ca9a3b000000000000000000000000640000000000000002000000aabb");

        let payload_hash = tola_crypto::blake3_hash(&unsigned_bytes);
        assert_eq!(
            payload_hash.to_hex(),
            "0xb3e845a99a136c2eab8250dc6deef7dfe47f3a8545c365c68d202dc5ecff2451"
        );

        let digest = compute_transaction_digest(&tx);
        assert_eq!(
            digest.to_hex(),
            "0x32f94669e75eaf7c3d4d68388eb100c9c7884a86e1d16af2ee5c7e808281b830"
        );

        wallet.sign_transaction(&mut tx).unwrap();
        let signed_wire = tx.to_canonical_bytes();
        assert!(signed_wire.starts_with(&unsigned_bytes));
        assert_eq!(signed_wire.len(), unsigned_bytes.len() + 1 + 32 + 32 + 1);
    }

    #[test]
    fn test_wallet_message_signing_and_verification() {
        let phrase = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
        let wallet = TolaWallet::recover(phrase, "").expect("Recover wallet");
        let msg = b"Hello TOLA Network 2026";

        let sig = wallet.sign_message(msg).expect("Sign message");
        let recovered = TolaWallet::recover_message_address(msg, &sig).expect("Recover address");
        assert_eq!(recovered, wallet.address());

        let sig_hex = wallet.sign_message_hex(msg).expect("Sign hex");
        assert!(sig_hex.starts_with("0x"));
        assert_eq!(sig_hex.len(), 2 + 130);
    }

    #[test]
    fn test_wallet_debug_redaction_and_aliases() {
        let priv_bytes = [0x55u8; 32];
        let wallet = TolaWallet::from_private_key(&priv_bytes).unwrap();
        assert_eq!(wallet.private_key(), priv_bytes);

        let debug_str = format!("{:?}", wallet);
        assert!(debug_str.contains("REDACTED"));
        let priv_hex = hex_encode(&priv_bytes);
        assert!(!debug_str.contains(&priv_hex));
    }

    #[test]
    fn test_wallet_sign_with_memo_and_verify() {
        let wallet = TolaWallet::generate();
        let bob = Address([0x42; 20]);
        let mut tx = Transaction {
            chain_id: ChainId::MAINNET,
            nonce: Nonce(5),
            from: wallet.address(),
            to: Some(bob),
            value: Amount(100_000_000),
            gas_limit: Gas(21_000),
            max_fee_per_gas: Amount(1_000_000_000),
            max_priority_fee_per_gas: Amount(1_000_000_000),
            valid_until_block: BlockHeight(1000),
            data: vec![],
            memo: Some("Invoice #1042".to_string()),
            signature: Signature::Secp256k1 {
                r: [0u8; 32],
                s: [0u8; 32],
                v: 0,
            },
            tx_type: 0,
            access_list: vec![],
        };

        wallet.sign_transaction(&mut tx).unwrap();
        let digest = compute_transaction_digest(&tx);
        let mut sig_65 = [0u8; 65];
        if let Signature::Secp256k1 { r, s, v } = tx.signature {
            sig_65[0..32].copy_from_slice(&r);
            sig_65[32..64].copy_from_slice(&s);
            sig_65[64] = v;
        }
        let recovered = recover_secp256k1_address(digest.as_bytes(), &sig_65).unwrap();
        assert_eq!(recovered, wallet.address());

        // Tampering with the memo must change digest and invalidate recovery
        tx.memo = Some("Invoice #9999".to_string());
        let bad_digest = compute_transaction_digest(&tx);
        assert_ne!(digest, bad_digest);
        let bad_recovered = recover_secp256k1_address(bad_digest.as_bytes(), &sig_65).unwrap();
        assert_ne!(bad_recovered, wallet.address());
    }

    fn hex_encode(data: &[u8]) -> String {
        let mut s = String::new();
        for b in data {
            s.push_str(&format!("{b:02x}"));
        }
        s
    }
}
