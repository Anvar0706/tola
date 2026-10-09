/**
 * Ergonomic Native Wallet and Key Management for TOLA Network.
 * Strictly conforms to TOLA Wallet Derivation Standard v1.0:
 * BIP-39 -> BIP-32 -> BIP-44 (m/44'/9999'/0'/0/0) -> Secp256k1 -> Keccak-256 -> 20-byte address -> Bech32m
 *
 * Implements strict private key security:
 * - Sensitive private keys and mnemonics stored in private class fields (#privateKey, #mnemonic).
 * - Zero secret leakage in console.log, JSON.stringify, or object enumeration.
 * - Model A unified account identity across Mainnet, Testnet, and Devnet.
 */

import {
  pointMultiply,
  publicKeyToAddress,
  compressPublicKey,
  uncompressedPublicKey,
  signDigest,
  bytesToBigint,
  N,
} from './crypto/secp256k1.js';
import { keccak256 } from './crypto/keccak256.js';
import { Address } from './address.js';
import { computeTransactionDigest } from './transaction.js';
import { generateMnemonic, validateMnemonic, mnemonicToSeed } from './crypto/bip39.js';
import { derivePath, CANONICAL_DERIVATION_PATH, LEGACY_TESTNET_PROVISIONAL_PATH } from './crypto/bip32.js';

export class TolaWallet {
  #privateKey;
  #mnemonic;
  #derivationPath;

  /**
   * Internal constructor. Use TolaWallet.generate(), TolaWallet.fromMnemonic(), or TolaWallet.fromPrivateKey().
   * @param {bigint} privateKey - Secp256k1 private scalar.
   * @param {object} [options={}] - Optional metadata (mnemonic, derivationPath).
   */
  constructor(privateKey, options = {}) {
    if (typeof privateKey !== 'bigint') {
      throw new Error('Private key must be a BigInt scalar');
    }
    if (privateKey <= 0n || privateKey >= N) {
      throw new Error('Private key outside valid range [1, N-1]');
    }

    this.#privateKey = privateKey;
    this.#mnemonic = options.mnemonic || null;
    this.#derivationPath = options.derivationPath || null;

    this.publicKeyPoint = pointMultiply(privateKey);
    const addrBytes = publicKeyToAddress(this.publicKeyPoint);
    this.address = new Address(addrBytes);
  }

  /**
   * Generate a fresh wallet with a cryptographically secure BIP-39 recovery phrase.
   * Derives using canonical standard path m/44'/9999'/0'/0/0.
   * @param {12|24} [wordCount=12] - Number of mnemonic words (12 or 24).
   * @param {string} [passphrase=""] - Optional user passphrase.
   * @returns {TolaWallet}
   */
  static generate(wordCount = 12, passphrase = '') {
    const mnemonic = generateMnemonic(wordCount);
    return TolaWallet.fromMnemonic(mnemonic, passphrase);
  }

  /**
   * Restore wallet from a BIP-39 mnemonic phrase.
   * Always derives using canonical path m/44'/9999'/0'/0/0 regardless of network (Model A).
   * @param {string} mnemonic - 12- or 24-word recovery phrase.
   * @param {string} [passphrase=""] - Optional user passphrase.
   * @returns {TolaWallet}
   */
  static fromMnemonic(mnemonic, passphrase = '') {
    const normalized = validateMnemonic(mnemonic);
    const seed = mnemonicToSeed(normalized, passphrase);
    const derived = derivePath(seed, CANONICAL_DERIVATION_PATH);
    const scalar = bytesToBigint(derived.key);

    return new TolaWallet(scalar, {
      mnemonic: normalized,
      derivationPath: CANONICAL_DERIVATION_PATH,
    });
  }

