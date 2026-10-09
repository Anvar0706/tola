"""
Tokenized External Asset Protocol Module for TOLA Network.
Represents on-chain TLS-721 tokenized representations of external platform assets
(e.g., PaxtaGram usernames, Gifts Numbers, messenger gifts, social handles)
minted via authorized third-party verifier attestations.

CRITICAL ARCHITECTURAL INVARIANT:
TOLA NFT ownership represents blockchain ownership of the representation on TOLA.
It does NOT automatically grant or synchronize legal control over the original external platform asset.
"""

import json
import struct
import time
from enum import Enum
from typing import Union, Dict, Any
from .address import Address
from .crypto import keccak256, recover_address
from .wallet import TolaWallet


EXTERNAL_ASSET_CONTRACT_ADDRESS = "0x0000000000000000000000000000000000007721"


class ExternalAssetError(Exception):
    """Raised when external asset validation or attestation verification fails."""
    pass


class ExternalAssetType(str, Enum):
    MESSENGER_USERNAME = "MESSENGER_USERNAME"
    MESSENGER_NUMBER = "MESSENGER_NUMBER"
    MESSENGER_GIFT = "MESSENGER_GIFT"
    MESSENGER_BADGE = "MESSENGER_BADGE"
    SOCIAL_USERNAME = "SOCIAL_USERNAME"
    SOCIAL_NUMBER = "SOCIAL_NUMBER"
    DIGITAL_COLLECTIBLE = "DIGITAL_COLLECTIBLE"
    OTHER_EXTERNAL_ASSET = "OTHER_EXTERNAL_ASSET"


EXTERNAL_ASSET_TYPE_IDS = {
    ExternalAssetType.MESSENGER_USERNAME: 1,
    ExternalAssetType.MESSENGER_NUMBER: 2,
    ExternalAssetType.MESSENGER_GIFT: 3,
    ExternalAssetType.MESSENGER_BADGE: 4,
    ExternalAssetType.SOCIAL_USERNAME: 5,
    ExternalAssetType.SOCIAL_NUMBER: 6,
    ExternalAssetType.DIGITAL_COLLECTIBLE: 7,
    ExternalAssetType.OTHER_EXTERNAL_ASSET: 8,
    "MESSENGER_USERNAME": 1,
    "MESSENGER_NUMBER": 2,
    "MESSENGER_GIFT": 3,
    "MESSENGER_BADGE": 4,
    "SOCIAL_USERNAME": 5,
    "SOCIAL_NUMBER": 6,
    "DIGITAL_COLLECTIBLE": 7,
    "OTHER_EXTERNAL_ASSET": 8,
}


def get_asset_type_id(asset_type: Union[ExternalAssetType, str, int]) -> int:
    if isinstance(asset_type, int):
        if 1 <= asset_type <= 8:
            return asset_type
        raise ExternalAssetError(f"Invalid numeric asset_type: {asset_type}")
    if asset_type in EXTERNAL_ASSET_TYPE_IDS:
        return EXTERNAL_ASSET_TYPE_IDS[asset_type]
    raise ExternalAssetError(f"Unknown asset_type: {asset_type}")


def compute_external_asset_canonical_id(
    source_platform: str,
    asset_type: Union[ExternalAssetType, str, int],
    external_asset_id: str
) -> bytes:
    """
    Compute canonical 32-byte identity for duplicate mint prevention.
    Format: Keccak256("TOLA_EXTERNAL_ASSET_CANONICAL_V1:" || source_len (4B BE) || source || asset_type (1B) || external_id_len (4B BE) || external_id)
    """
    if not source_platform or not isinstance(source_platform, str):
        raise ExternalAssetError("Invalid source_platform")
    if not external_asset_id or not isinstance(external_asset_id, str):
        raise ExternalAssetError("Invalid external_asset_id")

    type_id = get_asset_type_id(asset_type)
    prefix = b"TOLA_EXTERNAL_ASSET_CANONICAL_V1:"
    src_bytes = source_platform.encode("utf-8")
    id_bytes = external_asset_id.encode("utf-8")

    header = struct.pack(">IBI", len(src_bytes), type_id, len(id_bytes))
    pre_image = prefix + header[:4] + src_bytes + header[4:5] + header[5:] + id_bytes
    return keccak256(pre_image)


