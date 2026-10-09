//! TOLA 5-of-7 Community Security Council and Emergency Pause Multi-Sig Subsystem.

use tola_crypto::{verify_ed25519, Ed25519Keypair};
use tola_rpc::json::JsonValue;
use tola_types::Address;

pub const DOMAIN_SECURITY_COUNCIL_PAUSE: &[u8] = b"TOLA_SECURITY_COUNCIL_EMERGENCY_PAUSE_V1";

/// A registered signatory in the Community Security Council.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SecurityCouncilMember {
    pub name: String,
    pub address: Address,
    pub public_key: [u8; 32],
}

/// Multi-Sig governance policy for emergency actions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SecurityCouncilPolicy {
    pub threshold: usize,
    pub total_signers: usize,
    pub members: Vec<SecurityCouncilMember>,
}

impl SecurityCouncilPolicy {
    /// Generates the canonical 5-of-7 Community Security Council policy.
    pub fn default_mainnet_council() -> Self {
        let member_specs = [
            ("Foundation Council Leader", 0x10),
            ("Independent Security Auditor #1", 0x20),
            ("Independent Security Auditor #2", 0x30),
            ("Core Developer Lead", 0x40),
            ("Ecosystem Staker Representative", 0x50),
            ("Global Infrastructure Operator #1", 0x60),
            ("Global Infrastructure Operator #2", 0x70),
        ];

        let mut members = Vec::with_capacity(7);
        for (name, byte_val) in member_specs {
            let addr_bytes = [byte_val; 20];
            let pub_bytes = [byte_val; 32];
            members.push(SecurityCouncilMember {
                name: name.to_string(),
                address: Address(addr_bytes),
                public_key: pub_bytes,
            });
        }

        Self {
            threshold: 5,
            total_signers: 7,
            members,
        }
    }

    /// Serializes policy to JSON.
    pub fn to_json(&self) -> String {
        let mut members_json = String::new();
        for (i, m) in self.members.iter().enumerate() {
            if i > 0 {
                members_json.push_str(",\n");
            }
            members_json.push_str(&format!(
                "    {{\n      \"name\": \"{}\",\n      \"address\": \"{}\",\n      \"public_key\": \"0x{}\"\n    }}",
                m.name,
                m.address.to_hex(),
                hex::encode(m.public_key)
            ));
        }

        format!(
            "{{\n  \"threshold\": {},\n  \"total_signers\": {},\n  \"members\": [\n{}\n  ]\n}}",
            self.threshold, self.total_signers, members_json
        )
    }

    /// Parses policy from JSON string.
    pub fn from_json(json_str: &str) -> Result<Self, String> {
        let val = JsonValue::parse(json_str).map_err(|e| format!("Invalid JSON: {:?}", e))?;
        let obj = match val {
            JsonValue::Object(map) => map,
            _ => return Err("Expected JSON object".into()),
        };

        let threshold = obj
            .iter()
            .find(|(k, _)| k == "threshold")
            .and_then(|(_, v)| v.as_i64())
            .unwrap_or(5) as usize;

        let total_signers = obj
            .iter()
            .find(|(k, _)| k == "total_signers")
            .and_then(|(_, v)| v.as_i64())
            .unwrap_or(7) as usize;

        let members_val = obj
            .iter()
            .find(|(k, _)| k == "members")
            .map(|(_, v)| v)
            .ok_or("Missing members array")?;

        let members_arr = match members_val {
            JsonValue::Array(arr) => arr,
            _ => return Err("members must be array".into()),
        };

        let mut members = Vec::with_capacity(members_arr.len());
        for item in members_arr {
            let m_obj = match item {
                JsonValue::Object(m) => m,
                _ => return Err("member must be object".into()),
            };
            let name = m_obj
                .iter()
                .find(|(k, _)| k == "name")
                .and_then(|(_, v)| v.as_str())
                .unwrap_or("")
                .to_string();
            let addr_str = m_obj
                .iter()
                .find(|(k, _)| k == "address")
                .and_then(|(_, v)| v.as_str())
                .unwrap_or("");
            let pub_str = m_obj
                .iter()
                .find(|(k, _)| k == "public_key")
                .and_then(|(_, v)| v.as_str())
                .unwrap_or("");

            let address =
                Address::from_hex(addr_str).map_err(|e| format!("Invalid address: {:?}", e))?;
            let pub_clean = pub_str.strip_prefix("0x").unwrap_or(pub_str);
            let pub_bytes =
                hex::decode(pub_clean).map_err(|e| format!("Invalid pubkey: {:?}", e))?;
            if pub_bytes.len() != 32 {
                return Err("public_key must be 32 bytes".into());
            }
            let mut public_key = [0u8; 32];
            public_key.copy_from_slice(&pub_bytes);

            members.push(SecurityCouncilMember {
                name,
                address,
                public_key,
            });
        }

        Ok(Self {
            threshold,
            total_signers,
            members,
        })
    }
}

