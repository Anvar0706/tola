/**
 * Pure JavaScript implementation of Keccak-256 (NIST sponge with Keccak padding 0x01 ... 0x80).
 */

const RC = [
  0x0000000000000001n, 0x0000000000008082n, 0x800000000000808an, 0x8000000080008000n,
  0x000000000000808bn, 0x0000000080000001n, 0x8000000080008081n, 0x8000000000008009n,
  0x000000000000008an, 0x0000000000000088n, 0x0000000080008009n, 0x000000008000000an,
  0x000000008000808bn, 0x800000000000008bn, 0x8000000000008089n, 0x8000000000008003n,
  0x8000000000008002n, 0x8000000000000080n, 0x000000000000800an, 0x800000008000000an,
  0x8000000080008081n, 0x8000000000008080n, 0x0000000080000001n, 0x8000000080008008n,
];

const R = [
  [0, 36, 3, 41, 18],
  [1, 44, 10, 45, 2],
  [62, 6, 43, 15, 61],
  [28, 55, 25, 21, 56],
  [27, 20, 39, 8, 14],
];

function rotl(x, n) {
  const shift = BigInt(n % 64);
  return ((x << shift) | (x >> (64n - shift))) & 0xffffffffffffffffn;
}

function keccakF(A) {
  const C = new BigUint64Array(5);
  const D = new BigUint64Array(5);
  const B = [
    new BigUint64Array(5),
    new BigUint64Array(5),
    new BigUint64Array(5),
    new BigUint64Array(5),
    new BigUint64Array(5),
  ];

  for (let round = 0; round < 24; round++) {
    // Theta
    for (let x = 0; x < 5; x++) {
      C[x] = A[x][0] ^ A[x][1] ^ A[x][2] ^ A[x][3] ^ A[x][4];
    }
    for (let x = 0; x < 5; x++) {
      D[x] = C[(x + 4) % 5] ^ rotl(C[(x + 1) % 5], 1);
    }
    for (let x = 0; x < 5; x++) {
      for (let y = 0; y < 5; y++) {
        A[x][y] ^= D[x];
      }
    }

    // Rho and Pi
    for (let x = 0; x < 5; x++) {
      for (let y = 0; y < 5; y++) {
        B[y][(2 * x + 3 * y) % 5] = rotl(A[x][y], R[x][y]);
      }
    }

    // Chi
    for (let x = 0; x < 5; x++) {
      for (let y = 0; y < 5; y++) {
        A[x][y] = B[x][y] ^ (~B[(x + 1) % 5][y] & B[(x + 2) % 5][y]);
      }
    }

    // Iota
    A[0][0] ^= RC[round];
  }
}

/**
 * Compute 32-byte Keccak-256 digest of input bytes.
 */
export function keccak256(data) {
  const rate = 136;
  const A = [
    new BigUint64Array(5),
    new BigUint64Array(5),
    new BigUint64Array(5),
    new BigUint64Array(5),
    new BigUint64Array(5),
  ];

  const padLen = rate - (data.length % rate);
  const padded = new Uint8Array(data.length + padLen);
  padded.set(data);
  padded[data.length] = 0x01;
  padded[padded.length - 1] ^= 0x80;

  for (let i = 0; i < padded.length; i += rate) {
    const block = padded.subarray(i, i + rate);
    for (let j = 0; j < 17; j++) {
      let w = 0n;
      for (let b = 0; b < 8; b++) {
        w |= BigInt(block[j * 8 + b]) << BigInt(b * 8);
      }
      const x = j % 5;
      const y = Math.floor(j / 5);
      A[x][y] ^= w;
    }
    keccakF(A);
  }

  const out = new Uint8Array(32);
  for (let j = 0; j < 4; j++) {
    const x = j % 5;
    const y = Math.floor(j / 5);
    const w = A[x][y];
    for (let b = 0; b < 8; b++) {
      out[j * 8 + b] = Number((w >> BigInt(b * 8)) & 0xffn);
    }
  }

  return out;
}
