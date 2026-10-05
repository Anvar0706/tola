# SUBSYSTEM: Public Testnet Architecture & Faucet Operations

## Objective
Design the architecture, genesis parameters, faucet infrastructure, validator onboarding, and release lifecycle for the public TOLA Testnet.

## Why It Exists
Before launching Mainnet, the network must undergo rigorous public battle-testing with independent external validators, third-party developers, and stress workloads in an environment where tokens have no financial value.

## Dependencies
- `prompts/14-validator-system.md`
- `prompts/20-native-asset.md`
- `prompts/26-rpc.md`
- `prompts/39-node-deployment.md`

## Non-Goals
- Permanent historical state retention (testnets may be reset after major protocol upgrades).

## Requirements
- Network Identity:
  - Chain ID: `9998` (Testnet).
  - Address Prefix: `ttola1...`.
  - Native Asset: Testnet TOLA (`tTOLA`).
- Testnet Faucet: Automated rate-limited Web / API faucet distributing testnet tokens to developers (e.g. 10 tTOLA per request per 24 hours).
- Public Endpoints: High-availability public RPC nodes and official block explorer.
- Independent Validator Set: Onboard at least 15 independent community validators across diverse geographic locations.

## Architecture
```
Developer / User -> Web Faucet -> Rate Limiter / Captcha -> Faucet Wallet
                 -> Signed Tx -> Testnet RPC -> Public Testnet (15+ Validators)
```

## Interfaces
```rust
pub trait FaucetService {
    fn request_funds(&mut self, recipient: Address, ip_addr: &str) -> Result<TxHash, FaucetError>;
    fn get_daily_limit(&self) -> u128;
}
```

## Data Structures
```rust
pub struct TestnetGenesisConfig {
    pub chain_id: u64,
    pub genesis_time: u64,
    pub initial_validators: Vec<GenesisValidator>,
    pub initial_allocations: Vec<GenesisAllocation>,
}
```

## Security Requirements
- Anti-DDoS faucet protection: IP rate limiting, Proof-of-Work puzzle or hCaptcha, and strict recipient address quotas.
- Network isolation: Absolute firewalling to prevent cross-contamination between testnet and mainnet.

## Failure Modes
- Faucet drained of tokens by automated bots.
- Community validators falling out of sync due to misconfigured firewall ports.

## Testing Requirements
- 30-day continuous stability test on public testnet with zero unplanned hard resets.

## Performance Requirements
- Public RPC response time $< 100\text{ ms}$ under community developer usage.

## Migration Requirements
- Structured migration playbook for resetting testnet state during major breaking releases.

## Acceptance Criteria
- Testnet configuration and faucet architecture documented in `docs/deployment/testnet.md`.
- Testnet genesis specification committed.

## Open Questions
- What anti-bot mechanism offers the best developer experience without excessive friction?

## Implementation Tasks
1. Draft `testnet-genesis.json`.
2. Build open-source faucet backend service in Rust/Node.js.
3. Write official Testnet Validator Onboarding Guide.

## Completion Criteria
Testnet genesis finalized, faucet code prepared, and onboarding documentation published.
