"""
Wallet and cryptographic key management for TOLA Network.
Strictly conforms to TOLA Wallet Derivation Standard v1.0:
BIP-39 -> BIP-32 -> BIP-44 (m/44'/9999'/0'/0/0) -> Secp256k1 -> Keccak-256 -> 20-byte address -> Bech32m
"""

import secrets
from typing import Union, Optional, Dict, Any
from .types import UnsignedTransaction, Transaction, Secp256k1Signature, Network
from .address import Address
from .crypto import (
    N,
    point_multiply,
    compress_public_key,
    uncompressed_public_key,
    public_key_to_address,
    sign_digest,
    keccak256,
    recover_address,
)
from .transaction import compute_transaction_digest
from .bip39 import generate_mnemonic, validate_mnemonic, mnemonic_to_seed
from .bip32 import (
    derive_path,
    CANONICAL_DERIVATION_PATH,
    LEGACY_TESTNET_PROVISIONAL_PATH,
)


class TolaWallet:
    """
    Cryptographic wallet for signing transactions, messages, and deriving addresses.
    Implements strict memory security and zero secret exposure in repr/str/dict.
    """

    def __init__(
        self,
        private_key: int,
        mnemonic: Optional[str] = None,
        derivation_path: Optional[str] = None,
    ):
        if not (1 <= private_key < N):
            raise ValueError("Private key outside valid scalar range [1, N-1]")
        self._private_key = private_key
        self._mnemonic = mnemonic
        self._derivation_path = derivation_path
        self.public_point = point_multiply(private_key)
        addr_bytes = public_key_to_address(self.public_point)
        self.address = Address(addr_bytes)

    @classmethod
    def generate(cls, word_count: int = 12, passphrase: str = "") -> "TolaWallet":
        """
        Generate a fresh random wallet accompanied by its BIP-39 recovery phrase.
        Derives using canonical standard path m/44'/9999'/0'/0/0.
        """
        phrase = generate_mnemonic(word_count)
        return cls.from_mnemonic(phrase, passphrase)

    @classmethod
    def from_mnemonic(
        cls,
        mnemonic: str,
        passphrase: str = "",
        network: Union[Network, str] = Network.MAINNET,
    ) -> "TolaWallet":
        """
        Restore wallet from a BIP-39 mnemonic phrase.
        Always derives along canonical path m/44'/9999'/0'/0/0 regardless of network (Model A).
        """
        return cls.from_mnemonic_with_path(
            mnemonic, passphrase, CANONICAL_DERIVATION_PATH
        )

    @classmethod
    def from_mnemonic_with_path(
        cls,
        mnemonic: str,
        passphrase: str = "",
        path: str = CANONICAL_DERIVATION_PATH,
    ) -> "TolaWallet":
        """
        Restore wallet from a BIP-39 mnemonic using an explicit derivation path.
        """
        normalized = validate_mnemonic(mnemonic)
        seed = mnemonic_to_seed(normalized, passphrase)
        derived = derive_path(seed, path)
        scalar = int.from_bytes(derived.key, "big")
        return cls(scalar, mnemonic=normalized, derivation_path=path)

    @classmethod
    def from_mnemonic_legacy_testnet(
        cls, mnemonic: str, passphrase: str = ""
    ) -> "TolaWallet":
        """
        Restore wallet using deprecated provisional testnet path m/44'/102'/0'/0/0.
        Retained strictly for legacy migration verification.
        """
        return cls.from_mnemonic_with_path(
            mnemonic, passphrase, LEGACY_TESTNET_PROVISIONAL_PATH
        )

    @classmethod
    def from_private_key(cls, key: Union[int, bytes, str]) -> "TolaWallet":
        """
        Instantiate wallet from raw private key (int scalar, 32-byte bytes, or hex string).
        Preserves backward compatibility with legacy private key imports.
        """
        if isinstance(key, int):
            return cls(key)
        elif isinstance(key, (bytes, bytearray)):
            return cls.from_private_key_bytes(bytes(key))
        elif isinstance(key, str):
            return cls.from_hex_private_key(key)
        else:
            raise TypeError(
                f"Unsupported private key type: {type(key)}. Expected int, bytes, or hex string."
            )

    @classmethod
    def from_private_key_bytes(cls, data: bytes) -> "TolaWallet":
        """Instantiate wallet from 32-byte private key scalar."""
        if len(data) != 32:
            raise ValueError(f"Private key must be 32 bytes, got {len(data)}")
        return cls(int.from_bytes(data, "big"))

    @classmethod
    def from_hex_private_key(cls, hex_str: str) -> "TolaWallet":
        """Instantiate wallet from 64-character hex string (with or without '0x')."""
        clean = hex_str[2:] if hex_str.startswith(("0x", "0X")) else hex_str
        if len(clean) != 64:
            raise ValueError(f"Private key hex must be 64 characters, got {len(clean)}")
        return cls(int(clean, 16))

    @property
    def mnemonic(self) -> Optional[str]:
        """Return recovery mnemonic phrase if wallet was generated or restored from mnemonic."""
        return self._mnemonic

    @property
    def derivation_path(self) -> Optional[str]:
        """Return BIP-44 derivation path used if restored from mnemonic."""
        return self._derivation_path

    @property
    def raw_address(self) -> str:
        """Return 0x-prefixed 40-character hex address."""
        return self.address.to_hex()

    @property
    def private_key_hex(self) -> str:
        """
        Return 0x-prefixed 64-character hex private key string.
        CAUTION: Sensitive secret material!
        """
        return "0x" + self._private_key.to_bytes(32, "big").hex()

    def export_private_key_hex(self) -> str:
        """Explicit method to export private key hex string."""
        return self.private_key_hex

    @property
    def public_key_bytes(self) -> bytes:
        """Return 33-byte compressed Secp256k1 public key."""
        return compress_public_key(self.public_point)

    def get_public_key(self, compressed: bool = True) -> bytes:
        """Return 33-byte compressed (default) or 65-byte uncompressed public key."""
        if compressed:
            return compress_public_key(self.public_point)
        return uncompressed_public_key(self.public_point)

    def get_address(self, network: Union[Network, str] = Network.MAINNET) -> str:
        """
        Return checksummed Bech32m address for target network.
        e.g. tola1... (mainnet), ttola1... (testnet), dtola1... (devnet).
        """
        return self.address.to_bech32m(network)

    def address_bech32m(self, network: Union[Network, str] = Network.MAINNET) -> str:
        """Return checksummed Bech32m address (backward-compatible alias)."""
        return self.get_address(network)

    def sign_digest(self, digest32: bytes) -> Secp256k1Signature:
        """Sign 32-byte pre-hashed digest returning canonical Secp256k1 signature."""
        r, s, v = sign_digest(self._private_key, digest32)
        return Secp256k1Signature(r=r, s=s, v=v)

    def sign_transaction(self, tx: UnsignedTransaction) -> Transaction:
        """Sign transaction matching protocol wire format and mempool verification."""
        from_addr = Address.from_hex(tx.from_address)
        if from_addr != self.address:
            raise ValueError(
                f"Transaction from ({tx.from_address}) does not match wallet ({self.address.to_hex()})"
            )

        digest = compute_transaction_digest(tx)
        sig = self.sign_digest(digest)

        return Transaction(
            chain_id=tx.chain_id,
            nonce=tx.nonce,
            from_address=tx.from_address,
            to_address=tx.to_address,
            value=tx.value,
            gas_limit=tx.gas_limit,
            max_fee_per_gas=tx.max_fee_per_gas,
            max_priority_fee_per_gas=tx.max_priority_fee_per_gas,
            valid_until_block=tx.valid_until_block,
            data=tx.data,
            memo=tx.memo,
            signature=sig,
        )

    def sign_message(self, message: Union[str, bytes]) -> str:
        """
        Sign arbitrary text or binary message with standard prefix:
        Keccak-256("\\x19TOLA Signed Message:\\n" + len + message)
        Returns 0x-prefixed 130-character hex signature (65 bytes: r || s || v).
        """
        msg_bytes = (
            message.encode("utf-8") if isinstance(message, str) else bytes(message)
        )
        prefix = f"\x19TOLA Signed Message:\n{len(msg_bytes)}".encode("utf-8")
        pre_image = prefix + msg_bytes
        digest = keccak256(pre_image)
        sig = self.sign_digest(digest)
        return "0x" + sig.r.hex() + sig.s.hex() + f"{sig.v:02x}"

    @staticmethod
    def recover_message_address(message: Union[str, bytes], sig_hex: str) -> Address:
        """
        Recover signer Address from arbitrary message and 65-byte hex signature.
        """
        clean = sig_hex[2:] if sig_hex.startswith(("0x", "0X")) else sig_hex
        if len(clean) != 130:
            raise ValueError(
                f"Invalid signature length: {len(clean)} hex chars. Expected 130 (65 bytes)."
            )

        r_bytes = bytes.fromhex(clean[0:64])
        s_bytes = bytes.fromhex(clean[64:128])
        v = int(clean[128:130], 16)

        msg_bytes = (
            message.encode("utf-8") if isinstance(message, str) else bytes(message)
        )
        prefix = f"\x19TOLA Signed Message:\n{len(msg_bytes)}".encode("utf-8")
        pre_image = prefix + msg_bytes
        digest = keccak256(pre_image)

        addr_bytes = recover_address(digest, r_bytes, s_bytes, v)
        return Address(addr_bytes)

    def to_dict(self) -> Dict[str, Any]:
        """Safe dictionary representation exposing only public information."""
        return {
            "address": self.get_address(Network.MAINNET),
            "raw_address": self.raw_address,
        }

    def __repr__(self) -> str:
        """Safe string representation: redacts all private key and mnemonic material."""
        return f"<TolaWallet address={self.get_address(Network.MAINNET)}>"

    def __str__(self) -> str:
        return self.get_address(Network.MAINNET)
