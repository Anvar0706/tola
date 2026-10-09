/**
 * Pure JavaScript implementation of Secp256k1 elliptic curve arithmetic,
 * RFC 6979 deterministic ECDSA signing, public key recovery, and Ethereum-compatible address derivation.
 */

import { keccak256 } from './keccak256.js';

// Secp256k1 Curve Constants
export const P = 0xfffffffffffffffffffffffffffffffffffffffffffffffffffffffefffffc2fn;
export const N = 0xfffffffffffffffffffffffffffffffebaaedce6af48a03bbfd25e8cd0364141n;
export const HALF_N = N >> 1n;
export const Gx = 0x79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798n;
export const Gy = 0x483ada7726a3c4655da4fbfc0e1108a8fd17b448a68554199c47d08ffb10d4b8n;

export const G = { x: Gx, y: Gy };

function mod(a, m) {
  const result = a % m;
  return result >= 0n ? result : result + m;
}

function modInverse(k, m = P) {
  if (k === 0n) throw new Error('Division by zero');
  let [lm, hm] = [1n, 0n];
  let [low, high] = [mod(k, m), m];
  while (low > 1n) {
    const ratio = high / low;
    const nm = hm - lm * ratio;
    const newLow = high - low * ratio;
    hm = lm;
    lm = nm;
    high = low;
    low = newLow;
  }
  return mod(lm, m);
}

export function pointAdd(p1, p2) {
  if (!p1) return p2;
  if (!p2) return p1;
  if (p1.x === p2.x) {
    if (p1.y !== p2.y) return null; // Point at infinity
    // Double
    const slope = mod(3n * p1.x * p1.x * modInverse(2n * p1.y, P), P);
    const rx = mod(slope * slope - 2n * p1.x, P);
    const ry = mod(slope * (p1.x - rx) - p1.y, P);
    return { x: rx, y: ry };
  }
  // Add
  const slope = mod((p2.y - p1.y) * modInverse(p2.x - p1.x, P), P);
  const rx = mod(slope * slope - p1.x - p2.x, P);
  const ry = mod(slope * (p1.x - rx) - p1.y, P);
  return { x: rx, y: ry };
}

export function pointMultiply(k, p = G) {
  let scalar = mod(k, N);
  if (scalar === 0n) throw new Error('Invalid scalar 0');
  let current = p;
  let result = null;
  while (scalar > 0n) {
    if ((scalar & 1n) === 1n) {
      result = pointAdd(result, current);
    }
    current = pointAdd(current, current);
    scalar >>= 1n;
  }
  if (!result) throw new Error('Invalid point at infinity');
  return result;
}

export function compressPublicKey(point) {
  const out = new Uint8Array(33);
  out[0] = (point.y & 1n) === 0n ? 0x02 : 0x03;
  const xBytes = bigintToBytes(point.x, 32);
  out.set(xBytes, 1);
  return out;
}

export function uncompressedPublicKey(point) {
  const out = new Uint8Array(65);
  out[0] = 0x04;
  out.set(bigintToBytes(point.x, 32), 1);
  out.set(bigintToBytes(point.y, 32), 33);
  return out;
}

export function publicKeyToAddress(point) {
  const uncompressed = uncompressedPublicKey(point);
  const hash = keccak256(uncompressed.subarray(1, 65));
  return hash.subarray(12, 32);
}

export function bytesToBigint(bytes) {
  let hex = '';
  for (const b of bytes) {
    hex += b.toString(16).padStart(2, '0');
  }
  return BigInt('0x' + (hex || '0'));
}

export function bigintToBytes(value, length) {
  const hex = value.toString(16).padStart(length * 2, '0');
  const bytes = new Uint8Array(length);
  for (let i = 0; i < length; i++) {
    bytes[i] = parseInt(hex.substring(i * 2, i * 2 + 2), 16);
  }
  return bytes;
}

async function getWebCrypto() {
  if (typeof globalThis !== 'undefined' && globalThis.crypto?.subtle) {
    return globalThis.crypto;
  }
  const { webcrypto } = await import('node:crypto');
  return webcrypto;
}

