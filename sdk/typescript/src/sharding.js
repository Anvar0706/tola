/**
 * TOLA Network Sharding & Cross-Shard State Proof Utilities.
 * Strictly conforms to ADR-0014.
 */

import { blake3 } from './crypto/blake3.js';
import { Address } from './address.js';

export const DEFAULT_NUM_SHARDS = 4;
export const DOMAIN_RECEIPT_HASH = new TextEncoder().encode('TOLA-RECEIPT-HASH-v1.0.0--');
export const DOMAIN_RECEIPT_NULLIFIER = new TextEncoder().encode('TOLA-RECEIPT-NULLIFIER-v1-');

function parseAddress(addr) {
  if (addr instanceof Address) return addr;
  if (typeof addr === 'string') {
    if (addr.startsWith('tola1') || addr.startsWith('ttola1') || addr.startsWith('dtola1')) {
      return Address.fromBech32m(addr).address;
    }
    return Address.fromHex(addr);
  }
  if (addr instanceof Uint8Array) return Address.fromBytes(addr);
  throw new Error('Unsupported address format');
}

/**
 * Computes the deterministic shard ID for an address.
 * Formula: ((addrBytes[0] << 8) | addrBytes[1]) % numShards.
 */
export function computeShardId(address, numShards = DEFAULT_NUM_SHARDS) {
  if (numShards <= 0) return 0;
  const addr = parseAddress(address);
  const high = (addr.bytes[0] << 8) | addr.bytes[1];
  return high % numShards;
}

/**
 * Computes unique 32-byte receipt nullifier for cross-shard replay immunity.
 */
export function computeReceiptNullifier(sourceShard, targetShard, receiptNonce, senderAddress) {
  const addr = parseAddress(senderAddress);
  const buf = new Uint8Array(DOMAIN_RECEIPT_NULLIFIER.length + 4 + 4 + 8 + 20);
  buf.set(DOMAIN_RECEIPT_NULLIFIER, 0);

  let offset = DOMAIN_RECEIPT_NULLIFIER.length;
  const view = new DataView(buf.buffer, buf.byteOffset, buf.byteLength);

  view.setUint32(offset, Number(sourceShard), true);
  offset += 4;
  view.setUint32(offset, Number(targetShard), true);
  offset += 4;
  view.setBigUint64(offset, BigInt(receiptNonce), true);
  offset += 8;
  buf.set(addr.bytes, offset);

  return blake3(buf);
}

/**
 * Verifies a binary Merkle inclusion proof against a root.
 */
export function verifyMerkleInclusionProof(root, leaf, proof, index) {
  let current = leaf;
  let currentIdx = index;

  for (const sibling of proof) {
    const concat = new Uint8Array(64);
    if (currentIdx % 2 === 0) {
      concat.set(current, 0);
      concat.set(sibling, 32);
    } else {
      concat.set(sibling, 0);
      concat.set(current, 32);
    }
    current = blake3(concat);
    currentIdx = Math.floor(currentIdx / 2);
  }

  if (current.length !== root.length) return false;
  for (let i = 0; i < current.length; i++) {
    if (current[i] !== root[i]) return false;
  }
  return true;
}
