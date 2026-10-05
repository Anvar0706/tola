# SUBSYSTEM: RPC Engine & Public API Interface

## Objective
Design and implement the high-throughput, standards-compliant JSON-RPC 2.0 and WebSocket API engine for TOLA Network, providing both standard Ethereum compatibility and native TOLA interfaces.

## Why It Exists
External applications, developer tools, block explorers, and wallets interact with the blockchain exclusively through the RPC layer. An unstable or non-standard RPC blocks developer adoption.

## Dependencies
- `prompts/06-transaction-model.md`
- `prompts/09-storage.md`
- `prompts/11-mempool.md`

## Non-Goals
- Direct state mutation without passing through mempool validation.

## Requirements
- Dual API Surface:
  1. **Standard EVM JSON-RPC**: `eth_blockNumber`, `eth_getBalance`, `eth_sendRawTransaction`, `eth_call`, `eth_estimateGas`, `eth_getLogs`, `net_version`, `web3_clientVersion`.
  2. **Native TOLA RPC**: `tola_getNetworkInfo`, `tola_getValidators`, `tola_getShardInfo`, `tola_resolveUsername`, `tola_getGift`.
- WebSocket Event Streams: PubSub subscriptions for new block headers (`newHeads`), pending transactions (`newPendingTransactions`), and contract logs (`logs`).
- Security & Rate Limiting: IP-based token bucket rate limiting, query parameter size bounding (e.g. max 2,000 blocks for log queries), and timeout guards.

## Architecture
```
Client / Wallet -> HTTP/WS Reverse Proxy -> JSON-RPC 2.0 Dispatcher
                -> Rate Limiter & Cache -> State DB / Mempool Query -> Response
```

## Interfaces
```rust
pub trait RpcService {
    fn handle_http_request(&self, request: &str) -> String;
    fn handle_ws_message(&self, client_id: u64, msg: &str);
}
```

## Data Structures
```rust
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: serde_json::Value,
    pub method: String,
    pub params: serde_json::Value,
}

pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: serde_json::Value,
    pub result: Option<serde_json::Value>,
    pub error: Option<JsonRpcError>,
}
```

## Security Requirements
- DoS prevention: Rejection of deeply nested JSON payloads or unbounded regex filters.
- Protection of node resources: Read queries execute against memory snapshots without holding write locks.

## Failure Modes
- Slow queries pinning worker threads (resolved via strict 5-second query timeouts).
- RPC node desynchronization during chain reorganization.

## Testing Requirements
- Comprehensive API conformance suite comparing outputs against standard Ethereum RPC specs.
- Stress testing with 5,000 concurrent WebSocket connections.

## Performance Requirements
- Simple read query latency $< 10\text{ ms}$.
- Throughput $> 10,000$ queries/sec on standard 8-core RPC node.

## Migration Requirements
- Backward-compatible versioning in API paths (`/v1/rpc`).

## Acceptance Criteria
- RPC API specification documented in `docs/rpc/rpc-specification.md`.
- Conformance test suite running and passing.

## Open Questions
- Should gRPC / Protobuf endpoints be supported alongside JSON-RPC for internal indexer pipelines?

## Implementation Tasks
1. Implement JSON-RPC 2.0 parser and router.
2. Author EVM compatibility method handlers (`eth_*`).
3. Author native TOLA method handlers (`tola_*`).
4. Implement WebSocket PubSub subscription engine.

## Completion Criteria
RPC server fully functional, tested against standard EVM tools (Foundry/Ethers.js), and documented.
