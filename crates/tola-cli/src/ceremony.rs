//! TOLA Mainnet Genesis Key Ceremony and Public Entropy Incorporation Subsystem.

use tola_crypto::{blake3_hash, verify_ed25519, Ed25519Keypair};
use tola_node::config::GenesisConfig;
use tola_rpc::json::JsonValue;
use tola_types::{Address, Hash, PublicKey, ValidatorId, ValidatorInfo};

pub const DOMAIN_CEREMONY_POP: &[u8] = b"TOLA_MAINNET_GENESIS_CEREMONY_POP_V1";
pub const DOMAIN_DRAND_ENTROPY: &[u8] = b"TOLA_GENESIS_DRAND_ENTROPY_V1";

/// Represents a single validator operator's submitted ceremony registration record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CeremonyRecord {
    pub operator_id: Address,
    pub organization: String,
    pub consensus_pubkey: [u8; 32],
    pub pop_signature: [u8; 64],
    pub timestamp: u64,
}

impl CeremonyRecord {
    /// Serializes ceremony record to JSON format.
    pub fn to_json(&self) -> String {
        format!(
            "{{\n  \"operator_id\": \"{}\",\n  \"organization\": \"{}\",\n  \"consensus_pubkey\": \"0x{}\",\n  \"pop_signature\": \"0x{}\",\n  \"timestamp\": {}\n}}",
            self.operator_id.to_hex(),
            self.organization,
            hex::encode(self.consensus_pubkey),
            hex::encode(self.pop_signature),
            self.timestamp
        )
    }

    /// Parses ceremony record from JSON string.
    pub fn from_json(json_str: &str) -> Result<Self, String> {
        let val = JsonValue::parse(json_str).map_err(|e| format!("Invalid JSON: {:?}", e))?;
        let obj = match val {
            JsonValue::Object(map) => map,
            _ => return Err("Expected JSON object".into()),
        };

        let find = |k: &str| -> Result<&JsonValue, String> {
            obj.iter()
                .find(|(key, _)| key == k)
                .map(|(_, v)| v)
                .ok_or_else(|| format!("Missing required key: {}", k))
        };

        let op_str = find("operator_id")?
            .as_str()
            .ok_or("operator_id not string")?;
        let operator_id =
            Address::from_hex(op_str).map_err(|e| format!("Invalid operator_id: {:?}", e))?;

        let organization = find("organization")?
            .as_str()
            .unwrap_or("Unknown")
            .to_string();

        let pub_hex = find("consensus_pubkey")?
            .as_str()
            .ok_or("consensus_pubkey not string")?;
        let pub_clean = pub_hex.strip_prefix("0x").unwrap_or(pub_hex);
        let pub_bytes =
            hex::decode(pub_clean).map_err(|e| format!("Invalid pubkey hex: {:?}", e))?;
        if pub_bytes.len() != 32 {
            return Err("consensus_pubkey must be 32 bytes".into());
        }
        let mut consensus_pubkey = [0u8; 32];
        consensus_pubkey.copy_from_slice(&pub_bytes);

        let sig_hex = find("pop_signature")?
            .as_str()
            .ok_or("pop_signature not string")?;
        let sig_clean = sig_hex.strip_prefix("0x").unwrap_or(sig_hex);
        let sig_bytes =
            hex::decode(sig_clean).map_err(|e| format!("Invalid signature hex: {:?}", e))?;
        if sig_bytes.len() != 64 {
            return Err("pop_signature must be 64 bytes".into());
        }
        let mut pop_signature = [0u8; 64];
        pop_signature.copy_from_slice(&sig_bytes);

        let timestamp = find("timestamp")?.as_i64().unwrap_or(0) as u64;

        Ok(Self {
            operator_id,
            organization,
            consensus_pubkey,
            pop_signature,
            timestamp,
        })
    }
}

/// Constructs the canonical Proof of Possession message payload.
pub fn construct_pop_payload(
    operator: &Address,
    consensus_pubkey: &[u8; 32],
    chain_id: u64,
) -> Vec<u8> {
    let mut payload = Vec::with_capacity(128);
    payload.extend_from_slice(DOMAIN_CEREMONY_POP);
    payload.push(b'\n');
    payload.extend_from_slice(format!("ChainId:{}\n", chain_id).as_bytes());
    payload.extend_from_slice(format!("Operator:{}\n", operator.to_hex()).as_bytes());
    payload.extend_from_slice(
        format!("ConsensusPubkey:0x{}\n", hex::encode(consensus_pubkey)).as_bytes(),
    );
    payload
}

/// Signs a canonical Proof of Possession with an Ed25519 consensus private key.
pub fn sign_proof_of_possession(
    keypair: &Ed25519Keypair,
    operator: &Address,
    chain_id: u64,
) -> [u8; 64] {
    let pubkey = keypair.public_key_bytes();
    let msg = construct_pop_payload(operator, &pubkey, chain_id);
    match keypair.sign(&msg) {
        tola_types::Signature::Ed25519(sig) => sig,
        _ => unreachable!(),
    }
}

