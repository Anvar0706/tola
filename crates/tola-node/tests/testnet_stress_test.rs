//! Testnet stress testing and high-throughput multi-account transaction workload suite.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use tola_core::StateDB;
use tola_crypto::Ed25519Keypair;
use tola_rpc::server::RpcServer;
use tola_sdk::builder::TransactionBuilder;
use tola_sdk::client::TolaClient;
use tola_sdk::wallet::TolaWallet;
use tola_storage::PersistentStateDB;
use tola_types::*;

use tola_node::config::{GenesisAccount, GenesisConfig, NodeConfig};
use tola_node::engine::TolaNode;

fn unique_temp_dir(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("tola-test-stress-{}-{}", label, nanos));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn test_concurrent_multi_account_transfer_load() {
    let chain_id = ChainId(102);
    let temp_dir = unique_temp_dir("multi-acct-stress");
    let state_db = PersistentStateDB::open(temp_dir.join("state")).unwrap();

    // 1. Generate 5 sender wallets and 5 distinct recipient addresses
    let num_senders = 5;
    let txs_per_sender = 10;
    let initial_balance_per_sender = 10_000_000_000_000_000_000u128; // 10 TOLA

    let mut senders = Vec::new();
    let mut genesis_accounts = Vec::new();

    for _ in 0..num_senders {
        let wallet = TolaWallet::generate();
        let addr = wallet.address();
        genesis_accounts.push(GenesisAccount {
            address: addr,
            balance: Amount(initial_balance_per_sender),
        });
        senders.push(wallet);
    }

    let val_kp = Ed25519Keypair::generate();
    let val_addr = Address([0x99; 20]);
    let val_info = ValidatorInfo {
        validator_id: ValidatorId(val_addr),
        consensus_pubkey: val_kp.public_key(),
        voting_power: 100_000,
        commission_bp: 500,
    };

    let genesis_cfg = GenesisConfig {
        chain_id,
        genesis_time: 1770000000,
        accounts: genesis_accounts,
        initial_validators: vec![val_info.clone()],
    };

    let committee = ValidatorSet {
        epoch: Epoch(0),
        validators: vec![val_info],
        total_voting_power: 100_000,
    };

    let node_cfg = NodeConfig {
        chain_id,
        data_dir: temp_dir.clone(),
        is_validator: true,
        validator_keypair: Some(val_kp),
        operator_address: Some(val_addr),
        p2p_port: 39500,
        rpc_port: 8590,
        genesis: genesis_cfg,
    };

    let node = TolaNode::new(node_cfg, state_db, committee).expect("Initialize node");

    // 2. Build 50 signed transactions across the 5 sender accounts
    let transfer_amount = Amount(10_000_000); // 10,000,000 base units per tx
    let mut all_txs = Vec::new();

    for (s_idx, sender) in senders.iter().enumerate() {
        for nonce_val in 0..txs_per_sender {
            let recipient = Address([(s_idx * 10 + nonce_val + 1) as u8; 20]);
            let tx = TransactionBuilder::new(chain_id)
                .to(recipient)
                .value(transfer_amount)
                .nonce(Nonce(nonce_val as u64))
                .gas_limit(Gas(21_000))
                .max_fee_per_gas(Amount(1_000_000_000))
                .max_priority_fee_per_gas(Amount(1_000_000_000))
                .build_and_sign(sender)
                .expect("Build and sign transaction");
            all_txs.push(tx);
        }
    }

    assert_eq!(all_txs.len(), num_senders * txs_per_sender);

    // 3. Ingest all 50 transactions into node mempool
    {
        let mut mp = node.mempool.write().unwrap();
        let state = node.state_db.read().unwrap();
        let h = state.current_height();
        for tx in all_txs {
            let res = mp.insert(tx, &*state, h);
            assert!(res.is_ok(), "Mempool insert must succeed: {:?}", res);
        }
        assert_eq!(mp.stats().total_transactions, num_senders * txs_per_sender);
    }

    // 4. Propose block: BlockAssembler groups transactions, Block-STM executes in parallel
    let start = std::time::Instant::now();
    let block = node
        .propose_block(None, Round(0), 1770001000 * 1000)
        .expect("Propose and execute block with 50 txs");
    let elapsed = start.elapsed();
    assert!(
        elapsed < std::time::Duration::from_secs(15),
        "Block execution took too long: {:?}",
        elapsed
    );

    assert_eq!(
        block.transactions.len(),
        50,
        "Block must include all 50 transactions"
    );
    assert_ne!(
        block.header.state_root,
        Hash::ZERO,
        "State root must be computed"
    );
    assert_eq!(node.current_height(), BlockHeight(1));

    // 5. Verify state accounts: all senders must have advanced nonces and debited balances
    {
        let state = node.state_db.read().unwrap();
        for sender in &senders {
            let acct = state.get_account(&sender.address()).unwrap().unwrap();
            assert_eq!(
                acct.nonce,
                Nonce(txs_per_sender as u64),
                "Nonce must advance to 10"
            );
            assert!(
                acct.balance.0 < initial_balance_per_sender,
                "Balance must be debited"
            );
        }
    }

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_client_sdk_end_to_end_against_node() {
    let chain_id = ChainId(102);
    let temp_dir = unique_temp_dir("sdk-client-e2e");
    let state_db = PersistentStateDB::open(temp_dir.join("state")).unwrap();

    let alice_wallet = TolaWallet::generate();
    let alice_addr = alice_wallet.address();
    let initial_balance = Amount(5_000_000_000_000_000_000); // 5 TOLA

    let val_kp = Ed25519Keypair::generate();
    let val_addr = Address([0x88; 20]);
    let val_info = ValidatorInfo {
        validator_id: ValidatorId(val_addr),
        consensus_pubkey: val_kp.public_key(),
        voting_power: 100_000,
        commission_bp: 500,
    };

    let genesis_cfg = GenesisConfig {
        chain_id,
        genesis_time: 1770000000,
        accounts: vec![GenesisAccount {
            address: alice_addr,
            balance: initial_balance,
        }],
        initial_validators: vec![val_info.clone()],
    };

    let committee = ValidatorSet {
        epoch: Epoch(0),
        validators: vec![val_info],
        total_voting_power: 100_000,
    };

    let node_cfg = NodeConfig {
        chain_id,
        data_dir: temp_dir.clone(),
        is_validator: false,
        validator_keypair: None,
        operator_address: None,
        p2p_port: 39510,
        rpc_port: 0,
        genesis: genesis_cfg,
    };

    let node = TolaNode::new(node_cfg, state_db, committee).unwrap();

    let mut server = RpcServer::new("127.0.0.1:0".parse().unwrap(), node.rpc.clone(), None);
    let bound = server.start().expect("Start HTTP RPC server");

    // Connect typed SDK client to the running node
    let client = TolaClient::new(&bound.to_string());

    // 1. Check health
    assert!(client.check_health().expect("Health check"));

    // 2. Query chain ID and block number
    assert_eq!(client.chain_id().expect("Chain ID"), 102);
    assert_eq!(client.block_number().expect("Block number"), 0);

    // 3. Query balance of Alice
    let balance = client.get_balance(&alice_addr).expect("Get balance");
    assert_eq!(balance, initial_balance);

    // 4. Query nonce of Alice
    let nonce = client.get_nonce(&alice_addr).expect("Get nonce");
    assert_eq!(nonce, Nonce(0));

    // 5. Send transaction via SDK
    let bob = Address([0xBB; 20]);
    let tx = TransactionBuilder::new(chain_id)
        .to(bob)
        .value(Amount(1_000_000_000_000_000_000)) // 1 TOLA
        .nonce(Nonce(0))
        .gas_limit(Gas(21_000))
        .max_fee_per_gas(Amount(1_000_000_000))
        .max_priority_fee_per_gas(Amount(1_000_000_000))
        .build_and_sign(&alice_wallet)
        .expect("Build and sign tx");

    let tx_hash = client.send_transaction(&tx).expect("Send transaction");
    assert_ne!(tx_hash, Hash::ZERO);

    // Verify transaction reached node's mempool
    assert_eq!(node.mempool.read().unwrap().stats().total_transactions, 1);

    server.shutdown();
    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_seed_phrase_wallet_end_to_end_against_node() {
    let chain_id = ChainId(102);
    let temp_dir = unique_temp_dir("seed-wallet-e2e");
    let state_db = PersistentStateDB::open(temp_dir.join("state")).unwrap();

    // 1. Derive wallet from canonical BIP-39 mnemonic (Vector 1) under v1.0 Standard (m/44'/9999'/0'/0/0)
    let phrase = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
    let seed_wallet =
        TolaWallet::from_mnemonic(phrase, "", &Network::Testnet).expect("Derive seed wallet");
    let seed_addr = seed_wallet.address();

    // Verify canonical v1.0 test vector outputs (Model A: identical address across networks)
    assert_eq!(
        seed_addr.to_hex(),
        "0x3b45664e195dbacd0035046b511491e09bd3ca8a"
    );
    assert_eq!(
        seed_wallet.address_bech32m(&Network::Mainnet),
        "tola18dzkvnsetkav6qp4q344z9y3uzda8j5263ez6j"
    );
    assert_eq!(
        seed_wallet.address_bech32m(&Network::Testnet),
        "ttola18dzkvnsetkav6qp4q344z9y3uzda8j527xgjrh"
    );

    // Also verify legacy testnet path is distinguishable
    let legacy_wallet =
        TolaWallet::from_mnemonic_legacy_testnet(phrase, "").expect("Derive legacy");
    assert_eq!(
        legacy_wallet.address().to_hex(),
        "0x8e3719da26b9408f27fa9602c4362d9fbc49dc73"
    );

    let initial_balance = Amount(10_000_000_000_000_000_000); // 10 TOLA

    let val_kp = Ed25519Keypair::generate();
    let val_addr = Address([0x77; 20]);
    let val_info = ValidatorInfo {
        validator_id: ValidatorId(val_addr),
        consensus_pubkey: val_kp.public_key(),
        voting_power: 100_000,
        commission_bp: 500,
    };

    let genesis_cfg = GenesisConfig {
        chain_id,
        genesis_time: 1770000000,
        accounts: vec![GenesisAccount {
            address: seed_addr,
            balance: initial_balance,
        }],
        initial_validators: vec![val_info.clone()],
    };

    let committee = ValidatorSet {
        epoch: Epoch(0),
        validators: vec![val_info],
        total_voting_power: 100_000,
    };

    let node_cfg = NodeConfig {
        chain_id,
        data_dir: temp_dir.clone(),
        is_validator: true,
        validator_keypair: Some(val_kp),
        operator_address: Some(val_addr),
        p2p_port: 39520,
        rpc_port: 0,
        genesis: genesis_cfg,
    };

    let node = TolaNode::new(node_cfg, state_db, committee).unwrap();
    let mut server = RpcServer::new("127.0.0.1:0".parse().unwrap(), node.rpc.clone(), None);
    let bound = server.start().expect("Start HTTP RPC server");

    let client = TolaClient::new(&bound.to_string());

    // 2. Query node health and seed account balance via RPC
    assert!(client.check_health().expect("Health check"));
    assert_eq!(
        client.get_balance(&seed_addr).expect("Query balance"),
        initial_balance
    );
    assert_eq!(client.get_nonce(&seed_addr).expect("Query nonce"), Nonce(0));

    // 3. Construct real transaction and sign locally with seed-derived wallet
    let recipient = Address([0xEE; 20]);
    let transfer_amount = Amount(2_500_000_000_000_000_000); // 2.5 TOLA
    let tx = TransactionBuilder::new(chain_id)
        .to(recipient)
        .value(transfer_amount)
        .nonce(Nonce(0))
        .gas_limit(Gas(21_000))
        .max_fee_per_gas(Amount(1_000_000_000))
        .max_priority_fee_per_gas(Amount(1_000_000_000))
        .build_and_sign(&seed_wallet)
        .expect("Build and sign tx with seed phrase wallet");

    // 4. Submit to local node via RPC (eth_sendRawTransaction)
    let tx_hash = client.send_transaction(&tx).expect("Send transaction");
    assert_ne!(tx_hash, Hash::ZERO);

    // 5. Node validates Secp256k1 signature and admits to mempool
    assert_eq!(node.mempool.read().unwrap().stats().total_transactions, 1);

    // 6. Propose and execute block in node runtime
    let block = node
        .propose_block(None, Round(0), 1770001000 * 1000)
        .expect("Propose and execute block with seed wallet transaction");

    assert_eq!(block.transactions.len(), 1);
    assert_eq!(block.transactions[0].from, seed_addr);
    assert_eq!(node.current_height(), BlockHeight(1));

    // 7. Verify post-execution state
    {
        let state = node.state_db.read().unwrap();
        let sender_acct = state.get_account(&seed_addr).unwrap().unwrap();
        assert_eq!(sender_acct.nonce, Nonce(1));
        assert!(sender_acct.balance.0 < initial_balance.0 - transfer_amount.0);

        let recip_acct = state.get_account(&recipient).unwrap().unwrap();
        assert_eq!(recip_acct.balance, transfer_amount);
    }

    server.shutdown();
    let _ = std::fs::remove_dir_all(&temp_dir);
}

fn to_hex_prefixed(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(2 + bytes.len() * 2);
    s.push_str("0x");
    for b in bytes {
        use std::fmt::Write;
        write!(s, "{:02x}", b).unwrap();
    }
    s
}

#[test]
fn test_phase18_ecosystem_end_to_end_against_node() {
    let chain_id = ChainId(102);
    let temp_dir = unique_temp_dir("phase18-e2e");
    let state_db = PersistentStateDB::open(temp_dir.join("state")).unwrap();

    let alice_wallet = TolaWallet::generate();
    let alice_addr = alice_wallet.address();
    let initial_balance = Amount(10_000_000_000_000_000_000); // 10 TOLA

    let val_key = Ed25519Keypair::generate();
    let val_info = ValidatorInfo {
        validator_id: ValidatorId(Address([0x01; 20])),
        consensus_pubkey: val_key.public_key(),
        voting_power: 100_000,
        commission_bp: 0,
    };

    let genesis_cfg = GenesisConfig {
        chain_id,
        genesis_time: 1770000000,
        accounts: vec![GenesisAccount {
            address: alice_addr,
            balance: initial_balance,
        }],
        initial_validators: vec![val_info.clone()],
    };

    let committee = ValidatorSet {
        epoch: Epoch(0),
        validators: vec![val_info],
        total_voting_power: 100_000,
    };

    let node_cfg = NodeConfig {
        chain_id,
        data_dir: temp_dir.clone(),
        is_validator: false,
        validator_keypair: None,
        operator_address: None,
        p2p_port: 39512,
        rpc_port: 0,
        genesis: genesis_cfg,
    };

    let node = TolaNode::new(node_cfg, state_db, committee).unwrap();

    let mut server = RpcServer::new("127.0.0.1:0".parse().unwrap(), node.rpc.clone(), None);
    let bound = server.start().expect("Start HTTP RPC server");

    let client = TolaClient::new(&bound.to_string());
    assert!(client.check_health().expect("Health check"));

    // ------------------------------------------------------------------------
    // 1. TOLA Connect Flow: Clear-Signing Modal + Memo + Transaction Execution
    // ------------------------------------------------------------------------
    let bob = Address([0xBB; 20]);
    let memo_str = "Invoice #1042 - Payment for services";

    let prompt = tola_types::ecosystem::ClearSigningPrompt {
        domain: "app.tola.network".into(),
        action_name: "Transfer with Memo".into(),
        target_address: bob,
        amount: Amount(2_000_000_000_000_000_000), // 2 TOLA
        asset_symbol: "TOLA".into(),
        max_fee: Gas(21_000),
        nonce: Nonce(0),
        chain_id: 102,
        memo: Some(memo_str.into()),
    };
    assert!(prompt.validate().is_ok());

    let tx = TransactionBuilder::new(chain_id)
        .to(bob)
        .value(Amount(2_000_000_000_000_000_000))
        .nonce(Nonce(0))
        .gas_limit(Gas(21_000))
        .max_fee_per_gas(Amount(1_000_000_000))
        .max_priority_fee_per_gas(Amount(1_000_000_000))
        .memo(memo_str)
        .build_and_sign(&alice_wallet)
        .expect("Build and sign tx with memo");

    let tx_hash = client.send_transaction(&tx).expect("Send transaction");
    assert_ne!(tx_hash, Hash::ZERO);
    assert_eq!(node.mempool.read().unwrap().stats().total_transactions, 1);

    // Node executes Block-STM and commits
    let block = node
        .propose_block(None, Round(0), 1770001000 * 1000)
        .expect("Execute block");
    assert_eq!(block.transactions.len(), 1);
    assert_eq!(block.transactions[0].memo.as_deref(), Some(memo_str));

    // ------------------------------------------------------------------------
    // 2. Negative Assertions: Native Identity & Gifts DO NOT EXIST in RPC
    // ------------------------------------------------------------------------
    let bad_user_res = client.call(
        "tola_resolveUsername",
        vec![tola_rpc::json::JsonValue::String("@alice_test".into())],
    );
    assert!(
        bad_user_res.is_err(),
        "tola_resolveUsername must return error / method not found"
    );

    let bad_gift_res = client.call(
        "tola_getGiftDetails",
        vec![tola_rpc::json::JsonValue::Integer(777)],
    );
    assert!(
        bad_gift_res.is_err(),
        "tola_getGiftDetails must return error / method not found"
    );

    let bad_badge_res = client.call(
        "tola_verifyBadgeAttestation",
        vec![tola_rpc::json::JsonValue::Object(vec![])],
    );
    assert!(
        bad_badge_res.is_err(),
        "tola_verifyBadgeAttestation must return error / method not found"
    );

    // ------------------------------------------------------------------------
    // 3. External Asset Tokenization Flow via Live HTTP JSON-RPC
    // ------------------------------------------------------------------------
    let verifier_kp = tola_crypto::Secp256k1Keypair::generate();
    let verifier_addr = verifier_kp.address();
    let token_id = 777u64;
    let nonce = 1u64;
    let expires_at = 2500000000u64;
    let source_platform = "paxtagram";
    let asset_type = ExternalAssetType::MessengerUsername;
    let external_asset_id = "alice_test";

    let digest = tola_crypto::compute_external_asset_attestation_digest(
        chain_id.0,
        nonce,
        &alice_addr,
        expires_at,
        source_platform,
        asset_type,
        external_asset_id,
    );
    let sig_65 = verifier_kp.sign_recoverable(&digest).unwrap();

    let attestation_obj = tola_rpc::json::JsonValue::Object(vec![
        (
            "chainId".into(),
            tola_rpc::json::JsonValue::Integer(chain_id.0 as i64),
        ),
        (
            "nonce".into(),
            tola_rpc::json::JsonValue::Integer(nonce as i64),
        ),
        (
            "destinationWallet".into(),
            tola_rpc::json::JsonValue::String(alice_addr.to_hex()),
        ),
        (
            "expiresAt".into(),
            tola_rpc::json::JsonValue::Integer(expires_at as i64),
        ),
        (
            "sourcePlatform".into(),
            tola_rpc::json::JsonValue::String(source_platform.into()),
        ),
        (
            "assetType".into(),
            tola_rpc::json::JsonValue::String(asset_type.as_str().into()),
        ),
        (
            "externalAssetId".into(),
            tola_rpc::json::JsonValue::String(external_asset_id.into()),
        ),
        (
            "verifierAddress".into(),
            tola_rpc::json::JsonValue::String(verifier_addr.to_hex()),
        ),
        (
            "signature".into(),
            tola_rpc::json::JsonValue::Object(vec![
                (
                    "r".into(),
                    tola_rpc::json::JsonValue::String(to_hex_prefixed(&sig_65[0..32])),
                ),
                (
                    "s".into(),
                    tola_rpc::json::JsonValue::String(to_hex_prefixed(&sig_65[32..64])),
                ),
                (
                    "v".into(),
                    tola_rpc::json::JsonValue::Integer(sig_65[64] as i64),
                ),
            ]),
        ),
    ]);

    let verify_res = client
        .call("tola_verifyExternalAssetAttestation", vec![attestation_obj])
        .expect("Verify legitimate external asset attestation");
    assert_eq!(
        verify_res.get("valid").and_then(|v| v.as_bool()),
        Some(true)
    );

    let canonical_id = tola_crypto::compute_external_asset_canonical_id(
        source_platform,
        asset_type,
        external_asset_id,
    );
    node.rpc
        .register_external_asset(ExternalAsset {
            token_id,
            canonical_id,
            source_platform: source_platform.to_string(),
            asset_type,
            external_asset_id: external_asset_id.to_string(),
            external_reference: "ref_e2e_777".to_string(),
            owner: alice_addr,
            authorizer: verifier_addr,
            verifier_address: verifier_addr,
            minted_at: 1770001500,
            metadata_uri: "ipfs://bafybeie2e777".to_string(),
        })
        .expect("Register external asset in node");

    let asset_details = client
        .call(
            "tola_getExternalAsset",
            vec![tola_rpc::json::JsonValue::Integer(token_id as i64)],
        )
        .expect("Get external asset details");
    assert_eq!(
        asset_details
            .get("externalAssetId")
            .and_then(|v| v.as_str()),
        Some("alice_test")
    );
    assert_eq!(
        asset_details.get("owner").and_then(|v| v.as_str()),
        Some(alice_addr.to_hex()).as_deref()
    );

    let owner_res = client
        .call(
            "tola_getExternalAssetOwner",
            vec![tola_rpc::json::JsonValue::Integer(token_id as i64)],
        )
        .expect("Get external asset owner");
    assert_eq!(owner_res.as_str(), Some(alice_addr.to_hex()).as_deref());

    let mallory = tola_crypto::Secp256k1Keypair::generate();
    let mal_digest = tola_crypto::compute_external_asset_attestation_digest(
        chain_id.0,
        nonce,
        &alice_addr,
        expires_at,
        source_platform,
        asset_type,
        external_asset_id,
    );
    let mal_sig = mallory.sign_recoverable(&mal_digest).unwrap();
    let mal_obj = tola_rpc::json::JsonValue::Object(vec![
        (
            "chainId".into(),
            tola_rpc::json::JsonValue::Integer(chain_id.0 as i64),
        ),
        (
            "nonce".into(),
            tola_rpc::json::JsonValue::Integer(nonce as i64),
        ),
        (
            "destinationWallet".into(),
            tola_rpc::json::JsonValue::String(alice_addr.to_hex()),
        ),
        (
            "expiresAt".into(),
            tola_rpc::json::JsonValue::Integer(expires_at as i64),
        ),
        (
            "sourcePlatform".into(),
            tola_rpc::json::JsonValue::String(source_platform.into()),
        ),
        (
            "assetType".into(),
            tola_rpc::json::JsonValue::String(asset_type.as_str().into()),
        ),
        (
            "externalAssetId".into(),
            tola_rpc::json::JsonValue::String(external_asset_id.into()),
        ),
        (
            "verifierAddress".into(),
            tola_rpc::json::JsonValue::String(verifier_addr.to_hex()),
        ),
        (
            "signature".into(),
            tola_rpc::json::JsonValue::Object(vec![
                (
                    "r".into(),
                    tola_rpc::json::JsonValue::String(to_hex_prefixed(&mal_sig[0..32])),
                ),
                (
                    "s".into(),
                    tola_rpc::json::JsonValue::String(to_hex_prefixed(&mal_sig[32..64])),
                ),
                (
                    "v".into(),
                    tola_rpc::json::JsonValue::Integer(mal_sig[64] as i64),
                ),
            ]),
        ),
    ]);
    let mal_verify_res = client
        .call("tola_verifyExternalAssetAttestation", vec![mal_obj])
        .expect("Call verification endpoint");
    assert_eq!(
        mal_verify_res.get("valid").and_then(|v| v.as_bool()),
        Some(false),
        "Forged attestation must fail verification"
    );

    server.shutdown();
    let _ = std::fs::remove_dir_all(&temp_dir);
}
