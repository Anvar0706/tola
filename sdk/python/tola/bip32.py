"""
BIP-32 Hierarchical Deterministic (HD) Key Derivation for TOLA Network.
Secp256k1 child key derivation strictly conforming to TOLA Wallet Derivation Standard v1.0.
"""

import hmac
import hashlib
from dataclasses import dataclass
from typing import Tuple
from .crypto import N, point_multiply, compress_public_key


CANONICAL_DERIVATION_PATH = "m/44'/9999'/0'/0/0"
DEFAULT_MAINNET_PATH = CANONICAL_DERIVATION_PATH
DEFAULT_TESTNET_PATH = CANONICAL_DERIVATION_PATH
DEFAULT_DEVNET_PATH = CANONICAL_DERIVATION_PATH
LEGACY_TESTNET_PROVISIONAL_PATH = "m/44'/102'/0'/0/0"


@dataclass(frozen=True)
class ExtendedKey:
    """BIP-32 Extended Key containing 32-byte private key scalar and 32-byte chain code."""
    key: bytes
    chain_code: bytes

    def __post_init__(self):
        if len(self.key) != 32:
            raise ValueError(f"Private key must be 32 bytes, got {len(self.key)}")
        if len(self.chain_code) != 32:
            raise ValueError(f"Chain code must be 32 bytes, got {len(self.chain_code)}")


def derive_master_node(seed: bytes) -> ExtendedKey:
    """Derive BIP-32 master node from 64-byte binary seed."""
    if len(seed) < 16 or len(seed) > 64:
        raise ValueError(f"Seed length must be between 16 and 64 bytes, got {len(seed)}")

    i_digest = hmac.new(b"Bitcoin seed", seed, hashlib.sha512).digest()
    key_bytes = i_digest[:32]
    chain_code = i_digest[32:]

    scalar = int.from_bytes(key_bytes, "big")
    if scalar == 0 or scalar >= N:
        raise ValueError("Derived master key scalar outside valid range [1, N-1]")

    return ExtendedKey(key=key_bytes, chain_code=chain_code)


def derive_child_node(parent: ExtendedKey, index: int) -> ExtendedKey:
    """
    Derive child extended key from parent extended key at index.
    Indices >= 0x80000000 are hardened.
    """
    if not (0 <= index <= 0xFFFFFFFF):
        raise ValueError(f"Child index out of bounds [0, 2^32-1]: {index}")

    is_hardened = index >= 0x80000000

    if is_hardened:
        data = b"\x00" + parent.key + index.to_bytes(4, "big")
    else:
        parent_scalar = int.from_bytes(parent.key, "big")
        pt = point_multiply(parent_scalar)
        pub = compress_public_key(pt)
        data = pub + index.to_bytes(4, "big")

    i_digest = hmac.new(parent.chain_code, data, hashlib.sha512).digest()
    il = int.from_bytes(i_digest[:32], "big")
    child_chain_code = i_digest[32:]

    if il >= N:
        raise ValueError("Child scalar addition overflow >= N")

    child_scalar = (il + int.from_bytes(parent.key, "big")) % N
    if child_scalar == 0:
        raise ValueError("Child scalar is 0")

    return ExtendedKey(key=child_scalar.to_bytes(32, "big"), chain_code=child_chain_code)


def derive_path(seed: bytes, path: str = CANONICAL_DERIVATION_PATH) -> ExtendedKey:
    """
    Derive an extended private key from a binary seed along a BIP-32/BIP-44 path.
    Example path: m/44'/9999'/0'/0/0
    """
    if not path or not path.startswith(("m", "M")):
        raise ValueError(f"Invalid derivation path: {path}. Path must start with 'm'")

    current = derive_master_node(seed)
    segments = path.split("/")[1:]

    for seg in segments:
        clean = seg.strip()
        if not clean:
            continue
        is_hardened = clean.endswith(("'", "h", "H"))
        num_str = clean[:-1] if is_hardened else clean
        try:
            val = int(num_str)
        except ValueError:
            raise ValueError(f"Invalid path segment: '{seg}' in path '{path}'")

        if is_hardened:
            index = val + 0x80000000
        else:
            index = val

        current = derive_child_node(current, index)

    return current
