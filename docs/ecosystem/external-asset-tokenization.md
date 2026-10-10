# External Asset Tokenization Architecture & Implementation

## 1. Architectural Overview

The TOLA External Asset Tokenization Architecture enables independent third-party tokenization platforms to bridge external identity identifiers, virtual credentials, digital collectibles, real-world assets (RWA), gaming items, and custom external assets into TOLA's TLS-721 token ecosystem.

Under this model:
- The **TOLA blockchain** remains entirely passive, sovereign, and agnostic to external service operations.
- The **Independent Tokenization Service** authenticates external platform users or real-world authorities off-chain and issues cryptographically signed attestations.
- The **TOLA TLS-721 Smart Contract / Node Runtime** verifies these attestations deterministically and mints on-chain representations.

---

## 2. Independent Tokenization Service (Fragment-like Model)

The tokenization workflow decouples platform governance from blockchain settlement:

```
┌─────────────────┐       1. OAuth / Verify       ┌────────────────────────┐
│  User / Client  │ ────────────────────────────► │   External Platform    │
│ (PaxtaGram etc.)│ ◄──────────────────────────── │      (PaxtaGram)       │
└────────┬────────┘      2. Proves Ownership      └────────────────────────┘
         │
         │ 3. Request Attestation
         ▼
┌─────────────────────────────────┐
│ Independent Tokenization Service│
│ (Fragment-like Entity)          │
│ • Maintains Verifier Secp256k1  │
│ • Verifies off-chain ownership  │
│ • Enforces rate limits & KYC    │
└────────┬────────────────────────┘
         │
         │ 4. Issue Signed ExternalAssetAttestation
         ▼
┌─────────────────┐       5. Submit Mint TX       ┌────────────────────────┐
│   TOLA Wallet   │ ────────────────────────────► │     TOLA Blockchain    │
│  (Alice / Bob)  │ ◄──────────────────────────── │ • Verify Attestation   │
└─────────────────┘      6. TLS-721 Minted        │ • Prevent Duplicate ID │
                                                  │ • Track On-chain Owner │
                                                  └────────────────────────┘
```

---

## 3. Cryptographic Attestation Specification

To prevent forgery, replay, front-running, and front-chain hijacking, attestations are bound to a strict 32-byte Keccak-256 digest:

$$\text{Digest} = \text{Keccak-256}\Big(\text{"TOLA\_EXTERNAL\_ASSET\_ATTESTATION\_V1:"} \parallel \text{chain\_id}_{8B} \parallel \text{nonce}_{8B} \parallel \text{dest}_{20B} \parallel \text{expiry}_{8B} \parallel \text{plat\_len}_{4B} \parallel \text{platform} \parallel \text{type}_{1B} \parallel \text{id\_len}_{4B} \parallel \text{id}\Big)$$

### Attestation JSON Schema (`ExternalAssetAttestation`):
```json
{
  "chainId": 102,
  "nonce": 42,
  "destinationWallet": "0x7777777777777777777777777777777777777777",
  "expiresAt": 1775000000,
  "sourcePlatform": "paxtagram",
  "assetType": "MESSENGER_USERNAME",
  "externalAssetId": "anvar",
  "verifierAddress": "0x8888888888888888888888888888888888888888",
  "signature": {
    "r": "0x...",
    "s": "0x...",
    "v": 27
  }
}
```

---

## 4. Canonical ID Derivation & Duplicate Mint Prevention

To guarantee that a given external platform asset can only be tokenized once across the entire network, every tokenized external asset derives a unique 32-byte **Canonical ID**:

$$\text{Canonical ID} = \text{Keccak-256}\Big(\text{"TOLA\_EXTERNAL\_ASSET\_CANONICAL\_V1:"} \parallel \text{plat\_len}_{4B} \parallel \text{platform} \parallel \text{type}_{1B} \parallel \text{id\_len}_{4B} \parallel \text{id}\Big)$$

### Duplicate Prevention Rules:
1. **Contract Mapping:** Smart contracts maintain `mapping(bytes32 => uint256) public canonicalIdToToken`.
2. **First-Mint Locks:** If `canonicalIdToToken[canonicalId] != 0`, subsequent mint calls revert immediately with `AssetAlreadyTokenized`.
3. **Deterministic Identity:** The canonical ID is independent of the recipient wallet address, preventing multiple parties from minting claims to the same handle.

