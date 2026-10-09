/**
 * BIP-350 Bech32m Address Encoding and Decoding for TOLA Network.
 * Conforms to ADR-0006: Versioned Bech32m Checksummed Address Format.
 */

const CHARSET = 'qpzry9x8gf2tvdw0s3jn54khce6mua7l';
const BECH32M_CONST = 0x2bc830a3;
const GEN = [0x3b6a57b2, 0x26508e6d, 0x1ea119fa, 0x3d4233dd, 0x2a1462b3];

function polymod(values) {
  let chk = 1;
  for (const v of values) {
    const b = chk >> 25;
    chk = ((chk & 0x1ffffff) << 5) ^ v;
    for (let i = 0; i < 5; i++) {
      if ((b >> i) & 1) {
        chk ^= GEN[i];
      }
    }
  }
  return chk >>> 0;
}

function hrpExpand(hrp) {
  const ret = [];
  for (let i = 0; i < hrp.length; i++) {
    ret.push(hrp.charCodeAt(i) >> 5);
  }
  ret.push(0);
  for (let i = 0; i < hrp.length; i++) {
    ret.push(hrp.charCodeAt(i) & 31);
  }
  return ret;
}

function convertBits(data, fromBits, toBits, pad) {
  let acc = 0;
  let bits = 0;
  const ret = [];
  const maxv = (1 << toBits) - 1;
  const maxAcc = (1 << (fromBits + toBits - 1)) - 1;

  for (let i = 0; i < data.length; i++) {
    const value = data[i];
    if (value >> fromBits !== 0) {
      throw new Error('Invalid bit conversion data');
    }
    acc = ((acc << fromBits) | value) & maxAcc;
    bits += fromBits;
    while (bits >= toBits) {
      bits -= toBits;
      ret.push((acc >> bits) & maxv);
    }
  }

  if (pad) {
    if (bits > 0) {
      ret.push((acc << (toBits - bits)) & maxv);
    }
  } else if (bits >= fromBits || ((acc << (toBits - bits)) & maxv) !== 0) {
    throw new Error('Invalid padding in bit conversion');
  }

  return ret;
}

/**
 * Encode raw 20-byte address into Bech32m string with specified HRP.
 */
export function encodeBech32m(hrp, data20) {
  if (data20.length !== 20) {
    throw new Error(`Expected 20-byte address, got ${data20.length} bytes`);
  }
  const lowHrp = hrp.toLowerCase();
  const data5bit = convertBits(data20, 8, 5, true);
  const values = hrpExpand(lowHrp).concat(data5bit).concat([0, 0, 0, 0, 0, 0]);
  const poly = (polymod(values) ^ BECH32M_CONST) >>> 0;

  const checksum = [];
  for (let i = 0; i < 6; i++) {
    checksum.push((poly >> (5 * (5 - i))) & 31);
  }

  let result = lowHrp + '1';
  for (const d of data5bit) {
    result += CHARSET[d];
  }
  for (const c of checksum) {
    result += CHARSET[c];
  }
  return result;
}

/**
 * Decode Bech32m string into HRP and 20-byte address payload.
 */
export function decodeBech32m(s) {
  if (s.length < 8 || s.length > 90) {
    throw new Error(`Invalid Bech32m string length: ${s.length}`);
  }

  const hasLower = /[a-z]/.test(s);
  const hasUpper = /[A-Z]/.test(s);
  if (hasLower && hasUpper) {
    throw new Error('Mixed-case Bech32m strings are invalid');
  }

  const clean = s.toLowerCase();
  const sepIdx = clean.lastIndexOf('1');
  if (sepIdx < 1 || sepIdx + 7 > clean.length) {
    throw new Error("Missing or invalid '1' separator in Bech32m string");
  }

  const hrp = clean.substring(0, sepIdx);
  const dataChars = clean.substring(sepIdx + 1);

  const values = [];
  for (let i = 0; i < dataChars.length; i++) {
    const c = dataChars[i];
    const idx = CHARSET.indexOf(c);
    if (idx === -1) {
      throw new Error(`Invalid character in Bech32m string: '${c}'`);
    }
    values.push(idx);
  }

  const verifyValues = hrpExpand(hrp).concat(values);
  if ((polymod(verifyValues) ^ BECH32M_CONST) !== 0) {
    throw new Error('Invalid Bech32m checksum');
  }

  const data5bit = values.slice(0, values.length - 6);
  const data8bit = convertBits(new Uint8Array(data5bit), 5, 8, false);

  if (data8bit.length !== 20) {
    throw new Error(`Expected decoded 20-byte address, got ${data8bit.length} bytes`);
  }

  return { hrp, data: new Uint8Array(data8bit) };
}
