"""
Cryptographic primitives for TOLA Network Python SDK.
Pure Python implementations of Keccak-256, BLAKE3, Secp256k1 (RFC 6979), and Domain Separation.
"""

import hmac
import hashlib
from typing import Tuple, Optional


# ============================================================================
# 1. Keccak-256 (NIST sponge with 0x01 .. 0x80 padding)
# ============================================================================

RC = [
    0x0000000000000001, 0x0000000000008082, 0x800000000000808a, 0x8000000080008000,
    0x000000000000808b, 0x0000000080000001, 0x8000000080008081, 0x8000000000008009,
    0x000000000000008a, 0x0000000000000088, 0x0000000080008009, 0x000000008000000a,
    0x000000008000808b, 0x800000000000008b, 0x8000000000008089, 0x8000000000008003,
    0x8000000000008002, 0x8000000000000080, 0x000000000000800a, 0x800000008000000a,
    0x8000000080008081, 0x8000000000008080, 0x0000000080000001, 0x8000000080008008,
]

R = [
    [0, 36, 3, 41, 18],
    [1, 44, 10, 45, 2],
    [62, 6, 43, 15, 61],
    [28, 55, 25, 21, 56],
    [27, 20, 39, 8, 14],
]


def _rotl64(x: int, n: int) -> int:
    shift = n % 64
    return ((x << shift) | (x >> (64 - shift))) & 0xFFFFFFFFFFFFFFFF


def _keccak_f(A: list) -> None:
    for round_idx in range(24):
        # Theta
        C = [A[x][0] ^ A[x][1] ^ A[x][2] ^ A[x][3] ^ A[x][4] for x in range(5)]
        D = [C[(x + 4) % 5] ^ _rotl64(C[(x + 1) % 5], 1) for x in range(5)]
        for x in range(5):
            for y in range(5):
                A[x][y] ^= D[x]

        # Rho and Pi
        B = [[0] * 5 for _ in range(5)]
        for x in range(5):
            for y in range(5):
                B[y][(2 * x + 3 * y) % 5] = _rotl64(A[x][y], R[x][y])

        # Chi
        for x in range(5):
            for y in range(5):
                A[x][y] = B[x][y] ^ ((~B[(x + 1) % 5][y]) & B[(x + 2) % 5][y])

        # Iota
        A[0][0] ^= RC[round_idx]


def keccak256(data: bytes) -> bytes:
    """Compute 32-byte Keccak-256 digest."""
    rate = 136
    state = [[0] * 5 for _ in range(5)]
    pad_len = rate - (len(data) % rate)
    padded = bytearray(data)
    padded.append(0x01)
    padded.extend(b"\x00" * (pad_len - 1))
    padded[-1] ^= 0x80

    for i in range(0, len(padded), rate):
        block = padded[i : i + rate]
        for j in range(17):
            w = int.from_bytes(block[j * 8 : (j + 1) * 8], "little")
            x = j % 5
            y = j // 5
            state[x][y] ^= w
        _keccak_f(state)

    out = bytearray()
    for j in range(4):
        x = j % 5
        y = j // 5
        out.extend(state[x][y].to_bytes(8, "little"))
    return bytes(out)


# ============================================================================
# 2. BLAKE3 Cryptographic Hash Function
# ============================================================================

IV = [
    0x6A09E667, 0xBB67AE85, 0x3C6EF372, 0xA54FF53A, 0x510E527F, 0x9B05688C, 0x1F83D9AB, 0x5BE0CD19,
]

MSG_PERMUTATION = [2, 6, 3, 10, 7, 0, 4, 13, 1, 11, 12, 5, 9, 14, 15, 8]

CHUNK_START = 1
CHUNK_END = 2
PARENT = 4
ROOT = 8


def _rotr32(w: int, b: int) -> int:
    return ((w >> b) | (w << (32 - b))) & 0xFFFFFFFF


