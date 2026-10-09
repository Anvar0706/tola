"""
TOLA Network Sharding & Cross-Shard State Proof Utilities.
Strictly conforms to ADR-0014.
"""

from typing import Union, List
from .address import Address
from .crypto import blake3

DEFAULT_NUM_SHARDS: int = 4
DOMAIN_RECEIPT_HASH: bytes = b"TOLA-RECEIPT-HASH-v1.0.0--"
DOMAIN_RECEIPT_NULLIFIER: bytes = b"TOLA-RECEIPT-NULLIFIER-v1-"


def _parse_address(addr: Union[str, Address, bytes]) -> Address:
    if isinstance(addr, Address):
        return addr
    if isinstance(addr, str):
        if addr.startswith(("tola1", "ttola1", "dtola1")):
            return Address.from_bech32m(addr)[0]
        return Address.from_hex(addr)
    if isinstance(addr, (bytes, bytearray)):
        return Address(bytes(addr))
    raise TypeError(f"Unsupported address type: {type(addr)}")


def compute_shard_id(addr: Union[str, Address, bytes], num_shards: int = DEFAULT_NUM_SHARDS) -> int:
    """
    Computes deterministic shard ID for an address.
    Formula: ((addr.raw[0] << 8) | addr.raw[1]) % num_shards.
    """
    if num_shards <= 0:
        return 0
    parsed = _parse_address(addr)
    high = (parsed.raw[0] << 8) | parsed.raw[1]
    return high % num_shards


def compute_receipt_nullifier(
    source_shard: int,
    target_shard: int,
    receipt_nonce: int,
    sender: Union[str, Address, bytes],
) -> bytes:
    """
    Computes unique 32-byte receipt nullifier for cross-shard replay immunity.
    """
    parsed = _parse_address(sender)
    buf = bytearray()
    buf.extend(DOMAIN_RECEIPT_NULLIFIER)
    buf.extend(source_shard.to_bytes(4, "little"))
    buf.extend(target_shard.to_bytes(4, "little"))
    buf.extend(receipt_nonce.to_bytes(8, "little"))
    buf.extend(parsed.raw)
    return blake3(bytes(buf))


def verify_merkle_inclusion_proof(
    root: bytes,
    leaf: bytes,
    proof: List[bytes],
    index: int,
) -> bool:
    """
    Verifies binary Merkle inclusion proof against root using BLAKE3.
    """
    current = leaf
    current_idx = index

    for sibling in proof:
        if current_idx % 2 == 0:
            concat = current + sibling
        else:
            concat = sibling + current
        current = blake3(concat)
        current_idx //= 2

    return current == root
