"""
Unit test suite for TOLA Network Python SDK (`tola`).
Validates full compliance with TOLA Wallet Derivation Standard v1.0,
cryptographic parity with Rust tola-crypto/tola-sdk and TypeScript @tola/sdk,
BIP-39 recovery scenarios, security hardening, and live node integration.
"""

import unittest
import sys
import os
import json
import time
import random
import subprocess
import tempfile
import shutil

# Add sdk/python to sys.path
sys.path.insert(0, os.path.abspath(os.path.join(os.path.dirname(__file__), "..")))

from tola import (
    Address,
    Network,
    TolaWallet,
    TransactionBuilder,
    TolaClient,
    keccak256,
    blake3,
    recover_address,
    serialize_unsigned_transaction,
    serialize_signed_transaction,
    compute_transaction_digest,
    generate_mnemonic,
    validate_mnemonic,
    mnemonic_to_seed,
    CANONICAL_DERIVATION_PATH,
    LEGACY_TESTNET_PROVISIONAL_PATH,
    ClearSigningPrompt,
    ClearSigningValidationError,
    ConnectMessageType,
    DeviceRegistry,
    TolaConnectSession,
    MAX_MEMO_BYTES,
    compute_shard_id,
    compute_receipt_nullifier,
    verify_merkle_inclusion_proof,
    EXTERNAL_ASSET_CONTRACT_ADDRESS,
    ExternalAssetType,
    ExternalAssetError,
    compute_external_asset_canonical_id,
    compute_external_asset_attestation_digest,
    sign_external_asset_attestation,
    verify_external_asset_attestation,
    validate_external_asset_metadata,
    encode_mint_external_asset_calldata,
    encode_transfer_external_asset_calldata,
    encode_burn_external_asset_calldata,
)


