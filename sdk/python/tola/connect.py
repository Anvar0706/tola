"""
TOLA Connect Protocol & Clear-Signing Validation for Python SDK.
"""

import time
import secrets
from typing import Union, Optional, Dict, Any, List
from .address import Address


class ClearSigningValidationError(Exception):
    """Raised when a clear-signing prompt fails validation."""
    pass


class ConnectMessageType:
    """The 11 canonical TOLA Connect protocol message types."""
    SESSION_INIT = "SESSION_INIT"
    SESSION_ACCEPT = "SESSION_ACCEPT"
    DEVICE_REGISTER = "DEVICE_REGISTER"
    DEVICE_REVOKE = "DEVICE_REVOKE"
    SIGN_REQUEST = "SIGN_REQUEST"
    SIGN_APPROVAL = "SIGN_APPROVAL"
    SIGN_REJECTION = "SIGN_REJECTION"
    TRANSACTION_REQUEST = "TRANSACTION_REQUEST"
    TRANSACTION_APPROVAL = "TRANSACTION_APPROVAL"
    TRANSACTION_SUBMISSION = "TRANSACTION_SUBMISSION"
    TRANSACTION_RESULT = "TRANSACTION_RESULT"


class DeviceRegistry:
    """Multi-device authorization and session manager."""
    def __init__(self):
        self.devices: Dict[str, Dict[str, Any]] = {}

    def register_device(
        self,
        device_id: str,
        device_name: str = "",
        public_key: Optional[str] = None,
        timestamp: Optional[int] = None
    ) -> Dict[str, Any]:
        if not device_id or not isinstance(device_id, str):
            raise ValueError("Device ID must be a non-empty string")
        existing = self.devices.get(device_id)
        if existing:
            if existing["is_revoked"]:
                raise ValueError(f"Cannot re-register revoked device '{device_id}'")
            raise ValueError(f"Device '{device_id}' is already registered")
        record = {
            "device_id": device_id,
            "device_name": device_name or device_id,
            "public_key": public_key,
            "authorized_at": timestamp if timestamp is not None else int(time.time()),
            "is_revoked": False,
        }
        self.devices[device_id] = record
        return record

    def revoke_device(self, device_id: str) -> Dict[str, Any]:
        if device_id not in self.devices:
            raise KeyError(f"Device '{device_id}' not found")
        self.devices[device_id]["is_revoked"] = True
        return self.devices[device_id]

    def is_authorized(self, device_id: str) -> bool:
        dev = self.devices.get(device_id)
        return bool(dev and not dev["is_revoked"])

    def list_devices(self) -> List[Dict[str, Any]]:
        return list(self.devices.values())


class TolaConnectSession:
    """End-to-End Encrypted TOLA Connect Session."""
    def __init__(
        self,
        session_id: str,
        domain: str,
        app_name: str,
        session_key: bytes,
        created_at: int,
        expires_at: int
    ):
        self.session_id = session_id
        self.domain = domain
        self.app_name = app_name
        self.session_key = session_key
        self.created_at = created_at
        self.expires_at = expires_at
        self.last_seen_nonce = 0

    def __repr__(self) -> str:
        return f"TolaConnectSession(session_id='{self.session_id}', domain='{self.domain}', key=[REDACTED])"

    def __str__(self) -> str:
        return self.__repr__()

    @classmethod
    def create(cls, domain: str, app_name: str, ttl_seconds: int = 86400) -> "TolaConnectSession":
        session_id = secrets.token_hex(16)
        session_key = secrets.token_bytes(32)
        now = int(time.time())
        return cls(session_id, domain, app_name, session_key, now, now + ttl_seconds)

    def is_expired(self, current_time: Optional[int] = None) -> bool:
        now = current_time if current_time is not None else int(time.time())
        return now > self.expires_at

    def create_envelope(
        self,
        msg_type: str,
        payload: Dict[str, Any],
        request_id: str,
        nonce: int,
        origin: Optional[str] = None
    ) -> Dict[str, Any]:
        now = int(time.time())
        return {
            "version": "1.0",
            "sessionId": self.session_id,
            "type": msg_type,
            "requestId": str(request_id),
            "nonce": nonce,
            "timestamp": now,
            "expiresAt": min(now + 300, self.expires_at),
            "domain": self.domain,
            "origin": origin or f"https://{self.domain}",
            "appName": self.app_name,
            "payload": payload,
        }

    def validate_envelope(self, envelope: Dict[str, Any], current_time: Optional[int] = None) -> bool:
        if not isinstance(envelope, dict):
            raise ValueError("Envelope must be a dictionary")
        if envelope.get("version") != "1.0":
            raise ValueError(f"Incompatible protocol version: {envelope.get('version')}")
        if envelope.get("sessionId") != self.session_id:
            raise ValueError(f"Session ID mismatch: expected {self.session_id}")
        valid_types = {
            ConnectMessageType.SESSION_INIT,
            ConnectMessageType.SESSION_ACCEPT,
            ConnectMessageType.DEVICE_REGISTER,
            ConnectMessageType.DEVICE_REVOKE,
            ConnectMessageType.SIGN_REQUEST,
            ConnectMessageType.SIGN_APPROVAL,
            ConnectMessageType.SIGN_REJECTION,
            ConnectMessageType.TRANSACTION_REQUEST,
            ConnectMessageType.TRANSACTION_APPROVAL,
            ConnectMessageType.TRANSACTION_SUBMISSION,
            ConnectMessageType.TRANSACTION_RESULT,
        }
        msg_type = envelope.get("type")
        if msg_type not in valid_types:
            raise ValueError(f"Unknown message type: {msg_type}")
        req_id = envelope.get("requestId")
        if not req_id or not isinstance(req_id, str) or not req_id.strip():
            raise ValueError("Missing or empty requestId in envelope")
        if self.is_expired(current_time):
            raise ValueError("TOLA Connect session has expired")
        now = current_time if current_time is not None else int(time.time())
        if envelope.get("timestamp") and envelope["timestamp"] > now + 60:
            raise ValueError("Message timestamp is in the future (clock skew exceeds 60s)")
        if envelope.get("expiresAt") and now > envelope["expiresAt"]:
            raise ValueError("Message envelope has expired")
        nonce = int(envelope.get("nonce", 0))
        if nonce <= self.last_seen_nonce:
            raise ValueError(f"Replay attack detected: non-monotonic nonce {nonce} <= {self.last_seen_nonce}")
        if envelope.get("domain") and envelope["domain"].lower() != self.domain.lower():
            raise ValueError(f"Domain mismatch: expected {self.domain}, got {envelope['domain']}")
        self.last_seen_nonce = nonce
        return True