/// Cryptographically verifies a validator operator's Proof of Possession.
pub fn verify_proof_of_possession(
    consensus_pubkey: &[u8; 32],
    operator: &Address,
    chain_id: u64,
    signature: &[u8; 64],
) -> Result<(), String> {
    let msg = construct_pop_payload(operator, consensus_pubkey, chain_id);
    verify_ed25519(consensus_pubkey, &msg, signature)
        .map_err(|e| format!("PoP verification failed: {:?}", e))
}

/// Mixes public verifiable Drand entropy into the genesis configuration.
pub fn mix_drand_entropy(
    genesis: &mut GenesisConfig,
    round: u64,
    drand_sig_bytes: &[u8],
) -> Result<Hash, String> {
    if drand_sig_bytes.len() < 32 {
        return Err("Drand signature must be at least 32 bytes".into());
    }
    let mut entropy_input = Vec::with_capacity(drand_sig_bytes.len() + 64);
    entropy_input.extend_from_slice(DOMAIN_DRAND_ENTROPY);
    entropy_input.extend_from_slice(&round.to_be_bytes());
    entropy_input.extend_from_slice(drand_sig_bytes);

    let entropy_hash = blake3_hash(&entropy_input);

    // Deterministically inject entropy anchor into initial genesis timestamp if not zero
    if round > 0 {
        genesis.genesis_time = genesis.genesis_time.saturating_add(round % 60);
    }

    Ok(entropy_hash)
}

/// Ingests 4 verified operator ceremony records and produces the canonical Mainnet Genesis specification.
pub fn assemble_mainnet_genesis(
    template: &GenesisConfig,
    records: &[CeremonyRecord],
) -> Result<GenesisConfig, String> {
    if records.len() != 4 {
        return Err(format!(
            "Ceremony requires exactly 4 validators, got {}",
            records.len()
        ));
    }

    // Verify all 4 Proofs of Possession and detect duplicate keys
    let mut seen_pubkeys = std::collections::HashSet::new();
    for (idx, rec) in records.iter().enumerate() {
        if !seen_pubkeys.insert(rec.consensus_pubkey) {
            return Err(format!(
                "Duplicate consensus pubkey found at record index {}",
                idx
            ));
        }
        verify_proof_of_possession(
            &rec.consensus_pubkey,
            &rec.operator_id,
            template.chain_id.0,
            &rec.pop_signature,
        )
        .map_err(|e| format!("Validator {} PoP verification failed: {}", idx + 1, e))?;
    }

    let mut assembled = template.clone();
    assembled.initial_validators.clear();

    for rec in records.iter() {
        assembled.initial_validators.push(ValidatorInfo {
            validator_id: ValidatorId(rec.operator_id),
            consensus_pubkey: PublicKey::Ed25519(rec.consensus_pubkey),
            voting_power: 50_000_000,
            commission_bp: 500,
        });
    }

    // Validate 100% supply conservation invariant
    let total_supply: u128 = assembled.accounts.iter().map(|a| a.balance.0).sum();
    let expected_supply: u128 = 1_000_000_000 * 1_000_000_000_000_000_000;
    if total_supply != expected_supply {
        return Err(format!(
            "Genesis supply mismatch: expected {}, got {}",
            expected_supply, total_supply
        ));
    }

    Ok(assembled)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_proof_of_possession_generation_and_verification() {
        let keypair = Ed25519Keypair::generate();
        let pubkey = keypair.public_key_bytes();
        let operator = Address([0x77; 20]);
        let chain_id = 9999;

        let sig = sign_proof_of_possession(&keypair, &operator, chain_id);
        assert!(verify_proof_of_possession(&pubkey, &operator, chain_id, &sig).is_ok());

        // Test corruption / mismatch
        let wrong_operator = Address([0x88; 20]);
        assert!(verify_proof_of_possession(&pubkey, &wrong_operator, chain_id, &sig).is_err());

        let wrong_chain = 102;
        assert!(verify_proof_of_possession(&pubkey, &operator, wrong_chain, &sig).is_err());
    }

    #[test]
    fn test_ceremony_record_json_roundtrip() {
        let keypair = Ed25519Keypair::generate();
        let pubkey = keypair.public_key_bytes();
        let operator = Address([0xAA; 20]);
        let sig = sign_proof_of_possession(&keypair, &operator, 9999);

        let rec = CeremonyRecord {
            operator_id: operator,
            organization: "Institutional Node Alpha".into(),
            consensus_pubkey: pubkey,
            pop_signature: sig,
            timestamp: 1770000000,
        };

        let json = rec.to_json();
        let parsed = CeremonyRecord::from_json(&json).expect("Parse JSON");
        assert_eq!(rec, parsed);
    }

    #[test]
    fn test_drand_entropy_mixing() {
        let mut genesis = GenesisConfig::mainnet_genesis();
        let dummy_sig = [0x5au8; 48];
        let hash1 = mix_drand_entropy(&mut genesis, 12345, &dummy_sig).expect("Mix drand");
        assert_ne!(hash1, Hash::ZERO);
    }
}
