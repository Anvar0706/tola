# SUBSYSTEM: Fungible Token Standard (TLS-20)

## Objective
Design the canonical fungible token standard for TOLA Network (designated **TLS-20**), providing high performance, seamless EVM tooling compatibility, and first-class messenger wallet support.

## Why It Exists
Tokens represent ecosystem assets, community currencies, and decentralized finance primitives. A rigorous, standardized token interface is required for interoperability across wallets, DEXs, and Mini Apps.

## Dependencies
- `prompts/18-smart-contract-vm.md`
- `prompts/27-sdk.md`

## Non-Goals
- Proprietary non-standard token interfaces that break standard EVM tools.

## Requirements
- Full standard interface compatibility with ERC-20 / EIP-2612:
  - `totalSupply()`, `balanceOf(owner)`, `transfer(to, value)`, `transferFrom(from, to, value)`, `approve(spender, value)`, `allowance(owner, spender)`.
  - Gasless permits via `permit(owner, spender, value, deadline, v, r, s)` to allow frictionless transfers without requiring native TOLA for gas.
- Metadata Extensions: `name()`, `symbol()`, `decimals()`, `iconURI()`.
- Safe Transfer Patterns: Prevention of accidental transfers to zero addresses or uninitialized contract addresses.

## Architecture
```
User / dApp -> TLS-20 Contract -> Balance Mapping & Allowances
            -> Event Logs (Transfer, Approval) -> Wallet / Indexer Feed
```

## Interfaces (Solidity / ABI)
```solidity
interface ITLS20 {
    event Transfer(address indexed from, address indexed to, uint256 value);
    event Approval(address indexed owner, address indexed spender, uint256 value);

    function totalSupply() external view returns (uint256);
    function balanceOf(address account) external view returns (uint256);
    function transfer(address to, uint256 amount) external returns (bool);
    function allowance(address owner, address spender) external view returns (uint256);
    function approve(address spender, uint256 amount) external returns (bool);
    function transferFrom(address from, address to, uint256 amount) external returns (bool);
    function permit(address owner, address spender, uint256 value, uint256 deadline, uint8 v, bytes32 r, bytes32 s) external;
}
```

## Security Requirements
- Re-entrancy protection on all external transfer hooks.
- Strict checks against zero-address recipients to prevent accidental burns.
- Permissive unlimited approval risks clearly flagged in wallet approval modals.

## Failure Modes
- Phishing dApps requesting unlimited token allowances.
- Rounding errors in fractional token transfer logic.

## Testing Requirements
- Full test suite execution using Foundry/Hardhat against OpenZeppelin reference implementations.
- Fuzz testing of permit signatures and replay protections.

## Performance Requirements
- Standard token transfer execution gas $< 50,000$ gas.

## Migration Requirements
- Standard proxy patterns (ERC-1967) for upgradeable token deployments where requested by creators.

## Acceptance Criteria
- Token specification documented in `docs/smart-contracts/tls-20-token-standard.md`.
- Audited reference Solidity implementation committed.

## Open Questions
- Should native multi-token support (similar to Solana SPL or Cosmos Bank) be implemented at the protocol level in later phases?

## Implementation Tasks
1. Author reference TLS-20 smart contract.
2. Implement EIP-2612 permit signature verification.
3. Integrate token discovery into TOLA SDK.

## Completion Criteria
TLS-20 contract suite audited, tested with 100% code coverage, and documented.