/// An authorized Emergency Pause Signal requiring 5-of-7 signatures.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmergencyPauseSignal {
    pub reason: String,
    pub chain_id: u64,
    pub timestamp: u64,
    pub signatures: Vec<([u8; 32], [u8; 64])>, // (member_pubkey, signature)
}

impl EmergencyPauseSignal {
    /// Constructs canonical pause signal payload for signing.
    pub fn construct_payload(reason: &str, chain_id: u64, timestamp: u64) -> Vec<u8> {
        let mut msg = Vec::with_capacity(128);
        msg.extend_from_slice(DOMAIN_SECURITY_COUNCIL_PAUSE);
        msg.push(b'\n');
        msg.extend_from_slice(format!("ChainId:{}\n", chain_id).as_bytes());
        msg.extend_from_slice(format!("Timestamp:{}\n", timestamp).as_bytes());
        msg.extend_from_slice(format!("Reason:{}\n", reason).as_bytes());
        msg
    }

    /// Signs an emergency pause action with an authorized member keypair.
    pub fn sign(
        keypair: &Ed25519Keypair,
        reason: &str,
        chain_id: u64,
        timestamp: u64,
    ) -> ([u8; 32], [u8; 64]) {
        let pubkey = keypair.public_key_bytes();
        let payload = Self::construct_payload(reason, chain_id, timestamp);
        let sig = match keypair.sign(&payload) {
            tola_types::Signature::Ed25519(s) => s,
            _ => unreachable!(),
        };
        (pubkey, sig)
    }

    /// Verifies the multi-sig quorum threshold against the council policy.
    pub fn verify(&self, policy: &SecurityCouncilPolicy) -> Result<usize, String> {
        let payload = Self::construct_payload(&self.reason, self.chain_id, self.timestamp);
        let mut valid_signers = std::collections::HashSet::new();

        for (pubkey, sig) in &self.signatures {
            // Verify public key is a recognized council member
            if !policy.members.iter().any(|m| m.public_key == *pubkey) {
                return Err(format!(
                    "Unrecognized council member public key: 0x{}",
                    hex::encode(pubkey)
                ));
            }

            // Verify Ed25519 signature
            verify_ed25519(pubkey, &payload, sig).map_err(|e| {
                format!("Invalid signature from 0x{}: {:?}", hex::encode(pubkey), e)
            })?;

            if !valid_signers.insert(*pubkey) {
                return Err(format!(
                    "Duplicate signature from council member 0x{}",
                    hex::encode(pubkey)
                ));
            }
        }

        if valid_signers.len() < policy.threshold {
            return Err(format!(
                "Insufficient council signatures: got {}, required {}",
                valid_signers.len(),
                policy.threshold
            ));
        }

        Ok(valid_signers.len())
    }

    /// Serializes pause signal to JSON.
    pub fn to_json(&self) -> String {
        let mut sigs_json = String::new();
        for (i, (pubkey, sig)) in self.signatures.iter().enumerate() {
            if i > 0 {
                sigs_json.push_str(",\n");
            }
            sigs_json.push_str(&format!(
                "    {{\n      \"public_key\": \"0x{}\",\n      \"signature\": \"0x{}\"\n    }}",
                hex::encode(pubkey),
                hex::encode(sig)
            ));
        }

        format!(
            "{{\n  \"reason\": \"{}\",\n  \"chain_id\": {},\n  \"timestamp\": {},\n  \"signatures\": [\n{}\n  ]\n}}",
            self.reason, self.chain_id, self.timestamp, sigs_json
        )
    }

