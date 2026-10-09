"""
Production BIP-39 Mnemonic Generator and Validator for TOLA Network.
Strictly adheres to official BIP-39 specification and TOLA Wallet Derivation Standard v1.0.
"""

import hashlib
import secrets
import unicodedata
from typing import List
from .wordlist import WORDLIST, WORD_INDEX_MAP


def generate_mnemonic(word_count: int = 12) -> str:
    """
    Generate a cryptographically secure BIP-39 mnemonic phrase.
    Supported word counts: 12 (128-bit entropy) or 24 (256-bit entropy).
    """
    if word_count == 12:
        entropy_bytes = 16
        cs_len = 4
    elif word_count == 24:
        entropy_bytes = 32
        cs_len = 8
    else:
        raise ValueError(f"Unsupported word count: {word_count}. Expected 12 or 24 words.")

    entropy = secrets.token_bytes(entropy_bytes)
    cs_hash = hashlib.sha256(entropy).digest()
    cs_val = cs_hash[0] >> (8 - cs_len)

    combined = (int.from_bytes(entropy, "big") << cs_len) | cs_val

    words: List[str] = []
    for i in range(word_count):
        shift = (word_count - 1 - i) * 11
        idx = (combined >> shift) & 0x7FF
        words.append(WORDLIST[idx])

    return " ".join(words)


def validate_mnemonic(mnemonic: str) -> str:
    """
    Validate a BIP-39 mnemonic phrase.
    Applies NFKD normalization, checks wordlist membership, and verifies checksum.
    Returns the normalized lowercase mnemonic string.
    """
    if not isinstance(mnemonic, str):
        raise ValueError("Mnemonic must be a string")

    normalized = unicodedata.normalize("NFKD", mnemonic.strip())
    words = [w.lower() for w in normalized.split()]

    word_count = len(words)
    if word_count not in (12, 24):
        raise ValueError(f"Invalid word count: {word_count}. Expected 12 or 24 words.")

    if word_count == 12:
        cs_len = 4
        ent_len = 16
    else:
        cs_len = 8
        ent_len = 32

    combined = 0
    for w in words:
        if w not in WORD_INDEX_MAP:
            raise ValueError(f"Word not in BIP-39 wordlist: {w}")
        combined = (combined << 11) | WORD_INDEX_MAP[w]

    cs_val = combined & ((1 << cs_len) - 1)
    ent_val = combined >> cs_len
    entropy = ent_val.to_bytes(ent_len, "big")

    expected_cs = hashlib.sha256(entropy).digest()[0] >> (8 - cs_len)
    if cs_val != expected_cs:
        raise ValueError("Invalid BIP-39 checksum in mnemonic phrase")

    return " ".join(words)


def mnemonic_to_seed(mnemonic: str, passphrase: str = "") -> bytes:
    """
    Convert a BIP-39 mnemonic phrase to a 64-byte binary seed.
    Uses PBKDF2-HMAC-SHA512 with 2048 iterations and salt 'mnemonic' + NFKD(passphrase).
    """
    normalized_phrase = validate_mnemonic(mnemonic)
    normalized_passphrase = unicodedata.normalize("NFKD", passphrase) if passphrase else ""
    salt = b"mnemonic" + normalized_passphrase.encode("utf-8")

    return hashlib.pbkdf2_hmac(
        "sha512",
        normalized_phrase.encode("utf-8"),
        salt,
        2048,
    )
