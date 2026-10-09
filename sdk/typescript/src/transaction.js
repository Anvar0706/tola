/**
 * Canonical Transaction Serialization, Digest Calculation, and Fluent Builder for TOLA Network.
 * Strictly conforms to tola-codec and tola-crypto specifications.
 */

import { blake3 } from './crypto/blake3.js';
import { domainSeparatedHash, DOMAIN_TRANSACTION_PAYLOAD } from './crypto/domain.js';
import { Address } from './address.js';

function writeU64LE(view, offset, value) {
  view.setBigUint64(offset, BigInt(value), true);
}

function writeU128LE(target, offset, value) {
  const v = BigInt(value);
  const low = v & 0xffffffffffffffffn;
  const high = (v >> 64n) & 0xffffffffffffffffn;
  const view = new DataView(target.buffer, target.byteOffset + offset, 16);
  view.setBigUint64(0, low, true);
  view.setBigUint64(8, high, true);
}

function parseHexBytes(data) {
  if (!data) return new Uint8Array(0);
  if (data instanceof Uint8Array) return data;
  const clean = data.startsWith('0x') || data.startsWith('0X') ? data.slice(2) : data;
  if (clean.length % 2 !== 0) {
    throw new Error('Hex string must have an even number of characters');
  }
  const bytes = new Uint8Array(clean.length / 2);
  for (let i = 0; i < bytes.length; i++) {
    bytes[i] = parseInt(clean.substring(i * 2, i * 2 + 2), 16);
  }
  return bytes;
}

export const MAX_MEMO_BYTES = 256;

/**
 * Serialize unsigned transaction to canonical bytes for payload hashing.
 */
export function serializeUnsignedTransaction(tx) {
  const dataBytes = parseHexBytes(tx.data);
  const fromAddr = Address.fromHex(tx.from);
  const toAddr = tx.to ? Address.fromHex(tx.to) : null;

  const memoStr = tx.memo !== undefined && tx.memo !== null ? String(tx.memo).trim() : null;
  let memoBytes = null;
  if (memoStr && memoStr.length > 0) {
    memoBytes = new TextEncoder().encode(memoStr);
    if (memoBytes.length > MAX_MEMO_BYTES) {
      throw new Error(
        `Memo length ${memoBytes.length} bytes exceeds maximum allowed ${MAX_MEMO_BYTES} bytes`
      );
    }
  }

  const memoLen = memoBytes ? 4 + memoBytes.length : 0;
  const totalLen =
    8 + 8 + 20 + (toAddr ? 1 + 20 : 1) +
    16 + 8 + 16 + 16 +
    8 + 4 + dataBytes.length + memoLen;

  const buf = new Uint8Array(totalLen);
  const view = new DataView(buf.buffer, buf.byteOffset, buf.byteLength);

  let offset = 0;

  // 1. chain_id
  writeU64LE(view, offset, tx.chainId);
  offset += 8;

  // 2. nonce
  writeU64LE(view, offset, tx.nonce);
  offset += 8;

  // 3. from
  buf.set(fromAddr.bytes, offset);
  offset += 20;

  // 4. to option
  if (toAddr) {
    buf[offset] = 1;
    offset += 1;
    buf.set(toAddr.bytes, offset);
    offset += 20;
  } else {
    buf[offset] = 0;
    offset += 1;
  }

  // 5. value
  writeU128LE(buf, offset, tx.value);
  offset += 16;

  // 6. gas_limit
  writeU64LE(view, offset, tx.gasLimit);
  offset += 8;

  // 7. max_fee_per_gas
  writeU128LE(buf, offset, tx.maxFeePerGas);
  offset += 16;

  // 8. max_priority_fee_per_gas
  writeU128LE(buf, offset, tx.maxPriorityFeePerGas);
  offset += 16;

  // 9. valid_until_block
  writeU64LE(view, offset, tx.validUntilBlock);
  offset += 8;

  // 10. data
  view.setUint32(offset, dataBytes.length, true);
  offset += 4;
  buf.set(dataBytes, offset);
  offset += dataBytes.length;

  // 11. memo (optional)
  if (memoBytes) {
    view.setUint32(offset, memoBytes.length, true);
    offset += 4;
    buf.set(memoBytes, offset);
    offset += memoBytes.length;
  }

  return buf;
}