    /// Parses pause signal from JSON string.
    pub fn from_json(json_str: &str) -> Result<Self, String> {
        let val = JsonValue::parse(json_str).map_err(|e| format!("Invalid JSON: {:?}", e))?;
        let obj = match val {
            JsonValue::Object(map) => map,
            _ => return Err("Expected JSON object".into()),
        };

        let reason = obj
            .iter()
            .find(|(k, _)| k == "reason")
            .and_then(|(_, v)| v.as_str())
            .unwrap_or("")
            .to_string();
        let chain_id = obj
            .iter()
            .find(|(k, _)| k == "chain_id")
            .and_then(|(_, v)| v.as_i64())
            .unwrap_or(0) as u64;
        let timestamp = obj
            .iter()
            .find(|(k, _)| k == "timestamp")
            .and_then(|(_, v)| v.as_i64())
            .unwrap_or(0) as u64;

        let sigs_val = obj
            .iter()
            .find(|(k, _)| k == "signatures")
            .map(|(_, v)| v)
            .ok_or("Missing signatures")?;
        let sigs_arr = match sigs_val {
            JsonValue::Array(arr) => arr,
            _ => return Err("signatures must be array".into()),
        };

        let mut signatures = Vec::with_capacity(sigs_arr.len());
        for item in sigs_arr {
            let s_obj = match item {
                JsonValue::Object(o) => o,
                _ => return Err("signature must be object".into()),
            };
            let pub_str = s_obj
                .iter()
                .find(|(k, _)| k == "public_key")
                .and_then(|(_, v)| v.as_str())
                .unwrap_or("");
            let sig_str = s_obj
                .iter()
                .find(|(k, _)| k == "signature")
                .and_then(|(_, v)| v.as_str())
                .unwrap_or("");

            let pub_clean = pub_str.strip_prefix("0x").unwrap_or(pub_str);
            let pub_bytes =
                hex::decode(pub_clean).map_err(|e| format!("Invalid pubkey: {:?}", e))?;
            let sig_clean = sig_str.strip_prefix("0x").unwrap_or(sig_str);
            let sig_bytes =
                hex::decode(sig_clean).map_err(|e| format!("Invalid signature: {:?}", e))?;

            if pub_bytes.len() != 32 || sig_bytes.len() != 64 {
                return Err("Invalid pubkey or signature length".into());
            }

            let mut public_key = [0u8; 32];
            public_key.copy_from_slice(&pub_bytes);
            let mut signature = [0u8; 64];
            signature.copy_from_slice(&sig_bytes);

            signatures.push((public_key, signature));
        }

        Ok(Self {
            reason,
            chain_id,
            timestamp,
            signatures,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_security_council_multisig_quorum_and_rejection() {
        let mut keypairs = Vec::new();
        let mut members = Vec::new();

        for i in 1..=7 {
            let mut seed = [0u8; 32];
            seed[0] = i as u8;
            let kp = Ed25519Keypair::from_seed(&seed);
            let pub_bytes = kp.public_key_bytes();
            let mut addr = [0u8; 20];
            addr.copy_from_slice(&pub_bytes[..20]);

            members.push(SecurityCouncilMember {
                name: format!("Council Signer #{}", i),
                address: Address(addr),
                public_key: pub_bytes,
            });
            keypairs.push(kp);
        }

        let policy = SecurityCouncilPolicy {
            threshold: 5,
            total_signers: 7,
            members,
        };

        let reason = "Emergency 0-Day Mitigation Advisory Pause";
        let chain_id = 9999;
        let timestamp = 1770000000;

        // 1. Collect only 4 signatures (below 5-of-7 threshold)
        let mut sigs_4 = Vec::new();
        for kp in &keypairs[..4] {
            sigs_4.push(EmergencyPauseSignal::sign(kp, reason, chain_id, timestamp));
        }

        let signal_4 = EmergencyPauseSignal {
            reason: reason.to_string(),
            chain_id,
            timestamp,
            signatures: sigs_4,
        };
        assert!(
            signal_4.verify(&policy).is_err(),
            "Must reject with only 4/7 signatures"
        );

        // 2. Collect 5th signature (meets 5-of-7 threshold)
        let mut sigs_5 = signal_4.signatures.clone();
        sigs_5.push(EmergencyPauseSignal::sign(
            &keypairs[4],
            reason,
            chain_id,
            timestamp,
        ));

        let signal_5 = EmergencyPauseSignal {
            reason: reason.to_string(),
            chain_id,
            timestamp,
            signatures: sigs_5,
        };
        assert_eq!(
            signal_5.verify(&policy).unwrap(),
            5,
            "Must pass with 5/7 signatures"
        );

        // 3. Test duplicate signature rejection
        let mut sigs_dup = signal_4.signatures.clone();
        sigs_dup.push(signal_4.signatures[0]); // Duplicate signer 0
        let signal_dup = EmergencyPauseSignal {
            reason: reason.to_string(),
            chain_id,
            timestamp,
            signatures: sigs_dup,
        };
        assert!(
            signal_dup.verify(&policy).is_err(),
            "Must reject duplicate signature"
        );
    }

    #[test]
    fn test_policy_and_signal_json_roundtrip() {
        let policy = SecurityCouncilPolicy::default_mainnet_council();
        let json = policy.to_json();
        let parsed = SecurityCouncilPolicy::from_json(&json).expect("Parse policy JSON");
        assert_eq!(policy, parsed);
    }
}
