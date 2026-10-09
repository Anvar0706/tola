# External Digital Gifts Tokenization on TOLA Network

> [!IMPORTANT]
> **ARCHITECTURAL SOURCE OF TRUTH**
> TOLA Network does **NOT** create external messenger gifts, does **NOT** create Gifts Numbers, and does **NOT** operate a native gift ecosystem.
> Digital gifts and collectibles represented on TOLA originate on **external platforms** (such as PaxtaGram, Telegram, or other digital platforms) and are tokenized via independent third-party tokenization applications into TLS-721 tokens (`ExternalAssetType::MessengerGift`, `ExternalAssetType::MessengerBadge`, or `ExternalAssetType::DigitalCollectible`).

---

## 1. Architecture & Pipeline

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

### 1.1 Decoupled Ownership Invariant
- **TOLA NFT Ownership:** Represents cryptographic ownership of the on-chain TLS-721 token on TOLA.
- **External Platform Gift:** The original item in the messenger's proprietary database remains governed strictly by the external platform.
- Secondary market sales on TOLA update the blockchain owner, but **do not automatically alter the external messenger's internal database**.

---

## 2. Canonical Identity & Attestation Verification

To guarantee that an external gift cannot be tokenized more than once, TOLA computes a deterministic canonical ID:

$$\text{Canonical ID} = \text{Keccak-256}(\text{"TOLA\_EXTERNAL\_ASSET\_CANONICAL\_V1:"} \parallel \text{platform\_len} \parallel \text{platform} \parallel \text{asset\_type} \parallel \text{id\_len} \parallel \text{id})$$

Where `asset_type = 3` for `MessengerGift`, `asset_type = 4` for `MessengerBadge`, and `asset_type = 7` for `DigitalCollectible`.
Attestations are signed off-chain by authorized verifiers and verified on-chain via ECDSA Secp256k1 before minting.