def compute_external_asset_attestation_digest(
    chain_id: int,
    nonce: int,
    destination_wallet: Union[Address, str],
    expires_at: int,
    source_platform: str,
    asset_type: Union[ExternalAssetType, str, int],
    external_asset_id: str
) -> bytes:
    """
    Compute canonical Keccak-256 digest for external asset mint attestation.
    Format: Keccak256("TOLA_EXTERNAL_ASSET_ATTESTATION_V1:" || chain_id (8B BE) || nonce (8B BE) || destination (20B) || expires_at (8B BE) || source_len (4B BE) || source || asset_type (1B) || external_id_len (4B BE) || external_id)
    """
    type_id = get_asset_type_id(asset_type)
    prefix = b"TOLA_EXTERNAL_ASSET_ATTESTATION_V1:"
    src_bytes = source_platform.encode("utf-8")
    id_bytes = external_asset_id.encode("utf-8")

    if isinstance(destination_wallet, Address):
        dest_bytes = destination_wallet.bytes
    elif isinstance(destination_wallet, str):
        if destination_wallet.startswith(("tola1", "ttola1", "dtola1")):
            dest_addr, _ = Address.from_bech32m(destination_wallet)
            dest_bytes = dest_addr.bytes
        else:
            dest_bytes = Address.from_hex(destination_wallet).bytes
    else:
        raise TypeError("destination_wallet must be an Address or string")

    p1 = struct.pack(">QQ", chain_id, nonce)
    p2 = struct.pack(">QI", expires_at, len(src_bytes))
    p3 = struct.pack(">BI", type_id, len(id_bytes))

    pre_image = prefix + p1 + dest_bytes + p2 + src_bytes + p3 + id_bytes
    return keccak256(pre_image)


def sign_external_asset_attestation(
    verifier_wallet: TolaWallet,
    chain_id: int,
    nonce: int,
    destination_wallet: Union[Address, str],
    expires_at: int,
    source_platform: str,
    asset_type: Union[ExternalAssetType, str, int],
    external_asset_id: str
) -> Dict[str, Any]:
    """Sign an external asset mint attestation by an authorized verifier key."""
    digest = compute_external_asset_attestation_digest(
        chain_id,
        nonce,
        destination_wallet,
        expires_at,
        source_platform,
        asset_type,
        external_asset_id
    )

    dest_hex = destination_wallet.to_hex() if isinstance(destination_wallet, Address) else destination_wallet

    sig = verifier_wallet.sign_digest(digest)
    type_str = asset_type.value if isinstance(asset_type, ExternalAssetType) else str(asset_type)

    return {
        "chainId": int(chain_id),
        "nonce": int(nonce),
        "destinationWallet": dest_hex,
        "expiresAt": int(expires_at),
        "claimExpiresAt": int(expires_at),
        "sourcePlatform": source_platform,
        "assetType": type_str,
        "externalAssetId": external_asset_id,
        "verifierAddress": verifier_wallet.address.to_hex(),
        "signature": {
            "r": sig.r.hex(),
            "s": sig.s.hex(),
            "v": sig.v,
        },
    }


def verify_external_asset_attestation(
    attestation: Dict[str, Any],
    expected_verifier: Union[Address, str],
    current_time: int = None
) -> bool:
    """Verify an external asset attestation signature, expiry, and verifier address."""
    if current_time is None:
        current_time = int(time.time())

    chain_id = attestation["chainId"]
    nonce = attestation["nonce"]
    dest = attestation["destinationWallet"]
    expires_at = int(attestation.get("claimExpiresAt", attestation.get("expiresAt")))
    source_platform = attestation["sourcePlatform"]
    asset_type = attestation["assetType"]
    external_asset_id = attestation["externalAssetId"]
    sig = attestation["signature"]

    if current_time > expires_at:
        raise ExternalAssetError("Attestation has expired")

    if isinstance(expected_verifier, Address):
        exp_addr = expected_verifier
    elif isinstance(expected_verifier, str):
        if expected_verifier.startswith(("tola1", "ttola1", "dtola1")):
            exp_addr, _ = Address.from_bech32m(expected_verifier)
        else:
            exp_addr = Address.from_hex(expected_verifier)
    else:
        raise TypeError("expected_verifier must be an Address or string")

    digest = compute_external_asset_attestation_digest(
        chain_id,
        nonce,
        dest,
        expires_at,
        source_platform,
        asset_type,
        external_asset_id
    )

    r_bytes = bytes.fromhex(sig["r"])
    s_bytes = bytes.fromhex(sig["s"])
    v = int(sig["v"])

    recovered_bytes = recover_address(digest, r_bytes, s_bytes, v)
    recovered_addr = Address(recovered_bytes)

    if recovered_addr != exp_addr:
        raise ExternalAssetError(
            f"Verifier address mismatch: expected {exp_addr.to_hex()}, got {recovered_addr.to_hex()}"
        )

    return True