  /**
   * Restore wallet using a custom or legacy derivation path.
   * @param {string} mnemonic - 12- or 24-word recovery phrase.
   * @param {string} [passphrase=""] - Optional user passphrase.
   * @param {string} [path=CANONICAL_DERIVATION_PATH] - Derivation path.
   * @returns {TolaWallet}
   */
  static fromMnemonicWithPath(mnemonic, passphrase = '', path = CANONICAL_DERIVATION_PATH) {
    const normalized = validateMnemonic(mnemonic);
    const seed = mnemonicToSeed(normalized, passphrase);
    const derived = derivePath(seed, path);
    const scalar = bytesToBigint(derived.key);

    return new TolaWallet(scalar, {
      mnemonic: normalized,
      derivationPath: path,
    });
  }

  /**
   * Restore wallet using deprecated legacy testnet path m/44'/102'/0'/0/0 (for migration verification).
   * @param {string} mnemonic - Recovery phrase.
   * @param {string} [passphrase=""] - Optional user passphrase.
   * @returns {TolaWallet}
   */
  static fromMnemonicLegacyTestnet(mnemonic, passphrase = '') {
    return TolaWallet.fromMnemonicWithPath(mnemonic, passphrase, LEGACY_TESTNET_PROVISIONAL_PATH);
  }

  /**
   * Instantiate wallet from raw Secp256k1 private key (BigInt, Uint8Array, or Hex string).
   * Preserves full backward compatibility with legacy private-key imports.
   * @param {bigint|Uint8Array|string} privateKey
   * @returns {TolaWallet}
   */
  static fromPrivateKey(privateKey) {
    if (typeof privateKey === 'bigint') {
      return new TolaWallet(privateKey);
    }
    if (privateKey instanceof Uint8Array || Array.isArray(privateKey)) {
      return TolaWallet.fromPrivateKeyBytes(new Uint8Array(privateKey));
    }
    if (typeof privateKey === 'string') {
      return TolaWallet.fromPrivateKeyHex(privateKey);
    }
    throw new Error('Unsupported private key type. Expected BigInt, Uint8Array, or hex string.');
  }

  /**
   * Instantiate wallet from 32-byte private key scalar.
   * @param {Uint8Array} bytes - 32-byte private key scalar.
   * @returns {TolaWallet}
   */
  static fromPrivateKeyBytes(bytes) {
    if (bytes.length !== 32) {
      throw new Error(`Private key must be 32 bytes, got ${bytes.length}`);
    }
    const scalar = bytesToBigint(bytes);
    return new TolaWallet(scalar);
  }

  /**
   * Instantiate wallet from 64-character hex private key string (with or without '0x').
   * @param {string} hexStr
   * @returns {TolaWallet}
   */
  static fromPrivateKeyHex(hexStr) {
    const clean = hexStr.startsWith('0x') || hexStr.startsWith('0X') ? hexStr.slice(2) : hexStr;
    if (clean.length !== 64) {
      throw new Error(`Private key hex must be 64 characters, got ${clean.length}`);
    }
    const scalar = BigInt('0x' + clean);
    return new TolaWallet(scalar);
  }

  /**
   * Return the canonical recovery mnemonic phrase (if wallet was generated or restored from mnemonic).
   * @returns {string|null}
   */
  get mnemonic() {
    return this.#mnemonic;
  }

  /**
   * Return the derivation path used (if wallet was restored from mnemonic).
   * @returns {string|null}
   */
  get derivationPath() {
    return this.#derivationPath;
  }

  /**
   * Return 32-byte private key as hex string.
   * CAUTION: Sensitive secret material!
   * @returns {string} 0x-prefixed 64-character hex string.
   */
  get privateKeyHex() {
    return '0x' + this.#privateKey.toString(16).padStart(64, '0');
  }

  /**
   * Explicit method to export private key hex string.
   * @returns {string}
   */
  exportPrivateKeyHex() {
    return this.privateKeyHex;
  }

  /**
   * Return raw 20-byte address as hex string.
   * @returns {string} 0x-prefixed 40-character hex address.
   */
  get rawAddress() {
    return this.address.toHex();
  }

