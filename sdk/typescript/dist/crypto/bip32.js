/**
 * BIP-32 Hierarchical Deterministic (HD) Key Derivation for Secp256k1.
 * Conforms to BIP-32 / BIP-44 specifications for TOLA Network.
 */

import { createHmac } from 'node:crypto';
import {
  pointMultiply,
  compressPublicKey,
  bytesToBigint,
  bigintToBytes,
  N,
} from './secp256k1.js';

export const CANONICAL_DERIVATION_PATH = "m/44'/9999'/0'/0/0";
export const LEGACY_TESTNET_PROVISIONAL_PATH = "m/44'/102'/0'/0/0";

const BITCOIN_SEED = Buffer.from('Bitcoin seed', 'utf8');

function hmacSha512(key, data) {
  return createHmac('sha512', key).update(data).digest();
}

/**
 * Derive master node from 64-byte seed.
 * @param {Uint8Array} seed - 512-bit master seed.
 * @returns {{ key: Uint8Array, chainCode: Uint8Array }}
 */
export function deriveMasterNode(seed) {
  if (seed.length !== 64) {
    throw new Error(`Master seed must be 64 bytes, got ${seed.length}`);
  }
  const I = hmacSha512(BITCOIN_SEED, seed);
  const key = new Uint8Array(I.subarray(0, 32));
  const chainCode = new Uint8Array(I.subarray(32, 64));
  return { key, chainCode };
}

/**
 * Derive a single child node from parent node and index.
 * @param {{ key: Uint8Array, chainCode: Uint8Array }} parent
 * @param {number} index - 32-bit index (>= 0x80000000 for hardened).
 * @returns {{ key: Uint8Array, chainCode: Uint8Array }}
 */
export function deriveChildNode(parent, index) {
  const isHardened = (index & 0x80000000) !== 0 || index >= 0x80000000;
  const data = Buffer.alloc(37);

  if (isHardened) {
    data[0] = 0x00;
    Buffer.from(parent.key).copy(data, 1);
    data.writeUInt32BE(Number(BigInt(index) & 0xffffffffn), 33);
  } else {
    const k = bytesToBigint(parent.key);
    const pt = pointMultiply(k);
    const pub = compressPublicKey(pt);
    Buffer.from(pub).copy(data, 0);
    data.writeUInt32BE(Number(BigInt(index) & 0xffffffffn), 33);
  }

  const I = hmacSha512(Buffer.from(parent.chainCode), data);
  const IL = bytesToBigint(I.subarray(0, 32));
  if (IL >= N) {
    throw new Error('Child key IL exceeds Secp256k1 curve order');
  }

  const kPar = bytesToBigint(parent.key);
  const childKeyScalar = (IL + kPar) % N;
  if (childKeyScalar === 0n) {
    throw new Error('Child key scalar is zero');
  }

  const childKeyBytes = bigintToBytes(childKeyScalar, 32);
  const childChainCode = new Uint8Array(I.subarray(32, 64));

  return { key: childKeyBytes, chainCode: childChainCode };
}

/**
 * Derive private key along a BIP-32 / BIP-44 path string.
 * @param {Uint8Array} seed - 64-byte master seed.
 * @param {string} [path=CANONICAL_DERIVATION_PATH] - Derivation path (default: m/44'/9999'/0'/0/0).
 * @returns {{ key: Uint8Array, chainCode: Uint8Array }}
 */
export function derivePath(seed, path = CANONICAL_DERIVATION_PATH) {
  const segments = path.split('/').map(s => s.trim());
  if (segments[0] !== 'm') {
    throw new Error(`Invalid derivation path: must start with 'm', got ${path}`);
  }

  let node = deriveMasterNode(seed);

  for (let i = 1; i < segments.length; i++) {
    const seg = segments[i];
    if (!seg) continue;

    const isHardened = seg.endsWith("'") || seg.endsWith('h') || seg.endsWith('H');
    const rawNum = isHardened ? seg.slice(0, -1) : seg;
    const num = parseInt(rawNum, 10);

    if (isNaN(num) || num < 0) {
      throw new Error(`Invalid path segment: ${seg} in ${path}`);
    }

    const index = isHardened ? num + 0x80000000 : num;
    node = deriveChildNode(node, index);
  }

  return node;
}
