# SUBSYSTEM: Decoupled Messenger Integration Architecture

## Objective
Design the integration layer connecting external social messengers (specifically the future TOLA Messenger) to TOLA Network via clean SDK interfaces and the TOLA Connect bridge protocol.

## Why It Exists
Social messengers represent the primary consumer gateway to decentralized payments, digital collectibles, and Mini Apps. This integration must provide first-class UX while maintaining strict protocol decoupling.

## Dependencies
- `prompts/26-rpc.md`
- `prompts/27-sdk.md`
- `prompts/28-wallet.md`
- `prompts/45-identity-and-privacy.md`

## Non-Goals
- Placing messenger chat logs, media files, or contact graphs on the blockchain.
- Granting the messenger privileged admin rights on-chain.

## Requirements
- Decoupling Doctrine: The blockchain functions 100% autonomously without the messenger. If the messenger shuts down, the blockchain is unaffected.
- Client Architecture: The messenger embeds the client-side non-custodial TOLA Wallet and TOLA Connect SDK.
- Mini-App Platform: Sandboxed webview environment allowing third-party Mini Apps to request payments and signatures via clear-signing user approval modals.
- Real-Time Balance Updates: WebSocket subscriptions for instantaneous payment notifications within chat feeds.

## Architecture
```
                  ┌───────────────────────────────┐
                  │     TOLA Layer-1 Network      │
                  └───────────────▲───────────────┘
                                  │ Standard RPC / WS
                  ┌───────────────▼───────────────┐
                  │      TOLA Client SDK          │
                  └───────────────▲───────────────┘
                                  │ E2EE Protocol
                  ┌───────────────▼───────────────┐
                  │         TOLA Connect          │
                  └───────┬───────────────┬───────┘
                          │               │
       ┌──────────────────▼──┐         ┌──▼──────────────────┐
       │   TOLA Messenger    │         │ Standalone Web/App  │
       │   (Ecosystem Client)│         │       dApps         │
       └─────────────────────┘         └─────────────────────┘
```

## Interfaces
```typescript
export interface IMessengerIntegration {
    sendPeerPayment(recipientAddress: string, amount: bigint): Promise<string>;
    bindProfileBadge(giftTokenId: string): Promise<boolean>;
    resolveContactAddress(username: string): Promise<string | null>;
    launchMiniApp(appUrl: string): void;
}
```

## Data Structures
```typescript
export struct ClearSigningPrompt {
    domain: string;
    actionName: string;
    targetAddress: string;
    amount: bigint;
    assetSymbol: string;
    maxFee: bigint;
}
```

## Security Requirements
- Non-custodial guarantee: User private keys never leave the local mobile device; messenger servers possess zero key access.
- Clear signing: The wallet UI must explicitly show the recipient, exact amount, and method before any signature is produced.

## Failure Modes
- Malicious Mini App attempting to phish user funds (neutralized by clear-signing prompts and app domain scanning).
- Network disconnections during in-chat payments (graceful retry with idempotency nonces).

## Testing Requirements
- E2E tests executing payments, badge displays, and Mini App signature flows against local Devnet.

## Performance Requirements
- In-chat payment UI initiation to signature creation $< 200\text{ ms}$.

## Migration Requirements
- Messenger app updates must not alter underlying blockchain account keys.

## Acceptance Criteria
- Integration architecture documented in `docs/messenger-integration.md` and `docs/messenger/messenger-architecture.md`.
- TOLA Connect interaction lifecycle validated.

## Open Questions
- What IPC bridge mechanism provides the fastest communication between mobile webview Mini Apps and the native wallet?

## Implementation Tasks
1. Draft `docs/messenger/messenger-architecture.md`.
2. Define TOLA Connect clear-signing parameter schema.
3. Author reference Mini App sandbox security policy.

## Completion Criteria
Messenger integration specification complete, decoupled boundaries verified, and security policies approved.