---

## 5. Smart Contract Reference Implementation

A reference smart contract [`contracts/TolaExternalAssetToken.sol`](../../contracts/TolaExternalAssetToken.sol) defines the complete TLS-721 tokenization implementation:
- Inherits TLS-721 NFT semantics with standard `ownerOf`, `transfer`, `approve`, and `transferFrom`.
- Implements `mintWithAttestation(ExternalAssetAttestation calldata attestation, string calldata metadataUri)`.
- Validates cryptographic authorization proof and records authorizer key on-chain for provenance.
- Enforces strict Low-S signature checks and replay defenses using `usedAttestations[attestationHash]`.

---

## 6. JSON-RPC Protocol Endpoints

The TOLA node exposes 4 dedicated RPC endpoints for querying and verifying tokenized external assets:

### 1. `tola_getExternalAsset`
- **Params:** `[tokenId: number | canonicalId: string]`
- **Returns:** Complete external asset record including `tokenId`, `canonicalId`, `sourcePlatform`, `assetType`, `externalAssetId`, `owner`, `verifierAddress`, `metadataUri`, and `mintTimestamp`.

### 2. `tola_getExternalAssetOwner`
- **Params:** `[tokenId: number]`
- **Returns:** Current 20-byte TOLA owner address in hex format (`"0x..."`).

### 3. `tola_getExternalAssetMetadata`
- **Params:** `[tokenId: number]`
- **Returns:** Metadata object containing `tokenId`, `metadataUri`, `assetType`, and `externalAssetId`.

### 4. `tola_verifyExternalAssetAttestation`
- **Params:** `[attestationObject: object]`
- **Returns:** Verification report:
  ```json
  {
    "valid": true,
    "canonicalId": "0xe51c65e1f87b606cb656a4a931e3f4319af4880be892bea1ac5335026265f9f0",
    "sourcePlatform": "paxtagram",
    "assetType": "MESSENGER_USERNAME",
    "externalAssetId": "anvar",
    "destinationWallet": "0x7777777777777777777777777777777777777777",
    "verifierAddress": "0x8888888888888888888888888888888888888888",
    "alreadyMintedTokenId": null
  }
  ```

---

## 7. Multi-Language SDK Lifecycle

### TypeScript SDK (`@tola/sdk`):
```typescript
import {
  ExternalAssetType,
  computeExternalAssetCanonicalId,
  signExternalAssetAttestation,
  verifyExternalAssetAttestation,
  validateExternalAssetMetadata
} from '@tola/sdk';

// Compute canonical identity
const canonicalId = computeExternalAssetCanonicalId("paxtagram", ExternalAssetType.MessengerUsername, "anvar");

// Sign attestation
const attestation = signExternalAssetAttestation(verifierWallet, {
  chainId: 102n,
  nonce: 1n,
  destinationWallet: aliceAddress,
  expiresAt: BigInt(Date.now() / 1000 + 3600),
  sourcePlatform: "paxtagram",
  assetType: ExternalAssetType.MessengerUsername,
  externalAssetId: "anvar"
});

// Verify attestation
const isValid = verifyExternalAssetAttestation(attestation, verifierWallet.getAddress());
```

### Python SDK (`tola`):
```python
from tola.external import (
    ExternalAssetType,
    compute_external_asset_canonical_id,
    sign_external_asset_attestation,
    verify_external_asset_attestation,
    validate_external_asset_metadata
)

# Canonical ID
cid = compute_external_asset_canonical_id("paxtagram", ExternalAssetType.MESSENGER_USERNAME, "anvar")

# Sign attestation
att = sign_external_asset_attestation(
    verifier_wallet=verifier,
    chain_id=102,
    nonce=1,
    destination_wallet=alice_addr,
    expires_at=future_ts,
    source_platform="paxtagram",
    asset_type=ExternalAssetType.MESSENGER_USERNAME,
    external_asset_id="anvar"
)

# Verify
assert verify_external_asset_attestation(att, verifier.address) is True
```
