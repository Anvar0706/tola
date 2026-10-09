"""
Canonical transaction serialization, digest computation, and builder for TOLA Network.
Conforms to tola-codec and tola-crypto specifications.
"""

from typing import Optional, Union
from .types import UnsignedTransaction, Transaction, Secp256k1Signature, Network, MAX_MEMO_BYTES
from .address import Address
from .crypto import blake3, domain_separated_hash, DOMAIN_TRANSACTION_PAYLOAD



def serialize_unsigned_transaction(tx: UnsignedTransaction) -> bytes:
    """Serialize unsigned transaction to canonical wire bytes."""
    data_bytes = tx.data if isinstance(tx.data, bytes) else bytes(tx.data)
    from_addr = Address.from_hex(tx.from_address)
    to_addr = Address.from_hex(tx.to_address) if tx.to_address else None

    out = bytearray()
    # 1. chain_id (8B LE)
    out.extend(tx.chain_id.to_bytes(8, "little"))
    # 2. nonce (8B LE)
    out.extend(tx.nonce.to_bytes(8, "little"))
    # 3. from (20B)
    out.extend(from_addr.raw)
    # 4. to option (1B + 20B or 1B)
    if to_addr:
        out.append(1)
        out.extend(to_addr.raw)
    else:
        out.append(0)
    # 5. value (16B LE)
    out.extend(tx.value.to_bytes(16, "little"))
    # 6. gas_limit (8B LE)
    out.extend(tx.gas_limit.to_bytes(8, "little"))
    # 7. max_fee_per_gas (16B LE)
    out.extend(tx.max_fee_per_gas.to_bytes(16, "little"))
    # 8. max_priority_fee_per_gas (16B LE)
    out.extend(tx.max_priority_fee_per_gas.to_bytes(16, "little"))
    # 9. valid_until_block (8B LE)
    out.extend(tx.valid_until_block.to_bytes(8, "little"))
    # 10. data (4B LE len + bytes)
    out.extend(len(data_bytes).to_bytes(4, "little"))
    out.extend(data_bytes)
    # 11. memo (optional)
    if tx.memo is not None:
        memo_str = tx.memo.strip() if isinstance(tx.memo, str) else str(tx.memo).strip()
        if memo_str:
            memo_bytes = memo_str.encode("utf-8")
            if len(memo_bytes) > MAX_MEMO_BYTES:
                raise ValueError(
                    f"Memo length {len(memo_bytes)} bytes exceeds maximum allowed {MAX_MEMO_BYTES} bytes"
                )
            out.extend(len(memo_bytes).to_bytes(4, "little"))
            out.extend(memo_bytes)

    return bytes(out)


def serialize_signed_transaction(tx: Transaction) -> bytes:
    """Serialize signed transaction to canonical wire bytes."""
    unsigned_bytes = serialize_unsigned_transaction(tx)
    if not tx.signature:
        raise ValueError("Cannot serialize transaction without signature")

    out = bytearray(unsigned_bytes)
    out.extend(tx.signature.to_bytes())
    return bytes(out)


def compute_transaction_digest(tx: UnsignedTransaction) -> bytes:
    """Compute canonical transaction signing digest matching protocol mempool."""
    unsigned_bytes = serialize_unsigned_transaction(tx)
    payload_hash = blake3(unsigned_bytes)
    return domain_separated_hash(DOMAIN_TRANSACTION_PAYLOAD, tx.chain_id, 0, payload_hash)


class TransactionBuilder:
    """Fluent transaction builder."""

    def __init__(self, chain_id: int = 102):
        self._chain_id = chain_id
        self._nonce = 0
        self._to_address: Optional[str] = None
        self._value = 0
        self._gas_limit = 21000
        self._max_fee_per_gas = 1_000_000_000
        self._max_priority_fee_per_gas = 1_000_000_000
        self._valid_until_block = 1_000_000
        self._data = b""
        self._memo: Optional[str] = None

    def chain_id(self, chain_id: int) -> "TransactionBuilder":
        self._chain_id = chain_id
        return self

    def nonce(self, nonce: int) -> "TransactionBuilder":
        self._nonce = nonce
        return self

    def to(self, addr: Union[str, Address, None]) -> "TransactionBuilder":
        if addr is None:
            self._to_address = None
        elif isinstance(addr, Address):
            self._to_address = addr.to_hex()
        elif addr.startswith(("tola1", "ttola1", "dtola1")):
            self._to_address = Address.from_bech32m(addr)[0].to_hex()
        else:
            self._to_address = Address.from_hex(addr).to_hex()
        return self

    def value(self, val: int) -> "TransactionBuilder":
        self._value = val
        return self

    def gas_limit(self, gas: int) -> "TransactionBuilder":
        self._gas_limit = gas
        return self

    def max_fee_per_gas(self, fee: int) -> "TransactionBuilder":
        self._max_fee_per_gas = fee
        return self

    def max_priority_fee_per_gas(self, fee: int) -> "TransactionBuilder":
        self._max_priority_fee_per_gas = fee
        return self

    def valid_until_block(self, height: int) -> "TransactionBuilder":
        self._valid_until_block = height
        return self

    def data(self, data: Union[bytes, str]) -> "TransactionBuilder":
        if isinstance(data, str):
            clean = data[2:] if data.startswith(("0x", "0X")) else data
            self._data = bytes.fromhex(clean)
        else:
            self._data = bytes(data)
        return self

    def memo(self, memo: Optional[str]) -> "TransactionBuilder":
        """Set optional memo text (max 256 bytes UTF-8). None or empty clears memo."""
        if memo is None:
            self._memo = None
        else:
            m = memo.strip()
            self._memo = m if m else None
        return self

    def build(self, from_addr: Union[str, Address]) -> UnsignedTransaction:
        if isinstance(from_addr, Address):
            from_hex = from_addr.to_hex()
        elif from_addr.startswith(("tola1", "ttola1", "dtola1")):
            from_hex = Address.from_bech32m(from_addr)[0].to_hex()
        else:
            from_hex = Address.from_hex(from_addr).to_hex()

        return UnsignedTransaction(
            chain_id=self._chain_id,
            nonce=self._nonce,
            from_address=from_hex,
            to_address=self._to_address,
            value=self._value,
            gas_limit=self._gas_limit,
            max_fee_per_gas=self._max_fee_per_gas,
            max_priority_fee_per_gas=self._max_priority_fee_per_gas,
            valid_until_block=self._valid_until_block,
            data=self._data,
            memo=self._memo,
        )

