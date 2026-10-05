# SUBSYSTEM: Digital Gifts & Social Collectibles

## Objective
Design the digital gift asset model, smart contract contracts, profile badge binding protocols, and secondary marketplace interfaces for TOLA Network.

## Why It Exists
Digital gifts provide vibrant social expression, creator monetization, and community gifting within social messengers and apps. They bridge digital asset ownership with social status.

## Dependencies
- `prompts/18-smart-contract-vm.md`
- `prompts/22-nft-and-assets.md`
- `prompts/42-messenger-integration.md`

## Non-Goals
- Forcing social profiles to custody the underlying blockchain private keys.

## Requirements
- Separation of Ownership and Profile Display:
  - **Ownership**: The gift is an immutable TLS-721 token owned by a TOLA blockchain address.
  - **Social Display**: The owner issues a signed off-chain attestation allowing their messenger profile to showcase the badge.
- Gifting Mechanics:
  - Users can send gifts directly to another user's address or `@username`.
  - Optional custom gift messages and on-chain animations.
- Supply Models:
  - **Limited Editions**: Verifiably capped total supplies (e.g. 500 units worldwide) with serial numbers.
  - **Dynamic Attributes**: Upgradable visual traits based on ecosystem activity.
- Creator Royalties: Native ERC-2981 royalty enforcement on all secondary market sales.

## Architecture
```
     On-Chain Gift Contract (TLS-721)               Off-Chain Messenger Profile
┌────────────────────────────────────────┐         ┌───────────────────────────┐
│ Token ID: #104 (Limited 500)           │         │ Profile: Alice            │
│ Owner: 0x8A4...b1C                     │◄──Proof─┤ Showcase Badge: Gift #104 │
│ Transferable: true                     │         │ Verification: Active      │
└────────────────────────────────────────┘         └───────────────────────────┘
```

## Interfaces (Solidity / ABI)
```solidity
interface ITolaGift is ITLS721 {
    event GiftSent(address indexed from, address indexed to, uint256 indexed giftId, string message);

    function mintLimitedEdition(string calldata giftType, uint256 maxSupply, uint256 price) external;
    function sendGift(address recipient, uint256 giftTypeId, string calldata message) external payable returns (uint256 tokenId);
    function getGiftDetails(uint256 tokenId) external view returns (string memory giftType, uint256 serialNumber, uint256 maxSupply);
}
```

## Data Structures
```solidity
struct GiftMetadata {
    string giftType;
    uint32 serialNumber;
    uint32 maxSupply;
    uint64 mintedTimestamp;
    string metadataURI;
}
```

## Security Requirements
- Unlinkable gifting option: Support sending gifts without exposing sender's main wallet balance.
- Royalties cannot be bypassed by wrapping contracts without violating marketplace standards.

## Failure Modes
- Displaying a gift badge on a social profile after the underlying NFT has been sold (indexer sync immediately revokes badge).

## Testing Requirements
- Unit tests for limited edition mint limits, serial number accuracy, and gift transfers.
- Gas benchmarking for batch gift distribution.

## Performance Requirements
- Gift minting and sending gas $< 100,000$ gas.

## Migration Requirements
- Modular metadata URI management allowing high-fidelity decentralized asset upgrades.

## Acceptance Criteria
- Digital gifts specification documented in `docs/gifts/digital-gifts.md`.
- Reference smart contract and profile binding protocols verified.

## Open Questions
- Should gifts support burn-to-redeem mechanisms for native TOLA rewards?

## Implementation Tasks
1. Author `TolaDigitalGift.sol` smart contract.
2. Implement cryptographic display attestation verification in SDK.
3. Design gift packaging and unboxing metadata format.

## Completion Criteria
Gift contracts tested, audited, and integrated with messenger profile display protocols.
