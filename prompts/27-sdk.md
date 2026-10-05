# SUBSYSTEM: Developer Client SDKs (TypeScript & Rust)

## Objective
Design and implement the official client software development kits (SDKs) for TOLA Network, providing intuitive, typed libraries for wallet management, transaction signing, RPC communication, and contract interactions.

## Why It Exists
Developers need ergonomic, well-documented, and reliable client libraries to build dApps, mobile applications, bots, and backend integrations.

## Dependencies
- `prompts/03-cryptography.md`
- `prompts/05-addresses.md`
- `prompts/06-transaction-model.md`
- `prompts/26-rpc.md`

## Non-Goals
- Centralized proprietary backend services.

## Requirements
- Primary Language: **TypeScript / JavaScript** (`@tola/sdk` for Node.js, Web, React Native, and Telegram Mini Apps).
- Secondary Language: **Rust** (`tola-sdk` for backend services and high-performance bots).
- Core Modules:
  1. `crypto`: Keypair generation, mnemonic derivation, Ed25519 and Secp256k1 signing.
  2. `address`: Bech32m address formatting, network validation, hex conversions.
  3. `transaction`: Transaction building, gas estimation, serialization, clear-signing preview.
  4. `client`: Typed JSON-RPC / WebSocket client with automatic retry and failover.
  5. `contract`: ABI encoding/decoding, contract deployment, typed method invocation.
  6. `ecosystem`: First-class helpers for usernames, gifts, and TOLA Connect.

## Architecture
```
                  ┌───────────────────────────────┐
                  │          @tola/sdk            │
                  └───────────────┬───────────────┘
                                  │
         ┌────────────────────────┼────────────────────────┐
         │                        │                        │
┌────────▼──────────────┐┌────────▼──────────────┐┌────────▼──────────────┐
│  tola.wallet (Crypto) ││  tola.rpc (API Client)││ tola.connect (Bridge) │
└───────────────────────┘└───────────────────────┘└───────────────────────┘
```

## Interfaces (TypeScript)
```typescript
export interface ITolaClient {
    getBalance(address: string): Promise<bigint>;
    getNonce(address: string): Promise<number>;
    sendTransaction(signedTx: string): Promise<string>;
    waitForTransaction(txHash: string, timeoutMs?: number): Promise<TransactionReceipt>;
}

export interface ITolaWallet {
    readonly address: string;
    signTransaction(tx: UnsignedTransaction): Promise<string>;
    signMessage(message: Uint8Array | string): Promise<string>;
}
```

## Security Requirements
- Zero telemetry or leakage of private keys or mnemonics.
- Strict dependency auditing: Zero unverified third-party cryptographic npm packages.

## Failure Modes
- Mismatched gas estimation causing transaction out-of-gas errors.
- Unhandled network disconnects during transaction polling.

## Testing Requirements
- Unit tests covering key derivation, signing, and serialization against official vectors.
- End-to-end integration tests executing live transactions against local testnet.

## Performance Requirements
- Browser bundle footprint $< 150\text{ KB}$ minified and gzipped.
- Key derivation and signing $< 5\text{ ms}$ on mobile devices.

## Migration Requirements
- Semantic versioning with strict backward compatibility guarantees for dApps.

## Acceptance Criteria
- SDK architecture and API reference documented in `docs/sdk/typescript-sdk.md`.
- Automated test suites passing across Node.js and browser environments.

## Open Questions
- Should Python and Go SDKs be developed concurrently or deferred to Phase 12?

## Implementation Tasks
1. Setup `@tola/sdk` monorepo.
2. Implement cryptographic signing and address encoding modules.
3. Implement JSON-RPC client and contract wrapper.
4. Author comprehensive test suite and usage examples.

## Completion Criteria
TypeScript SDK published to local workspace, tested with 100% vector compliance, and documented.
