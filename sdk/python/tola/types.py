"""
Core type definitions for TOLA Network Python SDK.
"""

from dataclasses import dataclass, field
from enum import Enum
from typing import Optional, Union, List, Dict, Any


class Network(str, Enum):
    MAINNET = "mainnet"
    TESTNET = "testnet"
    DEVNET = "devnet"

    @property
    def hrp(self) -> str:
        if self == Network.MAINNET:
            return "tola"
        elif self == Network.TESTNET:
            return "ttola"
        elif self == Network.DEVNET:
            return "dtola"
        return "tola"

    @property
    def chain_id(self) -> int:
        if self == Network.MAINNET:
            return 9999
        elif self == Network.TESTNET:
            return 102
        elif self == Network.DEVNET:
            return 1337
        return 9999


@dataclass
class Secp256k1Signature:
    r: bytes  # 32 bytes
    s: bytes  # 32 bytes
    v: int    # 0 or 1

    def to_bytes(self) -> bytes:
        return b"\x02" + self.r + self.s + bytes([self.v])


MAX_MEMO_BYTES: int = 256
"""Maximum allowed UTF-8 byte length for transaction memo field."""


@dataclass
class UnsignedTransaction:
    chain_id: int
    nonce: int
    from_address: str  # 20-byte hex address
    to_address: Optional[str]  # 20-byte hex address or None
    value: int
    gas_limit: int = 21000
    max_fee_per_gas: int = 1_000_000_000
    max_priority_fee_per_gas: int = 1_000_000_000
    valid_until_block: int = 1_000_000
    data: bytes = field(default_factory=bytes)
    memo: Optional[str] = None
    """Optional UTF-8 memo attached to this transaction. Max 256 bytes."""


@dataclass
class Transaction(UnsignedTransaction):
    signature: Optional[Secp256k1Signature] = None



@dataclass
class TransactionReceipt:
    transaction_hash: str
    transaction_index: int
    block_hash: str
    block_number: int
    from_address: str
    to_address: Optional[str]
    gas_used: int
    status: str
    logs: List[Any] = field(default_factory=list)


@dataclass
class NodeHealth:
    status: str
    chain_id: int
    block_height: int
    finalized_height: int
    current_round: int
    peer_count: int
    state_root: str
    mempool_size: int
