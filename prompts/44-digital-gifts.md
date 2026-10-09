# SUBSYSTEM: External Digital Gifts & Collectibles Tokenization

## Objective
Specify the tokenization and on-chain ownership model for external digital gifts, collectibles, and badges (e.g. from PaxtaGram, Telegram, or other digital platforms) on TOLA Network.

## Architectural Source of Truth
TOLA Network does **NOT** create external messenger gifts, does **NOT** create Gifts Numbers, and does **NOT** operate a native gift ecosystem or profile badge registry.
Digital gifts and collectibles represented on TOLA originate exclusively on **external platforms** and are tokenized via independent third-party tokenization applications into TLS-721 tokens (`contracts/TolaExternalAssetToken.sol`) under `ExternalAssetType::MessengerGift`, `ExternalAssetType::MessengerBadge`, or `ExternalAssetType::DigitalCollectible`.

## Why It Exists
Users across social messengers and creator platforms wish to hold verifiable blockchain ownership and open-market transferability of their external digital gifts without coupling the TOLA blockchain to the messenger platform's internal servers.

## Dependencies
- `prompts/18-smart-contract-vm.md`
- `prompts/22-nft-and-assets.md`
- `contracts/TolaExternalAssetToken.sol`

## Non-Goals
- Native on-chain gift minting or native gift number generation (`tola_getGiftDetails`, `tola_verifyBadgeAttestation`).
- Direct integration with or mutation of external platform gift inventories.

## Requirements
- Decoupled Ownership Invariant:
  - TOLA TLS-721 token ownership represents blockchain ownership on TOLA.
  - The original gift record inside the messenger's proprietary database remains governed strictly by the external platform.
  - Secondary market transfers on TOLA do not automatically mutate the external messenger database.
- Canonical ID Determinism & Duplicate Prevention:
  - Canonical ID is computed deterministically from platform, asset type, and external asset ID.
  - Prevents double-tokenization of the same external gift.
- Attestation-Based Minting:
  - Authorized verifiers sign off-chain ECDSA Secp256k1 attestations.
  - TOLA smart contract enforces cryptographic verification and single-use replay protection.

## Architecture
```
[ External Platform DB: Gift #987 ]
         │
         ▼
[ Independent 3rd-Party Tokenizer ] ──(signs attestation)──► [ TOLA Wallet ]
                                                                   │
                                                                   ▼
                                                            [ TOLA Blockchain ]
                                                            (TLS-721 Token Mint)
```

## Security Requirements
- Replay defense preventing reuse of mint attestations across nonces or accounts.
- Cryptographic attestation verification with expiry checks.
- Zero dependency on external messenger backend availability or consensus logic.