  /**
   * Return 33-byte compressed Secp256k1 public key.
   * @returns {Uint8Array}
   */
  get publicKeyBytes() {
    return compressPublicKey(this.publicKeyPoint);
  }

  /**
   * Get public key bytes.
   * @param {boolean} [compressed=true] - Return 33-byte compressed (true) or 65-byte uncompressed (false).
   * @returns {Uint8Array}
   */
  getPublicKey(compressed = true) {
    return compressed ? compressPublicKey(this.publicKeyPoint) : uncompressedPublicKey(this.publicKeyPoint);
  }

  /**
   * Return checksummed Bech32m address for target network.
   * @param {'mainnet'|'testnet'|'devnet'|string} [network='mainnet']
   * @returns {string} e.g. tola1..., ttola1..., dtola1...
   */
  getAddress(network = 'mainnet') {
    return this.address.toBech32m(network);
  }

  /**
   * Return checksummed Bech32m address (backward-compatible alias).
   * @param {'mainnet'|'testnet'|'devnet'|string} [network='mainnet']
   * @returns {string}
   */
  addressBech32m(network = 'mainnet') {
    return this.getAddress(network);
  }

  /**
   * Sign a 32-byte digest returning canonical recoverable signature.
   * @param {Uint8Array} digest32
   * @returns {Promise<{ type: 'Secp256k1', r: Uint8Array, s: Uint8Array, v: number }>}
   */
  async signDigest(digest32) {
    const { r, s, v } = await signDigest(this.#privateKey, digest32);
    return {
      type: 'Secp256k1',
      r,
      s,
      v,
    };
  }

  /**
   * Sign a transaction object matching protocol wire and mempool verification.
   * @param {object} tx - Transaction payload with { from, to, chainId, nonce, ... }.
   * @returns {Promise<object>} Signed transaction object.
   */
  async signTransaction(tx) {
    const fromAddr = Address.fromHex(tx.from);
    if (!fromAddr.equals(this.address)) {
      throw new Error(`Transaction 'from' (${tx.from}) does not match wallet address (${this.address.toHex()})`);
    }

    const digest = computeTransactionDigest(tx);
    const signature = await this.signDigest(digest);

    return {
      ...tx,
      signature,
    };
  }

  /**
   * Sign an arbitrary text or binary message with standard prefix.
   * Digest: Keccak-256("\x19TOLA Signed Message:\n" + len + message)
   * @param {string|Uint8Array} message
   * @returns {Promise<string>} Hex-encoded recoverable signature (0x + 65 bytes: r || s || v).
   */
  async signMessage(message) {
    const messageBytes = typeof message === 'string'
      ? new TextEncoder().encode(message)
      : message instanceof Uint8Array
      ? message
      : new Uint8Array(Buffer.from(message));

    const prefix = new TextEncoder().encode(`\x19TOLA Signed Message:\n${messageBytes.length}`);
    const preImage = new Uint8Array(prefix.length + messageBytes.length);
    preImage.set(prefix, 0);
    preImage.set(messageBytes, prefix.length);

    const digest = keccak256(preImage);
    const sig = await this.signDigest(digest);

    const rHex = Array.from(sig.r).map(b => b.toString(16).padStart(2, '0')).join('');
    const sHex = Array.from(sig.s).map(b => b.toString(16).padStart(2, '0')).join('');
    const vHex = sig.v.toString(16).padStart(2, '0');

    return `0x${rHex}${sHex}${vHex}`;
  }

  /**
   * Safe JSON representation exposing only public information.
   * Private keys and mnemonics are NEVER serialized.
   */
  toJSON() {
    return {
      address: this.getAddress('mainnet'),
      rawAddress: this.rawAddress,
    };
  }

  /**
   * Safe Node.js console.log representation: redacts all private key material.
   */
  [Symbol.for('nodejs.util.inspect.custom')]() {
    return `[TolaWallet: ${this.getAddress('mainnet')}]`;
  }
}