def _g(v: list, a: int, b: int, c: int, d: int, mx: int, my: int) -> None:
    v[a] = (v[a] + v[b] + mx) & 0xFFFFFFFF
    v[d] = _rotr32(v[d] ^ v[a], 16)
    v[c] = (v[c] + v[d]) & 0xFFFFFFFF
    v[b] = _rotr32(v[b] ^ v[c], 12)
    v[a] = (v[a] + v[b] + my) & 0xFFFFFFFF
    v[d] = _rotr32(v[d] ^ v[a], 8)
    v[c] = (v[c] + v[d]) & 0xFFFFFFFF
    v[b] = _rotr32(v[b] ^ v[c], 7)


def _blake3_round(v: list, m: list) -> None:
    _g(v, 0, 4, 8, 12, m[0], m[1])
    _g(v, 1, 5, 9, 13, m[2], m[3])
    _g(v, 2, 6, 10, 14, m[4], m[5])
    _g(v, 3, 7, 11, 15, m[6], m[7])
    _g(v, 0, 5, 10, 15, m[8], m[9])
    _g(v, 1, 6, 11, 12, m[10], m[11])
    _g(v, 2, 7, 8, 13, m[12], m[13])
    _g(v, 3, 4, 9, 14, m[14], m[15])


def _compress(cv: list, block_words: list, block_len: int, counter: int, flags: int) -> list:
    v = list(cv) + [
        IV[0], IV[1], IV[2], IV[3],
        counter & 0xFFFFFFFF,
        (counter >> 32) & 0xFFFFFFFF,
        block_len,
        flags,
    ]
    m = list(block_words)
    for _ in range(7):
        _blake3_round(v, m)
        m = [m[MSG_PERMUTATION[i]] for i in range(16)]

    out = [0] * 16
    for i in range(8):
        out[i] = v[i] ^ v[i + 8]
        out[i + 8] = v[i + 8] ^ cv[i]
    return out


def _bytes_to_words(b: bytes) -> list:
    full = bytearray(64)
    full[: len(b)] = b
    words = []
    for i in range(16):
        w = int.from_bytes(full[i * 4 : (i + 1) * 4], "little")
        words.append(w)
    return words


class _ChunkState:
    def __init__(self, key: list, chunk_counter: int, flags: int):
        self.cv = list(key)
        self.chunk_counter = chunk_counter
        self.flags = flags
        self.buf = bytearray(64)
        self.buf_len = 0
        self.blocks_compressed = 0

    def len(self) -> int:
        return self.blocks_compressed * 64 + self.buf_len

    def start_flag(self) -> int:
        return CHUNK_START if self.blocks_compressed == 0 else 0

    def update(self, data: bytes) -> None:
        pos = 0
        while pos < len(data):
            if self.buf_len == 64:
                words = _bytes_to_words(self.buf)
                out = _compress(self.cv, words, 64, self.chunk_counter, self.flags | self.start_flag())
                self.cv = out[:8]
                self.blocks_compressed += 1
                self.buf_len = 0
            take = min(64 - self.buf_len, len(data) - pos)
            self.buf[self.buf_len : self.buf_len + take] = data[pos : pos + take]
            self.buf_len += take
            pos += take

    def output(self, is_root: bool) -> list:
        words = _bytes_to_words(self.buf[: self.buf_len])
        flags = self.flags | self.start_flag() | CHUNK_END | (ROOT if is_root else 0)
        return _compress(self.cv, words, self.buf_len, self.chunk_counter, flags)


