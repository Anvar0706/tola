# TOLA Network: Ecosystem Architecture & External Asset Protocols

## 1. Executive Summary & Domain-Neutrality Standard

**TOLA Network** is an independent, sovereign, general-purpose Layer-1 blockchain. TOLA does not prescribe or privilege any specific asset class, platform, application type, or business vertical. Its core mission is to provide high-throughput, deterministic finality, EVM Shanghai execution, and open infrastructure for decentralized ownership, transactions, and verifiable external integrations.

External systems, third-party applications, and sovereign client layers integrate with TOLA via open, platform-neutral protocol standards:

1. **Tokenized External Asset Protocol (TLS-721 / ERC-721):** An open, non-custodial framework enabling third-party platforms to tokenize digital and physical assets into cryptographic representations on TOLA.
2. **TOLA Connect Protocol:** An end-to-end encrypted (E2EE) communication protocol connecting arbitrary web, desktop, mobile, and embedded client applications to user wallets with human-readable clear-signing.

---

## 2. Tokenized External Asset Taxonomy (TLS-721)

TOLA does **not** operate native identity namespaces, closed registries, or centralized platform directories. The TLS-721 standard implements an extensible asset taxonomy supporting arbitrary numeric categories ($1 \le \text{asset\_type} \le 255$):

| Type ID | Canonical Classification | Description | Legacy / Backward Compatibility |
| :---: | :--- | :--- | :--- |
| `1` | `IDENTITY_IDENTIFIER` | External usernames, decentralized IDs, domain names | `MESSENGER_USERNAME` |
| `2` | `VIRTUAL_NUMBER` | Virtual numbers, device identifiers, secure credentials | `MESSENGER_NUMBER` |
| `3` | `GIFT_ASSET` | Promotional items, digital tokens, platform gifts | `MESSENGER_GIFT` |
| `4` | `ATTESTATION_BADGE` | Reputation badges, verified marks, status attestations | `MESSENGER_BADGE` |
| `5` | `SOCIAL_IDENTIFIER` | Social profiles, channel handles, community IDs | `SOCIAL_USERNAME` |
| `6` | `ACCESS_CREDENTIAL` | Security tokens, access passes, membership credentials | `SOCIAL_NUMBER` |
| `7` | `DIGITAL_COLLECTIBLE` | Non-fungible tokens, digital art, creative media | `DIGITAL_COLLECTIBLE` |
| `8` | `OTHER_EXTERNAL_ASSET` | Unclassified external assets | `OTHER_EXTERNAL_ASSET` |
| `9` | `REAL_WORLD_ASSET` (RWA) | Real estate, invoices, physical luxury, commodities | *New Canonical Standard* |
| `10` | `GAMING_ASSET` | In-game equipment, virtual skins, character items | *New Canonical Standard* |
| `11..255` | `CUSTOM(u8)` | Open user-defined third-party asset categories | *Extensible Arbitrary IDs* |

---

## 3. Core vs. External Integration Decoupling

| Dimension | TOLA Core Protocol | External Integrations & 3rd-Party Applications |
| :--- | :--- | :--- |
| **Asset State** | Passive, non-custodial on-chain TLS-721 tokens | Off-chain databases, external platform state |
| **Authentication** | Cryptographic verification of Secp256k1 attestations | OAuth, biometrics, third-party platform logic |
| **Token Ownership** | Controlled exclusively by TOLA private keys | Controlled by external platform rules & accounts |
| **Settlement** | Deterministic 2-second block finality | Asynchronous external synchronization |

---

## 4. Key Specifications

- [Tokenized External Asset Architecture](external-asset-tokenization.md)
- [External Asset Security & Replay Defenses](external-asset-security.md)
- [TLS-721 ERC-721 Reference Specification](tls-721.md)
- [TOLA Connect Protocol Specification](tola-connect-protocol.md)
- [TOLA Connect Threat Model & Security](tola-connect-threat-model.md)
- [Reference Smart Contract](../../contracts/TolaExternalAssetToken.sol)
