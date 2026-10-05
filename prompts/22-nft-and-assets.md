# SUBSYSTEM: Non-Fungible Token & Digital Asset Standard (TLS-721)

## Objective
Design the canonical non-fungible token and digital collectible standard for TOLA Network (designated **TLS-721**), providing support for unique digital collectibles, usernames, and profile assets.

## Why It Exists
Digital collectibles, social badges, domain names, and gaming items require unique asset IDs, verifiable provenance, and off-chain metadata integration.

## Dependencies
- `prompts/18-smart-contract-vm.md`
- `prompts/21-token-standard.md`

## Non-Goals
- Storing large media files (images, audio, video) on-chain (metadata points to decentralized IPFS/Arweave or HTTPS URIs).

## Requirements
- Full interface compatibility with ERC-721 / ERC-2981 (Royalties):
  - `balanceOf(owner)`, `ownerOf(tokenId)`, `safeTransferFrom(from, to, tokenId)`, `transferFrom(from, to, tokenId)`, `approve(to, tokenId)`, `setApprovalForAll(operator, approved)`.
  - `tokenURI(tokenId)`: Returns canonical metadata JSON conforming to OpenSea/ERC-721 standard.
  - `royaltyInfo(tokenId, salePrice)`: Native creator royalty enforcement.
- Enumerable extension for easy indexing of tokens owned by a user address.

## Architecture
```
Token ID -> Owner Address -> Token URI (Off-Chain IPFS/HTTPS)
         -> Transfer Event -> Indexer / Messenger Showcase
```

## Interfaces (Solidity / ABI)
```solidity
interface ITLS721 {
    event Transfer(address indexed from, address indexed to, uint256 indexed tokenId);
    event Approval(address indexed owner, address indexed approved, uint256 indexed tokenId);
    event ApprovalForAll(address indexed owner, address indexed operator, bool approved);

    function ownerOf(uint256 tokenId) external view returns (address);
    function safeTransferFrom(address from, address to, uint256 tokenId) external;
    function tokenURI(uint256 tokenId) external view returns (string memory);
    function royaltyInfo(uint256 tokenId, uint256 salePrice) external view returns (address receiver, uint256 royaltyAmount);
}
```

## Security Requirements
- Strict ownership verification before transfer execution.
- Safe transfer checks to prevent tokens from getting trapped in contracts lacking ERC-721 receiver hooks.

## Failure Modes
- Dead metadata links if creator hosts metadata on ephemeral web servers.
- Gas exhaustion when querying large enumerable token collections.

## Testing Requirements
- Unit tests validating transfer semantics, approvals, and royalties.
- Fuzz testing token ID uniqueness and edge cases.

## Performance Requirements
- Standard NFT transfer gas consumption $< 85,000$ gas.

## Migration Requirements
- Metadata URI update governance rules for dynamic collectible assets.

## Acceptance Criteria
- NFT standard specification documented in `docs/smart-contracts/tls-721-nft-standard.md`.
- Reference implementation passing all automated tests.

## Open Questions
- Should we provide a native semi-fungible standard (TLS-1155) simultaneously?

## Implementation Tasks
1. Author reference TLS-721 smart contract.
2. Implement ERC-2981 royalty standard.
3. Integrate NFT rendering utilities into TOLA SDK.

## Completion Criteria
TLS-721 reference contracts fully tested, audited, and documented.
