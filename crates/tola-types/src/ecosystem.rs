//! Ecosystem primitives: Tokenized External Assets (TLS-721), TOLA Connect, and Clear-Signing Prompts.
//! Note: TOLA does not operate native usernames or native gifts. All identity and collectibles
//! originate on external platforms and are represented on-chain via third-party tokenization.

use crate::{Address, Amount, Gas, Nonce};

/// Generic classification of external platform assets tokenizable on TOLA Network.
/// Provides an open, extensible taxonomy for identity, credentials, collectibles,
/// real-world assets (RWA), gaming items, and custom third-party asset categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExternalAssetType {
    /// Canonical identity identifiers (e.g., usernames, domain names, decentralized IDs)
    IdentityIdentifier,
    /// Virtual / numeric identifiers (e.g., phone numbers, device IDs, SIMs)
    VirtualNumber,
    /// Collectible gifts / tokens (e.g., platform gifts, promotional collectibles)
    GiftAsset,
    /// Attestation badges / credentials (e.g., verified status, achievements)
    AttestationBadge,
    /// Social media identifiers / handles
    SocialIdentifier,
    /// Access credentials / security tokens
    AccessCredential,
    /// Digital collectibles / NFTs
    DigitalCollectible,
    /// Unclassified external assets
    OtherExternalAsset,
    /// Real World Assets (e.g., real estate, commodities, invoices, physical luxury)
    RealWorldAsset,
    /// Gaming assets (e.g., in-game items, virtual equipment, skins)
    GamingAsset,
    /// User-defined or third-party custom asset category (11..=255)
    Custom(u8),
}

impl ExternalAssetType {
    // Backward compatibility aliases for legacy naming
    #[allow(non_upper_case_globals)]
    pub const MessengerUsername: Self = Self::IdentityIdentifier;
    #[allow(non_upper_case_globals)]
    pub const MessengerNumber: Self = Self::VirtualNumber;
    #[allow(non_upper_case_globals)]
    pub const MessengerGift: Self = Self::GiftAsset;
    #[allow(non_upper_case_globals)]
    pub const MessengerBadge: Self = Self::AttestationBadge;
    #[allow(non_upper_case_globals)]
    pub const SocialUsername: Self = Self::SocialIdentifier;
    #[allow(non_upper_case_globals)]
    pub const SocialNumber: Self = Self::AccessCredential;

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::IdentityIdentifier => "IDENTITY_IDENTIFIER",
            Self::VirtualNumber => "VIRTUAL_NUMBER",
            Self::GiftAsset => "GIFT_ASSET",
            Self::AttestationBadge => "ATTESTATION_BADGE",
            Self::SocialIdentifier => "SOCIAL_IDENTIFIER",
            Self::AccessCredential => "ACCESS_CREDENTIAL",
            Self::DigitalCollectible => "DIGITAL_COLLECTIBLE",
            Self::OtherExternalAsset => "OTHER_EXTERNAL_ASSET",
            Self::RealWorldAsset => "REAL_WORLD_ASSET",
            Self::GamingAsset => "GAMING_ASSET",
            Self::Custom(_) => "CUSTOM_ASSET",
        }
    }

    pub fn to_u8(&self) -> u8 {
        match self {
            Self::IdentityIdentifier => 1,
            Self::VirtualNumber => 2,
            Self::GiftAsset => 3,
            Self::AttestationBadge => 4,
            Self::SocialIdentifier => 5,
            Self::AccessCredential => 6,
            Self::DigitalCollectible => 7,
            Self::OtherExternalAsset => 8,
            Self::RealWorldAsset => 9,
            Self::GamingAsset => 10,
            Self::Custom(val) => *val,
        }
    }

    pub fn from_u8(val: u8) -> Option<Self> {
        match val {
            0 => None,
            1 => Some(Self::IdentityIdentifier),
            2 => Some(Self::VirtualNumber),
            3 => Some(Self::GiftAsset),
            4 => Some(Self::AttestationBadge),
            5 => Some(Self::SocialIdentifier),
            6 => Some(Self::AccessCredential),
            7 => Some(Self::DigitalCollectible),
            8 => Some(Self::OtherExternalAsset),
            9 => Some(Self::RealWorldAsset),
            10 => Some(Self::GamingAsset),
            val => Some(Self::Custom(val)),
        }
    }
}

