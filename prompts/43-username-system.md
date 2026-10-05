# SUBSYSTEM: Decentralized Username Registry (@username)

## Objective
Design the decentralized, blockchain-owned username system for TOLA Network, mapping human-readable `@username` identifiers to blockchain addresses and identity records.

## Why It Exists
Hexadecimal and Bech32m addresses are difficult for humans to memorize and verify. A decentralized username registry enables intuitive social payments (`@bob`) and decentralized identity.

## Dependencies
- `prompts/18-smart-contract-vm.md`
- `prompts/22-nft-and-assets.md`
- `prompts/45-identity-and-privacy.md`

## Non-Goals
- Centralized DNS servers or proprietary domain databases.

## Requirements
- Blockchain Ownership: Usernames are represented as unique on-chain digital assets (TLS-721 compatible).
- Registry Features:
  - **Register**: Mint username record via on-chain contract.
  - **Resolve**: Lookup TOLA address corresponding to `@username` in $O(1)$ contract storage read.
  - **Transfer**: Transfer username ownership to another address.
  - **Renew**: Annual renewal fee model to reclaim abandoned usernames.
- Anti-Squatting Rules:
  - Minimum character pricing (e.g. 4-character names cost significantly more than 10-character names).
  - Reserved namespace for critical system and brand identities.
  - Progressive Vickrey auctions for high-demand short names.

## Architecture
```
@username Lookup -> Local Indexer Cache -> TOLA Username Registry Contract
                 -> Verified Owner Address -> Instant In-Chat Transfer
```

## Interfaces (Solidity / ABI)
```solidity
interface ITolaUsernameRegistry {
    event UsernameRegistered(string indexed username, address indexed owner, uint256 expiration);
    event UsernameTransferred(string indexed username, address indexed oldOwner, address indexed newOwner);

    function register(string calldata username, uint256 durationYears) external payable;
    function resolve(string calldata username) external view returns (address owner, address targetAddress);
    function transferName(string calldata username, address newOwner) external;
    function renew(string calldata username, uint256 durationYears) external payable;
}
```

## Data Structures
```solidity
struct UsernameRecord {
    address owner;
    address targetAddress;
    uint64 registeredAt;
    uint64 expiresAt;
    bytes32 metadataHash;
}
```

## Security Requirements
- Homoglyph / Punycode attack prevention: Strict character set validation (lowercase alphanumeric `[a-z0-9_]`) enforced at contract level.
- Front-running prevention: Two-phase commit-reveal registration scheme to prevent MEV bots from stealing requested usernames.

## Failure Modes
- User losing ownership due to unrenewed expiration (grace period required before public re-auction).
- Name spoofing using visually identical Unicode characters.

## Testing Requirements
- Unit tests validating commit-reveal registration, renewals, expirations, and homoglyph rejections.
- Fuzz testing character lengths and edge-case name inputs.

## Performance Requirements
- On-chain username resolution gas $< 30,000$ gas.
- Cached off-chain resolution $< 5\text{ ms}$.

## Migration Requirements
- Registry contract upgradeable via governance without losing existing registered name mappings.

## Acceptance Criteria
- Username specification documented in `docs/usernames/username-system.md`.
- Audited reference smart contract implementation committed.

## Open Questions
- What length grace period (e.g. 30 days vs 90 days) should precede username expiration auctions?

## Implementation Tasks
1. Author reference `TolaUsernameRegistry.sol` smart contract.
2. Implement commit-reveal registration to block front-running.
3. Integrate username resolution into TOLA SDK.

## Completion Criteria
Username smart contracts fully tested, audited, and integrated into SDK resolution library.