class ClearSigningPrompt:
    """Structured Clear-Signing Prompt ensuring user transparency."""
    def __init__(
        self,
        domain: str,
        action_name: str,
        target_address: Union[Address, str],
        amount: int,
        asset_symbol: str = "TOLA",
        max_fee: int = 21000,
        chain_id: int = 9999,
        nonce: int = 0,
        memo: Optional[str] = None
    ):
        self.domain = domain
        self.action_name = action_name
        if isinstance(target_address, Address):
            self.target_address = target_address
        elif isinstance(target_address, str):
            if target_address.startswith(("tola1", "ttola1", "dtola1")):
                self.target_address, _ = Address.from_bech32m(target_address)
            else:
                self.target_address = Address.from_hex(target_address)
        else:
            raise TypeError("Target address must be an Address or string")
        self.amount = amount

        self.asset_symbol = asset_symbol
        self.max_fee = max_fee
        self.chain_id = chain_id
        self.nonce = nonce
        self.memo = memo
        self.validate()

    def validate(self) -> bool:
        if not self.domain or not self.domain.strip():
            raise ClearSigningValidationError("Domain must be a non-empty string")
        if not self.action_name or not self.action_name.strip():
            raise ClearSigningValidationError("Action name must be a non-empty string")
        if not isinstance(self.amount, int) or self.amount < 0:
            raise ClearSigningValidationError("Amount must be a non-negative integer")
        if not self.asset_symbol or not self.asset_symbol.strip():
            raise ClearSigningValidationError("Asset symbol must be a non-empty string")
        if not isinstance(self.max_fee, int) or self.max_fee < 0:
            raise ClearSigningValidationError("Max fee must be a non-negative integer")
        if self.chain_id <= 0:
            raise ClearSigningValidationError("Chain ID must be greater than zero")
        if self.memo is not None:
            if not isinstance(self.memo, str):
                raise ClearSigningValidationError("Memo must be a string")
            byte_len = len(self.memo.encode("utf-8"))
            if byte_len > 256:
                raise ClearSigningValidationError(f"Memo exceeds maximum length of 256 bytes (got {byte_len})")
        return True

    def format_summary(self) -> str:
        self.validate()
        whole = self.amount // (10**18)
        fraction = f"{self.amount % (10**18):018d}"[:4]
        memo_str = self.memo.strip() if self.memo else "(None)"
        return "\n".join([
            "=== TOLA Connect Clear-Signing Authorization ===",
            f"Domain:     {self.domain.strip()}",
            f"Action:     {self.action_name.strip()}",
            f"Target:     {self.target_address.to_bech32m()}",
            f"Amount:     {whole}.{fraction} {self.asset_symbol.strip()}",
            f"Max Fee:    {self.max_fee} gas",
            f"Chain ID:   {self.chain_id}",
            f"Memo:       {memo_str}",
        ])