def validate_external_asset_metadata(metadata: Dict[str, Any]) -> bool:
    """Validate external asset metadata against size bounds and SSRF vectors."""
    if not isinstance(metadata, dict):
        raise ExternalAssetError("Metadata must be a dictionary")

    payload_str = json.dumps(metadata)
    if len(payload_str.encode("utf-8")) > 65536:
        raise ExternalAssetError("Metadata payload exceeds maximum bound of 64 KB")

    if "sourcePlatform" not in metadata or not metadata["sourcePlatform"]:
        raise ExternalAssetError("Metadata missing valid sourcePlatform")
    if "externalAssetId" not in metadata or not metadata["externalAssetId"]:
        raise ExternalAssetError("Metadata missing valid externalAssetId")

    media_uri = metadata.get("mediaUri")
    if media_uri and isinstance(media_uri, str):
        uri_lower = media_uri.lower()
        if any(bad in uri_lower for bad in ["localhost", "127.0.0.1", "169.254.169.254", "10.", "192.168."]):
            raise ExternalAssetError("SSRF protection: Private network and loopback URIs are forbidden")

    return True


def encode_mint_external_asset_calldata(
    attestation: Dict[str, Any],
    metadata_uri: str = ""
) -> bytes:
    """Encode calldata for calling ExternalAssetContract mintWithAttestation."""
    chain_id = int(attestation["chainId"])
    nonce = int(attestation["nonce"])
    dest = attestation["destinationWallet"]
    if isinstance(dest, Address):
        dest_bytes = dest.bytes
    elif isinstance(dest, str):
        if dest.startswith(("tola1", "ttola1", "dtola1")):
            dest_addr, _ = Address.from_bech32m(dest)
            dest_bytes = dest_addr.bytes
        else:
            dest_bytes = Address.from_hex(dest).bytes
    else:
        raise TypeError("destinationWallet must be an Address or string")

    expires_at = int(attestation.get("claimExpiresAt", attestation.get("expiresAt")))
    src_bytes = attestation["sourcePlatform"].encode("utf-8")
    type_id = get_asset_type_id(attestation["assetType"])
    id_bytes = attestation["externalAssetId"].encode("utf-8")

    verifier = attestation["verifierAddress"]
    if isinstance(verifier, Address):
        ver_bytes = verifier.bytes
    elif isinstance(verifier, str):
        if verifier.startswith(("tola1", "ttola1", "dtola1")):
            ver_addr, _ = Address.from_bech32m(verifier)
            ver_bytes = ver_addr.bytes
        else:
            ver_bytes = Address.from_hex(verifier).bytes
    else:
        raise TypeError("verifierAddress must be an Address or string")

    sig = attestation["signature"]
    r_hex = sig["r"].replace("0x", "")
    s_hex = sig["s"].replace("0x", "")
    r_bytes = bytes.fromhex(r_hex)
    s_bytes = bytes.fromhex(s_hex)
    v = int(sig["v"])

    meta_bytes = metadata_uri.encode("utf-8")

    buf = bytearray()
    buf.append(1)  # TAG_MINT
    buf.extend(struct.pack(">QQ", chain_id, nonce))
    buf.extend(dest_bytes)
    buf.extend(struct.pack(">QI", expires_at, len(src_bytes)))
    buf.extend(src_bytes)
    buf.append(type_id)
    buf.extend(struct.pack(">I", len(id_bytes)))
    buf.extend(id_bytes)
    buf.extend(ver_bytes)
    buf.extend(r_bytes)
    buf.extend(s_bytes)
    buf.append(v)
    buf.extend(struct.pack(">I", len(meta_bytes)))
    buf.extend(meta_bytes)
    return bytes(buf)


def encode_transfer_external_asset_calldata(
    from_wallet: Union[Address, str],
    to_wallet: Union[Address, str],
    token_id: int
) -> bytes:
    """Encode calldata for calling ExternalAssetContract transferFrom."""
    if isinstance(from_wallet, Address):
        from_bytes = from_wallet.bytes
    elif isinstance(from_wallet, str):
        if from_wallet.startswith(("tola1", "ttola1", "dtola1")):
            addr, _ = Address.from_bech32m(from_wallet)
            from_bytes = addr.bytes
        else:
            from_bytes = Address.from_hex(from_wallet).bytes
    else:
        raise TypeError("from_wallet must be an Address or string")

    if isinstance(to_wallet, Address):
        to_bytes = to_wallet.bytes
    elif isinstance(to_wallet, str):
        if to_wallet.startswith(("tola1", "ttola1", "dtola1")):
            addr, _ = Address.from_bech32m(to_wallet)
            to_bytes = addr.bytes
        else:
            to_bytes = Address.from_hex(to_wallet).bytes
    else:
        raise TypeError("to_wallet must be an Address or string")

    buf = bytearray()
    buf.append(2)  # TAG_TRANSFER_FROM
    buf.extend(from_bytes)
    buf.extend(to_bytes)
    buf.extend(struct.pack(">Q", int(token_id)))
    return bytes(buf)


def encode_burn_external_asset_calldata(token_id: int) -> bytes:
    """Encode calldata for calling ExternalAssetContract burn."""
    buf = bytearray()
    buf.append(6)  # TAG_BURN
    buf.extend(struct.pack(">Q", int(token_id)))
    return bytes(buf)