/**
 * Serialize signed transaction to wire bytes.
 */
export function serializeSignedTransaction(tx) {
  const unsignedBytes = serializeUnsignedTransaction(tx);
  let sigLen = 0;

  if (tx.signature.type === 'Secp256k1') {
    sigLen = 1 + 32 + 32 + 1;
  } else if (tx.signature.type === 'Ed25519') {
    sigLen = 1 + 64;
  } else {
    throw new Error('Unsupported signature type');
  }

  const out = new Uint8Array(unsignedBytes.length + sigLen);
  out.set(unsignedBytes, 0);

  let offset = unsignedBytes.length;
  if (tx.signature.type === 'Secp256k1') {
    out[offset] = 0x02;
    offset += 1;
    out.set(tx.signature.r, offset);
    offset += 32;
    out.set(tx.signature.s, offset);
    offset += 32;
    out[offset] = tx.signature.v;
  } else if (tx.signature.type === 'Ed25519') {
    out[offset] = 0x01;
    offset += 1;
    out.set(tx.signature.signature, offset);
  }

  return out;
}

/**
 * Compute canonical transaction signing digest matching protocol mempool verification.
 */
export function computeTransactionDigest(tx) {
  const unsignedBytes = serializeUnsignedTransaction(tx);
  const payloadHash = blake3(unsignedBytes);
  return domainSeparatedHash(DOMAIN_TRANSACTION_PAYLOAD, tx.chainId, 0n, payloadHash);
}

/**
 * Fluent Transaction Builder for creating native transfer and smart contract transactions.
 */
export class TransactionBuilder {
  constructor(chainId = 102n) {
    this._chainId = BigInt(chainId);
    this._nonce = 0n;
    this._to = null;
    this._value = 0n;
    this._gasLimit = 21000n;
    this._maxFeePerGas = 1000000000n;
    this._maxPriorityFeePerGas = 1000000000n;
    this._validUntilBlock = 1000000n;
    this._data = new Uint8Array(0);
    this._memo = null;
  }

  chainId(id) {
    this._chainId = BigInt(id);
    return this;
  }

  nonce(n) {
    this._nonce = BigInt(n);
    return this;
  }

  to(addr) {
    if (!addr) {
      this._to = null;
    } else if (typeof addr === 'string') {
      if (addr.startsWith('tola1') || addr.startsWith('ttola1') || addr.startsWith('dtola1')) {
        this._to = Address.fromBech32m(addr).address.toHex();
      } else {
        this._to = Address.fromHex(addr).toHex();
      }
    } else {
      this._to = addr.toHex();
    }
    return this;
  }

  value(val) {
    this._value = BigInt(val);
    return this;
  }

  gasLimit(gas) {
    this._gasLimit = BigInt(gas);
    return this;
  }

  maxFeePerGas(fee) {
    this._maxFeePerGas = BigInt(fee);
    return this;
  }

  maxPriorityFeePerGas(priorityFee) {
    this._maxPriorityFeePerGas = BigInt(priorityFee);
    return this;
  }

  validUntilBlock(height) {
    this._validUntilBlock = BigInt(height);
    return this;
  }

  data(data) {
    this._data = parseHexBytes(data);
    return this;
  }

  memo(memo) {
    if (memo === undefined || memo === null) {
      this._memo = null;
    } else {
      const trimmed = String(memo).trim();
      this._memo = trimmed.length > 0 ? trimmed : null;
    }
    return this;
  }

  build(fromAddress) {
    const fromHex = typeof fromAddress === 'string'
      ? (fromAddress.startsWith('0x') ? fromAddress : Address.fromBech32m(fromAddress).address.toHex())
      : fromAddress.toHex();

    return {
      chainId: this._chainId,
      nonce: this._nonce,
      from: fromHex,
      to: this._to,
      value: this._value,
      gasLimit: this._gasLimit,
      maxFeePerGas: this._maxFeePerGas,
      maxPriorityFeePerGas: this._maxPriorityFeePerGas,
      validUntilBlock: this._validUntilBlock,
      data: this._data,
      memo: this._memo,
    };
  }
}
