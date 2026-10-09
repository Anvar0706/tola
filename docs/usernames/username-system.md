# External Username Tokenization on TOLA Network

> [!IMPORTANT]
> **ARCHITECTURAL SOURCE OF TRUTH**
> TOLA Network does **NOT** operate a native username registry, does **NOT** create social identities, and does **NOT** operate a sovereign naming namespace.
> All usernames represented on TOLA originate exclusively on **external platforms** (e.g. PaxtaGram, Telegram, X, Discord) and are tokenized via independent third-party tokenization applications onto the TOLA blockchain as TLS-721 tokens (`ExternalAssetType::MessengerUsername` or `ExternalAssetType::SocialUsername`).

---

## 1. The Tokenization Pipeline

Usernames on external platforms exist within that platform's proprietary databases and administrative controls. TOLA provides an independent, non-custodial ownership layer:

```
[ External Platform DB: @anvar ]
         │
         ▼
[ Independent 3rd-Party Tokenizer ] ──(signs attestation)──► [ TOLA Wallet ]
                                                                   │
                                                                   ▼
                                                            [ TOLA Blockchain ]
                                                            (TLS-721 Token Mint)
```

### 1.1 Decoupled Ownership Invariant
- **TOLA NFT Ownership:** Represents cryptographic ownership of the on-chain representation on TOLA.
- **External Platform Handle:** Ownership of the original handle remains governed strictly by the external platform's database.
- Selling or transferring the TOLA NFT on secondary markets changes the blockchain token owner, but **never automatically mutates the external platform's database**.

---

## 2. Canonical Identity & Duplicate Mint Protection

To prevent duplicate tokenization of the same external username, TOLA computes a deterministic 32-byte `canonical_id`:

$$\text{Canonical ID} = \text{Keccak-256}(\text{"TOLA\_EXTERNAL\_ASSET\_CANONICAL\_V1:"} \parallel \text{platform\_len} \parallel \text{platform} \parallel \text{asset\_type} \parallel \text{id\_len} \parallel \text{id})$$

Where `asset_type = 1` for `MessengerUsername` and `asset_type = 5` for `SocialUsername`.

Any attempt to mint a token with an already-registered `canonical_id` is rejected by the TOLA node.
