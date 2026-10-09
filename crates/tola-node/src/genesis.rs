//! Deterministic genesis state generation, validation, and JSON encoding.

use tola_core::{StateDB, StateDelta};
use tola_rpc::json::JsonValue;
use tola_state::MemoryStateJournal;
use tola_types::{
    AccountState, Address, Amount, BlockHeight, ChainId, Hash, PublicKey, ValidatorId,
    ValidatorInfo,
};

use crate::config::{GenesisAccount, GenesisConfig};

/// Computes the deterministic 32-byte Merkle state root for a given genesis specification.
/// Guarantees that account insertion order does not perturb the resulting state root.
pub fn compute_genesis_state_root(genesis: &GenesisConfig) -> Hash {
    let mut state = MemoryStateJournal::new();

    // Sort accounts deterministically by Address
    let mut sorted_accounts = genesis.accounts.clone();
    sorted_accounts.sort_by_key(|a| a.address);

    let mut deltas = Vec::with_capacity(sorted_accounts.len());
    for acct in sorted_accounts {
        deltas.push((
            acct.address,
            Some(AccountState::new_with_balance(acct.balance)),
        ));
    }

    if !deltas.is_empty() {
        let _ = state.apply_delta(StateDelta {
            accounts: deltas,
            storage: vec![],
            code: vec![],
        });
    }

    state.commit(BlockHeight(0)).unwrap_or(Hash::ZERO)
}

/// Serializes a `GenesisConfig` to a human-readable JSON string.
pub fn serialize_genesis_json(genesis: &GenesisConfig) -> String {
    let mut root_obj = Vec::new();
    root_obj.push((
        "chain_id".into(),
        JsonValue::Integer(genesis.chain_id.0 as i64),
    ));
    root_obj.push((
        "genesis_time".into(),
        JsonValue::Integer(genesis.genesis_time as i64),
    ));

    let mut accts_arr = Vec::new();
    for acct in &genesis.accounts {
        let mut a_obj = Vec::new();
        a_obj.push(("address".into(), JsonValue::String(acct.address.to_hex())));
        a_obj.push((
            "balance".into(),
            JsonValue::String(acct.balance.0.to_string()),
        ));
        accts_arr.push(JsonValue::Object(a_obj));
    }
    root_obj.push(("accounts".into(), JsonValue::Array(accts_arr)));

    let mut vals_arr = Vec::new();
    for val in &genesis.initial_validators {
        let mut v_obj = Vec::new();
        v_obj.push((
            "validator_id".into(),
            JsonValue::String(val.validator_id.0.to_hex()),
        ));
        let pubkey_hex = match val.consensus_pubkey {
            PublicKey::Ed25519(bytes) => {
                let mut s = String::with_capacity(66);
                s.push_str("0x");
                for b in bytes {
                    s.push_str(&format!("{b:02x}"));
                }
                s
            }
            PublicKey::Secp256k1(bytes) => {
                let mut s = String::with_capacity(68);
                s.push_str("0x");
                for b in bytes {
                    s.push_str(&format!("{b:02x}"));
                }
                s
            }
        };
        v_obj.push(("consensus_pubkey".into(), JsonValue::String(pubkey_hex)));
        v_obj.push((
            "voting_power".into(),
            JsonValue::Integer(val.voting_power as i64),
        ));
        v_obj.push((
            "commission_bp".into(),
            JsonValue::Integer(val.commission_bp as i64),
        ));
        vals_arr.push(JsonValue::Object(v_obj));
    }
    root_obj.push(("validators".into(), JsonValue::Array(vals_arr)));

    JsonValue::Object(root_obj).serialize()
}

