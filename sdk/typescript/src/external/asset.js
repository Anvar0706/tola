/**
 * Tokenized External Asset Protocol for TOLA Network.
 * Represents on-chain TLS-721 tokenized representations of external platform assets
 * (e.g. PaxtaGram usernames, Gifts Numbers, messenger gifts, social handles)
 * minted via authorized third-party verifier attestations.
 *
 * CRITICAL ARCHITECTURAL INVARIANT:
 * TOLA NFT ownership represents blockchain ownership of the representation on TOLA.
 * It does NOT automatically grant or synchronize legal control over the original external platform asset.
 */

import { keccak256 } from '../crypto/keccak256.js';
import { recoverAddress } from '../crypto/secp256k1.js';
import { Address } from '../address.js';

export const EXTERNAL_ASSET_CONTRACT_ADDRESS =
  '0x0000000000000000000000000000000000007721';

export class ExternalAssetError extends Error {
  constructor(message) {
    super(message);
    this.name = 'ExternalAssetError';
  }
}

export const ExternalAssetType = Object.freeze({
  // Canonical platform-neutral asset types
  IDENTITY_IDENTIFIER: 'IDENTITY_IDENTIFIER',
  VIRTUAL_NUMBER: 'VIRTUAL_NUMBER',
  GIFT_ASSET: 'GIFT_ASSET',
  ATTESTATION_BADGE: 'ATTESTATION_BADGE',
  SOCIAL_IDENTIFIER: 'SOCIAL_IDENTIFIER',
  ACCESS_CREDENTIAL: 'ACCESS_CREDENTIAL',
  DIGITAL_COLLECTIBLE: 'DIGITAL_COLLECTIBLE',
  OTHER_EXTERNAL_ASSET: 'OTHER_EXTERNAL_ASSET',
  REAL_WORLD_ASSET: 'REAL_WORLD_ASSET',
  GAMING_ASSET: 'GAMING_ASSET',

  // Backward-compatible legacy aliases
  MESSENGER_USERNAME: 'MESSENGER_USERNAME',
  MESSENGER_NUMBER: 'MESSENGER_NUMBER',
  MESSENGER_GIFT: 'MESSENGER_GIFT',
  MESSENGER_BADGE: 'MESSENGER_BADGE',
  SOCIAL_USERNAME: 'SOCIAL_USERNAME',
  SOCIAL_NUMBER: 'SOCIAL_NUMBER',
});

export const EXTERNAL_ASSET_TYPE_IDS = Object.freeze({
  // Canonical identifiers
  IDENTITY_IDENTIFIER: 1,
  VIRTUAL_NUMBER: 2,
  GIFT_ASSET: 3,
  ATTESTATION_BADGE: 4,
  SOCIAL_IDENTIFIER: 5,
  ACCESS_CREDENTIAL: 6,
  DIGITAL_COLLECTIBLE: 7,
  OTHER_EXTERNAL_ASSET: 8,
  REAL_WORLD_ASSET: 9,
  GAMING_ASSET: 10,

  // Backward-compatible mappings
  MESSENGER_USERNAME: 1,
  MESSENGER_NUMBER: 2,
  MESSENGER_GIFT: 3,
  MESSENGER_BADGE: 4,
  SOCIAL_USERNAME: 5,
  SOCIAL_NUMBER: 6,
});

export function getAssetTypeId(assetType) {
  if (typeof assetType === 'number') {
    if (Number.isInteger(assetType) && assetType >= 1 && assetType <= 255) return assetType;
    throw new ExternalAssetError(`Invalid numeric assetType: ${assetType}`);
  }
  const id = EXTERNAL_ASSET_TYPE_IDS[assetType];
  if (id) return id;
  if (typeof assetType === 'string' && /^\d+$/.test(assetType)) {
    const parsed = parseInt(assetType, 10);
    if (parsed >= 1 && parsed <= 255) return parsed;
  }
  throw new ExternalAssetError(`Unknown assetType: ${assetType}`);
}

/**
 * Computes canonical 32-byte identity for an external asset to enforce duplicate mint protection.
 * Format: Keccak256("TOLA_EXTERNAL_ASSET_CANONICAL_V1:" || source_len(4B BE) || source || asset_type(1B) || external_id_len(4B BE) || external_id)
 */