impl core::str::FromStr for ExternalAssetType {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "IDENTITY_IDENTIFIER" | "MESSENGER_USERNAME" => Ok(Self::IdentityIdentifier),
            "VIRTUAL_NUMBER" | "MESSENGER_NUMBER" => Ok(Self::VirtualNumber),
            "GIFT_ASSET" | "MESSENGER_GIFT" => Ok(Self::GiftAsset),
            "ATTESTATION_BADGE" | "MESSENGER_BADGE" => Ok(Self::AttestationBadge),
            "SOCIAL_IDENTIFIER" | "SOCIAL_USERNAME" => Ok(Self::SocialIdentifier),
            "ACCESS_CREDENTIAL" | "SOCIAL_NUMBER" => Ok(Self::AccessCredential),
            "DIGITAL_COLLECTIBLE" => Ok(Self::DigitalCollectible),
            "OTHER_EXTERNAL_ASSET" => Ok(Self::OtherExternalAsset),
            "REAL_WORLD_ASSET" | "RWA" => Ok(Self::RealWorldAsset),
            "GAMING_ASSET" | "GAME_ASSET" => Ok(Self::GamingAsset),
            _ => {
                if let Ok(num) = s.parse::<u8>() {
                    if num > 0 {
                        return Self::from_u8(num).ok_or(());
                    }
                }
                Err(())
            }
        }
    }
}

/// Canonical system contract address for Tokenized External Assets (TLS-721).
pub const EXTERNAL_ASSET_CONTRACT_ADDRESS: Address = Address([
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x77, 0x21,
]);

/// On-chain state representation of a Tokenized External Asset (TLS-721 NFT).
/// Represents blockchain ownership of the representation on TOLA, decoupled from external ownership.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalAsset {
    pub token_id: u64,
    pub canonical_id: [u8; 32], // keccak256(source_platform || ":" || asset_type || ":" || external_asset_id)
    pub source_platform: String,
    pub asset_type: ExternalAssetType,
    pub external_asset_id: String,
    pub external_reference: String,
    pub metadata_uri: String,
    pub owner: Address,
    pub authorizer: Address,
    pub verifier_address: Address,
    pub minted_at: u64,
}

impl ExternalAsset {
    /// Authorizer key that authenticated the tokenization proof on behalf of the external platform.
    pub fn authorizer(&self) -> Address {
        self.authorizer
    }
}

/// Cryptographically verifiable mint authorization proof issued by an external platform authority.
/// Binds destination wallet, chain ID, nonce, expiry, and external asset details to prevent replay attacks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalAssetAttestation {
    pub chain_id: u64,
    pub nonce: u64,
    pub destination_wallet: Address,
    pub expires_at: u64,
    pub source_platform: String,
    pub asset_type: ExternalAssetType,
    pub external_asset_id: String,
    pub authorizer_address: Address,
    pub verifier_address: Address,
    pub signature: [u8; 65], // recoverable Secp256k1 [r(32) || s(32) || v(1)]
}

/// Generic platform-neutral type alias for ExternalAssetAttestation.
pub type ExternalAssetAuthorization = ExternalAssetAttestation;

impl ExternalAssetAttestation {
    /// Alias for expires_at representing claim validity boundary.
    pub fn claim_expires_at(&self) -> u64 {
        self.expires_at
    }

    /// Key of the external platform authorizer.
    pub fn authorizer(&self) -> Address {
        self.authorizer_address
    }
}

/// The 11 canonical TOLA Connect protocol message types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConnectMessageType {
    SessionInit,
    SessionAccept,
    DeviceRegister,
    DeviceRevoke,
    SignRequest,
    SignApproval,
    SignRejection,
    TransactionRequest,
    TransactionApproval,
    TransactionSubmission,
    TransactionResult,
}

