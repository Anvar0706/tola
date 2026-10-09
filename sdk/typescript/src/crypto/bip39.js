/**
 * BIP-39 Mnemonic Generation, Validation, and Seed Derivation for TOLA Network.
 * Strictly adheres to BIP-39 English wordlist, NFKD normalization, and PBKDF2-HMAC-SHA512.
 */

import { createHash, pbkdf2Sync, randomBytes } from 'node:crypto';
import { WORDLIST, WORD_INDEX_MAP } from './wordlist.js';

function getCryptoRandomBytes(len) {
  if (typeof globalThis !== 'undefined' && globalThis.crypto?.getRandomValues) {
    const bytes = new Uint8Array(len);
    globalThis.crypto.getRandomValues(bytes);
    return bytes;
  }
  return new Uint8Array(randomBytes(len));
}

function sha256(data) {
  return createHash('sha256').update(data).digest();
}

/**
 * Generate a fresh BIP-39 mnemonic phrase using CSPRNG.
 * @param {12|24} wordCount - Word count (12 for 128-bit entropy, 24 for 256-bit entropy). Default: 12.
 * @returns {string} Space-separated mnemonic phrase.
 */
export function generateMnemonic(wordCount = 12) {
  if (wordCount !== 12 && wordCount !== 24) {
    throw new Error(`Unsupported word count: ${wordCount}. Expected 12 or 24 words.`);
  }

  const entBytesLen = wordCount === 12 ? 16 : 32;
  const entropy = getCryptoRandomBytes(entBytesLen);
  const hash = sha256(entropy);
  const csBitsLen = entBytesLen / 4; // 4 bits for 128-bit, 8 bits for 256-bit

  let bits = '';
  for (let i = 0; i < entropy.length; i++) {
    bits += entropy[i].toString(2).padStart(8, '0');
  }
  const hashBits = hash[0].toString(2).padStart(8, '0');
  bits += hashBits.slice(0, csBitsLen);

  const words = [];
  for (let i = 0; i < bits.length; i += 11) {
    const idx = parseInt(bits.slice(i, i + 11), 2);
    words.push(WORDLIST[idx]);
  }

  return words.join(' ');
}

/**
 * Validate and normalize a BIP-39 mnemonic phrase.
 * Enforces NFKD normalization, English wordlist inclusion, and SHA-256 checksum verification.
 * @param {string} phrase - Mnemonic phrase to validate.
 * @returns {string} Normalized mnemonic phrase.
 * @throws {Error} If phrase is invalid, word not in wordlist, or checksum fails.
 */
export function validateMnemonic(phrase) {
  if (!phrase || typeof phrase !== 'string') {
    throw new Error('Mnemonic phrase must be a non-empty string');
  }

  const normalized = phrase.normalize('NFKD').trim().toLowerCase().split(/\s+/).join(' ');
  const words = normalized.split(' ');

  if (words.length !== 12 && words.length !== 24) {
    throw new Error(`Unsupported word count: ${words.length}. Expected 12 or 24 words.`);
  }

  let bits = '';
  for (const w of words) {
    const idx = WORD_INDEX_MAP.get(w);
    if (idx === undefined) {
      throw new Error(`Word not in BIP-39 wordlist: ${w}`);
    }
    bits += idx.toString(2).padStart(11, '0');
  }

  const entBitsLen = words.length === 12 ? 128 : 256;
  const csBitsLen = words.length === 12 ? 4 : 8;
  const entBits = bits.slice(0, entBitsLen);
  const csBits = bits.slice(entBitsLen);

  const entBytes = new Uint8Array(entBitsLen / 8);
  for (let i = 0; i < entBytes.length; i++) {
    entBytes[i] = parseInt(entBits.slice(i * 8, i * 8 + 8), 2);
  }

  const hash = sha256(entBytes);
  const expectedCsBits = hash[0].toString(2).padStart(8, '0').slice(0, csBitsLen);
  if (csBits !== expectedCsBits) {
    throw new Error('Invalid mnemonic checksum');
  }

  return normalized;
}

/**
 * Derive 512-bit master seed from mnemonic phrase using PBKDF2-HMAC-SHA512 (2048 iterations).
 * @param {string} phrase - Mnemonic phrase.
 * @param {string} [passphrase=""] - Optional user passphrase.
 * @returns {Uint8Array} 64-byte master seed.
 */
export function mnemonicToSeed(phrase, passphrase = '') {
  const normalizedPhrase = validateMnemonic(phrase);
  const normalizedPassphrase = (passphrase || '').normalize('NFKD');
  const salt = `mnemonic${normalizedPassphrase}`;

  const seedBuffer = pbkdf2Sync(
    Buffer.from(normalizedPhrase, 'utf8'),
    Buffer.from(salt, 'utf8'),
    2048,
    64,
    'sha512'
  );

  return new Uint8Array(seedBuffer);
}