export function computeExternalAssetCanonicalId(sourcePlatform, assetType, externalAssetId) {
  if (!sourcePlatform || typeof sourcePlatform !== 'string') {
    throw new ExternalAssetError('Invalid sourcePlatform');
  }
  if (!externalAssetId || typeof externalAssetId !== 'string') {
    throw new ExternalAssetError('Invalid externalAssetId');
  }

  const typeId = getAssetTypeId(assetType);
  const prefix = new TextEncoder().encode('TOLA_EXTERNAL_ASSET_CANONICAL_V1:');
  const srcBytes = new TextEncoder().encode(sourcePlatform);
  const idBytes = new TextEncoder().encode(externalAssetId);

  const numBuffer = new ArrayBuffer(4 + 1 + 4);
  const view = new DataView(numBuffer);
  view.setUint32(0, srcBytes.length, false);
  view.setUint8(4, typeId);
  view.setUint32(5, idBytes.length, false);
  const numBytes = new Uint8Array(numBuffer);

  const totalLen = prefix.length + 4 + srcBytes.length + 1 + 4 + idBytes.length;
  const preImage = new Uint8Array(totalLen);

  let offset = 0;
  preImage.set(prefix, offset); offset += prefix.length;
  preImage.set(numBytes.slice(0, 4), offset); offset += 4; // src_len(4)
  preImage.set(srcBytes, offset); offset += srcBytes.length;
  preImage.set(numBytes.slice(4, 5), offset); offset += 1; // typeId(1)
  preImage.set(numBytes.slice(5, 9), offset); offset += 4; // id_len(4)
  preImage.set(idBytes, offset);

  return keccak256(preImage);
}

/**
 * Computes canonical 32-byte digest for external asset mint attestation signing.
 * Format: Keccak256("TOLA_EXTERNAL_ASSET_ATTESTATION_V1:" || chain_id(8B BE) || nonce(8B BE) || destination(20B) || expires_at(8B BE) || source_len(4B BE) || source || asset_type(1B) || external_id_len(4B BE) || external_id)
 */
export function computeExternalAssetAttestationDigest(
  chainId,
  nonce,
  destinationWallet,
  expiresAt,
  sourcePlatform,
  assetType,
  externalAssetId
) {
  const typeId = getAssetTypeId(assetType);
  const prefix = new TextEncoder().encode('TOLA_EXTERNAL_ASSET_ATTESTATION_V1:');
  const srcBytes = new TextEncoder().encode(sourcePlatform);
  const idBytes = new TextEncoder().encode(externalAssetId);

  const destAddr = destinationWallet instanceof Address
    ? destinationWallet
    : (destinationWallet.startsWith('0x') || destinationWallet.startsWith('0X')
        ? Address.fromHex(destinationWallet)
        : Address.fromBech32m(destinationWallet).address);

  const numBuffer = new ArrayBuffer(8 + 8 + 8 + 4 + 1 + 4);
  const view = new DataView(numBuffer);
  view.setBigUint64(0, BigInt(chainId), false);
  view.setBigUint64(8, BigInt(nonce), false);
  view.setBigUint64(16, BigInt(expiresAt), false);
  view.setUint32(24, srcBytes.length, false);
  view.setUint8(28, typeId);
  view.setUint32(29, idBytes.length, false);
  const numBytes = new Uint8Array(numBuffer);

  const totalLen = prefix.length + 8 + 8 + 20 + 8 + 4 + srcBytes.length + 1 + 4 + idBytes.length;
  const preImage = new Uint8Array(totalLen);

  let offset = 0;
  preImage.set(prefix, offset); offset += prefix.length;
  preImage.set(numBytes.slice(0, 8), offset); offset += 8; // chainId(8)
  preImage.set(numBytes.slice(8, 16), offset); offset += 8; // nonce(8)
  preImage.set(destAddr.bytes, offset); offset += 20; // destination(20)
  preImage.set(numBytes.slice(16, 24), offset); offset += 8; // expiresAt(8)
  preImage.set(numBytes.slice(24, 28), offset); offset += 4; // src_len(4)
  preImage.set(srcBytes, offset); offset += srcBytes.length;
  preImage.set(numBytes.slice(28, 29), offset); offset += 1; // typeId(1)
  preImage.set(numBytes.slice(29, 33), offset); offset += 4; // id_len(4)
  preImage.set(idBytes, offset);

  return keccak256(preImage);
}