class TestTolaSdk(unittest.TestCase):
    PHRASE_12 = (
        "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about"
    )
    PHRASE_24 = (
        "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon "
        "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon art"
    )

    # =========================================================================
    # CORE CRYPTO & SERIALIZATION TESTS
    # =========================================================================

    def test_keccak256_vector(self):
        empty = keccak256(b"").hex()
        self.assertEqual(
            empty,
            "c5d2460186f7233c927e7db2dcc703c0e500b653ca82273b7bfad8045d85a470",
        )

    def test_blake3_vector(self):
        empty = blake3(b"").hex()
        self.assertEqual(
            empty,
            "af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262",
        )

    def test_bech32m_roundtrip(self):
        raw = bytes(
            [
                0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC, 0xDE, 0xF0,
                0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88,
                0x99, 0xAA, 0xBB, 0xCC,
            ]
        )
        addr = Address(raw)

        # Mainnet
        main_str = addr.to_bech32m(Network.MAINNET)
        self.assertTrue(main_str.startswith("tola1"))
        dec_addr, hrp = Address.from_bech32m(main_str)
        self.assertEqual(hrp, "tola")
        self.assertEqual(dec_addr, addr)

        # Testnet
        test_str = addr.to_bech32m(Network.TESTNET)
        self.assertTrue(test_str.startswith("ttola1"))
        dec_addr, hrp = Address.from_bech32m(test_str)
        self.assertEqual(hrp, "ttola")
        self.assertEqual(dec_addr, addr)

        # Devnet
        dev_str = addr.to_bech32m(Network.DEVNET)
        self.assertTrue(dev_str.startswith("dtola1"))
        dec_addr, hrp = Address.from_bech32m(dev_str)
        self.assertEqual(hrp, "dtola")
        self.assertEqual(dec_addr, addr)

        # Corruption detection
        corrupted = test_str[:-1] + ("p" if test_str.endswith("q") else "q")
        with self.assertRaises(ValueError):
            Address.from_bech32m(corrupted)

        # Mixed case rejection
        mixed = "TtoLa" + test_str[5:]
        with self.assertRaises(ValueError):
            Address.from_bech32m(mixed)

    def test_canonical_vector_parity_with_rust_and_ts(self):
        priv_hex = "0x4242424242424242424242424242424242424242424242424242424242424242"
        wallet = TolaWallet.from_hex_private_key(priv_hex)

        # 1. Compressed pubkey
        self.assertEqual(
            wallet.public_key_bytes.hex(),
            "0324653eac434488002cc06bbfb7f10fe18991e35f9fe4302dbea6d2353dc0ab1c",
        )

        # 2. Address hex
        self.assertEqual(
            wallet.address.to_hex(), "0x17c5185167401ed00cf5f5b2fc97d9bbfdb7d025"
        )

        # 3. Bech32m addresses
        self.assertEqual(
            wallet.address_bech32m(Network.MAINNET),
            "tola1zlz3s5t8gq0dqr847ke0e97eh07m05p9t9nfc9",
        )
        self.assertEqual(
            wallet.address_bech32m(Network.TESTNET),
            "ttola1zlz3s5t8gq0dqr847ke0e97eh07m05p90jzepq",
        )

        # 4. Construct transaction
        tx = (
            TransactionBuilder(102)
            .nonce(1)
            .to(wallet.address.to_hex())
            .value(1_000_000_000_000_000_000)
            .gas_limit(21000)
            .max_fee_per_gas(1_000_000_000)
            .max_priority_fee_per_gas(1_000_000_000)
            .valid_until_block(100)
            .data("0xaabb")
            .build(wallet.address)
        )

        # 5. Unsigned bytes serialization
        unsigned_bytes = serialize_unsigned_transaction(tx)
        expected_unsigned_hex = (
            "6600000000000000010000000000000017c5185167401ed00cf5f5b2fc97d9bbfdb7d0250117c5185167401ed00cf5f5b2fc97d9bbfdb7d025000064a7b3b6e00d0000000000000000085200000000000000ca9a3b00000000000000000000000000ca9a3b000000000000000000000000640000000000000002000000aabb"
        )
        self.assertEqual(unsigned_bytes.hex(), expected_unsigned_hex)

        # 6. Payload hash
        payload_hash = blake3(unsigned_bytes)
        self.assertEqual(
            "0x" + payload_hash.hex(),
            "0xb3e845a99a136c2eab8250dc6deef7dfe47f3a8545c365c68d202dc5ecff2451",
        )

        # 7. Transaction signing digest
        digest = compute_transaction_digest(tx)
        self.assertEqual(
            "0x" + digest.hex(),
            "0x32f94669e75eaf7c3d4d68388eb100c9c7884a86e1d16af2ee5c7e808281b830",
        )

        # 8. Sign transaction
        signed_tx = wallet.sign_transaction(tx)
        self.assertEqual(len(signed_tx.signature.r), 32)
        self.assertEqual(len(signed_tx.signature.s), 32)
        self.assertIn(signed_tx.signature.v, (0, 1))

        # 9. Signed wire bytes
        wire = serialize_signed_transaction(signed_tx)
        self.assertTrue(wire.hex().startswith(expected_unsigned_hex))
        self.assertEqual(len(wire), len(unsigned_bytes) + 1 + 32 + 32 + 1)

    def test_wallet_generation(self):
        w1 = TolaWallet.generate()
        w2 = TolaWallet.generate()
        self.assertNotEqual(w1.private_key_hex, w2.private_key_hex)
        self.assertNotEqual(w1.address.to_hex(), w2.address.to_hex())

    def test_client_initialization(self):
        c = TolaClient("http://127.0.0.1:8545/")
        self.assertEqual(c.rpc_url, "http://127.0.0.1:8545")

    def test_canonical_network_chain_ids(self):
        self.assertEqual(Network.MAINNET.chain_id, 9999)
        self.assertEqual(Network.TESTNET.chain_id, 102)
        self.assertEqual(Network.DEVNET.chain_id, 1337)

    def test_clear_signing_prompt(self):
        prompt = ClearSigningPrompt(
            domain="dapp.tola.network",
            action_name="Send Application Transfer",
            target_address="tola1zlz3s5t8gq0dqr847ke0e97eh07m05p9t9nfc9",
            amount=50 * (10**18),
            asset_symbol="TOLA",
            max_fee=21000,
            chain_id=9999,
        )
        self.assertTrue(prompt.validate())
        summary = prompt.format_summary()
        self.assertIn("dapp.tola.network", summary)
        self.assertIn("Send Application Transfer", summary)
        self.assertIn("50.0000 TOLA", summary)

        # Invalid prompt raises error
        with self.assertRaises(ClearSigningValidationError):
            ClearSigningPrompt(
                domain="",
                action_name="Tip",
                target_address="0x17c5185167401ed00cf5f5b2fc97d9bbfdb7d025",
                amount=100,
            )

    def test_connect_device_management_lifecycle(self):
        reg = DeviceRegistry()
        dev1 = reg.register_device("device_mac", "MacBook Pro M3")
        self.assertEqual(dev1["device_id"], "device_mac")
        self.assertTrue(reg.is_authorized("device_mac"))

        devices = reg.list_devices()
        self.assertEqual(len(devices), 1)

        reg.revoke_device("device_mac")
        self.assertFalse(reg.is_authorized("device_mac"))
        with self.assertRaises(KeyError):
            reg.revoke_device("non_existent_device")

    def test_connect_session_and_envelope_security(self):
        session = TolaConnectSession.create("app.tola.network", "TolaApp", 300)

        # 1. Valid envelope with nonce 1
        env1 = session.create_envelope(ConnectMessageType.TRANSACTION_REQUEST, {"amount": 500}, "req_1", 1)
        self.assertTrue(session.validate_envelope(env1))

        # 2. Monotonic nonce replay check
        env_replay = session.create_envelope(ConnectMessageType.TRANSACTION_REQUEST, {"amount": 500}, "req_2", 1)
        with self.assertRaises(ValueError):
            session.validate_envelope(env_replay)

        # 3. Next monotonic nonce succeeds
        env2 = session.create_envelope(ConnectMessageType.TRANSACTION_REQUEST, {"amount": 600}, "req_3", 2)
        self.assertTrue(session.validate_envelope(env2))

        # 4. Domain mismatch fails
        bad_domain = dict(env2)
        bad_domain["domain"] = "evil.phishing.network"
        bad_domain["nonce"] = 3
        with self.assertRaises(ValueError):
            session.validate_envelope(bad_domain)

        # 5. Expired envelope fails
        bad_expiry = dict(env2)
        bad_expiry["nonce"] = 4
        bad_expiry["expiresAt"] = int(time.time()) - 20
        with self.assertRaises(ValueError):
            session.validate_envelope(bad_expiry)

    def test_clear_signing_memo_validation_and_bounds(self):
        prompt = ClearSigningPrompt(
            domain="dex.tola.network",
            action_name="Trade TOLA",
            target_address="tola1zlz3s5t8gq0dqr847ke0e97eh07m05p9t9nfc9",
            amount=10 * (10**18),
            memo="Invoice #999",
        )
        self.assertTrue(prompt.validate())
        summary = prompt.format_summary()
        self.assertIn("Invoice #999", summary)

        # 256 bytes memo ok
        ok_prompt = ClearSigningPrompt(
            domain="dex.tola.network",
            action_name="Trade TOLA",
            target_address="tola1zlz3s5t8gq0dqr847ke0e97eh07m05p9t9nfc9",
            amount=10,
            memo="a" * 256,
        )
        self.assertTrue(ok_prompt.validate())

        # 257 bytes memo rejected
        with self.assertRaises(ClearSigningValidationError):
            ClearSigningPrompt(
                domain="dex.tola.network",
                action_name="Trade TOLA",
                target_address="tola1zlz3s5t8gq0dqr847ke0e97eh07m05p9t9nfc9",
                amount=10,
                memo="a" * 257,
            )
    def test_phase18_adversarial_security_hardening(self):
        # 1. Device Re-registration and Duplicate Protection
        reg = DeviceRegistry()
        reg.register_device("device_beta", "Device Beta")
        with self.assertRaises(ValueError):
            reg.register_device("device_beta", "Duplicate Beta")
        reg.revoke_device("device_beta")
        with self.assertRaises(ValueError):
            reg.register_device("device_beta", "Revoked Beta")

        # 2. Secret Leakage Protection
        session = TolaConnectSession.create("app.tola.network", "TolaApp", 300)
        session_repr = repr(session)
        self.assertIn("key=[REDACTED]", session_repr)
        self.assertNotIn(session.session_key.hex(), session_repr)

        # 3. Unknown Message Type Rejection
        bad_type_env = session.create_envelope(ConnectMessageType.TRANSACTION_REQUEST, {"amount": 100}, "req_1", 1)
        bad_type_env["type"] = "MALICIOUS_CUSTOM_TYPE"
        with self.assertRaises(ValueError):
            session.validate_envelope(bad_type_env)

        # 4. Clock Skew Injection Attack (> 60s in future)
        future_env = session.create_envelope(ConnectMessageType.TRANSACTION_REQUEST, {"amount": 100}, "req_skew", 2)
        future_env["timestamp"] = int(time.time()) + 120
        with self.assertRaises(ValueError):
            session.validate_envelope(future_env)

        # 5. Missing / Empty Request ID
        no_req_id = session.create_envelope(ConnectMessageType.TRANSACTION_REQUEST, {"amount": 100}, "", 3)
        with self.assertRaises(ValueError):
            session.validate_envelope(no_req_id)

    # =========================================================================
    # FROZEN TOLA WALLET DERIVATION STANDARD v1.0 TEST SUITE
    # BIP-39 -> BIP-32 -> BIP-44 (m/44'/9999'/0'/0/0) -> Secp256k1 -> Keccak-256
    # =========================================================================

    def test_canonical_derivation_vector_1_parity(self):
        wallet = TolaWallet.from_mnemonic(self.PHRASE_12)

        # 1. Path & Mnemonic preservation
        self.assertEqual(wallet.derivation_path, CANONICAL_DERIVATION_PATH)
        self.assertEqual(wallet.mnemonic, self.PHRASE_12)

        # 2. Private scalar hex
        self.assertEqual(
            wallet.export_private_key_hex(),
            "0xf77999a4d727d792213ae012f18621208ea91a7246e5bf69a4317942abeca750",
        )

        # 3. Compressed & uncompressed public keys
        self.assertEqual(
            wallet.get_public_key(compressed=True).hex(),
            "03e95745eb5f028b4e23fb4c5af97fb49426d522c4228fe6dd922f2585e6b1bf96",
        )
        self.assertEqual(
            wallet.get_public_key(compressed=False).hex(),
            "04e95745eb5f028b4e23fb4c5af97fb49426d522c4228fe6dd922f2585e6b1bf9689629921a91231749eaf52b0198c172ea8a2cce8ee2c9f62f8a6c2fed81c217f",
        )

        # 4. Raw 20-byte address
        self.assertEqual(
            wallet.raw_address, "0x3b45664e195dbacd0035046b511491e09bd3ca8a"
        )

        # 5. Network Bech32m addresses (Model A unified raw address)
        self.assertEqual(
            wallet.get_address("mainnet"),
            "tola18dzkvnsetkav6qp4q344z9y3uzda8j5263ez6j",
        )
        self.assertEqual(
            wallet.get_address("testnet"),
            "ttola18dzkvnsetkav6qp4q344z9y3uzda8j527xgjrh",
        )
        self.assertEqual(
            wallet.get_address("devnet"),
            "dtola18dzkvnsetkav6qp4q344z9y3uzda8j52x0hhl4",
        )

    def test_canonical_derivation_vector_2_parity(self):
        seed = mnemonic_to_seed(self.PHRASE_24, "")
        self.assertEqual(
            seed.hex(),
            "408b285c123836004f4b8842c89324c1f01382450c0d439af345ba7fc49acf705489c6fc77dbd4e3dc1dd8cc6bc9f043db8ada1e243c4a0eafb290d399480840",
        )

        wallet = TolaWallet.from_mnemonic(self.PHRASE_24)
        self.assertEqual(
            wallet.raw_address, "0x37061e043e1b89972ed615c46ff3e5090514c909"
        )
        self.assertEqual(
            wallet.get_address("mainnet"),
            "tola1xurpupp7rwyewtkkzhzxlul9pyz3fjgf4wludn",
        )
        self.assertEqual(
            wallet.get_public_key(compressed=True).hex(),
            "03d20a0a3b709e063b279a5810d91b922c50b5b62ba09ef16b6c3916f1e5d66e3f",
        )

    def test_legacy_testnet_provisional_path_backward_compatibility(self):
        # Vector 1 legacy
        w1 = TolaWallet.from_mnemonic_legacy_testnet(self.PHRASE_12)
        self.assertEqual(
            w1.raw_address, "0x8e3719da26b9408f27fa9602c4362d9fbc49dc73"
        )
        self.assertEqual(
            w1.get_address("testnet"),
            "ttola13cm3nk3xh9qg7fl6jcpvgd3dn77ynhrnukw78v",
        )
        self.assertEqual(
            w1.get_public_key(compressed=True).hex(),
            "02b74bcb92d52c5cfe20ece37e6cc6ae0b12dd58fc328c509e8f904dfc62977403",
        )

        # Vector 2 legacy
        w2 = TolaWallet.from_mnemonic_legacy_testnet(self.PHRASE_24)
        self.assertEqual(
            w2.raw_address, "0x1073f501701b4a14dd4bc6a1f1746fca1fb8de64"
        )

    def test_model_a_unified_account_identity_roundtrip(self):
        wallet = TolaWallet.from_mnemonic(self.PHRASE_12)

        main_str = wallet.get_address("mainnet")
        test_str = wallet.get_address("testnet")
        dev_str = wallet.get_address("devnet")

        dec_main, hrp_main = Address.from_bech32m(main_str)
        dec_test, hrp_test = Address.from_bech32m(test_str)
        dec_dev, hrp_dev = Address.from_bech32m(dev_str)

        self.assertEqual(dec_main.to_hex(), wallet.raw_address)
        self.assertEqual(dec_test.to_hex(), wallet.raw_address)
        self.assertEqual(dec_dev.to_hex(), wallet.raw_address)
        self.assertEqual(hrp_main, "tola")
        self.assertEqual(hrp_test, "ttola")
        self.assertEqual(hrp_dev, "dtola")

    def test_cross_sdk_transaction_signing_and_ecdsa_recovery(self):
        wallet = TolaWallet.from_mnemonic(self.PHRASE_12)

        tx = (
            TransactionBuilder(9999)
            .nonce(42)
            .to("0x8888888888888888888888888888888888888888")
            .value(500000000000000000)  # 0.5 TOLA
            .gas_limit(21000)
            .max_fee_per_gas(1000000000)
            .max_priority_fee_per_gas(1000000000)
            .valid_until_block(200)
            .data(b"")
            .build(wallet.raw_address)
        )

        signed_tx = wallet.sign_transaction(tx)
        self.assertEqual(len(signed_tx.signature.r), 32)
        self.assertEqual(len(signed_tx.signature.s), 32)

        digest = compute_transaction_digest(tx)
        recovered_bytes = recover_address(
            digest,
            signed_tx.signature.r,
            signed_tx.signature.s,
            signed_tx.signature.v,
        )
        self.assertEqual(Address(recovered_bytes).to_hex(), wallet.raw_address)

    # =========================================================================
    # BIP-39 NATIVE RECOVERY STANDARD (9 TEST SCENARIOS)
    # =========================================================================

    def test_bip39_1_random_12_word_cycle(self):
        w = TolaWallet.generate(12)
        self.assertIsNotNone(w.mnemonic)
        self.assertEqual(len(w.mnemonic.split()), 12)
        self.assertEqual(validate_mnemonic(w.mnemonic), w.mnemonic)

        restored = TolaWallet.from_mnemonic(w.mnemonic)
        self.assertEqual(restored.raw_address, w.raw_address)
        self.assertEqual(
            restored.export_private_key_hex(), w.export_private_key_hex()
        )

    def test_bip39_2_random_24_word_cycle(self):
        w = TolaWallet.generate(24)
        self.assertIsNotNone(w.mnemonic)
        self.assertEqual(len(w.mnemonic.split()), 24)
        self.assertEqual(validate_mnemonic(w.mnemonic), w.mnemonic)

        restored = TolaWallet.from_mnemonic(w.mnemonic)
        self.assertEqual(restored.raw_address, w.raw_address)
        self.assertEqual(
            restored.export_private_key_hex(), w.export_private_key_hex()
        )

    def test_bip39_3_12_word_canonical_recovery(self):
        restored = TolaWallet.from_mnemonic(self.PHRASE_12)
        self.assertEqual(
            restored.raw_address, "0x3b45664e195dbacd0035046b511491e09bd3ca8a"
        )

    def test_bip39_4_24_word_canonical_recovery(self):
        restored = TolaWallet.from_mnemonic(self.PHRASE_24)
        self.assertEqual(
            restored.raw_address, "0x37061e043e1b89972ed615c46ff3e5090514c909"
        )

    def test_bip39_5_distinct_mnemonics_produce_distinct_keys(self):
        w1 = TolaWallet.from_mnemonic(self.PHRASE_12)
        w2 = TolaWallet.from_mnemonic(self.PHRASE_24)
        self.assertNotEqual(w1.raw_address, w2.raw_address)
        self.assertNotEqual(
            w1.export_private_key_hex(), w2.export_private_key_hex()
        )

    def test_bip39_6_invalid_checksum_rejected(self):
        bad_checksum = (
            "abandon abandon abandon abandon abandon abandon abandon abandon "
            "abandon abandon abandon zoo"
        )
        with self.assertRaises(ValueError):
            TolaWallet.from_mnemonic(bad_checksum)

    def test_bip39_7_invalid_word_rejected(self):
        bad_word = (
            "abandon abandon abandon abandon abandon abandon abandon abandon "
            "abandon abandon abandon tolanetwork"
        )
        with self.assertRaises(ValueError):
            TolaWallet.from_mnemonic(bad_word)

    def test_bip39_8_passphrase_salt_variation(self):
        w_no_pass = TolaWallet.from_mnemonic(self.PHRASE_12, "")
        w_with_pass = TolaWallet.from_mnemonic(
            self.PHRASE_12, "vault-passphrase-2026"
        )
        self.assertNotEqual(w_no_pass.raw_address, w_with_pass.raw_address)
        self.assertNotEqual(
            w_no_pass.export_private_key_hex(),
            w_with_pass.export_private_key_hex(),
        )

    def test_bip39_9_whitespace_case_nfkd_normalization(self):
        messy = (
            "   ABANDON   abandon   Abandon   abandon abandon abandon "
            "abandon abandon abandon abandon abandon about  "
        )
        clean = TolaWallet.from_mnemonic(self.PHRASE_12)
        normalized = TolaWallet.from_mnemonic(messy)
        self.assertEqual(normalized.raw_address, clean.raw_address)
        self.assertEqual(
            normalized.export_private_key_hex(), clean.export_private_key_hex()
        )

    # =========================================================================
    # CRYPTOGRAPHIC SECURITY: TAMPERING, REPLAY PROTECTION & SECRET ISOLATION
    # =========================================================================

    def test_signature_tampering_rejection_all_fields(self):
        wallet = TolaWallet.from_mnemonic(self.PHRASE_12)
        base_tx = (
            TransactionBuilder(9999)
            .nonce(1)
            .to("0x8888888888888888888888888888888888888888")
            .value(1000000000000000000)
            .gas_limit(21000)
            .max_fee_per_gas(1000000000)
            .max_priority_fee_per_gas(1000000000)
            .valid_until_block(100)
            .data("0x1234")
            .build(wallet.raw_address)
        )

        signed_tx = wallet.sign_transaction(base_tx)
        r, s, v = (
            signed_tx.signature.r,
            signed_tx.signature.s,
            signed_tx.signature.v,
        )

        mutations = [
            (
                "value",
                TransactionBuilder(9999)
                .nonce(1)
                .to("0x8888888888888888888888888888888888888888")
                .value(9999999999999999999)
                .gas_limit(21000)
                .max_fee_per_gas(1000000000)
                .max_priority_fee_per_gas(1000000000)
                .valid_until_block(100)
                .data("0x1234")
                .build(wallet.raw_address),
            ),
            (
                "to",
                TransactionBuilder(9999)
                .nonce(1)
                .to("0x1111111111111111111111111111111111111111")
                .value(1000000000000000000)
                .gas_limit(21000)
                .max_fee_per_gas(1000000000)
                .max_priority_fee_per_gas(1000000000)
                .valid_until_block(100)
                .data("0x1234")
                .build(wallet.raw_address),
            ),
            (
                "nonce",
                TransactionBuilder(9999)
                .nonce(2)
                .to("0x8888888888888888888888888888888888888888")
                .value(1000000000000000000)
                .gas_limit(21000)
                .max_fee_per_gas(1000000000)
                .max_priority_fee_per_gas(1000000000)
                .valid_until_block(100)
                .data("0x1234")
                .build(wallet.raw_address),
            ),
            (
                "gas_limit",
                TransactionBuilder(9999)
                .nonce(1)
                .to("0x8888888888888888888888888888888888888888")
                .value(1000000000000000000)
                .gas_limit(50000)
                .max_fee_per_gas(1000000000)
                .max_priority_fee_per_gas(1000000000)
                .valid_until_block(100)
                .data("0x1234")
                .build(wallet.raw_address),
            ),
            (
                "max_fee_per_gas",
                TransactionBuilder(9999)
                .nonce(1)
                .to("0x8888888888888888888888888888888888888888")
                .value(1000000000000000000)
                .gas_limit(21000)
                .max_fee_per_gas(2000000000)
                .max_priority_fee_per_gas(1000000000)
                .valid_until_block(100)
                .data("0x1234")
                .build(wallet.raw_address),
            ),
            (
                "valid_until_block",
                TransactionBuilder(9999)
                .nonce(1)
                .to("0x8888888888888888888888888888888888888888")
                .value(1000000000000000000)
                .gas_limit(21000)
                .max_fee_per_gas(1000000000)
                .max_priority_fee_per_gas(1000000000)
                .valid_until_block(500)
                .data("0x1234")
                .build(wallet.raw_address),
            ),
            (
                "chain_id",
                TransactionBuilder(102)
                .nonce(1)
                .to("0x8888888888888888888888888888888888888888")
                .value(1000000000000000000)
                .gas_limit(21000)
                .max_fee_per_gas(1000000000)
                .max_priority_fee_per_gas(1000000000)
                .valid_until_block(100)
                .data("0x1234")
                .build(wallet.raw_address),
            ),
            (
                "data",
                TransactionBuilder(9999)
                .nonce(1)
                .to("0x8888888888888888888888888888888888888888")
                .value(1000000000000000000)
                .gas_limit(21000)
                .max_fee_per_gas(1000000000)
                .max_priority_fee_per_gas(1000000000)
                .valid_until_block(100)
                .data("0x9988")
                .build(wallet.raw_address),
            ),
            (
                "memo",
                TransactionBuilder(9999)
                .nonce(1)
                .to("0x8888888888888888888888888888888888888888")
                .value(1000000000000000000)
                .gas_limit(21000)
                .max_fee_per_gas(1000000000)
                .max_priority_fee_per_gas(1000000000)
                .valid_until_block(100)
                .data("0x1234")
                .memo("Tampered Memo")
                .build(wallet.raw_address),
            ),
        ]

        for field, mutated in mutations:
            tampered_digest = compute_transaction_digest(mutated)
            recovered_hex = None
            try:
                rec_bytes = recover_address(tampered_digest, r, s, v)
                recovered_hex = Address(rec_bytes).to_hex()
            except Exception:
                pass  # Elliptic curve point error also safely rejects
            self.assertNotEqual(
                recovered_hex,
                wallet.raw_address,
                f"Tampering field '{field}' must not recover original sender address",
            )

    def test_chain_id_cross_network_replay_protection(self):
        wallet = TolaWallet.from_mnemonic(self.PHRASE_12)

        tx_mainnet = (
            TransactionBuilder(9999)
            .nonce(0)
            .to("0x8888888888888888888888888888888888888888")
            .value(1000000000000000000)
            .build(wallet.raw_address)
        )

        signed_mainnet = wallet.sign_transaction(tx_mainnet)
        digest_mainnet = compute_transaction_digest(tx_mainnet)

        # Attempt replay on Testnet
        tx_testnet = (
            TransactionBuilder(102)
            .nonce(0)
            .to("0x8888888888888888888888888888888888888888")
            .value(1000000000000000000)
            .build(wallet.raw_address)
        )
        digest_testnet = compute_transaction_digest(tx_testnet)

        self.assertNotEqual(digest_mainnet, digest_testnet)

        rec_testnet = recover_address(
            digest_testnet,
            signed_mainnet.signature.r,
            signed_mainnet.signature.s,
            signed_mainnet.signature.v,
        )
        self.assertNotEqual(
            Address(rec_testnet).to_hex(),
            wallet.raw_address,
            "Mainnet signature must not be valid for Testnet digest",
        )

    def test_legacy_wallet_compatibility_input_types(self):
        priv_scalar = (
            0xF77999A4D727D792213AE012F18621208EA91A7246E5BF69A4317942ABECA750
        )
        priv_bytes = priv_scalar.to_bytes(32, "big")
        priv_hex_0x = (
            "0xf77999a4d727d792213ae012f18621208ea91a7246e5bf69a4317942abeca750"
        )
        priv_hex_no0x = (
            "f77999a4d727d792213ae012f18621208ea91a7246e5bf69a4317942abeca750"
        )

        w1 = TolaWallet.from_private_key(priv_scalar)
        w2 = TolaWallet.from_private_key(priv_bytes)
        w3 = TolaWallet.from_private_key(priv_hex_0x)
        w4 = TolaWallet.from_private_key(priv_hex_no0x)
        w5 = TolaWallet.from_hex_private_key(priv_hex_0x)

        self.assertEqual(
            w1.raw_address, "0x3b45664e195dbacd0035046b511491e09bd3ca8a"
        )
        self.assertEqual(w2.raw_address, w1.raw_address)
        self.assertEqual(w3.raw_address, w1.raw_address)
        self.assertEqual(w4.raw_address, w1.raw_address)
        self.assertEqual(w5.raw_address, w1.raw_address)

        self.assertIsNone(w1.mnemonic)
        self.assertIsNone(w2.mnemonic)

    def test_secret_leakage_audit_and_memory_isolation(self):
        wallet = TolaWallet.from_mnemonic(self.PHRASE_12)

        # 1. repr() representation
        repr_str = repr(wallet)
        self.assertNotIn("f77999a4", repr_str)
        self.assertNotIn("abandon", repr_str)
        self.assertIn("tola1", repr_str)

        # 2. str() representation
        str_val = str(wallet)
        self.assertNotIn("f77999a4", str_val)
        self.assertNotIn("abandon", str_val)

        # 3. to_dict() representation
        d = wallet.to_dict()
        self.assertNotIn("private_key", d)
        self.assertNotIn("mnemonic", d)
        self.assertIn("address", d)
        self.assertIn("raw_address", d)
        self.assertEqual(
            d["raw_address"], "0x3b45664e195dbacd0035046b511491e09bd3ca8a"
        )

    def test_arbitrary_message_signing_and_prefix_recovery(self):
        wallet = TolaWallet.from_mnemonic(self.PHRASE_12)
        message = "Hello TOLA Network Genesis 2026"

        sig_hex = wallet.sign_message(message)
        self.assertTrue(sig_hex.startswith("0x"))
        self.assertEqual(len(sig_hex), 2 + 130)  # 65 bytes in hex

        # Direct static recovery
        recovered_addr = TolaWallet.recover_message_address(message, sig_hex)
        self.assertEqual(recovered_addr, wallet.address)

        # Manual digest recovery verification
        msg_bytes = message.encode("utf-8")
        prefix = f"\x19TOLA Signed Message:\n{len(msg_bytes)}".encode("utf-8")
        digest = keccak256(prefix + msg_bytes)

        r_bytes = bytes.fromhex(sig_hex[2:66])
        s_bytes = bytes.fromhex(sig_hex[66:130])
        v = int(sig_hex[130:132], 16)
        manual_rec = recover_address(digest, r_bytes, s_bytes, v)
        self.assertEqual(Address(manual_rec), wallet.address)

    def test_cross_language_parity_verification(self):
        wallet = TolaWallet.from_mnemonic(self.PHRASE_12)
        message = "Hello TOLA Network Genesis 2026"
        expected_sig = (
            "0xb2c3fd06ac1b3044e522b6e0c8e9aa43f931825bac33cb0230399d1b4098d9bf"
            "0810ab4014e2f1b7e73e36afedbda48b26a1be47abab0dd63ae198ef7915c37d01"
        )
        # 1. Exact deterministic signature parity with TypeScript @tola/sdk
        actual_sig = wallet.sign_message(message)
        self.assertEqual(actual_sig, expected_sig)

        # 2. Recover signer address from the cross-language signature
        recovered_addr = TolaWallet.recover_message_address(message, expected_sig)
        self.assertEqual(
            recovered_addr.to_hex(),
            "0x3b45664e195dbacd0035046b511491e09bd3ca8a",
        )


    # =========================================================================
    # END-TO-END LIVE NODE JSON-RPC INTEGRATION
    # =========================================================================

    def test_live_node_transaction_lifecycle(self):
        candidates = [
            os.path.abspath(
                os.path.join(
                    os.path.dirname(__file__),
                    "..",
                    "..",
                    "..",
                    "target",
                    "debug",
                    "tola-node.exe",
                )
            ),
            os.path.abspath("target/debug/tola-node.exe"),
        ]
        node_bin = next((p for p in candidates if os.path.exists(p)), None)
        if not node_bin:
            self.skipTest("tola-node.exe binary not found, skipping live daemon test")
            return

        tmp_dir = tempfile.mkdtemp(prefix="tola-e2e-py-")
        genesis_path = os.path.join(tmp_dir, "genesis.json")

        canonical_addr = "0x3b45664e195dbacd0035046b511491e09bd3ca8a"
        initial_fund = 10_000 * 10**18  # 10,000 TOLA

        genesis = {
            "chain_id": 102,
            "genesis_time": 1770000000,
            "accounts": [
                {
                    "address": canonical_addr,
                    "balance": str(initial_fund),
                }
            ],
            "validators": [],
        }
        with open(genesis_path, "w", encoding="utf-8") as f:
            json.dump(genesis, f)

        rpc_port = random.randint(38500, 39500)
        p2p_port = random.randint(30500, 31500)

        proc = subprocess.Popen(
            [
                node_bin,
                "--chain-id",
                "102",
                "--data-dir",
                tmp_dir,
                "--p2p-addr",
                f"127.0.0.1:{p2p_port}",
                "--rpc-addr",
                f"127.0.0.1:{rpc_port}",
                "--validator",
                "--genesis",
                genesis_path,
                "--log-level",
                "error",
            ],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )

        client = TolaClient(f"http://127.0.0.1:{rpc_port}", timeout=5.0)

        try:
            # 1. Wait for node startup
            ready = False
            for _ in range(30):
                try:
                    if client.check_health():
                        ready = True
                        break
                except Exception:
                    pass
                time.sleep(0.15)
            self.assertTrue(ready, "Node failed to report healthy within timeout")

            # 2. Query initial balance of canonical Vector 1 address
            init_bal = client.get_balance(canonical_addr)
            self.assertEqual(init_bal, initial_fund)

            # 3. Restore wallet from Vector 1 mnemonic
            wallet = TolaWallet.from_mnemonic(self.PHRASE_12)
            self.assertEqual(wallet.raw_address, canonical_addr)

            # 4. Construct and sign transfer transaction
            recipient = "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"
            transfer_amount = 25 * 10**17  # 2.5 TOLA

            tx = (
                TransactionBuilder(102)
                .to(recipient)
                .value(transfer_amount)
                .nonce(0)
                .gas_limit(21000)
                .max_fee_per_gas(1_000_000_000)
                .max_priority_fee_per_gas(1_000_000_000)
                .valid_until_block(1000)
                .build(wallet.raw_address)
            )

            signed_tx = wallet.sign_transaction(tx)

            # 5. Submit via eth_sendRawTransaction
            tx_hash = client.send_transaction(signed_tx)
            self.assertTrue(tx_hash.startswith("0x"))

            # 6. Wait for block execution
            executed = False
            for _ in range(30):
                time.sleep(0.15)
                height = client.get_block_number()
                if height >= 1:
                    executed = True
                    break
            self.assertTrue(executed, "Block #1 failed to commit within timeout")

            # 7. Verify state updates
            recip_bal = client.get_balance(recipient)
            self.assertEqual(recip_bal, transfer_amount)

            sender_nonce = client.get_nonce(canonical_addr)
            self.assertEqual(sender_nonce, 1)

            sender_remaining = client.get_balance(canonical_addr)
            self.assertLess(sender_remaining, initial_fund - transfer_amount)

        finally:
            proc.terminate()
            try:
                proc.wait(timeout=2.0)
            except subprocess.TimeoutExpired:
                proc.kill()
            time.sleep(0.3)
            shutil.rmtree(tmp_dir, ignore_errors=True)

    # =========================================================================
    # TRANSACTION MEMO / COMMENT PARITY & LIMITS
    # =========================================================================

    def test_memo_builder_normalization(self):
        b1 = TransactionBuilder(102).memo("")
        self.assertIsNone(b1.build("0x3b45664e195dbacd0035046b511491e09bd3ca8a").memo)

        b2 = TransactionBuilder(102).memo("   ")
        self.assertIsNone(b2.build("0x3b45664e195dbacd0035046b511491e09bd3ca8a").memo)

        b3 = TransactionBuilder(102).memo("Invoice #1042")
        self.assertEqual(
            b3.build("0x3b45664e195dbacd0035046b511491e09bd3ca8a").memo,
            "Invoice #1042",
        )

    def test_memo_backward_compatibility(self):
        base_tx = (
            TransactionBuilder(102)
            .nonce(1)
            .to("0x3b45664e195dbacd0035046b511491e09bd3ca8a")
            .value(1000000000)
            .gas_limit(21000)
            .max_fee_per_gas(1000000000)
            .max_priority_fee_per_gas(1000000000)
            .valid_until_block(100)
            .data("0xaabb")
            .build("0x3b45664e195dbacd0035046b511491e09bd3ca8a")
        )

        tx_null = (
            TransactionBuilder(102)
            .nonce(1)
            .to("0x3b45664e195dbacd0035046b511491e09bd3ca8a")
            .value(1000000000)
            .gas_limit(21000)
            .max_fee_per_gas(1000000000)
            .max_priority_fee_per_gas(1000000000)
            .valid_until_block(100)
            .data("0xaabb")
            .memo(None)
            .build("0x3b45664e195dbacd0035046b511491e09bd3ca8a")
        )

        tx_empty = (
            TransactionBuilder(102)
            .nonce(1)
            .to("0x3b45664e195dbacd0035046b511491e09bd3ca8a")
            .value(1000000000)
            .gas_limit(21000)
            .max_fee_per_gas(1000000000)
            .max_priority_fee_per_gas(1000000000)
            .valid_until_block(100)
            .data("0xaabb")
            .memo("")
            .build("0x3b45664e195dbacd0035046b511491e09bd3ca8a")
        )

        b_base = serialize_unsigned_transaction(base_tx)
        b_null = serialize_unsigned_transaction(tx_null)
        b_empty = serialize_unsigned_transaction(tx_empty)

        self.assertEqual(b_null, b_base)
        self.assertEqual(b_empty, b_base)
        self.assertEqual(
            compute_transaction_digest(tx_null),
            compute_transaction_digest(base_tx),
        )

    def test_memo_utf8_multilingual_serialization(self):
        base_tx = (
            TransactionBuilder(102)
            .nonce(1)
            .to("0x3b45664e195dbacd0035046b511491e09bd3ca8a")
            .value(1000000000)
            .gas_limit(21000)
            .max_fee_per_gas(1000000000)
            .max_priority_fee_per_gas(1000000000)
            .valid_until_block(100)
            .data("0xaabb")
            .build("0x3b45664e195dbacd0035046b511491e09bd3ca8a")
        )
        legacy_bytes = serialize_unsigned_transaction(base_tx)

        memos = ["Invoice #1042", "To‘lov #1042", "💰💎🚀"]
        for m in memos:
            tx = (
                TransactionBuilder(102)
                .nonce(1)
                .to("0x3b45664e195dbacd0035046b511491e09bd3ca8a")
                .value(1000000000)
                .gas_limit(21000)
                .max_fee_per_gas(1000000000)
                .max_priority_fee_per_gas(1000000000)
                .valid_until_block(100)
                .data("0xaabb")
                .memo(m)
                .build("0x3b45664e195dbacd0035046b511491e09bd3ca8a")
            )
            serialized = serialize_unsigned_transaction(tx)
            expected_bytes = m.encode("utf-8")

            self.assertEqual(
                len(serialized),
                len(legacy_bytes) + 4 + len(expected_bytes),
            )

            # Check 4-byte LE length prefix
            memo_len = int.from_bytes(
                serialized[len(legacy_bytes) : len(legacy_bytes) + 4], "little"
            )
            self.assertEqual(memo_len, len(expected_bytes))

            # Check UTF-8 payload
            payload = serialized[len(legacy_bytes) + 4 :]
            self.assertEqual(payload, expected_bytes)

    def test_memo_max_length_bounds_enforcement(self):
        base_builder = (
            TransactionBuilder(102)
            .nonce(1)
            .to("0x3b45664e195dbacd0035046b511491e09bd3ca8a")
            .value(1000)
            .gas_limit(21000)
            .max_fee_per_gas(1000000000)
            .max_priority_fee_per_gas(1000000000)
            .valid_until_block(100)
            .data(b"")
        )

        memo_256 = "A" * 256
        tx_256 = base_builder.memo(memo_256).build("0x3b45664e195dbacd0035046b511491e09bd3ca8a")
        # Should not raise
        serialize_unsigned_transaction(tx_256)

        memo_257 = "A" * 257
        tx_257 = base_builder.memo(memo_257).build("0x3b45664e195dbacd0035046b511491e09bd3ca8a")
        with self.assertRaises(ValueError):
            serialize_unsigned_transaction(tx_257)

    def test_memo_cryptographic_protection_tampering(self):
        wallet = TolaWallet.from_mnemonic(self.PHRASE_12)
        recipient = "0x1111111111111111111111111111111111111111"

        tx = (
            TransactionBuilder(102)
            .to(recipient)
            .value(1000000000)
            .nonce(0)
            .memo("Invoice #1042")
            .build(wallet.raw_address)
        )

        signed_tx = wallet.sign_transaction(tx)
        digest = compute_transaction_digest(signed_tx)

        recovered = recover_address(
            digest,
            signed_tx.signature.r,
            signed_tx.signature.s,
            signed_tx.signature.v,
        )
        self.assertEqual(Address(recovered).to_hex(), wallet.raw_address)

        # Tampering with memo changes digest
        tampered_tx = (
            TransactionBuilder(102)
            .to(recipient)
            .value(1000000000)
            .nonce(0)
            .memo("Invoice #9999")
            .build(wallet.raw_address)
        )
        tampered_digest = compute_transaction_digest(tampered_tx)
        self.assertNotEqual(tampered_digest, digest)

        tampered_recovered = recover_address(
            tampered_digest,
            signed_tx.signature.r,
            signed_tx.signature.s,
            signed_tx.signature.v,
        )
        self.assertNotEqual(
            Address(tampered_recovered).to_hex(),
            wallet.raw_address,
        )

    # =========================================================================
    # CROSS-SHARD STATE PROOFS & DYNAMIC SHARDING (ADR-0014)
    # =========================================================================

    def test_sharding_deterministic_partitioning(self):
        # 0x00, 0x04 -> high = 4 -> 4 % 4 = 0
        addr0 = "0x0004000000000000000000000000000000000000"
        self.assertEqual(compute_shard_id(addr0, 4), 0)

        # 0x01, 0x01 -> high = 257 -> 257 % 4 = 1
        addr1 = "0x0101000000000000000000000000000000000000"
        self.assertEqual(compute_shard_id(addr1, 4), 1)

        # 0x01, 0x02 -> high = 258 -> 258 % 4 = 2
        addr2 = "0x0102000000000000000000000000000000000000"
        self.assertEqual(compute_shard_id(addr2, 4), 2)

        # 0x01, 0x03 -> high = 259 -> 259 % 4 = 3
        addr3 = "0x0103000000000000000000000000000000000000"
        self.assertEqual(compute_shard_id(addr3, 4), 3)

    def test_sharding_receipt_nullifier_determinism_and_uniqueness(self):
        sender = "0x1111111111111111111111111111111111111111"
        n1 = compute_receipt_nullifier(0, 1, 100, sender)
        self.assertEqual(len(n1), 32)

        n1_repeat = compute_receipt_nullifier(0, 1, 100, sender)
        self.assertEqual(n1, n1_repeat)

        # Mutating nonce yields different nullifier
        n2 = compute_receipt_nullifier(0, 1, 101, sender)
        self.assertNotEqual(n1, n2)

        # Mutating sender yields different nullifier
        sender2 = "0x2222222222222222222222222222222222222222"
        n3 = compute_receipt_nullifier(0, 1, 100, sender2)
        self.assertNotEqual(n1, n3)

    def test_sharding_merkle_inclusion_proof_verification(self):
        # 4-leaf tree
        leaves = [
            blake3(bytes([0])),
            blake3(bytes([1])),
            blake3(bytes([2])),
            blake3(bytes([3])),
        ]

        # Level 1
        h01 = blake3(leaves[0] + leaves[1])
        h23 = blake3(leaves[2] + leaves[3])

        # Root
        root = blake3(h01 + h23)

        # Proof for leaf 0: sibling is leaf[1], parent's sibling is h23
        proof0 = [leaves[1], h23]
        self.assertTrue(verify_merkle_inclusion_proof(root, leaves[0], proof0, 0))

        # Proof for leaf 3: sibling is leaf[2], parent's sibling is h01
        proof3 = [leaves[2], h01]
        self.assertTrue(verify_merkle_inclusion_proof(root, leaves[3], proof3, 3))

        # Corrupted leaf fails verification
        corrupted_leaf = blake3(b"tampered")
        self.assertFalse(verify_merkle_inclusion_proof(root, corrupted_leaf, proof0, 0))

        # Wrong index fails verification
        self.assertFalse(verify_merkle_inclusion_proof(root, leaves[0], proof0, 1))

    def test_external_asset_types_and_canonical_id_determinism(self):
        self.assertEqual(ExternalAssetType.MESSENGER_USERNAME.value, "MESSENGER_USERNAME")
        self.assertEqual(ExternalAssetType.MESSENGER_NUMBER.value, "MESSENGER_NUMBER")
        self.assertEqual(ExternalAssetType.MESSENGER_GIFT.value, "MESSENGER_GIFT")
        self.assertEqual(ExternalAssetType.MESSENGER_BADGE.value, "MESSENGER_BADGE")
        self.assertEqual(ExternalAssetType.SOCIAL_USERNAME.value, "SOCIAL_USERNAME")
        self.assertEqual(ExternalAssetType.SOCIAL_NUMBER.value, "SOCIAL_NUMBER")
        self.assertEqual(ExternalAssetType.DIGITAL_COLLECTIBLE.value, "DIGITAL_COLLECTIBLE")
        self.assertEqual(ExternalAssetType.OTHER_EXTERNAL_ASSET.value, "OTHER_EXTERNAL_ASSET")

        canon1 = compute_external_asset_canonical_id("SocialAppA", ExternalAssetType.MESSENGER_USERNAME, "@anvar")
        canon2 = compute_external_asset_canonical_id("SocialAppA", ExternalAssetType.MESSENGER_USERNAME, "@anvar")
        self.assertEqual(canon1, canon2)

        canon_diff_plat = compute_external_asset_canonical_id("SocialAppB", ExternalAssetType.MESSENGER_USERNAME, "@anvar")
        self.assertNotEqual(canon1, canon_diff_plat)

        canon_diff_type = compute_external_asset_canonical_id("SocialAppA", ExternalAssetType.SOCIAL_USERNAME, "@anvar")
        self.assertNotEqual(canon1, canon_diff_type)

    def test_absence_of_native_identity_and_gifts_in_python_sdk(self):
        import tola
        self.assertFalse(hasattr(tola, "normalize_username"))
        self.assertFalse(hasattr(tola, "validate_username"))
        self.assertFalse(hasattr(tola, "calculate_username_fee"))
        self.assertFalse(hasattr(tola, "compute_username_commitment"))
        self.assertFalse(hasattr(tola, "resolve_username"))
        self.assertFalse(hasattr(tola, "sign_badge_attestation"))
        self.assertFalse(hasattr(tola, "verify_badge_attestation"))
        self.assertFalse(hasattr(tola, "validate_tls721_metadata"))
        self.assertFalse(hasattr(tola, "TelegramMiniAppBridge"))
        self.assertFalse(hasattr(tola, "MiniAppBridge"))
        self.assertFalse(hasattr(tola, "AppWalletBridge"))

    def test_external_asset_attestation_lifecycle_and_defenses(self):
        verifier = TolaWallet.generate()
        user = TolaWallet.generate()
        chain_id = 9999
        nonce = 101
        future_time = int(time.time()) + 3600

        attestation = sign_external_asset_attestation(
            verifier,
            chain_id,
            nonce,
            user.address,
            future_time,
            "PaxtaGram",
            ExternalAssetType.MESSENGER_USERNAME,
            "@anvar",
        )

        # 1. Valid verification
        self.assertTrue(verify_external_asset_attestation(attestation, verifier.address))

        # 2. Expired attestation rejected
        with self.assertRaises(ExternalAssetError):
            verify_external_asset_attestation(attestation, verifier.address, future_time + 10)

        # 3. Cross-chain replay rejected
        cross_chain = dict(attestation)
        cross_chain["chainId"] = 102
        with self.assertRaises(ExternalAssetError):
            verify_external_asset_attestation(cross_chain, verifier.address)

        # 4. Cross-wallet replay rejected
        other_user = TolaWallet.generate()
        cross_wallet = dict(attestation)
        cross_wallet["destinationWallet"] = other_user.address.to_hex()
        with self.assertRaises(ExternalAssetError):
            verify_external_asset_attestation(cross_wallet, verifier.address)

        # 5. Wrong verifier rejected
        unauth_verifier = TolaWallet.generate()
        with self.assertRaises(ExternalAssetError):
            verify_external_asset_attestation(attestation, unauth_verifier.address)

    def test_external_asset_metadata_validation_and_ssrf(self):
        valid_meta = {
            "name": "PaxtaGram Handle @anvar",
            "sourcePlatform": "PaxtaGram",
            "externalAssetId": "@anvar",
            "mediaUri": "ipfs://bafybeianvarhandle",
        }
        self.assertTrue(validate_external_asset_metadata(valid_meta))

        # SSRF rejected
        ssrf_meta = dict(valid_meta)
        ssrf_meta["mediaUri"] = "https://127.0.0.1:8545/rpc"
        with self.assertRaises(ExternalAssetError):
            validate_external_asset_metadata(ssrf_meta)

        # Missing externalAssetId
        missing_id = {"name": "Asset", "sourcePlatform": "PaxtaGram"}
        with self.assertRaises(ExternalAssetError):
            validate_external_asset_metadata(missing_id)

    def test_external_asset_calldata_encoders_and_contract_address(self):
        self.assertEqual(EXTERNAL_ASSET_CONTRACT_ADDRESS, "0x0000000000000000000000000000000000007721")

        verifier = TolaWallet.generate()
        user = TolaWallet.generate()
        recipient = TolaWallet.generate()

        attestation = sign_external_asset_attestation(
            verifier,
            9999,
            1,
            user.address,
            int(time.time()) + 3600,
            "PaxtaGram",
            ExternalAssetType.MESSENGER_USERNAME,
            "@anvar",
        )
        self.assertIn("claimExpiresAt", attestation)

        # Test mint calldata encoding
        mint_calldata = encode_mint_external_asset_calldata(attestation, "ipfs://bafybeianvar")
        self.assertEqual(mint_calldata[0], 1)  # TAG_MINT
        self.assertGreater(len(mint_calldata), 150)

        # Test transfer calldata encoding
        transfer_calldata = encode_transfer_external_asset_calldata(user.address, recipient.address, 42)
        self.assertEqual(transfer_calldata[0], 2)  # TAG_TRANSFER_FROM
        self.assertEqual(len(transfer_calldata), 1 + 20 + 20 + 8)

        # Test burn calldata encoding
        burn_calldata = encode_burn_external_asset_calldata(42)
        self.assertEqual(burn_calldata[0], 6)  # TAG_BURN
        self.assertEqual(len(burn_calldata), 1 + 8)


if __name__ == "__main__":
    unittest.main()