/// Parses a `GenesisConfig` from a JSON string.
pub fn parse_genesis_json(json_str: &str) -> Result<GenesisConfig, String> {
    let parsed = JsonValue::parse(json_str)?;

    let chain_id = parsed
        .get("chain_id")
        .and_then(|v| v.as_u64())
        .map(ChainId)
        .ok_or_else(|| "Missing or invalid 'chain_id'".to_string())?;

    let genesis_time = parsed
        .get("genesis_time")
        .and_then(|v| v.as_u64())
        .unwrap_or(1770000000);

    let mut accounts = Vec::new();
    if let Some(accts_val) = parsed.get("accounts").and_then(|v| v.as_array()) {
        for acct_item in accts_val {
            let addr_str = acct_item
                .get("address")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "Missing 'address' in genesis account".to_string())?;
            let address = Address::from_hex(addr_str)
                .map_err(|e| format!("Invalid account address {}: {:?}", addr_str, e))?;

            let bal_val = acct_item
                .get("balance")
                .ok_or_else(|| "Missing 'balance' in genesis account".to_string())?;

            let balance_u128 = match bal_val {
                JsonValue::Integer(i) if *i >= 0 => *i as u128,
                JsonValue::String(s) => s
                    .parse::<u128>()
                    .map_err(|e| format!("Invalid balance numeric string {}: {}", s, e))?,
                _ => return Err("Invalid balance format".into()),
            };

            accounts.push(GenesisAccount {
                address,
                balance: Amount(balance_u128),
            });
        }
    }

    let mut initial_validators = Vec::new();
    if let Some(vals_val) = parsed.get("validators").and_then(|v| v.as_array()) {
        for val_item in vals_val {
            let vid_str = val_item
                .get("validator_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "Missing 'validator_id' in validator entry".to_string())?;
            let validator_id = ValidatorId(
                Address::from_hex(vid_str)
                    .map_err(|e| format!("Invalid validator address {}: {:?}", vid_str, e))?,
            );

            let pubkey_str = val_item
                .get("consensus_pubkey")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "Missing 'consensus_pubkey' in validator entry".to_string())?;

            let clean_pk = pubkey_str.strip_prefix("0x").unwrap_or(pubkey_str);
            let consensus_pubkey = if clean_pk.len() == 64 {
                let mut bytes = [0u8; 32];
                for i in 0..32 {
                    bytes[i] = u8::from_str_radix(&clean_pk[i * 2..i * 2 + 2], 16)
                        .map_err(|e| format!("Invalid pubkey hex: {}", e))?;
                }
                PublicKey::Ed25519(bytes)
            } else if clean_pk.len() == 66 {
                let mut bytes = [0u8; 33];
                for i in 0..33 {
                    bytes[i] = u8::from_str_radix(&clean_pk[i * 2..i * 2 + 2], 16)
                        .map_err(|e| format!("Invalid pubkey hex: {}", e))?;
                }
                PublicKey::Secp256k1(bytes)
            } else {
                return Err(format!(
                    "Invalid consensus_pubkey length: {}",
                    clean_pk.len()
                ));
            };

            let voting_power = val_item
                .get("voting_power")
                .and_then(|v| v.as_u64())
                .unwrap_or(100_000);

            let commission_bp = val_item
                .get("commission_bp")
                .and_then(|v| v.as_u64())
                .unwrap_or(500) as u16;

            initial_validators.push(ValidatorInfo {
                validator_id,
                consensus_pubkey,
                voting_power,
                commission_bp,
            });
        }
    }

    Ok(GenesisConfig {
        chain_id,
        genesis_time,
        accounts,
        initial_validators,
    })
}

pub const MIN_VALIDATOR_SELF_STAKE: Amount = Amount(10_000 * 1_000_000_000_000_000_000); // 10,000 TOLA

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum GenesisValidationError {
    InvalidChainId,
    SupplyMismatch {
        expected: u128,
        actual: u128,
    },
    DuplicateAccount(Address),
    DuplicateValidator(ValidatorId),
    ValidatorAccountNotFound(ValidatorId),
    ValidatorInsufficientSelfStake {
        validator_id: ValidatorId,
        required: Amount,
        available: Amount,
    },
    ZeroVotingPower(ValidatorId),
    EmptyValidators,
    EmptyAccounts,
}

impl std::fmt::Display for GenesisValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidChainId => write!(f, "Invalid chain id"),
            Self::SupplyMismatch { expected, actual } => {
                write!(
                    f,
                    "Total genesis balance mismatch: expected {expected}, got {actual}"
                )
            }
            Self::DuplicateAccount(addr) => write!(f, "Duplicate genesis account: {addr:?}"),
            Self::DuplicateValidator(id) => write!(f, "Duplicate genesis validator: {id:?}"),
            Self::ValidatorAccountNotFound(id) => {
                write!(
                    f,
                    "Genesis validator account not funded/found in accounts: {id:?}"
                )
            }
            Self::ValidatorInsufficientSelfStake {
                validator_id,
                required,
                available,
            } => {
                write!(f, "Validator {validator_id:?} has insufficient self-stake: required {required:?}, available {available:?}")
            }
            Self::ZeroVotingPower(id) => write!(f, "Validator {id:?} has zero voting power"),
            Self::EmptyValidators => write!(f, "Genesis has no initial validators"),
            Self::EmptyAccounts => write!(f, "Genesis has no accounts"),
        }
    }
}

