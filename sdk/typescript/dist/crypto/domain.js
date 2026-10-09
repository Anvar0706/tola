/**
 * Domain separation tags and hashing for TOLA Network.
 * Format: BLAKE3( DomainTag (24B) || ChainID (8B LE) || Epoch (8B LE) || PayloadHash (32B) )
 */

import { blake3 } from './blake3.js';

export const DOMAIN_TRANSACTION_PAYLOAD = new TextEncoder().encode('TOLA-TX-ENVELOPE-v1.0.0-');
export const DOMAIN_CONSENSUS_PREVOTE = new TextEncoder().encode('TOLA-CONSENSUS-PREVOTE-1');
export const DOMAIN_CONSENSUS_PRECOMMIT = new TextEncoder().encode('TOLA-CONSENSUS-PRECOMMIT');

export function domainSeparatedHash(domainTag, chainId, epoch, payloadHash32) {
  if (domainTag.length !== 24) {
    throw new Error(`Domain tag must be exactly 24 bytes, got ${domainTag.length}`);
  }
  if (payloadHash32.length !== 32) {
    throw new Error(`Payload hash must be exactly 32 bytes, got ${payloadHash32.length}`);
  }

  const preImage = new Uint8Array(24 + 8 + 8 + 32);
  preImage.set(domainTag, 0);

  const dv = new DataView(preImage.buffer, preImage.byteOffset, preImage.byteLength);
  dv.setBigUint64(24, BigInt(chainId), true);
  dv.setBigUint64(32, BigInt(epoch), true);
  preImage.set(payloadHash32, 40);

  return blake3(preImage);
}