async function rfc6979Nonce(privateKey, msgHash) {
  const privBytes = bigintToBytes(privateKey, 32);
  const crypto = await getWebCrypto();

  let v = new Uint8Array(32).fill(1);
  let k = new Uint8Array(32).fill(0);

  let hmacKey = await crypto.subtle.importKey('raw', k, { name: 'HMAC', hash: 'SHA-256' }, false, ['sign']);
  let data1 = new Uint8Array(32 + 1 + 32 + 32);
  data1.set(v, 0);
  data1[32] = 0;
  data1.set(privBytes, 33);
  data1.set(msgHash, 65);
  k = new Uint8Array(await crypto.subtle.sign('HMAC', hmacKey, data1));

  hmacKey = await crypto.subtle.importKey('raw', k, { name: 'HMAC', hash: 'SHA-256' }, false, ['sign']);
  v = new Uint8Array(await crypto.subtle.sign('HMAC', hmacKey, v));

  let data2 = new Uint8Array(32 + 1 + 32 + 32);
  data2.set(v, 0);
  data2[32] = 1;
  data2.set(privBytes, 33);
  data2.set(msgHash, 65);
  k = new Uint8Array(await crypto.subtle.sign('HMAC', hmacKey, data2));

  hmacKey = await crypto.subtle.importKey('raw', k, { name: 'HMAC', hash: 'SHA-256' }, false, ['sign']);
  v = new Uint8Array(await crypto.subtle.sign('HMAC', hmacKey, v));

  while (true) {
    v = new Uint8Array(await crypto.subtle.sign('HMAC', hmacKey, v));
    const candidate = bytesToBigint(v);
    if (candidate > 0n && candidate < N) {
      return candidate;
    }
    const data3 = new Uint8Array(33);
    data3.set(v, 0);
    data3[32] = 0;
    k = new Uint8Array(await crypto.subtle.sign('HMAC', hmacKey, data3));
    hmacKey = await crypto.subtle.importKey('raw', k, { name: 'HMAC', hash: 'SHA-256' }, false, ['sign']);
    v = new Uint8Array(await crypto.subtle.sign('HMAC', hmacKey, v));
  }
}

export async function signDigest(privateKey, digest32) {
  if (privateKey <= 0n || privateKey >= N) {
    throw new Error('Private key outside valid scalar range [1, N-1]');
  }

  const e = mod(bytesToBigint(digest32), N);
  let k = await rfc6979Nonce(privateKey, digest32);

  let R = pointMultiply(k);
  let r = mod(R.x, N);
  if (r === 0n) throw new Error('Calculated r = 0, retry');

  let s = mod(modInverse(k, N) * (e + r * privateKey), N);
  if (s === 0n) throw new Error('Calculated s = 0, retry');

  let v = Number(R.y & 1n);

  if (s > HALF_N) {
    s = N - s;
    v = v ^ 1;
  }

  return {
    r: bigintToBytes(r, 32),
    s: bigintToBytes(s, 32),
    v,
  };
}

export function modPow(base, exp, m = P) {
  let res = 1n;
  let b = mod(base, m);
  let e = exp;
  while (e > 0n) {
    if (e & 1n) res = mod(res * b, m);
    b = mod(b * b, m);
    e >>= 1n;
  }
  return res;
}

export function recoverPublicKey(digest32, rBytes, sBytes, v) {
  const r = bytesToBigint(rBytes);
  const s = bytesToBigint(sBytes);
  const e = mod(bytesToBigint(digest32), N);

  if (r <= 0n || r >= N || s <= 0n || s >= N) {
    throw new Error('Invalid signature r or s value');
  }

  // 1. Recover R point
  const x = r;
  const alpha = mod(x * x * x + 7n, P);
  const beta = modPow(alpha, (P + 1n) / 4n, P);
  if (mod(beta * beta, P) !== alpha) {
    throw new Error('Quadratic residue does not exist for R.x');
  }

  let y = beta;
  if ((y & 1n) !== BigInt(v & 1)) {
    y = P - y;
  }
  const R = { x, y };

  // 2. Compute Q = r^(-1) * (s * R - e * G)
  const rInv = modInverse(r, N);
  const sR = pointMultiply(s, R);
  const eG = pointMultiply(e, G);

  const negEG = { x: eG.x, y: P - eG.y };
  const diff = pointAdd(sR, negEG);
  if (!diff) throw new Error('Recovery yielded point at infinity');

  return pointMultiply(rInv, diff);
}

export function recoverAddress(digest32, rBytes, sBytes, v) {
  const pubPoint = recoverPublicKey(digest32, rBytes, sBytes, v);
  return publicKeyToAddress(pubPoint);
}