impl std::error::Error for GenesisValidationError {}

/// Formally validates a genesis specification according to protocol rules:
/// 1. Non-empty accounts and initial validators.
/// 2. For Mainnet (ChainId 9999), total account balances must strictly sum to 1,000,000,000 TOLA (10^27 base units).
/// 3. No duplicate account addresses.
/// 4. No duplicate initial validators.
/// 5. Every initial validator must have an explicit backing genesis account with at least 10,000 TOLA self-stake.
/// 6. Every initial validator must have non-zero voting power.
/// 7. Deterministic state root can be computed.
pub fn validate_genesis_config(genesis: &GenesisConfig) -> Result<(), GenesisValidationError> {
    if genesis.accounts.is_empty() {
        return Err(GenesisValidationError::EmptyAccounts);
    }
    if genesis.initial_validators.is_empty() {
        return Err(GenesisValidationError::EmptyValidators);
    }

    // 1. Fixed total supply verification for mainnet
    if genesis.chain_id == ChainId::MAINNET {
        let total_supply: u128 = genesis.accounts.iter().map(|a| a.balance.0).sum();
        let expected_supply: u128 = 1_000_000_000 * 1_000_000_000_000_000_000; // 1 Billion TOLA
        if total_supply != expected_supply {
            return Err(GenesisValidationError::SupplyMismatch {
                expected: expected_supply,
                actual: total_supply,
            });
        }
    }

    // 2. Duplicate account verification & account balance map
    let mut account_balances = std::collections::HashMap::new();
    for acct in &genesis.accounts {
        if account_balances
            .insert(acct.address, acct.balance)
            .is_some()
        {
            return Err(GenesisValidationError::DuplicateAccount(acct.address));
        }
    }

    // 3. Duplicate validator verification & self-stake funding verification
    let mut validator_ids = std::collections::HashSet::new();
    for val in &genesis.initial_validators {
        if !validator_ids.insert(val.validator_id) {
            return Err(GenesisValidationError::DuplicateValidator(val.validator_id));
        }
        if val.voting_power == 0 {
            return Err(GenesisValidationError::ZeroVotingPower(val.validator_id));
        }

        // Validator must have a funded account backing its required self-stake
        match account_balances.get(&val.validator_id.0) {
            None => {
                return Err(GenesisValidationError::ValidatorAccountNotFound(
                    val.validator_id,
                ));
            }
            Some(&balance) => {
                if balance < MIN_VALIDATOR_SELF_STAKE {
                    return Err(GenesisValidationError::ValidatorInsufficientSelfStake {
                        validator_id: val.validator_id,
                        required: MIN_VALIDATOR_SELF_STAKE,
                        available: balance,
                    });
                }
            }
        }
    }

    // 4. Deterministic state root computability verification
    let root = compute_genesis_state_root(genesis);
    if root == Hash::ZERO {
        return Err(GenesisValidationError::SupplyMismatch {
            expected: 1,
            actual: 0,
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_genesis_deterministic_root_invariance() {
        let addr1 = Address([1u8; 20]);
        let addr2 = Address([2u8; 20]);
        let addr3 = Address([3u8; 20]);

        let genesis_a = GenesisConfig {
            chain_id: ChainId::TESTNET,
            genesis_time: 1770000000,
            accounts: vec![
                GenesisAccount {
                    address: addr1,
                    balance: Amount(1_000_000_000),
                },
                GenesisAccount {
                    address: addr2,
                    balance: Amount(2_000_000_000),
                },
                GenesisAccount {
                    address: addr3,
                    balance: Amount(3_000_000_000),
                },
            ],
            initial_validators: vec![],
        };

        // Permuted order: addr3, addr1, addr2
        let genesis_b = GenesisConfig {
            chain_id: ChainId::TESTNET,
            genesis_time: 1770000000,
            accounts: vec![
                GenesisAccount {
                    address: addr3,
                    balance: Amount(3_000_000_000),
                },
                GenesisAccount {
                    address: addr1,
                    balance: Amount(1_000_000_000),
                },
                GenesisAccount {
                    address: addr2,
                    balance: Amount(2_000_000_000),
                },
            ],
            initial_validators: vec![],
        };

        let root_a = compute_genesis_state_root(&genesis_a);
        let root_b = compute_genesis_state_root(&genesis_b);

        assert_ne!(root_a, Hash::ZERO);
        assert_eq!(
            root_a, root_b,
            "Genesis state root must be strictly invariant under account order"
        );
    }

    #[test]
    fn test_genesis_json_roundtrip() {
        let addr = Address([0x42; 20]);
        let val_id = ValidatorId(Address([0x99; 20]));
        let pubkey = PublicKey::Ed25519([0xaa; 32]);

        let original = GenesisConfig {
            chain_id: ChainId(102),
            genesis_time: 1770000000,
            accounts: vec![GenesisAccount {
                address: addr,
                balance: Amount(500_000_000_000_000_000_000), // 500 TOLA
            }],
            initial_validators: vec![ValidatorInfo {
                validator_id: val_id,
                consensus_pubkey: pubkey,
                voting_power: 100_000,
                commission_bp: 500,
            }],
        };

        let json = serialize_genesis_json(&original);
        let parsed = parse_genesis_json(&json).expect("Parse genesis JSON");

        assert_eq!(parsed.chain_id, original.chain_id);
        assert_eq!(parsed.genesis_time, original.genesis_time);
        assert_eq!(parsed.accounts.len(), 1);
        assert_eq!(parsed.accounts[0].address, addr);
        assert_eq!(
            parsed.accounts[0].balance,
            Amount(500_000_000_000_000_000_000)
        );
        assert_eq!(parsed.initial_validators.len(), 1);
        assert_eq!(parsed.initial_validators[0].validator_id, val_id);
        assert_eq!(parsed.initial_validators[0].voting_power, 100_000);
        assert_eq!(parsed.initial_validators[0].commission_bp, 500);
    }

    #[test]
    fn test_validate_genesis_config_detects_unfunded_validator() {
        let val_addr = Address([0x99; 20]);
        let val_id = ValidatorId(val_addr);
        let pubkey = PublicKey::Ed25519([0xaa; 32]);

        // Account address is 0x42, but validator is 0x99 (unfunded!)
        let config = GenesisConfig {
            chain_id: ChainId::TESTNET,
            genesis_time: 1770000000,
            accounts: vec![GenesisAccount {
                address: Address([0x42; 20]),
                balance: Amount(500_000_000_000_000_000_000),
            }],
            initial_validators: vec![ValidatorInfo {
                validator_id: val_id,
                consensus_pubkey: pubkey,
                voting_power: 100_000,
                commission_bp: 500,
            }],
        };

        let err = validate_genesis_config(&config).unwrap_err();
        assert_eq!(
            err,
            GenesisValidationError::ValidatorAccountNotFound(val_id)
        );
    }

    #[test]
    fn test_validate_genesis_config_detects_insufficient_validator_stake() {
        let val_addr = Address([0x99; 20]);
        let val_id = ValidatorId(val_addr);
        let pubkey = PublicKey::Ed25519([0xaa; 32]);

        // Backing account exists, but has only 5,000 TOLA (threshold is 10,000 TOLA)
        let config = GenesisConfig {
            chain_id: ChainId::TESTNET,
            genesis_time: 1770000000,
            accounts: vec![GenesisAccount {
                address: val_addr,
                balance: Amount(5_000 * 1_000_000_000_000_000_000),
            }],
            initial_validators: vec![ValidatorInfo {
                validator_id: val_id,
                consensus_pubkey: pubkey,
                voting_power: 100_000,
                commission_bp: 500,
            }],
        };

        let err = validate_genesis_config(&config).unwrap_err();
        assert_eq!(
            err,
            GenesisValidationError::ValidatorInsufficientSelfStake {
                validator_id: val_id,
                required: MIN_VALIDATOR_SELF_STAKE,
                available: Amount(5_000 * 1_000_000_000_000_000_000),
            }
        );
    }

    #[test]
    fn test_validate_genesis_config_detects_duplicates() {
        let addr = Address([0x42; 20]);
        let val_id = ValidatorId(addr);
        let pubkey = PublicKey::Ed25519([0xaa; 32]);

        let dup_account_config = GenesisConfig {
            chain_id: ChainId::TESTNET,
            genesis_time: 1770000000,
            accounts: vec![
                GenesisAccount {
                    address: addr,
                    balance: MIN_VALIDATOR_SELF_STAKE,
                },
                GenesisAccount {
                    address: addr,
                    balance: MIN_VALIDATOR_SELF_STAKE,
                },
            ],
            initial_validators: vec![ValidatorInfo {
                validator_id: val_id,
                consensus_pubkey: pubkey,
                voting_power: 100_000,
                commission_bp: 500,
            }],
        };

        let err = validate_genesis_config(&dup_account_config).unwrap_err();
        assert_eq!(err, GenesisValidationError::DuplicateAccount(addr));
    }
}
