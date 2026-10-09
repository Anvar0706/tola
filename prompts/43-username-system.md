# SUBSYSTEM: External Username Tokenization

## Objective
Specify the tokenization and on-chain ownership model for external platform usernames (e.g. PaxtaGram, Telegram, X, Discord) on TOLA Network.

## Architectural Source of Truth
TOLA Network does **NOT** operate a native username registry, does **NOT** create social identities, and does **NOT** operate a sovereign naming namespace.
All usernames represented on TOLA originate exclusively on **external platforms** and are tokenized via independent third-party tokenization applications onto the TOLA blockchain as TLS-721 tokens (`contracts/TolaExternalAssetToken.sol`) under `ExternalAssetType::MessengerUsername` or `ExternalAssetType::SocialUsername`.

## Why It Exists
Users across social platforms want sovereign on-chain ownership, transferability, and secondary market liquidity for their existing digital platform handles without coupling the blockchain to centralized platform infrastructure.

## Dependencies
- `prompts/18-smart-contract-vm.md`
- `prompts/22-nft-and-assets.md`
- `contracts/TolaExternalAssetToken.sol`

## Non-Goals
- Operating a native on-chain name registration or resolution service (`tola_resolveUsername`).
- Directly integrating with or mutating external platform databases.
- Centralized DNS servers or proprietary domain databases.

## Requirements
- Decoupled Ownership Invariant:
  - On-chain ownership represents cryptographic control of the TLS-721 token on TOLA.
  - The external handle in the messenger/platform database remains governed exclusively by that platform.
- Canonical ID Determinism & Duplicate Prevention:
  - Canonical ID is computed as:
    $$\text{Keccak-256}(\text{"TOLA\_EXTERNAL\_ASSET\_CANONICAL\_V1:"} \parallel \text{platform\_len} \parallel \text{platform} \parallel \text{asset\_type} \parallel \text{id\_len} \parallel \text{id})$$
  - Prevents duplicate mints across different tokenizers.
- Attestation-Based Minting:
  - Independent third-party verifiers sign ECDSA Secp256k1 attestations off-chain.
  - TOLA smart contract verifies the attestation and enforces replay protection (`nonce`, `chainId`, `expiresAt`).

## Architecture
```
[ External Platform DB: @username ]
         │
         ▼
[ Independent 3rd-Party Tokenizer ] ──(signs attestation)──► [ TOLA Wallet ]
                                                                   │
                                                                   ▼
                                                            [ TOLA Blockchain ]
                                                            (TLS-721 Token Mint)
```

## Security Requirements
- Replay defense across chains, nonces, and destination addresses.
- Duplicate mint rejection for already tokenized canonical handles.
- Zero dependency on external platform backend availability or consensus logic.
