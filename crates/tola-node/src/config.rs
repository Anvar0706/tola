//! Node configuration, network definitions, and genesis specifications.

use std::path::PathBuf;
use tola_crypto::Ed25519Keypair;
use tola_types::*;

#[derive(Clone, Debug)]
pub struct GenesisAccount {
    pub address: Address,
    pub balance: Amount,
}

#[derive(Clone, Debug)]
pub struct GenesisConfig {
    pub chain_id: ChainId,
    pub genesis_time: u64,
    pub accounts: Vec<GenesisAccount>,
    pub initial_validators: Vec<ValidatorInfo>,
}

impl GenesisConfig {
    pub fn mainnet_genesis() -> Self {
        Self {
            chain_id: ChainId::MAINNET,
            genesis_time: 1770000000,
            accounts: vec![],
            initial_validators: vec![],
        }
    }

    pub fn testnet_genesis() -> Self {
        Self {
            chain_id: ChainId::TESTNET,
            genesis_time: 1770000000,
            accounts: vec![],
            initial_validators: vec![],
        }
    }
}

#[derive(Clone)]
pub struct NodeConfig {
    pub chain_id: ChainId,
    pub data_dir: PathBuf,
    pub is_validator: bool,
    pub validator_keypair: Option<Ed25519Keypair>,
    pub operator_address: Option<Address>,
    pub p2p_port: u16,
    pub rpc_port: u16,
    pub genesis: GenesisConfig,
}

impl Default for NodeConfig {
    fn default() -> Self {
        Self {
            chain_id: ChainId::MAINNET,
            data_dir: PathBuf::from("./data"),
            is_validator: false,
            validator_keypair: None,
            operator_address: None,
            p2p_port: 30303,
            rpc_port: 8545,
            genesis: GenesisConfig::mainnet_genesis(),
        }
    }
}