def blake3(data: bytes) -> bytes:
    """Compute 32-byte BLAKE3 cryptographic hash."""
    key = list(IV)
    chunk_state = _ChunkState(key, 0, 0)
    cv_stack = []
    pos = 0
    chunk_counter = 0

    while pos < len(data):
        if chunk_state.len() == 1024:
            chunk_cv = chunk_state.output(False)[:8]
            total_chunks = chunk_counter + 1
            while (total_chunks & 1) == 0:
                left_cv = cv_stack.pop()
                parent_block = left_cv + chunk_cv
                chunk_cv = _compress(key, parent_block, 64, 0, PARENT)[:8]
                total_chunks >>= 1
            cv_stack.append(chunk_cv)
            chunk_counter += 1
            chunk_state = _ChunkState(key, chunk_counter, 0)

        take = min(1024 - chunk_state.len(), len(data) - pos)
        chunk_state.update(data[pos : pos + take])
        pos += take

    current_output = chunk_state.output(len(cv_stack) == 0)
    current_cv = current_output[:8]

    while cv_stack:
        is_root = len(cv_stack) == 1
        left_cv = cv_stack.pop()
        parent_block = left_cv + current_cv
        current_output = _compress(key, parent_block, 64, 0, PARENT | (ROOT if is_root else 0))
        current_cv = current_output[:8]

    out = bytearray()
    for w in current_output[:8]:
        out.extend(w.to_bytes(4, "little"))
    return bytes(out)


# ============================================================================
# 3. Domain Separation
# ============================================================================

DOMAIN_TRANSACTION_PAYLOAD = b"TOLA-TX-ENVELOPE-v1.0.0-"
DOMAIN_CONSENSUS_PREVOTE = b"TOLA-CONSENSUS-PREVOTE-1"
DOMAIN_CONSENSUS_PRECOMMIT = b"TOLA-CONSENSUS-PRECOMMIT"


def domain_separated_hash(domain_tag: bytes, chain_id: int, epoch: int, payload_hash: bytes) -> bytes:
    """Format: BLAKE3( DomainTag (24B) || ChainID (8B LE) || Epoch (8B LE) || PayloadHash (32B) )"""
    if len(domain_tag) != 24:
        raise ValueError(f"Domain tag must be exactly 24 bytes, got {len(domain_tag)}")
    if len(payload_hash) != 32:
        raise ValueError(f"Payload hash must be exactly 32 bytes, got {len(payload_hash)}")

    pre_image = (
        domain_tag
        + chain_id.to_bytes(8, "little")
        + epoch.to_bytes(8, "little")
        + payload_hash
    )
    return blake3(pre_image)


# ============================================================================
# 4. Secp256k1 Curve Arithmetic & RFC 6979 ECDSA
# ============================================================================

P = 0xFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFEFFFFFC2F
N = 0xFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFEBAAEDCE6AF48A03BBFD25E8CD0364141
HALF_N = N >> 1
Gx = 0x79BE667EF9DCBBAC55A06295CE870B07029BFCDB2DCE28D959F2815B16F81798
Gy = 0x483ADA7726A3C4655DA4FBFC0E1108A8FD17B448A68554199C47D08FFB10D4B8
G = (Gx, Gy)



def _mod_inverse(k: int, m: int = P) -> int:
    return pow(k, -1, m)


def point_add(p1: Optional[Tuple[int, int]], p2: Optional[Tuple[int, int]]) -> Optional[Tuple[int, int]]:
    if not p1:
        return p2
    if not p2:
        return p1
    x1, y1 = p1
    x2, y2 = p2
    if x1 == x2:
        if y1 != y2:
            return None
        slope = (3 * x1 * x1 * _mod_inverse(2 * y1, P)) % P
        rx = (slope * slope - 2 * x1) % P
        ry = (slope * (x1 - rx) - y1) % P
        return (rx, ry)
    slope = ((y2 - y1) * _mod_inverse(x2 - x1, P)) % P
    rx = (slope * slope - x1 - x2) % P
    ry = (slope * (x1 - rx) - y1) % P
    return (rx, ry)


def point_multiply(k: int, p: Tuple[int, int] = (Gx, Gy)) -> Tuple[int, int]:
    scalar = k % N
    if scalar == 0:
        raise ValueError("Invalid scalar 0")
    current = p
    result = None
    while scalar > 0:
        if scalar & 1:
            result = point_add(result, current)
        current = point_add(current, current)
        scalar >>= 1
    if not result:
        raise ValueError("Point at infinity")
    return result