impl ConnectMessageType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::SessionInit => "SESSION_INIT",
            Self::SessionAccept => "SESSION_ACCEPT",
            Self::DeviceRegister => "DEVICE_REGISTER",
            Self::DeviceRevoke => "DEVICE_REVOKE",
            Self::SignRequest => "SIGN_REQUEST",
            Self::SignApproval => "SIGN_APPROVAL",
            Self::SignRejection => "SIGN_REJECTION",
            Self::TransactionRequest => "TRANSACTION_REQUEST",
            Self::TransactionApproval => "TRANSACTION_APPROVAL",
            Self::TransactionSubmission => "TRANSACTION_SUBMISSION",
            Self::TransactionResult => "TRANSACTION_RESULT",
        }
    }
}

impl core::str::FromStr for ConnectMessageType {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "SESSION_INIT" => Ok(Self::SessionInit),
            "SESSION_ACCEPT" => Ok(Self::SessionAccept),
            "DEVICE_REGISTER" => Ok(Self::DeviceRegister),
            "DEVICE_REVOKE" => Ok(Self::DeviceRevoke),
            "SIGN_REQUEST" => Ok(Self::SignRequest),
            "SIGN_APPROVAL" => Ok(Self::SignApproval),
            "SIGN_REJECTION" => Ok(Self::SignRejection),
            "TRANSACTION_REQUEST" => Ok(Self::TransactionRequest),
            "TRANSACTION_APPROVAL" => Ok(Self::TransactionApproval),
            "TRANSACTION_SUBMISSION" => Ok(Self::TransactionSubmission),
            "TRANSACTION_RESULT" => Ok(Self::TransactionResult),
            _ => Err(()),
        }
    }
}

/// A registered device session entry for multi-device management.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceRecord {
    pub device_id: String,
    pub device_name: String,
    pub authorized_at: u64,
    pub is_revoked: bool,
}

impl DeviceRecord {
    pub fn new(
        device_id: impl Into<String>,
        device_name: impl Into<String>,
        timestamp: u64,
    ) -> Self {
        Self {
            device_id: device_id.into(),
            device_name: device_name.into(),
            authorized_at: timestamp,
            is_revoked: false,
        }
    }

    pub fn revoke(&mut self) {
        self.is_revoked = true;
    }
}

/// A structured Clear-Signing Prompt for wallet UI modal display.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClearSigningPrompt {
    pub domain: String,
    pub action_name: String,
    pub target_address: Address,
    pub amount: Amount,
    pub asset_symbol: String,
    pub max_fee: Gas,
    pub nonce: Nonce,
    pub chain_id: u64,
    pub memo: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ClearSigningError {
    EmptyDomain,
    EmptyActionName,
    EmptyAssetSymbol,
    InvalidChainId,
    MemoTooLong(usize),
}

impl ClearSigningPrompt {
    pub const MAX_MEMO_BYTES: usize = 256;