/**
 * Sign an external asset mint attestation by an authorized verifier.
 */
export async function signExternalAssetAttestation(
  verifierWallet,
  chainId,
  nonce,
  destinationWallet,
  expiresAt,
  sourcePlatform,
  assetType,
  externalAssetId
) {
  const digest = computeExternalAssetAttestationDigest(
    chainId,
    nonce,
    destinationWallet,
    expiresAt,
    sourcePlatform,
    assetType,
    externalAssetId
  );
  const sig = await verifierWallet.signDigest(digest);
  const destHex = destinationWallet instanceof Address
    ? destinationWallet.toHex()
    : destinationWallet;

  return {
    chainId: Number(chainId),
    nonce: Number(nonce),
    destinationWallet: destHex,
    expiresAt: Number(expiresAt),
    claimExpiresAt: Number(expiresAt),
    sourcePlatform,
    assetType: typeof assetType === 'string' ? assetType : Object.keys(EXTERNAL_ASSET_TYPE_IDS).find(k => EXTERNAL_ASSET_TYPE_IDS[k] === assetType),
    externalAssetId,
    verifierAddress: verifierWallet.address.toHex(),
    signature: {
      r: Array.from(sig.r).map(b => b.toString(16).padStart(2, '0')).join(''),
      s: Array.from(sig.s).map(b => b.toString(16).padStart(2, '0')).join(''),
      v: sig.v,
    },
  };
}

/**
 * Verify an external asset attestation signature and expiration.
 */
export function verifyExternalAssetAttestation(
  attestation,
  expectedVerifier,
  currentTime = Math.floor(Date.now() / 1000)
) {
  const {
    chainId,
    nonce,
    destinationWallet,
    sourcePlatform,
    assetType,
    externalAssetId,
    verifierAddress,
    signature,
  } = attestation;

  const expiresAt = attestation.claimExpiresAt !== undefined
    ? Number(attestation.claimExpiresAt)
    : Number(attestation.expiresAt);

  if (currentTime > expiresAt) {
    throw new ExternalAssetError('Attestation has expired');
  }

  const verifierAddr = expectedVerifier instanceof Address
    ? expectedVerifier
    : (expectedVerifier.startsWith('0x') || expectedVerifier.startsWith('0X')
        ? Address.fromHex(expectedVerifier)
        : Address.fromBech32m(expectedVerifier).address);

  const digest = computeExternalAssetAttestationDigest(
    chainId,
    nonce,
    destinationWallet,
    expiresAt,
    sourcePlatform,
    assetType,
    externalAssetId
  );

  const rBytes = new Uint8Array(32);
  const sBytes = new Uint8Array(32);
  for (let i = 0; i < 32; i++) {
    rBytes[i] = parseInt(signature.r.slice(i * 2, i * 2 + 2), 16);
    sBytes[i] = parseInt(signature.s.slice(i * 2, i * 2 + 2), 16);
  }

  const recoveredAddrBytes = recoverAddress(digest, rBytes, sBytes, signature.v);
  const recoveredAddr = new Address(recoveredAddrBytes);

  if (!recoveredAddr.equals(verifierAddr)) {
    throw new ExternalAssetError(
      `Verifier address mismatch: expected ${verifierAddr.toHex()}, got ${recoveredAddr.toHex()}`
    );
  }

  return true;
}

/**
 * Validates external asset metadata against size bounds and SSRF vectors.
 */
export function validateExternalAssetMetadata(metadata) {
  if (!metadata || typeof metadata !== 'object') {
    throw new ExternalAssetError('Metadata must be a valid JSON object');
  }
  const jsonStr = JSON.stringify(metadata);
  if (new TextEncoder().encode(jsonStr).length > 65536) {
    throw new ExternalAssetError('Metadata payload exceeds maximum bound of 64 KB');
  }
  if (!metadata.sourcePlatform || typeof metadata.sourcePlatform !== 'string') {
    throw new ExternalAssetError('Metadata missing valid sourcePlatform');
  }
  if (!metadata.externalAssetId || typeof metadata.externalAssetId !== 'string') {
    throw new ExternalAssetError('Metadata missing valid externalAssetId');
  }
  if (metadata.mediaUri && typeof metadata.mediaUri === 'string') {
    const uri = metadata.mediaUri.toLowerCase();
    if (
      uri.includes('localhost') ||
      uri.includes('127.0.0.1') ||
      uri.includes('169.254.169.254') ||
      uri.includes('10.') ||
      uri.includes('192.168.')
    ) {
      throw new ExternalAssetError('SSRF protection: Private network and loopback URIs are forbidden');
    }
  }
  return true;
}