def compress_public_key(point: Tuple[int, int]) -> bytes:
    x, y = point
    prefix = b"\x02" if (y % 2 == 0) else b"\x03"
    return prefix + x.to_bytes(32, "big")


def uncompressed_public_key(point: Tuple[int, int]) -> bytes:
    x, y = point
    return b"\x04" + x.to_bytes(32, "big") + y.to_bytes(32, "big")


def public_key_to_address(point: Tuple[int, int]) -> bytes:
    uncompressed = uncompressed_public_key(point)
    h = keccak256(uncompressed[1:65])
    return h[12:32]


def _rfc6979_nonce(priv: int, msg_hash: bytes) -> int:
    priv_bytes = priv.to_bytes(32, "big")
    v = b"\x01" * 32
    k = b"\x00" * 32

    k = hmac.new(k, v + b"\x00" + priv_bytes + msg_hash, hashlib.sha256).digest()
    v = hmac.new(k, v, hashlib.sha256).digest()
    k = hmac.new(k, v + b"\x01" + priv_bytes + msg_hash, hashlib.sha256).digest()
    v = hmac.new(k, v, hashlib.sha256).digest()

    while True:
        v = hmac.new(k, v, hashlib.sha256).digest()
        candidate = int.from_bytes(v, "big")
        if 1 <= candidate < N:
            return candidate
        k = hmac.new(k, v + b"\x00", hashlib.sha256).digest()
        v = hmac.new(k, v, hashlib.sha256).digest()


def sign_digest(priv: int, digest32: bytes) -> Tuple[bytes, bytes, int]:
    """Sign 32-byte digest returning canonical (r, s, v) with Low-S enforcement."""
    if not (1 <= priv < N):
        raise ValueError("Private key outside valid range [1, N-1]")

    e = int.from_bytes(digest32, "big") % N
    k = _rfc6979_nonce(priv, digest32)
    rx, ry = point_multiply(k)
    r = rx % N
    if r == 0:
        raise ValueError("Calculated r = 0")

    s = (_mod_inverse(k, N) * (e + r * priv)) % N
    if s == 0:
        raise ValueError("Calculated s = 0")

    v = ry & 1
    if s > HALF_N:
        s = N - s
        v ^= 1

    return r.to_bytes(32, "big"), s.to_bytes(32, "big"), v


def recover_public_key(digest32: bytes, r_bytes: bytes, s_bytes: bytes, v: int) -> Tuple[int, int]:
    """Recover Secp256k1 public key point (x, y) from pre-hashed digest and signature."""
    r = int.from_bytes(r_bytes, "big")
    s = int.from_bytes(s_bytes, "big")
    e = int.from_bytes(digest32, "big") % N

    if not (1 <= r < N and 1 <= s < N):
        raise ValueError("Invalid signature r or s value")

    x = r
    alpha = (pow(x, 3, P) + 7) % P
    beta = pow(alpha, (P + 1) // 4, P)
    if pow(beta, 2, P) != alpha:
        raise ValueError("Quadratic residue does not exist for R.x")

    y = beta
    if (y & 1) != (v & 1):
        y = P - y
    R = (x, y)

    r_inv = _mod_inverse(r, N)
    sR = point_multiply(s, R)
    eG = point_multiply(e, G)
    neg_eG = (eG[0], P - eG[1])
    diff = point_add(sR, neg_eG)
    if not diff:
        raise ValueError("Point at infinity")

    return point_multiply(r_inv, diff)


def recover_address(digest32: bytes, r_bytes: bytes, s_bytes: bytes, v: int) -> bytes:
    """Recover 20-byte address from pre-hashed digest and signature."""
    pub_point = recover_public_key(digest32, r_bytes, s_bytes, v)
    return public_key_to_address(pub_point)

