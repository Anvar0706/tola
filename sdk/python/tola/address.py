"""
BIP-350 Bech32m Address Encoding and Address type for TOLA Network.
"""

from typing import Tuple, Union
from .types import Network


CHARSET = "qpzry9x8gf2tvdw0s3jn54khce6mua7l"
BECH32M_CONST = 0x2BC830A3
GEN = [0x3B6A57B2, 0x26508E6D, 0x1EA119FA, 0x3D4233DD, 0x2A1462B3]


def _polymod(values: list) -> int:
    chk = 1
    for v in values:
        b = chk >> 25
        chk = ((chk & 0x1FFFFFF) << 5) ^ v
        for i in range(5):
            if (b >> i) & 1:
                chk ^= GEN[i]
    return chk & 0xFFFFFFFF


def _hrp_expand(hrp: str) -> list:
    ret = [ord(x) >> 5 for x in hrp] + [0] + [ord(x) & 31 for x in hrp]
    return ret


def _convert_bits(data: bytes, from_bits: int, to_bits: int, pad: bool) -> list:
    acc = 0
    bits = 0
    ret = []
    maxv = (1 << to_bits) - 1
    max_acc = (1 << (from_bits + to_bits - 1)) - 1

    for value in data:
        if value >> from_bits != 0:
            raise ValueError("Invalid bit conversion data")
        acc = ((acc << from_bits) | value) & max_acc
        bits += from_bits
        while bits >= to_bits:
            bits -= to_bits
            ret.append((acc >> bits) & maxv)

    if pad:
        if bits > 0:
            ret.append((acc << (to_bits - bits)) & maxv)
    elif bits >= from_bits or ((acc << (to_bits - bits)) & maxv) != 0:
        raise ValueError("Invalid padding in bit conversion")

    return ret


def encode_bech32m(hrp: str, data20: bytes) -> str:
    """Encode 20-byte address into Bech32m string."""
    if len(data20) != 20:
        raise ValueError(f"Expected 20-byte address, got {len(data20)} bytes")
    low_hrp = hrp.lower()
    data5bit = _convert_bits(data20, 8, 5, True)
    values = _hrp_expand(low_hrp) + data5bit + [0] * 6
    poly = _polymod(values) ^ BECH32M_CONST

    checksum = []
    for i in range(6):
        checksum.append((poly >> (5 * (5 - i))) & 31)

    result = low_hrp + "1"
    for d in data5bit:
        result += CHARSET[d]
    for c in checksum:
        result += CHARSET[c]
    return result


def decode_bech32m(s: str) -> Tuple[str, bytes]:
    """Decode Bech32m string into HRP and 20-byte address."""
    if len(s) < 8 or len(s) > 90:
        raise ValueError(f"Invalid Bech32m string length: {len(s)}")

    has_lower = any(c.islower() for c in s)
    has_upper = any(c.isupper() for c in s)
    if has_lower and has_upper:
        raise ValueError("Mixed-case Bech32m strings are invalid")

    clean = s.lower()
    sep_idx = clean.rfind("1")
    if sep_idx < 1 or sep_idx + 7 > len(clean):
        raise ValueError("Missing or invalid '1' separator in Bech32m string")

    hrp = clean[:sep_idx]
    data_chars = clean[sep_idx + 1 :]

    values = []
    for c in data_chars:
        idx = CHARSET.find(c)
        if idx == -1:
            raise ValueError(f"Invalid character in Bech32m string: '{c}'")
        values.append(idx)

    verify_values = _hrp_expand(hrp) + values
    if (_polymod(verify_values) ^ BECH32M_CONST) != 0:
        raise ValueError("Invalid Bech32m checksum")

    data5bit = values[:-6]
    data8bit = bytes(_convert_bits(bytes(data5bit), 5, 8, False))
    if len(data8bit) != 20:
        raise ValueError(f"Expected decoded 20-byte address, got {len(data8bit)} bytes")

    return hrp, data8bit


class Address:
    """Canonical 20-byte address (EVM and TOLA compatible)."""

    def __init__(self, raw: bytes):
        if len(raw) != 20:
            raise ValueError(f"Address must be exactly 20 bytes, got {len(raw)}")
        self.raw = bytes(raw)

    @property
    def bytes(self) -> bytes:
        return self.raw


    @classmethod
    def zero(cls) -> "Address":
        return cls(bytes(20))

    @classmethod
    def from_hex(cls, hex_str: str) -> "Address":
        clean = hex_str[2:] if hex_str.startswith(("0x", "0X")) else hex_str
        if len(clean) != 40:
            raise ValueError(f"Invalid address hex length: {len(clean)} (expected 40)")
        return cls(bytes.fromhex(clean))

    @classmethod
    def from_bech32m(cls, s: str) -> Tuple["Address", str]:
        hrp, data = decode_bech32m(s)
        return cls(data), hrp

    def to_hex(self) -> str:
        return "0x" + self.raw.hex()

    def to_bech32m(self, network: Union[Network, str] = Network.MAINNET) -> str:
        if isinstance(network, Network):
            hrp = network.hrp
        elif isinstance(network, str):
            net_lower = network.lower()
            if net_lower == "mainnet":
                hrp = "tola"
            elif net_lower == "testnet":
                hrp = "ttola"
            elif net_lower == "devnet":
                hrp = "dtola"
            else:
                hrp = network
        else:
            hrp = "tola"
        return encode_bech32m(hrp, self.raw)

    def __eq__(self, other: object) -> bool:
        if isinstance(other, Address):
            return self.raw == other.raw
        return False

    def __repr__(self) -> str:
        return f"Address('{self.to_hex()}')"

    def __str__(self) -> str:
        return self.to_hex()