/**
 * Encode calldata for calling ExternalAssetContract mintWithAttestation.
 */
export function encodeMintExternalAssetCalldata(attestation, metadataUri = '') {
  const chainId = BigInt(attestation.chainId);
  const nonce = BigInt(attestation.nonce);
  const destAddr = attestation.destinationWallet instanceof Address
    ? attestation.destinationWallet
    : Address.fromHex(attestation.destinationWallet);
  const expiresAt = BigInt(attestation.claimExpiresAt !== undefined ? attestation.claimExpiresAt : attestation.expiresAt);
  const srcBytes = new TextEncoder().encode(attestation.sourcePlatform);
  const typeId = getAssetTypeId(attestation.assetType);
  const idBytes = new TextEncoder().encode(attestation.externalAssetId);
  const verifierAddr = attestation.verifierAddress instanceof Address
    ? attestation.verifierAddress
    : Address.fromHex(attestation.verifierAddress);

  const rClean = attestation.signature.r.replace(/^0x/i, '');
  const sClean = attestation.signature.s.replace(/^0x/i, '');
  const rBytes = new Uint8Array(32);
  const sBytes = new Uint8Array(32);
  for (let i = 0; i < 32; i++) {
    rBytes[i] = parseInt(rClean.slice(i * 2, i * 2 + 2), 16);
    sBytes[i] = parseInt(sClean.slice(i * 2, i * 2 + 2), 16);
  }
  const v = attestation.signature.v;
  const metaBytes = new TextEncoder().encode(metadataUri);

  const totalLen = 1 + 8 + 8 + 20 + 8 + 4 + srcBytes.length + 1 + 4 + idBytes.length + 20 + 65 + 4 + metaBytes.length;
  const buf = new Uint8Array(totalLen);
  const view = new DataView(buf.buffer, buf.byteOffset, buf.byteLength);

  let offset = 0;
  buf[offset] = 1; offset += 1; // TAG_MINT
  view.setBigUint64(offset, chainId, false); offset += 8;
  view.setBigUint64(offset, nonce, false); offset += 8;
  buf.set(destAddr.bytes, offset); offset += 20;
  view.setBigUint64(offset, expiresAt, false); offset += 8;
  view.setUint32(offset, srcBytes.length, false); offset += 4;
  buf.set(srcBytes, offset); offset += srcBytes.length;
  buf[offset] = typeId; offset += 1;
  view.setUint32(offset, idBytes.length, false); offset += 4;
  buf.set(idBytes, offset); offset += idBytes.length;
  buf.set(verifierAddr.bytes, offset); offset += 20;
  buf.set(rBytes, offset); offset += 32;
  buf.set(sBytes, offset); offset += 32;
  buf[offset] = v; offset += 1;
  view.setUint32(offset, metaBytes.length, false); offset += 4;
  buf.set(metaBytes, offset);

  return buf;
}

/**
 * Encode calldata for calling ExternalAssetContract transferFrom.
 */
export function encodeTransferExternalAssetCalldata(from, to, tokenId) {
  const fromAddr = from instanceof Address ? from : Address.fromHex(from);
  const toAddr = to instanceof Address ? to : Address.fromHex(to);
  const tid = BigInt(tokenId);

  const buf = new Uint8Array(1 + 20 + 20 + 8);
  const view = new DataView(buf.buffer, buf.byteOffset, buf.byteLength);
  buf[0] = 2; // TAG_TRANSFER_FROM
  buf.set(fromAddr.bytes, 1);
  buf.set(toAddr.bytes, 21);
  view.setBigUint64(41, tid, false);
  return buf;
}

/**
 * Encode calldata for calling ExternalAssetContract burn.
 */
export function encodeBurnExternalAssetCalldata(tokenId) {
  const tid = BigInt(tokenId);
  const buf = new Uint8Array(1 + 8);
  const view = new DataView(buf.buffer, buf.byteOffset, buf.byteLength);
  buf[0] = 6; // TAG_BURN
  view.setBigUint64(1, tid, false);
  return buf;
}
