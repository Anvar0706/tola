import { test, describe } from 'node:test';
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { mkdtempSync, writeFileSync, rmSync, existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import util from 'node:util';

import { keccak256 } from '../src/crypto/keccak256.js';
import { blake3 } from '../src/crypto/blake3.js';
import { encodeBech32m, decodeBech32m } from '../src/crypto/bech32m.js';
import {
  recoverAddress,
  recoverPublicKey,
  pointMultiply,
  publicKeyToAddress,
  compressPublicKey,
} from '../src/crypto/secp256k1.js';
import {
  generateMnemonic,
  validateMnemonic,
  mnemonicToSeed,
} from '../src/crypto/bip39.js';
import {
  derivePath,
  CANONICAL_DERIVATION_PATH,
  LEGACY_TESTNET_PROVISIONAL_PATH,
} from '../src/crypto/bip32.js';
import { Address } from '../src/address.js';
import { TolaWallet } from '../src/wallet.js';
import {
  TransactionBuilder,
  serializeUnsignedTransaction,
  serializeSignedTransaction,
  computeTransactionDigest,
} from '../src/transaction.js';
import { JsonRpcProvider, TolaProvider } from '../src/provider.js';
import { NETWORKS } from '../src/types.js';
import {
  validateClearSigningPrompt,
  formatClearSigningSummary,
  TolaConnectSession,
  AppWalletBridge,
  ConnectMessageType,
  DeviceRegistry,
} from '../src/connect/index.js';
import {
  EXTERNAL_ASSET_CONTRACT_ADDRESS,
  ExternalAssetType,
  EXTERNAL_ASSET_TYPE_IDS,
  getAssetTypeId,
  computeExternalAssetCanonicalId,
  computeExternalAssetAttestationDigest,
  signExternalAssetAttestation,
  verifyExternalAssetAttestation,
  validateExternalAssetMetadata,
  encodeMintExternalAssetCalldata,
  encodeTransferExternalAssetCalldata,
  encodeBurnExternalAssetCalldata,
  ExternalAssetError,
} from '../src/external/index.js';
import {
  computeShardId,
  computeReceiptNullifier,
  verifyMerkleInclusionProof,
} from '../src/sharding.js';




function toHex(bytes) {
  let s = '';
  for (const b of bytes) {
    s += b.toString(16).padStart(2, '0');
  }
  return s;
}

describe('TOLA TypeScript SDK (@tola/sdk)', () => {
  test('Keccak-256 standard vectors', () => {
    // Empty string
    const emptyHash = toHex(keccak256(new Uint8Array(0)));
    assert.equal(emptyHash, 'c5d2460186f7233c927e7db2dcc703c0e500b653ca82273b7bfad8045d85a470');

    // "TOLA"
    const tolaHash = toHex(keccak256(new TextEncoder().encode('TOLA')));
    assert.equal(tolaHash.length, 64);
  });

  test('BLAKE3 standard vectors', () => {
    // Empty string
    const emptyHash = toHex(blake3(new Uint8Array(0)));
    assert.equal(emptyHash, 'af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262');

    // "TOLA"
    const tolaHash = toHex(blake3(new TextEncoder().encode('TOLA')));
    assert.equal(tolaHash.length, 64);
  });

  test('Bech32m encoding, decoding, and network roundtrips', () => {
    const rawAddr = new Uint8Array([
      0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0,
      0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88,
      0x99, 0xaa, 0xbb, 0xcc,
    ]);
    const addr = new Address(rawAddr);

    // Mainnet
    const mainnetStr = addr.toBech32m('mainnet');
    assert.ok(mainnetStr.startsWith('tola1'));
    const decodedMain = Address.fromBech32m(mainnetStr);
    assert.equal(decodedMain.hrp, 'tola');
    assert.ok(decodedMain.address.equals(addr));

    // Testnet
    const testnetStr = addr.toBech32m('testnet');
    assert.ok(testnetStr.startsWith('ttola1'));
    const decodedTest = Address.fromBech32m(testnetStr);
    assert.equal(decodedTest.hrp, 'ttola');
    assert.ok(decodedTest.address.equals(addr));

    // Devnet
    const devnetStr = addr.toBech32m('devnet');
    assert.ok(devnetStr.startsWith('dtola1'));
    const decodedDev = Address.fromBech32m(devnetStr);
    assert.equal(decodedDev.hrp, 'dtola');
    assert.ok(decodedDev.address.equals(addr));

    // Corruption detection
    const corrupted = testnetStr.slice(0, -1) + (testnetStr.endsWith('q') ? 'p' : 'q');
    assert.throws(() => Address.fromBech32m(corrupted));

    // Mixed case rejection
    const mixed = 'TtoLa' + testnetStr.slice(5);
    assert.throws(() => Address.fromBech32m(mixed));
  });

  test('Canonical Vector bit-for-bit parity with Rust tola-sdk', async () => {
    const privHex = '0x4242424242424242424242424242424242424242424242424242424242424242';
    const wallet = TolaWallet.fromPrivateKeyHex(privHex);

    // 1. Compressed public key
    assert.equal(
      toHex(wallet.publicKeyBytes),
      '0324653eac434488002cc06bbfb7f10fe18991e35f9fe4302dbea6d2353dc0ab1c'
    );

    // 2. Address hex
    assert.equal(wallet.address.toHex(), '0x17c5185167401ed00cf5f5b2fc97d9bbfdb7d025');

    // 3. Bech32m addresses
    assert.equal(wallet.addressBech32m('mainnet'), 'tola1zlz3s5t8gq0dqr847ke0e97eh07m05p9t9nfc9');
    assert.equal(wallet.addressBech32m('testnet'), 'ttola1zlz3s5t8gq0dqr847ke0e97eh07m05p90jzepq');

    // 4. Construct identical transaction
    const tx = new TransactionBuilder(102n)
      .nonce(1n)
      .to(wallet.address.toHex())
      .value(1000000000000000000n)
      .gasLimit(21000n)
      .maxFeePerGas(1000000000n)
      .maxPriorityFeePerGas(1000000000n)
      .validUntilBlock(100n)
      .data('0xaabb')
      .build(wallet.address.toHex());

    // 5. Unsigned bytes serialization
    const unsignedBytes = serializeUnsignedTransaction(tx);
    const expectedUnsignedHex =
      '6600000000000000010000000000000017c5185167401ed00cf5f5b2fc97d9bbfdb7d0250117c5185167401ed00cf5f5b2fc97d9bbfdb7d025000064a7b3b6e00d0000000000000000085200000000000000ca9a3b00000000000000000000000000ca9a3b000000000000000000000000640000000000000002000000aabb';
    assert.equal(toHex(unsignedBytes), expectedUnsignedHex);

    // 6. Payload hash
    const payloadHash = toHex(blake3(unsignedBytes));
    assert.equal('0x' + payloadHash, '0xb3e845a99a136c2eab8250dc6deef7dfe47f3a8545c365c68d202dc5ecff2451');

    // 7. Transaction signing digest
    const digest = toHex(computeTransactionDigest(tx));
    assert.equal('0x' + digest, '0x32f94669e75eaf7c3d4d68388eb100c9c7884a86e1d16af2ee5c7e808281b830');

    // 8. Sign transaction
    const signedTx = await wallet.signTransaction(tx);
    assert.equal(signedTx.signature.type, 'Secp256k1');
    assert.equal(signedTx.signature.r.length, 32);
    assert.equal(signedTx.signature.s.length, 32);
    assert.ok(signedTx.signature.v === 0 || signedTx.signature.v === 1);

    // 9. Wire serialization
    const wireBytes = serializeSignedTransaction(signedTx);
    assert.ok(toHex(wireBytes).startsWith(expectedUnsignedHex));
    assert.equal(wireBytes.length, unsignedBytes.length + 1 + 32 + 32 + 1);
  });

  test('Wallet random generation and property sanity', () => {
    const w1 = TolaWallet.generate();
    const w2 = TolaWallet.generate();
    assert.notEqual(w1.privateKeyHex, w2.privateKeyHex);
    assert.notEqual(w1.address.toHex(), w2.address.toHex());
    assert.ok(w1.address.toBech32m('mainnet').startsWith('tola1'));
    assert.ok(w1.address.toBech32m('testnet').startsWith('ttola1'));
  });

  test('TolaProvider and JsonRpcProvider instance interfaces', async () => {
    const provider = new JsonRpcProvider('http://127.0.0.1:8545');
    assert.equal(provider.url, 'http://127.0.0.1:8545');

    const eip1193 = new TolaProvider('http://127.0.0.1:8545');
    let fired = false;
    eip1193.on('chainChanged', () => { fired = true; });
    eip1193.emit('chainChanged', '0x66');
    assert.equal(fired, true);
  });

  test('Canonical Network Chain IDs (Mainnet 9999, Testnet 102, Devnet 1337)', () => {
    assert.equal(NETWORKS.mainnet.chainId, 9999n);
    assert.equal(NETWORKS.testnet.chainId, 102n);
    assert.equal(NETWORKS.devnet.chainId, 1337n);
  });

  test('TOLA Connect: Clear-signing validation & human-readable formatting', () => {
    const validPrompt = {
      domain: 'shop.tola.network',
      actionName: 'Purchase Collectible',
      targetAddress: 'tola1zlz3s5t8gq0dqr847ke0e97eh07m05p9t9nfc9',
      amount: 25000000000000000000n, // 25 TOLA
      assetSymbol: 'TOLA',
      maxFee: 21000n,
      chainId: 9999n,
    };
    assert.equal(validateClearSigningPrompt(validPrompt), true);
    const summary = formatClearSigningSummary(validPrompt);
    assert.ok(summary.includes('shop.tola.network'));
    assert.ok(summary.includes('Purchase Collectible'));
    assert.ok(summary.includes('25.0000 TOLA'));

    // Rejection on empty domain
    assert.throws(() => validateClearSigningPrompt({ ...validPrompt, domain: '' }));
  });

  test('TOLA Connect: Encrypted Session Handshake (E2EE)', async () => {
    const session = await TolaConnectSession.create('app.tola.network', 'TOLA DEX', 3600);
    assert.equal(session.domain, 'app.tola.network');
    assert.equal(session.isExpired(), false);

    const secretPayload = { account: '0x17c5185167401ed00cf5f5b2fc97d9bbfdb7d025', authChallenge: 'xyz987' };
    const encrypted = await session.encrypt(secretPayload);
    assert.notEqual(encrypted, JSON.stringify(secretPayload));

    const decrypted = await session.decrypt(encrypted);
    assert.deepEqual(JSON.parse(decrypted), secretPayload);
  });

  test('TOLA Connect: AppWalletBridge payment approval & rejection', async () => {
    const wallet = TolaWallet.generate();
    let promptSeen = null;

    // Bridge with user approval
    const bridgeApproving = new AppWalletBridge(wallet, {
      onClearSign: async (prompt) => {
        promptSeen = prompt;
        return true;
      },
    });

    const accounts = await bridgeApproving.requestAccounts();
    assert.equal(accounts[0], wallet.address.toBech32m());

    const result = await bridgeApproving.requestPayment({
      domain: 'dapp.tola.network',
      to: 'tola1zlz3s5t8gq0dqr847ke0e97eh07m05p9t9nfc9',
      amount: 1000000000000000000n,
      actionName: 'Application Transfer',
    });
    assert.equal(result.status, 'signed');
    assert.equal(promptSeen.domain, 'dapp.tola.network');

    // Bridge with user rejection
    const bridgeRejecting = new AppWalletBridge(wallet, {
      onClearSign: async () => false,
    });
    await assert.rejects(
      async () => bridgeRejecting.requestPayment({
        domain: 'phishing.fake.app',
        to: 'tola1zlz3s5t8gq0dqr847ke0e97eh07m05p9t9nfc9',
        amount: 1000000000000000000n,
      }),
      (err) => err.code === 4001
    );
  });

  describe('Phase 18 Ecosystem Layer: Connect & Device Security', () => {
    test('TOLA Connect: Multi-Device Management, Authorization & Revocation', () => {
      const reg = new DeviceRegistry();
      const dev1 = reg.registerDevice('macbook_pro', 'Anvar MacBook Pro');
      assert.equal(dev1.deviceId, 'macbook_pro');
      assert.equal(reg.isAuthorized('macbook_pro'), true);

      // List devices
      assert.equal(reg.listDevices().length, 1);

      // Revoke device
      reg.revokeDevice('macbook_pro');
      assert.equal(reg.isAuthorized('macbook_pro'), false);
      assert.throws(() => reg.revokeDevice('unknown_device'));
    });

    test('TOLA Connect: Replay Protection, Monotonic Nonce & Session Expiry', async () => {
      const session = await TolaConnectSession.create('app.tola.network', 'TolaApp', 300);

      // 1. Valid envelope with nonce 1
      const env1 = session.createEnvelope(ConnectMessageType.TRANSACTION_REQUEST, { amount: 100 }, 'req_1', 1);
      assert.equal(session.validateEnvelope(env1), true);

      // 2. Replayed or descending nonce fails
      const envReplay = session.createEnvelope(ConnectMessageType.TRANSACTION_REQUEST, { amount: 100 }, 'req_2', 1);
      assert.throws(() => session.validateEnvelope(envReplay), /Replay attack detected/);

      // 3. Monotonically increasing nonce succeeds
      const env2 = session.createEnvelope(ConnectMessageType.TRANSACTION_REQUEST, { amount: 200 }, 'req_3', 2);
      assert.equal(session.validateEnvelope(env2), true);

      // 4. Domain mismatch fails
      const envMismatch = { ...env2, domain: 'evil.phishing.network', nonce: 3 };
      assert.throws(() => session.validateEnvelope(envMismatch), /Domain mismatch/);

      // 5. Expired envelope fails
      const envExpired = { ...env2, nonce: 4, expiresAt: Math.floor(Date.now() / 1000) - 10 };
      assert.throws(() => session.validateEnvelope(envExpired), /Message envelope has expired/);
    });

    test('TOLA Connect: Malicious Application Protection (Domain Binding, Chain Mismatch, Revoked Device)', async () => {
      const wallet = TolaWallet.generate();
      const deviceReg = new DeviceRegistry();
      deviceReg.registerDevice('phone_1', 'My Phone');

      const bridge = new AppWalletBridge(wallet, {
        expectedDomain: 'dex.tola.network',
        deviceRegistry: deviceReg,
        chainId: 9999n,
      });

      // 1. Domain spoofing rejected
      await assert.rejects(
        async () => bridge.requestPayment({
          domain: 'fake.tola.network',
          to: wallet.address.toHex(),
          amount: 1000n,
        }),
        /Domain mismatch/
      );

      // 2. Chain ID mismatch rejected
      await assert.rejects(
        async () => bridge.requestPayment({
          domain: 'dex.tola.network',
          to: wallet.address.toHex(),
          amount: 1000n,
          chainId: 102n, // requesting testnet on mainnet bridge
        }),
        /Chain ID mismatch/
      );

      // 3. Revoked device rejected
      deviceReg.revokeDevice('phone_1');
      await assert.rejects(
        async () => bridge.requestPayment({
          domain: 'dex.tola.network',
          to: wallet.address.toHex(),
          amount: 1000n,
          deviceId: 'phone_1',
        }),
        /Device 'phone_1' is not authorized or has been revoked/
      );
    });

    test('TOLA Connect: Clear-Signing with Memo bounds enforcement', () => {
      const basePrompt = {
        domain: 'app.tola.network',
        actionName: 'Transfer',
        targetAddress: 'tola1zlz3s5t8gq0dqr847ke0e97eh07m05p9t9nfc9',
        amount: 1000000000000000000n,
        assetSymbol: 'TOLA',
        maxFee: 21000n,
        chainId: 9999n,
        memo: 'Invoice #1042',
      };

      assert.equal(validateClearSigningPrompt(basePrompt), true);
      const summary = formatClearSigningSummary(basePrompt);
      assert.ok(summary.includes('Invoice #1042'));

      // 256 bytes memo ok
      const okMemo = 'a'.repeat(256);
      assert.equal(validateClearSigningPrompt({ ...basePrompt, memo: okMemo }), true);

      // 257 bytes memo rejected
      const badMemo = 'a'.repeat(257);
      assert.throws(() => validateClearSigningPrompt({ ...basePrompt, memo: badMemo }), /Memo exceeds maximum length of 256 bytes/);
    });

    test('Phase 18 Adversarial Security: Device Attacks, Secret Redaction, and AAD', async () => {
      // 1. Device Re-registration and Duplicate Protection
      const reg = new DeviceRegistry();
      reg.registerDevice('device_alpha', 'Device Alpha');
      assert.throws(() => reg.registerDevice('device_alpha', 'Duplicate Alpha'), /is already registered/);
      reg.revokeDevice('device_alpha');
      assert.throws(() => reg.registerDevice('device_alpha', 'Revoked Alpha'), /Cannot re-register revoked device/);

      // 2. Secret Leakage Protection
      const session = await TolaConnectSession.create('app.tola.network', 'TolaApp', 300);
      const jsonOutput = JSON.stringify(session);
      assert.ok(jsonOutput.includes('"sessionKey":"[REDACTED]"'), 'sessionKey must be redacted in JSON');
      assert.ok(!jsonOutput.includes(Array.from(session.sessionKey).join(',')), 'Raw key bytes must not appear in JSON');

      // 3. Envelope Clock Skew Injection Attack
      const futureEnvelope = session.createEnvelope(ConnectMessageType.TRANSACTION_REQUEST, { amount: 100 }, 'req_skew', 1);
      futureEnvelope.timestamp = Math.floor(Date.now() / 1000) + 120; // 120s in future
      assert.throws(() => session.validateEnvelope(futureEnvelope), /clock skew exceeds 60s/);

      // 4. Missing/Empty Request ID
      const noReqId = session.createEnvelope(ConnectMessageType.TRANSACTION_REQUEST, { amount: 100 }, '', 1);
      assert.throws(() => session.validateEnvelope(noReqId), /Missing or empty requestId/);

      // 5. Authenticated Additional Data (AAD) Binding
      const plainText = 'CONFIDENTIAL_PAYLOAD';
      const aad = 'SESSION_METADATA_HEADER_V1';
      const encrypted = await session.encrypt(plainText, aad);
      const decrypted = await session.decrypt(encrypted, aad);
      assert.equal(decrypted, plainText);
      // Tampered AAD fails decryption
      await assert.rejects(async () => session.decrypt(encrypted, 'TAMPERED_AAD'));

      // 6. ECDH Key Agreement & HKDF Key Derivation
      const crypto = await (typeof globalThis !== 'undefined' && globalThis.crypto?.subtle ? globalThis.crypto : (await import('node:crypto')).webcrypto);
      const aliceKeyPair = await crypto.subtle.generateKey(
        { name: 'ECDH', namedCurve: 'P-256' },
        true,
        ['deriveKey', 'deriveBits']
      );
      const bobKeyPair = await crypto.subtle.generateKey(
        { name: 'ECDH', namedCurve: 'P-256' },
        true,
        ['deriveKey', 'deriveBits']
      );
      const sessionAlice = await TolaConnectSession.fromECDH(aliceKeyPair.privateKey, bobKeyPair.publicKey, 'app.tola.network', 'AliceClient');
      const sessionBob = await TolaConnectSession.fromECDH(bobKeyPair.privateKey, aliceKeyPair.publicKey, 'app.tola.network', 'BobWallet');
      assert.deepEqual(sessionAlice.sessionKey, sessionBob.sessionKey, 'ECDH derived session keys must match');

      // 7. Clear-Signing Chain ID Protection (Chain ID <= 0)
      const invalidChainPrompt = {
        domain: 'app.tola.network',
        actionName: 'Transfer',
        targetAddress: 'tola1zlz3s5t8gq0dqr847ke0e97eh07m05p9t9nfc9',
        amount: 1000n,
        assetSymbol: 'TOLA',
        maxFee: 21000n,
        chainId: 0n,
      };
      assert.throws(() => validateClearSigningPrompt(invalidChainPrompt), /Chain ID must be greater than zero/);
    });
  });

  // ==========================================================================
  // PHASE 18.x: FROZEN TOLA WALLET DERIVATION STANDARD v1.0 TEST SUITE
  // BIP-39 -> BIP-32 -> BIP-44 (m/44'/9999'/0'/0/0) -> Secp256k1 -> Keccak-256
  // ==========================================================================

  describe('TOLA Wallet Derivation Standard v1.0 & Parity', () => {
    const PHRASE_12 = 'abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about';
    const PHRASE_24 = 'abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon art';

    test('Canonical Derivation Vector 1 (12 words) bit-for-bit parity with Rust tola-crypto', () => {
      const wallet = TolaWallet.fromMnemonic(PHRASE_12);

      // 1. Path & Mnemonic preservation
      assert.equal(wallet.derivationPath, CANONICAL_DERIVATION_PATH);
      assert.equal(wallet.mnemonic, PHRASE_12);

      // 2. Private scalar
      assert.equal(wallet.exportPrivateKeyHex(), '0xf77999a4d727d792213ae012f18621208ea91a7246e5bf69a4317942abeca750');

      // 3. Compressed & uncompressed public keys
      assert.equal(
        toHex(wallet.getPublicKey(true)),
        '03e95745eb5f028b4e23fb4c5af97fb49426d522c4228fe6dd922f2585e6b1bf96'
      );
      assert.equal(
        toHex(wallet.getPublicKey(false)),
        '04e95745eb5f028b4e23fb4c5af97fb49426d522c4228fe6dd922f2585e6b1bf9689629921a91231749eaf52b0198c172ea8a2cce8ee2c9f62f8a6c2fed81c217f'
      );

      // 4. Raw 20-byte address
      assert.equal(wallet.rawAddress, '0x3b45664e195dbacd0035046b511491e09bd3ca8a');

      // 5. Network Bech32m addresses (Model A unified raw address)
      assert.equal(wallet.getAddress('mainnet'), 'tola18dzkvnsetkav6qp4q344z9y3uzda8j5263ez6j');
      assert.equal(wallet.getAddress('testnet'), 'ttola18dzkvnsetkav6qp4q344z9y3uzda8j527xgjrh');
      assert.equal(wallet.getAddress('devnet'), 'dtola18dzkvnsetkav6qp4q344z9y3uzda8j52x0hhl4');
    });

    test('Canonical Derivation Vector 2 (24 words) bit-for-bit parity with Rust tola-crypto', () => {
      const seed = mnemonicToSeed(PHRASE_24, '');
      assert.equal(
        toHex(seed),
        '408b285c123836004f4b8842c89324c1f01382450c0d439af345ba7fc49acf705489c6fc77dbd4e3dc1dd8cc6bc9f043db8ada1e243c4a0eafb290d399480840'
      );

      const wallet = TolaWallet.fromMnemonic(PHRASE_24);
      assert.equal(wallet.rawAddress, '0x37061e043e1b89972ed615c46ff3e5090514c909');
      assert.equal(wallet.getAddress('mainnet'), 'tola1xurpupp7rwyewtkkzhzxlul9pyz3fjgf4wludn');
      assert.equal(
        toHex(wallet.getPublicKey(true)),
        '03d20a0a3b709e063b279a5810d91b922c50b5b62ba09ef16b6c3916f1e5d66e3f'
      );
    });

    test('Legacy testnet provisional path (m/44\'/102\'/0\'/0/0) backward compatibility verification', () => {
      // Vector 1 legacy
      const w1 = TolaWallet.fromMnemonicLegacyTestnet(PHRASE_12);
      assert.equal(w1.rawAddress, '0x8e3719da26b9408f27fa9602c4362d9fbc49dc73');
      assert.equal(w1.getAddress('testnet'), 'ttola13cm3nk3xh9qg7fl6jcpvgd3dn77ynhrnukw78v');
      assert.equal(
        toHex(w1.getPublicKey(true)),
        '02b74bcb92d52c5cfe20ece37e6cc6ae0b12dd58fc328c509e8f904dfc62977403'
      );

      // Vector 2 legacy
      const w2 = TolaWallet.fromMnemonicLegacyTestnet(PHRASE_24);
      assert.equal(w2.rawAddress, '0x1073f501701b4a14dd4bc6a1f1746fca1fb8de64');
    });

    test('Model A: Unified Account Identity roundtrip across Mainnet, Testnet, Devnet', () => {
      const wallet = TolaWallet.fromMnemonic(PHRASE_12);

      const mainStr = wallet.getAddress('mainnet');
      const testStr = wallet.getAddress('testnet');
      const devStr = wallet.getAddress('devnet');

      const decodedMain = Address.fromBech32m(mainStr);
      const decodedTest = Address.fromBech32m(testStr);
      const decodedDev = Address.fromBech32m(devStr);

      assert.equal(decodedMain.address.toHex(), wallet.rawAddress);
      assert.equal(decodedTest.address.toHex(), wallet.rawAddress);
      assert.equal(decodedDev.address.toHex(), wallet.rawAddress);
      assert.equal(decodedMain.hrp, 'tola');
      assert.equal(decodedTest.hrp, 'ttola');
      assert.equal(decodedDev.hrp, 'dtola');
    });

    test('Cross-SDK transaction signing and ECDSA address recovery parity', async () => {
      const wallet = TolaWallet.fromMnemonic(PHRASE_12);

      const tx = new TransactionBuilder(9999n)
        .nonce(42n)
        .to('0x8888888888888888888888888888888888888888')
        .value(500000000000000000n) // 0.5 TOLA
        .gasLimit(21000n)
        .maxFeePerGas(1000000000n)
        .maxPriorityFeePerGas(1000000000n)
        .validUntilBlock(200n)
        .data('0x')
        .build(wallet.rawAddress);

      const signedTx = await wallet.signTransaction(tx);
      assert.equal(signedTx.signature.type, 'Secp256k1');
      assert.equal(signedTx.signature.r.length, 32);
      assert.equal(signedTx.signature.s.length, 32);

      const digest = computeTransactionDigest(tx);
      const recoveredBytes = recoverAddress(
        digest,
        signedTx.signature.r,
        signedTx.signature.s,
        signedTx.signature.v
      );
      const recoveredHex = '0x' + toHex(recoveredBytes);
      assert.equal(recoveredHex, wallet.rawAddress);
    });
  });

  describe('BIP-39 Native Recovery Standard (9 Test Scenarios)', () => {
    const PHRASE_12 = 'abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about';
    const PHRASE_24 = 'abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon art';

    test('1. Random 12-word generation & recovery cycle', () => {
      const w = TolaWallet.generate(12);
      assert.ok(w.mnemonic);
      assert.equal(w.mnemonic.split(' ').length, 12);
      assert.equal(validateMnemonic(w.mnemonic), w.mnemonic);

      const restored = TolaWallet.fromMnemonic(w.mnemonic);
      assert.equal(restored.rawAddress, w.rawAddress);
      assert.equal(restored.exportPrivateKeyHex(), w.exportPrivateKeyHex());
    });

    test('2. Random 24-word generation & recovery cycle', () => {
      const w = TolaWallet.generate(24);
      assert.ok(w.mnemonic);
      assert.equal(w.mnemonic.split(' ').length, 24);
      assert.equal(validateMnemonic(w.mnemonic), w.mnemonic);

      const restored = TolaWallet.fromMnemonic(w.mnemonic);
      assert.equal(restored.rawAddress, w.rawAddress);
      assert.equal(restored.exportPrivateKeyHex(), w.exportPrivateKeyHex());
    });

    test('3. 12-word recovery produces canonical address', () => {
      const restored = TolaWallet.fromMnemonic(PHRASE_12);
      assert.equal(restored.rawAddress, '0x3b45664e195dbacd0035046b511491e09bd3ca8a');
    });

    test('4. 24-word recovery produces canonical address', () => {
      const restored = TolaWallet.fromMnemonic(PHRASE_24);
      assert.equal(restored.rawAddress, '0x37061e043e1b89972ed615c46ff3e5090514c909');
    });

    test('5. Distinct mnemonics produce distinct private keys and addresses', () => {
      const w1 = TolaWallet.fromMnemonic(PHRASE_12);
      const w2 = TolaWallet.fromMnemonic(PHRASE_24);
      assert.notEqual(w1.rawAddress, w2.rawAddress);
      assert.notEqual(w1.exportPrivateKeyHex(), w2.exportPrivateKeyHex());
    });

    test('6. Invalid checksum rejection throws error', () => {
      const invalidChecksum = 'abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon zoo';
      assert.throws(() => TolaWallet.fromMnemonic(invalidChecksum), /checksum/i);
    });

    test('7. Invalid word rejection throws error', () => {
      const invalidWord = 'abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon tolanetwork';
      assert.throws(() => TolaWallet.fromMnemonic(invalidWord), /word not in|unknown word|invalid mnemonic/i);
    });

    test('8. Passphrase salt variation produces completely different wallet', () => {
      const wNoPass = TolaWallet.fromMnemonic(PHRASE_12, '');
      const wWithPass = TolaWallet.fromMnemonic(PHRASE_12, 'vault-passphrase-2026');
      assert.notEqual(wNoPass.rawAddress, wWithPass.rawAddress);
      assert.notEqual(wNoPass.exportPrivateKeyHex(), wWithPass.exportPrivateKeyHex());
    });

    test('9. Whitespace, case, and NFKD normalization produces identical keys', () => {
      const messy = '   ABANDON   abandon   Abandon   abandon abandon abandon abandon abandon abandon abandon abandon about  ';
      const clean = TolaWallet.fromMnemonic(PHRASE_12);
      const normalized = TolaWallet.fromMnemonic(messy);
      assert.equal(normalized.rawAddress, clean.rawAddress);
      assert.equal(normalized.exportPrivateKeyHex(), clean.exportPrivateKeyHex());
    });
  });

  describe('Cryptographic Security: Tampering, Replay Protection, & Secret Isolation', () => {
    const PHRASE_12 = 'abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about';

    test('Signature tampering rejection across all transaction fields', async () => {
      const wallet = TolaWallet.fromMnemonic(PHRASE_12);
      const baseTx = new TransactionBuilder(9999n)
        .nonce(1n)
        .to('0x8888888888888888888888888888888888888888')
        .value(1000000000000000000n)
        .gasLimit(21000n)
        .maxFeePerGas(1000000000n)
        .maxPriorityFeePerGas(1000000000n)
        .validUntilBlock(100n)
        .data('0x1234')
        .build(wallet.rawAddress);

      const signedTx = await wallet.signTransaction(baseTx);
      const { r, s, v } = signedTx.signature;

      const mutations = [
        { field: 'value', mutated: { ...baseTx, value: 9999999999999999999n } },
        { field: 'to', mutated: { ...baseTx, to: '0x1111111111111111111111111111111111111111' } },
        { field: 'nonce', mutated: { ...baseTx, nonce: 2n } },
        { field: 'gasLimit', mutated: { ...baseTx, gasLimit: 50000n } },
        { field: 'maxFeePerGas', mutated: { ...baseTx, maxFeePerGas: 2000000000n } },
        { field: 'validUntilBlock', mutated: { ...baseTx, validUntilBlock: 500n } },
        { field: 'chainId', mutated: { ...baseTx, chainId: 102n } },
        { field: 'data', mutated: { ...baseTx, data: new Uint8Array([0x99, 0x88]) } },
      ];

      for (const { field, mutated } of mutations) {
        const tamperedDigest = computeTransactionDigest(mutated);
        let recoveredHex = null;
        try {
          const recovered = recoverAddress(tamperedDigest, r, s, v);
          recoveredHex = '0x' + toHex(recovered);
        } catch {
          // Recovery failed due to invalid curve point, which also safely rejects
        }
        assert.notEqual(
          recoveredHex,
          wallet.rawAddress,
          `Tampering '${field}' must not recover original sender address`
        );
      }
    });

    test('Chain ID Replay Protection: cross-network transaction isolation', async () => {
      const wallet = TolaWallet.fromMnemonic(PHRASE_12);

      const txMainnet = new TransactionBuilder(9999n)
        .nonce(0n)
        .to('0x8888888888888888888888888888888888888888')
        .value(1000000000000000000n)
        .build(wallet.rawAddress);

      const signedMainnet = await wallet.signTransaction(txMainnet);
      const digestMainnet = computeTransactionDigest(txMainnet);

      // Attempt replay on Testnet
      const txTestnet = { ...txMainnet, chainId: 102n };
      const digestTestnet = computeTransactionDigest(txTestnet);

      assert.notEqual(toHex(digestMainnet), toHex(digestTestnet));

      const recoveredOnTestnet = '0x' + toHex(
        recoverAddress(digestTestnet, signedMainnet.signature.r, signedMainnet.signature.s, signedMainnet.signature.v)
      );
      assert.notEqual(
        recoveredOnTestnet,
        wallet.rawAddress,
        'Mainnet signature must be invalid for Testnet digest'
      );
    });

    test('Legacy wallet compatibility: supports BigInt, Uint8Array, and hex string inputs', () => {
      const privScalar = 0xf77999a4d727d792213ae012f18621208ea91a7246e5bf69a4317942abeca750n;
      const privHexWith0x = '0xf77999a4d727d792213ae012f18621208ea91a7246e5bf69a4317942abeca750';
      const privHexNo0x = 'f77999a4d727d792213ae012f18621208ea91a7246e5bf69a4317942abeca750';

      const w1 = TolaWallet.fromPrivateKey(privScalar);
      const w2 = TolaWallet.fromPrivateKey(privHexWith0x);
      const w3 = TolaWallet.fromPrivateKey(privHexNo0x);
      const w4 = TolaWallet.fromPrivateKeyHex(privHexWith0x);

      assert.equal(w1.rawAddress, '0x3b45664e195dbacd0035046b511491e09bd3ca8a');
      assert.equal(w2.rawAddress, w1.rawAddress);
      assert.equal(w3.rawAddress, w1.rawAddress);
      assert.equal(w4.rawAddress, w1.rawAddress);

      // Legacy imports do not hold mnemonic
      assert.equal(w1.mnemonic, null);
      assert.equal(w2.mnemonic, null);
    });

    test('Secret Leakage Audit: private key & mnemonic never exposed to inspect or serialization', () => {
      const wallet = TolaWallet.fromMnemonic(PHRASE_12);

      // 1. inspect() / console.log representation
      const inspected = util.inspect(wallet);
      assert.equal(inspected, `[TolaWallet: ${wallet.getAddress('mainnet')}]`);
      assert.equal(inspected.includes('f77999a4'), false);
      assert.equal(inspected.includes('abandon'), false);

      // 2. JSON.stringify representation
      const json = JSON.stringify(wallet);
      const parsed = JSON.parse(json);
      assert.equal(parsed.address, wallet.getAddress('mainnet'));
      assert.equal(parsed.rawAddress, wallet.rawAddress);
      assert.equal(json.includes('f77999a4'), false);
      assert.equal(json.includes('abandon'), false);

      // 3. Object reflection
      assert.deepEqual(Object.keys(wallet), ['publicKeyPoint', 'address']);
      assert.equal(Object.getOwnPropertyNames(wallet).includes('privateKey'), false);
      assert.equal(Object.getOwnPropertyNames(wallet).includes('mnemonic'), false);
    });

    test('Arbitrary message clear-signing and prefix recovery', async () => {
      const wallet = TolaWallet.fromMnemonic(PHRASE_12);
      const message = 'Hello TOLA Network Genesis 2026';

      const sigHex = await wallet.signMessage(message);
      assert.ok(sigHex.startsWith('0x'));
      assert.equal(sigHex.length, 2 + 130); // 65 bytes in hex

      // Recover address
      const rBytes = new Uint8Array(Buffer.from(sigHex.slice(2, 66), 'hex'));
      const sBytes = new Uint8Array(Buffer.from(sigHex.slice(66, 130), 'hex'));
      const v = parseInt(sigHex.slice(130, 132), 16);

      const msgBytes = new TextEncoder().encode(message);
      const prefix = new TextEncoder().encode(`\x19TOLA Signed Message:\n${msgBytes.length}`);
      const preImage = new Uint8Array(prefix.length + msgBytes.length);
      preImage.set(prefix, 0);
      preImage.set(msgBytes, prefix.length);
      const digest = keccak256(preImage);

      const recoveredBytes = recoverAddress(digest, rBytes, sBytes, v);
      assert.equal('0x' + toHex(recoveredBytes), wallet.rawAddress);
    });
  });

  describe('End-to-End Live Node JSON-RPC Integration', () => {
    test('Live Node Transaction Lifecycle: Submit raw signed tx, propose block, verify execution', async () => {
      const candidates = [
        join(process.cwd(), 'target', 'debug', 'tola-node.exe'),
        join(process.cwd(), '..', '..', 'target', 'debug', 'tola-node.exe'),
      ];
      const nodeBinary = candidates.find((p) => existsSync(p));
      if (!nodeBinary) {
        console.log('[SKIP] tola-node.exe binary not found, skipping live daemon test');
        return;
      }

      const tmpDir = mkdtempSync(join(tmpdir(), 'tola-e2e-node-'));
      const genesisPath = join(tmpDir, 'genesis.json');

      const canonicalAddr = '0x3b45664e195dbacd0035046b511491e09bd3ca8a';
      const initialFund = 10000000000000000000000n; // 10,000 TOLA

      const genesis = {
        chain_id: 102,
        genesis_time: 1770000000,
        accounts: [
          {
            address: canonicalAddr,
            balance: initialFund.toString(),
          },
        ],
        validators: [],
      };
      writeFileSync(genesisPath, JSON.stringify(genesis));

      const rpcPort = 38000 + Math.floor(Math.random() * 500);
      const p2pPort = 30000 + Math.floor(Math.random() * 500);

      const child = spawn(
        nodeBinary,
        [
          '--chain-id', '102',
          '--data-dir', tmpDir,
          '--p2p-addr', `127.0.0.1:${p2pPort}`,
          '--rpc-addr', `127.0.0.1:${rpcPort}`,
          '--validator',
          '--genesis', genesisPath,
          '--log-level', 'error',
        ],
        { stdio: ['ignore', 'pipe', 'pipe'] }
      );

      const provider = new JsonRpcProvider(`http://127.0.0.1:${rpcPort}`);

      try {
        // 1. Wait for node startup
        let ready = false;
        for (let i = 0; i < 30; i++) {
          try {
            const health = await provider.send('tola_getHealth');
            if (health && health.status === 'ok') {
              ready = true;
              break;
            }
          } catch {
            await new Promise((r) => setTimeout(r, 150));
          }
        }
        assert.ok(ready, 'Node failed to report healthy within timeout');

        // 2. Query initial balance of Vector 1 address
        const initialBal = await provider.getBalance(canonicalAddr);
        assert.equal(initialBal, initialFund);

        // 3. Restore wallet from Vector 1 mnemonic
        const phrase = 'abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about';
        const wallet = TolaWallet.fromMnemonic(phrase);
        assert.equal(wallet.rawAddress, canonicalAddr);

        // 4. Construct and sign transfer transaction
        const recipient = '0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee';
        const transferAmount = 2500000000000000000n; // 2.5 TOLA

        const tx = new TransactionBuilder(102n)
          .to(recipient)
          .value(transferAmount)
          .nonce(0n)
          .gasLimit(21000n)
          .maxFeePerGas(1000000000n)
          .maxPriorityFeePerGas(1000000000n)
          .validUntilBlock(1000n)
          .build(wallet.rawAddress);

        const signedTx = await wallet.signTransaction(tx);
        const wireBytes = serializeSignedTransaction(signedTx);
        const hex = '0x' + toHex(wireBytes);

        // 5. Submit via eth_sendRawTransaction
        const txHash = await provider.sendRawTransaction(hex);
        assert.ok(txHash && txHash.startsWith('0x'));

        // 6. Wait for block execution
        let executed = false;
        for (let i = 0; i < 30; i++) {
          await new Promise((r) => setTimeout(r, 150));
          const height = await provider.getBlockNumber();
          if (height >= 1n) {
            executed = true;
            break;
          }
        }
        assert.ok(executed, 'Block #1 failed to commit within timeout');

        // 7. Verify state updates
        const recipBal = await provider.getBalance(recipient);
        assert.equal(recipBal, transferAmount);

        const senderNonce = await provider.getTransactionCount(canonicalAddr);
        assert.equal(senderNonce, 1n);

        const senderRemaining = await provider.getBalance(canonicalAddr);
        assert.ok(senderRemaining < initialFund - transferAmount);
      } finally {
        child.kill();
        await new Promise((r) => setTimeout(r, 300));
        try {
          rmSync(tmpDir, { recursive: true, force: true });
        } catch {
          // Best effort cleanup on Windows file lock
        }
      }
    });
  });

  describe('Transaction Memo / Comment Parity & Limits', () => {
    test('TransactionBuilder memo normalization', () => {
      const b1 = new TransactionBuilder(102n).memo('');
      assert.equal(b1.build('0x3b45664e195dbacd0035046b511491e09bd3ca8a').memo, null);

      const b2 = new TransactionBuilder(102n).memo('   ');
      assert.equal(b2.build('0x3b45664e195dbacd0035046b511491e09bd3ca8a').memo, null);

      const b3 = new TransactionBuilder(102n).memo('Invoice #1042');
      assert.equal(b3.build('0x3b45664e195dbacd0035046b511491e09bd3ca8a').memo, 'Invoice #1042');
    });

    test('Backward compatibility: transaction without memo produces legacy wire bytes', () => {
      const txLegacy = {
        chainId: 102n,
        nonce: 1n,
        from: '0x3b45664e195dbacd0035046b511491e09bd3ca8a',
        to: '0x3b45664e195dbacd0035046b511491e09bd3ca8a',
        value: 1000000000n,
        gasLimit: 21000n,
        maxFeePerGas: 1000000000n,
        maxPriorityFeePerGas: 1000000000n,
        validUntilBlock: 100n,
        data: new Uint8Array([0xaa, 0xbb]),
      };

      const txWithNullMemo = { ...txLegacy, memo: null };
      const txWithEmptyMemo = { ...txLegacy, memo: '' };

      const bLegacy = serializeUnsignedTransaction(txLegacy);
      const bNull = serializeUnsignedTransaction(txWithNullMemo);
      const bEmpty = serializeUnsignedTransaction(txWithEmptyMemo);

      assert.deepEqual(bNull, bLegacy);
      assert.deepEqual(bEmpty, bLegacy);
      assert.deepEqual(computeTransactionDigest(txWithNullMemo), computeTransactionDigest(txLegacy));
    });

    test('UTF-8 memo serialization for English, Uzbek, and Emoji strings', () => {
      const baseTx = {
        chainId: 102n,
        nonce: 1n,
        from: '0x3b45664e195dbacd0035046b511491e09bd3ca8a',
        to: '0x3b45664e195dbacd0035046b511491e09bd3ca8a',
        value: 1000000000n,
        gasLimit: 21000n,
        maxFeePerGas: 1000000000n,
        maxPriorityFeePerGas: 1000000000n,
        validUntilBlock: 100n,
        data: new Uint8Array([0xaa, 0xbb]),
      };

      const legacyBytes = serializeUnsignedTransaction(baseTx);

      const memos = ['Invoice #1042', 'To‘lov #1042', '💰💎🚀'];
      for (const m of memos) {
        const tx = { ...baseTx, memo: m };
        const serialized = serializeUnsignedTransaction(tx);
        const expectedBytes = new TextEncoder().encode(m);

        assert.equal(serialized.length, legacyBytes.length + 4 + expectedBytes.length);

        // Check 4-byte LE length prefix
        const view = new DataView(serialized.buffer, serialized.byteOffset, serialized.byteLength);
        const memoLen = view.getUint32(legacyBytes.length, true);
        assert.equal(memoLen, expectedBytes.length);

        // Check UTF-8 payload
        const payload = serialized.slice(legacyBytes.length + 4);
        assert.deepEqual(payload, expectedBytes);
      }
    });

    test('Memo max length bounds enforcement: 256 bytes accepted, 257 bytes rejected', () => {
      const baseTx = {
        chainId: 102n,
        nonce: 1n,
        from: '0x3b45664e195dbacd0035046b511491e09bd3ca8a',
        to: '0x3b45664e195dbacd0035046b511491e09bd3ca8a',
        value: 1000n,
        gasLimit: 21000n,
        maxFeePerGas: 1000000000n,
        maxPriorityFeePerGas: 1000000000n,
        validUntilBlock: 100n,
        data: new Uint8Array(0),
      };

      const memo256 = 'A'.repeat(256);
      const tx256 = { ...baseTx, memo: memo256 };
      assert.doesNotThrow(() => serializeUnsignedTransaction(tx256));

      const memo257 = 'A'.repeat(257);
      const tx257 = { ...baseTx, memo: memo257 };
      assert.throws(() => serializeUnsignedTransaction(tx257), /exceeds maximum allowed 256 bytes/);
    });

    test('Cryptographic protection: tampering with memo invalidates signature recovery', async () => {
      const PHRASE_12 = 'abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about';
      const wallet = TolaWallet.fromMnemonic(PHRASE_12);
      const recipient = '0x1111111111111111111111111111111111111111';

      const tx = new TransactionBuilder(102n)
        .to(recipient)
        .value(1000000000n)
        .nonce(0n)
        .memo('Invoice #1042')
        .build(wallet.rawAddress);

      const signedTx = await wallet.signTransaction(tx);
      const digest = computeTransactionDigest(signedTx);

      const recovered = recoverAddress(
        digest,
        signedTx.signature.r,
        signedTx.signature.s,
        signedTx.signature.v
      );
      assert.equal('0x' + toHex(recovered), wallet.rawAddress);

      // Tampering with memo changes digest
      const tamperedTx = { ...signedTx, memo: 'Invoice #9999' };
      const tamperedDigest = computeTransactionDigest(tamperedTx);
      assert.notDeepEqual(tamperedDigest, digest);

      const tamperedRecovered = recoverAddress(
        tamperedDigest,
        signedTx.signature.r,
        signedTx.signature.s,
        signedTx.signature.v
      );
      assert.notEqual('0x' + toHex(tamperedRecovered), wallet.rawAddress);
    });
  });

  describe('Cross-Shard State Proofs & Dynamic Sharding (ADR-0014)', () => {
    test('Deterministic shard ID partitioning parity with Rust', () => {
      // 0x00, 0x04 -> high = 4 -> 4 % 4 = 0
      const addr0 = '0x0004000000000000000000000000000000000000';
      assert.equal(computeShardId(addr0, 4), 0);

      // 0x01, 0x01 -> high = 257 -> 257 % 4 = 1
      const addr1 = '0x0101000000000000000000000000000000000000';
      assert.equal(computeShardId(addr1, 4), 1);

      // 0x01, 0x02 -> high = 258 -> 258 % 4 = 2
      const addr2 = '0x0102000000000000000000000000000000000000';
      assert.equal(computeShardId(addr2, 4), 2);

      // 0x01, 0x03 -> high = 259 -> 259 % 4 = 3
      const addr3 = '0x0103000000000000000000000000000000000000';
      assert.equal(computeShardId(addr3, 4), 3);
    });

    test('Receipt nullifier generation determinism & uniqueness', () => {
      const sender = '0x1111111111111111111111111111111111111111';
      const n1 = computeReceiptNullifier(0, 1, 100n, sender);
      assert.equal(n1.length, 32);

      const n1Repeat = computeReceiptNullifier(0, 1, 100n, sender);
      assert.deepEqual(n1, n1Repeat);

      // Mutating nonce yields different nullifier
      const n2 = computeReceiptNullifier(0, 1, 101n, sender);
      assert.notDeepEqual(n1, n2);

      // Mutating sender yields different nullifier
      const sender2 = '0x2222222222222222222222222222222222222222';
      const n3 = computeReceiptNullifier(0, 1, 100n, sender2);
      assert.notDeepEqual(n1, n3);
    });

    test('Binary Merkle inclusion proof verification', () => {
      // 4-leaf tree
      const leaves = [
        blake3(new Uint8Array([0])),
        blake3(new Uint8Array([1])),
        blake3(new Uint8Array([2])),
        blake3(new Uint8Array([3])),
      ];

      // Level 1
      const c01 = new Uint8Array(64);
      c01.set(leaves[0], 0);
      c01.set(leaves[1], 32);
      const h01 = blake3(c01);

      const c23 = new Uint8Array(64);
      c23.set(leaves[2], 0);
      c23.set(leaves[3], 32);
      const h23 = blake3(c23);

      // Root
      const cRoot = new Uint8Array(64);
      cRoot.set(h01, 0);
      cRoot.set(h23, 32);
      const root = blake3(cRoot);

      // Proof for leaf 0: sibling is leaf[1], parent's sibling is h23
      const proof0 = [leaves[1], h23];
      assert.equal(verifyMerkleInclusionProof(root, leaves[0], proof0, 0), true);

      // Proof for leaf 3: sibling is leaf[2], parent's sibling is h01
      const proof3 = [leaves[2], h01];
      assert.equal(verifyMerkleInclusionProof(root, leaves[3], proof3, 3), true);

      // Corrupted leaf fails verification
      const corruptedLeaf = blake3(new Uint8Array([99]));
      assert.equal(verifyMerkleInclusionProof(root, corruptedLeaf, proof0, 0), false);

      // Wrong index fails verification
      assert.equal(verifyMerkleInclusionProof(root, leaves[0], proof0, 1), false);
    });
  });

  describe('Tokenized External Assets (TLS-721) & Cross-Platform Parity', () => {
    test('ExternalAssetType and Canonical ID Determinism', () => {
      assert.equal(EXTERNAL_ASSET_TYPE_IDS[ExternalAssetType.MESSENGER_USERNAME], 1);
      assert.equal(EXTERNAL_ASSET_TYPE_IDS[ExternalAssetType.IDENTITY_IDENTIFIER], 1);
      assert.equal(EXTERNAL_ASSET_TYPE_IDS[ExternalAssetType.VIRTUAL_NUMBER], 2);
      assert.equal(EXTERNAL_ASSET_TYPE_IDS[ExternalAssetType.GIFT_ASSET], 3);
      assert.equal(EXTERNAL_ASSET_TYPE_IDS[ExternalAssetType.ATTESTATION_BADGE], 4);
      assert.equal(EXTERNAL_ASSET_TYPE_IDS[ExternalAssetType.SOCIAL_IDENTIFIER], 5);
      assert.equal(EXTERNAL_ASSET_TYPE_IDS[ExternalAssetType.ACCESS_CREDENTIAL], 6);
      assert.equal(EXTERNAL_ASSET_TYPE_IDS[ExternalAssetType.DIGITAL_COLLECTIBLE], 7);
      assert.equal(EXTERNAL_ASSET_TYPE_IDS[ExternalAssetType.OTHER_EXTERNAL_ASSET], 8);
      assert.equal(EXTERNAL_ASSET_TYPE_IDS[ExternalAssetType.REAL_WORLD_ASSET], 9);
      assert.equal(EXTERNAL_ASSET_TYPE_IDS[ExternalAssetType.GAMING_ASSET], 10);

      // Legacy aliases backwards compatibility
      assert.equal(EXTERNAL_ASSET_TYPE_IDS[ExternalAssetType.MESSENGER_USERNAME], 1);
      assert.equal(EXTERNAL_ASSET_TYPE_IDS[ExternalAssetType.MESSENGER_NUMBER], 2);
      assert.equal(EXTERNAL_ASSET_TYPE_IDS[ExternalAssetType.MESSENGER_GIFT], 3);
      assert.equal(EXTERNAL_ASSET_TYPE_IDS[ExternalAssetType.MESSENGER_BADGE], 4);
      assert.equal(EXTERNAL_ASSET_TYPE_IDS[ExternalAssetType.SOCIAL_USERNAME], 5);
      assert.equal(EXTERNAL_ASSET_TYPE_IDS[ExternalAssetType.SOCIAL_NUMBER], 6);

      // Open taxonomy: arbitrary custom asset types (1..255)
      assert.equal(getAssetTypeId(42), 42);
      assert.equal(getAssetTypeId('42'), 42);
      assert.throws(() => getAssetTypeId(0), /Invalid numeric assetType/);
      assert.throws(() => getAssetTypeId(256), /Invalid numeric assetType/);

      const canon1 = computeExternalAssetCanonicalId('SocialAppA', ExternalAssetType.IDENTITY_IDENTIFIER, '@anvar');
      const canon2 = computeExternalAssetCanonicalId('SocialAppA', ExternalAssetType.MESSENGER_USERNAME, '@anvar');
      assert.deepEqual(canon1, canon2);

      const canonRwa = computeExternalAssetCanonicalId('RealEstateRegistry', ExternalAssetType.REAL_WORLD_ASSET, 'parcel-1082');
      const canonGame = computeExternalAssetCanonicalId('GameStudio', ExternalAssetType.GAMING_ASSET, 'sword-excalibur');
      const canonCustom = computeExternalAssetCanonicalId('CustomPlatform', 42, 'item-xyz');
      assert.equal(canonRwa.length, 32);
      assert.equal(canonGame.length, 32);
      assert.equal(canonCustom.length, 32);

      const canonDiffPlatform = computeExternalAssetCanonicalId('SocialAppB', ExternalAssetType.IDENTITY_IDENTIFIER, '@anvar');
      assert.notDeepEqual(canon1, canonDiffPlatform);

      const canonDiffType = computeExternalAssetCanonicalId('SocialAppA', ExternalAssetType.SOCIAL_IDENTIFIER, '@anvar');
      assert.notDeepEqual(canon1, canonDiffType);
    });

    test('Absence of Native Identity and Native Gift primitives in SDK', async () => {
      const sdk = await import('../src/index.js');
      assert.equal(sdk.normalizeUsername, undefined);
      assert.equal(sdk.validateUsername, undefined);
      assert.equal(sdk.calculateUsernameFee, undefined);
      assert.equal(sdk.computeUsernameCommitment, undefined);
      assert.equal(sdk.UsernameResolver, undefined);
      assert.equal(sdk.signBadgeAttestation, undefined);
      assert.equal(sdk.verifyBadgeAttestation, undefined);
      assert.equal(sdk.validateTLS721Metadata, undefined);
      assert.equal(sdk.TelegramMiniAppBridge, undefined);
      assert.equal(sdk.MiniAppBridge, undefined);
    });

    test('External Asset Attestation Signing, Replay & Expiry Defenses', async () => {
      const verifier = TolaWallet.generate();
      const user = TolaWallet.generate();
      const chainId = 9999n;
      const nonce = 101n;
      const futureTime = Math.floor(Date.now() / 1000) + 3600;

      const attestation = await signExternalAssetAttestation(
        verifier,
        chainId,
        nonce,
        user.address,
        futureTime,
        'PaxtaGram',
        ExternalAssetType.MESSENGER_USERNAME,
        '@anvar'
      );

      // 1. Valid verification
      assert.equal(verifyExternalAssetAttestation(attestation, verifier.address), true);

      // 2. Expired attestation rejected
      assert.throws(
        () => verifyExternalAssetAttestation(attestation, verifier.address, futureTime + 10),
        /Attestation has expired/
      );

      // 3. Cross-chain replay rejected
      const crossChainAttestation = { ...attestation, chainId: 102 };
      assert.throws(
        () => verifyExternalAssetAttestation(crossChainAttestation, verifier.address),
        /Verifier address mismatch/
      );

      // 4. Cross-wallet replay rejected
      const otherUser = TolaWallet.generate();
      const crossWalletAttestation = { ...attestation, destinationWallet: otherUser.address.toHex() };
      assert.throws(
        () => verifyExternalAssetAttestation(crossWalletAttestation, verifier.address),
        /Verifier address mismatch/
      );

      // 5. Wrong verifier rejected
      const unauthorizedVerifier = TolaWallet.generate();
      assert.throws(
        () => verifyExternalAssetAttestation(attestation, unauthorizedVerifier.address),
        /Verifier address mismatch/
      );
    });

    test('Platform-Neutral AppWalletBridge and Absence of Telegram and MiniApp Legacy Naming', async () => {
      const wallet = TolaWallet.generate();
      const bridge = new AppWalletBridge(wallet);
      const accounts = await bridge.requestAccounts();
      assert.equal(accounts[0], wallet.address.toBech32m());

      // Assert that neither TelegramMiniAppBridge nor MiniAppBridge are exposed in SDK exports
      const sdk = await import('../src/index.js');
      assert.equal(sdk.TelegramMiniAppBridge, undefined);
      assert.equal(sdk.MiniAppBridge, undefined);
      assert.equal(typeof sdk.AppWalletBridge, 'function');
    });

    test('External Asset Metadata Validation and SSRF Filtering', () => {
      const validMeta = {
        name: 'PaxtaGram Handle @anvar',
        sourcePlatform: 'PaxtaGram',
        externalAssetId: '@anvar',
        mediaUri: 'ipfs://bafybeianvarhandle',
      };
      assert.equal(validateExternalAssetMetadata(validMeta), true);

      // SSRF rejected
      const ssrfMeta = { ...validMeta, mediaUri: 'https://127.0.0.1:8545/rpc' };
      assert.throws(() => validateExternalAssetMetadata(ssrfMeta), /SSRF protection/);

      // Missing externalAssetId
      const missingId = { name: 'Asset', sourcePlatform: 'PaxtaGram' };
      assert.throws(() => validateExternalAssetMetadata(missingId), /missing valid externalAssetId/);
    });

    test('External Asset Calldata Encoders & System Contract Address', async () => {
      assert.equal(EXTERNAL_ASSET_CONTRACT_ADDRESS, '0x0000000000000000000000000000000000007721');

      const verifier = TolaWallet.generate();
      const user = TolaWallet.generate();
      const recipient = TolaWallet.generate();

      const attestation = await signExternalAssetAttestation(
        verifier,
        9999n,
        1n,
        user.address,
        Math.floor(Date.now() / 1000) + 3600,
        'PaxtaGram',
        ExternalAssetType.MESSENGER_USERNAME,
        '@anvar'
      );
      assert.ok(attestation.claimExpiresAt !== undefined);

      const mintCalldata = encodeMintExternalAssetCalldata(attestation, 'ipfs://bafybeianvar');
      assert.equal(mintCalldata[0], 1); // TAG_MINT
      assert.ok(mintCalldata.length > 150);

      const transferCalldata = encodeTransferExternalAssetCalldata(user.address, recipient.address, 42n);
      assert.equal(transferCalldata[0], 2); // TAG_TRANSFER_FROM
      assert.equal(transferCalldata.length, 1 + 20 + 20 + 8);

      const burnCalldata = encodeBurnExternalAssetCalldata(42n);
      assert.equal(burnCalldata[0], 6); // TAG_BURN
      assert.equal(burnCalldata.length, 1 + 8);
    });
  });
});


