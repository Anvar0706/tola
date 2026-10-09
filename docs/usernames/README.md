# TOLA Network: External Username Tokenization Architecture

## Scope & Purpose
Specifies the tokenization pipeline, verification protocol, and on-chain ownership representation for external platform usernames (e.g., PaxtaGram, Telegram, X, Discord).

> [!IMPORTANT]
> **NO NATIVE USERNAME REGISTRY**
> TOLA Network does NOT operate a native username registry, does NOT issue social handles, and does NOT maintain a sovereign identity namespace. All usernames originate on external platforms and are tokenized via independent third parties into TLS-721 tokens (`contracts/TolaExternalAssetToken.sol`).

## Key References & Specifications
- External Username Tokenization Specification: [username-system.md](username-system.md)
- Reference Smart Contract: [contracts/TolaExternalAssetToken.sol](../../contracts/TolaExternalAssetToken.sol)
- Tokenized External Asset Specification: [docs/ecosystem/external-asset-tokenization.md](../ecosystem/external-asset-tokenization.md)
- Subsystem Prompt: [prompts/43-username-system.md](../../prompts/43-username-system.md)