    pub fn validate(&self) -> Result<(), ClearSigningError> {
        if self.domain.trim().is_empty() {
            return Err(ClearSigningError::EmptyDomain);
        }
        if self.action_name.trim().is_empty() {
            return Err(ClearSigningError::EmptyActionName);
        }
        if self.asset_symbol.trim().is_empty() {
            return Err(ClearSigningError::EmptyAssetSymbol);
        }
        if self.chain_id == 0 {
            return Err(ClearSigningError::InvalidChainId);
        }
        if let Some(ref m) = self.memo {
            if m.len() > Self::MAX_MEMO_BYTES {
                return Err(ClearSigningError::MemoTooLong(m.len()));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clear_signing_prompt_validation_and_memo() {
        let prompt = ClearSigningPrompt {
            domain: "app.tola.network".into(),
            action_name: "Transfer".into(),
            target_address: Address([0x11; 20]),
            amount: Amount::from_u128(100),
            asset_symbol: "TOLA".into(),
            max_fee: Gas(21000),
            nonce: Nonce::new(1),
            chain_id: 9999,
            memo: Some("Invoice #1042".into()),
        };
        assert!(prompt.validate().is_ok());

        let mut invalid = prompt.clone();
        invalid.domain = "   ".into();
        assert_eq!(invalid.validate(), Err(ClearSigningError::EmptyDomain));

        // Memo exactly 256 bytes accepted
        let mut ok_memo = prompt.clone();
        ok_memo.memo = Some("a".repeat(256));
        assert!(ok_memo.validate().is_ok());

        // Memo 257 bytes rejected
        let mut bad_memo = prompt.clone();
        bad_memo.memo = Some("a".repeat(257));
        assert_eq!(
            bad_memo.validate(),
            Err(ClearSigningError::MemoTooLong(257))
        );
    }

    #[test]
    fn test_connect_message_types_roundtrip() {
        let types = [
            ConnectMessageType::SessionInit,
            ConnectMessageType::SessionAccept,
            ConnectMessageType::DeviceRegister,
            ConnectMessageType::DeviceRevoke,
            ConnectMessageType::SignRequest,
            ConnectMessageType::SignApproval,
            ConnectMessageType::SignRejection,
            ConnectMessageType::TransactionRequest,
            ConnectMessageType::TransactionApproval,
            ConnectMessageType::TransactionSubmission,
            ConnectMessageType::TransactionResult,
        ];

        for t in types {
            let s = t.as_str();
            assert_eq!(s.parse::<ConnectMessageType>(), Ok(t));
        }
        assert_eq!("UNKNOWN".parse::<ConnectMessageType>(), Err(()));
    }

    #[test]
    fn test_device_record_lifecycle() {
        let mut dev = DeviceRecord::new("device_iphone_16", "Anvar iPhone", 1700000000);
        assert!(!dev.is_revoked);
        dev.revoke();
        assert!(dev.is_revoked);
    }

    #[test]
    fn test_external_asset_types_and_models() {
        let types = [
            ExternalAssetType::IdentityIdentifier,
            ExternalAssetType::VirtualNumber,
            ExternalAssetType::GiftAsset,
            ExternalAssetType::AttestationBadge,
            ExternalAssetType::SocialIdentifier,
            ExternalAssetType::AccessCredential,
            ExternalAssetType::DigitalCollectible,
            ExternalAssetType::OtherExternalAsset,
            ExternalAssetType::RealWorldAsset,
            ExternalAssetType::GamingAsset,
            ExternalAssetType::Custom(42),
        ];

        for t in types {
            let s = t.as_str();
            if let ExternalAssetType::Custom(_) = t {
                assert_eq!(t.to_u8(), 42);
                assert_eq!(ExternalAssetType::from_u8(42), Some(t));
            } else {
                assert_eq!(s.parse::<ExternalAssetType>(), Ok(t));
                let b = t.to_u8();
                assert_eq!(ExternalAssetType::from_u8(b), Some(t));
            }
        }

        // Backward compatibility parsing
        assert_eq!(
            "MESSENGER_USERNAME".parse::<ExternalAssetType>(),
            Ok(ExternalAssetType::IdentityIdentifier)
        );
        assert_eq!(
            "MESSENGER_NUMBER".parse::<ExternalAssetType>(),
            Ok(ExternalAssetType::VirtualNumber)
        );
        assert_eq!(
            "MESSENGER_GIFT".parse::<ExternalAssetType>(),
            Ok(ExternalAssetType::GiftAsset)
        );
        assert_eq!(
            "MESSENGER_BADGE".parse::<ExternalAssetType>(),
            Ok(ExternalAssetType::AttestationBadge)
        );
        assert_eq!(
            "SOCIAL_USERNAME".parse::<ExternalAssetType>(),
            Ok(ExternalAssetType::SocialIdentifier)
        );
        assert_eq!(
            "SOCIAL_NUMBER".parse::<ExternalAssetType>(),
            Ok(ExternalAssetType::AccessCredential)
        );
        assert_eq!(
            "RWA".parse::<ExternalAssetType>(),
            Ok(ExternalAssetType::RealWorldAsset)
        );
        assert_eq!(
            "GAME_ASSET".parse::<ExternalAssetType>(),
            Ok(ExternalAssetType::GamingAsset)
        );
        assert_eq!(
            "99".parse::<ExternalAssetType>(),
            Ok(ExternalAssetType::Custom(99))
        );

        assert_eq!("UNKNOWN".parse::<ExternalAssetType>(), Err(()));
        assert_eq!(ExternalAssetType::from_u8(0), None);
        assert_eq!(
            ExternalAssetType::from_u8(99),
            Some(ExternalAssetType::Custom(99))
        );
    }
}
