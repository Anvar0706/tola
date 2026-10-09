/**
 * Address representation and network encoding for TOLA Network.
 */

import { encodeBech32m, decodeBech32m } from './crypto/bech32m.js';
import { NETWORKS } from './types.js';

export class Address {
  constructor(bytes) {
    if (bytes.length !== 20) {
      throw new Error(`Address must be exactly 20 bytes, got ${bytes.length}`);
    }
    this.bytes = new Uint8Array(bytes);
  }

  static ZERO() {
    return new Address(new Uint8Array(20));
  }

  static fromBytes(bytes) {
    return new Address(bytes);
  }

  static fromHex(hexStr) {
    const clean = hexStr.startsWith('0x') || hexStr.startsWith('0X') ? hexStr.slice(2) : hexStr;
    if (clean.length !== 40) {
      throw new Error(`Invalid address hex length: ${clean.length} (expected 40 chars)`);
    }
    const bytes = new Uint8Array(20);
    for (let i = 0; i < 20; i++) {
      const byte = parseInt(clean.substring(i * 2, i * 2 + 2), 16);
      if (isNaN(byte)) {
        throw new Error(`Invalid hex character in address at index ${i * 2}`);
      }
      bytes[i] = byte;
    }
    return new Address(bytes);
  }

  static fromBech32m(s) {
    const { hrp, data } = decodeBech32m(s);
    return { address: new Address(data), hrp };
  }

  toHex() {
    let out = '0x';
    for (let i = 0; i < this.bytes.length; i++) {
      out += this.bytes[i].toString(16).padStart(2, '0');
    }
    return out;
  }

  toBech32m(networkOrHrp = 'mainnet') {
    const hrp = NETWORKS[networkOrHrp]?.hrp || networkOrHrp;
    return encodeBech32m(hrp, this.bytes);
  }

  equals(other) {
    if (!other || !other.bytes || this.bytes.length !== other.bytes.length) return false;
    for (let i = 0; i < this.bytes.length; i++) {
      if (this.bytes[i] !== other.bytes[i]) return false;
    }
    return true;
  }

  toString() {
    return this.toHex();
  }
}
