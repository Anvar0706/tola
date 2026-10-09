/**
 * Pure JavaScript implementation of BLAKE3.
 */

const IV = new Uint32Array([
  0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
]);

const MSG_PERMUTATION = [2, 6, 3, 10, 7, 0, 4, 13, 1, 11, 12, 5, 9, 14, 15, 8];

const CHUNK_START = 1;
const CHUNK_END = 2;
const PARENT = 4;
const ROOT = 8;

function rotr32(w, b) {
  return ((w >>> b) | (w << (32 - b))) >>> 0;
}

function g(v, a, b, c, d, mx, my) {
  v[a] = (v[a] + v[b] + mx) >>> 0;
  v[d] = rotr32(v[d] ^ v[a], 16);
  v[c] = (v[c] + v[d]) >>> 0;
  v[b] = rotr32(v[b] ^ v[c], 12);
  v[a] = (v[a] + v[b] + my) >>> 0;
  v[d] = rotr32(v[d] ^ v[a], 8);
  v[c] = (v[c] + v[d]) >>> 0;
  v[b] = rotr32(v[b] ^ v[c], 7);
}

function round(v, m) {
  g(v, 0, 4, 8, 12, m[0], m[1]);
  g(v, 1, 5, 9, 13, m[2], m[3]);
  g(v, 2, 6, 10, 14, m[4], m[5]);
  g(v, 3, 7, 11, 15, m[6], m[7]);
  g(v, 0, 5, 10, 15, m[8], m[9]);
  g(v, 1, 6, 11, 12, m[10], m[11]);
  g(v, 2, 7, 8, 13, m[12], m[13]);
  g(v, 3, 4, 9, 14, m[14], m[15]);
}

function compress(cv, blockWords, blockLen, counter, flags) {
  const v = new Uint32Array(16);
  v.set(cv, 0);
  v[8] = IV[0];
  v[9] = IV[1];
  v[10] = IV[2];
  v[11] = IV[3];
  v[12] = Number(counter & 0xffffffffn);
  v[13] = Number((counter >> 32n) & 0xffffffffn);
  v[14] = blockLen;
  v[15] = flags;

  const m = new Uint32Array(blockWords);

  for (let r = 0; r < 7; r++) {
    round(v, m);
    const perm = new Uint32Array(16);
    for (let i = 0; i < 16; i++) {
      perm[i] = m[MSG_PERMUTATION[i]];
    }
    m.set(perm);
  }

  const out = new Uint32Array(16);
  for (let i = 0; i < 8; i++) {
    out[i] = (v[i] ^ v[i + 8]) >>> 0;
    out[i + 8] = (v[i + 8] ^ cv[i]) >>> 0;
  }
  return out;
}

class ChunkState {
  constructor(key, chunkCounter, flags) {
    this.cv = new Uint32Array(key);
    this.chunkCounter = chunkCounter;
    this.flags = flags;
    this.buf = new Uint8Array(64);
    this.bufLen = 0;
    this.blocksCompressed = 0;
  }

  len() {
    return this.blocksCompressed * 64 + this.bufLen;
  }

  startFlag() {
    return this.blocksCompressed === 0 ? CHUNK_START : 0;
  }

  update(input) {
    let pos = 0;
    while (pos < input.length) {
      if (this.bufLen === 64) {
        const words = bytesToWords(this.buf);
        const out = compress(this.cv, words, 64, this.chunkCounter, this.flags | this.startFlag());
        this.cv.set(out.subarray(0, 8));
        this.blocksCompressed += 1;
        this.bufLen = 0;
      }
      const take = Math.min(64 - this.bufLen, input.length - pos);
      this.buf.set(input.subarray(pos, pos + take), this.bufLen);
      this.bufLen += take;
      pos += take;
    }
  }

  output(isRoot) {
    const blockWords = bytesToWords(this.buf.subarray(0, this.bufLen));
    const flags =
      this.flags |
      this.startFlag() |
      CHUNK_END |
      (isRoot ? ROOT : 0);
    return compress(this.cv, blockWords, this.bufLen, this.chunkCounter, flags);
  }
}

function bytesToWords(bytes) {
  const words = new Uint32Array(16);
  const fullBytes = new Uint8Array(64);
  fullBytes.set(bytes);
  for (let i = 0; i < 16; i++) {
    words[i] =
      (fullBytes[i * 4] |
        (fullBytes[i * 4 + 1] << 8) |
        (fullBytes[i * 4 + 2] << 16) |
        (fullBytes[i * 4 + 3] << 24)) >>>
      0;
  }
  return words;
}

/**
 * Compute 32-byte BLAKE3 cryptographic hash.
 */
export function blake3(data) {
  const key = new Uint32Array(IV);
  let chunkState = new ChunkState(key, 0n, 0);
  const cvStack = [];

  let pos = 0;
  let chunkCounter = 0n;

  while (pos < data.length) {
    if (chunkState.len() === 1024) {
      let chunkCv = chunkState.output(false).subarray(0, 8);
      let totalChunks = chunkCounter + 1n;
      while ((totalChunks & 1n) === 0n) {
        const leftCv = cvStack.pop();
        const parentBlock = new Uint32Array(16);
        parentBlock.set(leftCv, 0);
        parentBlock.set(chunkCv, 8);
        const parentOut = compress(key, parentBlock, 64, 0n, PARENT);
        chunkCv = parentOut.subarray(0, 8);
        totalChunks >>= 1n;
      }
      cvStack.push(new Uint32Array(chunkCv));
      chunkCounter += 1n;
      chunkState = new ChunkState(key, chunkCounter, 0);
    }

    const take = Math.min(1024 - chunkState.len(), data.length - pos);
    chunkState.update(data.subarray(pos, pos + take));
    pos += take;
  }

  let currentOutput = chunkState.output(cvStack.length === 0);
  let currentCv = currentOutput.subarray(0, 8);

  while (cvStack.length > 0) {
    const isRoot = cvStack.length === 1;
    const leftCv = cvStack.pop();
    const parentBlock = new Uint32Array(16);
    parentBlock.set(leftCv, 0);
    parentBlock.set(currentCv, 8);
    currentOutput = compress(key, parentBlock, 64, 0n, PARENT | (isRoot ? ROOT : 0));
    currentCv = currentOutput.subarray(0, 8);
  }

  const out = new Uint8Array(32);
  for (let i = 0; i < 8; i++) {
    const w = currentOutput[i];
    out[i * 4] = w & 0xff;
    out[i * 4 + 1] = (w >> 8) & 0xff;
    out[i * 4 + 2] = (w >> 16) & 0xff;
    out[i * 4 + 3] = (w >> 24) & 0xff;
  }
  return out;
}
