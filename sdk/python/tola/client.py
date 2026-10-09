"""
JSON-RPC 2.0 client for interacting with TOLA Network nodes.
Uses Python standard library (urllib.request, json).
"""

import json
import time
import urllib.request
import urllib.error
from typing import Any, Optional, Dict, List
from .types import NodeHealth, TransactionReceipt, Transaction
from .transaction import serialize_signed_transaction


import http.client
import urllib.parse

class RpcError(Exception):
    def __init__(self, code: int, message: str, data: Any = None):
        super().__init__(f"RPC Error {code}: {message}")
        self.code = code
        self.message = message
        self.data = data


class TolaClient:
    """Synchronous JSON-RPC client for TOLA Network nodes."""

    def __init__(self, rpc_url: str = "http://127.0.0.1:8545", timeout: float = 10.0):
        self.rpc_url = rpc_url.rstrip("/")
        self.timeout = timeout
        self._request_id = 1

    def _get_conn(self) -> tuple:
        parsed = urllib.parse.urlparse(self.rpc_url)
        host = parsed.hostname or "127.0.0.1"
        port = parsed.port or (443 if parsed.scheme == "https" else 80)
        path = parsed.path or "/"
        conn_cls = http.client.HTTPSConnection if parsed.scheme == "https" else http.client.HTTPConnection
        return conn_cls(host, port, timeout=self.timeout), path

    def call(self, method: str, params: Optional[list] = None) -> Any:
        """Execute JSON-RPC 2.0 method."""
        payload = {
            "jsonrpc": "2.0",
            "id": self._request_id,
            "method": method,
            "params": params if params is not None else [],
        }
        self._request_id += 1

        req_bytes = json.dumps(payload).encode("utf-8")
        conn, path = self._get_conn()
        try:
            conn.request("POST", path, req_bytes, {"Content-Type": "application/json", "Connection": "close"})
            resp = conn.getresponse()
            if resp.status >= 400:
                raise RuntimeError(f"HTTP Error {resp.status}: {resp.reason}")
            data = json.loads(resp.read().decode("utf-8"))
        finally:
            conn.close()

        if "error" in data and data["error"]:
            err = data["error"]
            raise RpcError(err.get("code", -32603), err.get("message", "Unknown error"), err.get("data"))

        return data.get("result")

    def check_health(self) -> bool:
        """Check if node is responsive and healthy."""
        try:
            conn, _ = self._get_conn()
            try:
                conn.request("GET", "/health", headers={"Connection": "close"})
                resp = conn.getresponse()
                data = json.loads(resp.read().decode("utf-8"))
                return data.get("status") in ("ok", "healthy")
            finally:
                conn.close()
        except Exception:
            try:
                self.get_block_number()
                return True
            except Exception:
                return False

    def get_chain_id(self) -> int:
        res = self.call("eth_chainId")
        return int(res, 16) if isinstance(res, str) else int(res)

    def get_block_number(self) -> int:
        res = self.call("eth_blockNumber")
        return int(res, 16) if isinstance(res, str) else int(res)

    def get_balance(self, address: Any, block_tag: str = "latest") -> int:
        addr_str = address.to_hex() if hasattr(address, "to_hex") else str(address)
        res = self.call("eth_getBalance", [addr_str, block_tag])
        return int(res, 16) if isinstance(res, str) else int(res)

    def get_nonce(self, address: Any, block_tag: str = "latest") -> int:
        addr_str = address.to_hex() if hasattr(address, "to_hex") else str(address)
        res = self.call("eth_getTransactionCount", [addr_str, block_tag])
        return int(res, 16) if isinstance(res, str) else int(res)

    def send_raw_transaction(self, signed_hex: str) -> str:
        formatted = signed_hex if signed_hex.startswith("0x") else "0x" + signed_hex
        return self.call("eth_sendRawTransaction", [formatted])

    def send_transaction(self, tx: Transaction) -> str:
        raw_bytes = serialize_signed_transaction(tx)
        return self.send_raw_transaction(raw_bytes.hex())

    def get_transaction_receipt(self, tx_hash: str) -> Optional[TransactionReceipt]:
        res = self.call("eth_getTransactionReceipt", [tx_hash])
        if not res:
            return None
        return TransactionReceipt(
            transaction_hash=res.get("transactionHash", tx_hash),
            transaction_index=int(res.get("transactionIndex", "0x0"), 16),
            block_hash=res.get("blockHash", ""),
            block_number=int(res.get("blockNumber", "0x0"), 16),
            from_address=res.get("from", ""),
            to_address=res.get("to"),
            gas_used=int(res.get("gasUsed", "0x0"), 16),
            status=res.get("status", "0x1"),
            logs=res.get("logs", []),
        )

    def wait_for_transaction_receipt(
        self, tx_hash: str, timeout: float = 30.0, poll_interval: float = 1.0
    ) -> TransactionReceipt:
        start = time.time()
        while time.time() - start < timeout:
            receipt = self.get_transaction_receipt(tx_hash)
            if receipt:
                return receipt
            time.sleep(poll_interval)
        raise TimeoutError(f"Timed out waiting for receipt: {tx_hash}")
