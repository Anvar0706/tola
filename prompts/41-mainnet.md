# SUBSYSTEM: Mainnet Launch Readiness & Genesis Protocol

## Objective
Establish the definitive readiness criteria, security checklists, genesis block ceremony, and operational safeguards required before launching the TOLA Network Mainnet.

## Why It Exists
Mainnet launch is an irreversible event. Once public economic value is secured by the protocol, bugs cannot be fixed with casual resets. Every subsystem must satisfy rigorous definitions of done.

## Dependencies
- `prompts/00-project-foundation.md` through `prompts/40-testnet.md`

## Non-Goals
- Launching Mainnet before completing formal security audits and stability milestones.

## Requirements
- Mainnet Identity:
  - Chain ID: `9999` (Mainnet).
  - Address Prefix: `tola1...`.
  - Native Asset: TOLA.
- Readiness Gates (Definition of Launch):
  1. 100% of subsystem prompts verified as COMPLETE.
  2. Zero open high or critical severity security audit findings.
  3. Minimum 60 days of continuous bug-free operation on public testnet.
  4. Minimum 30 independent geographically dispersed production validators committed.
  5. Multi-sig disaster recovery and emergency pause gates established.
- Genesis Ceremony: Cryptographically verifiable genesis block generation incorporating public community entropy.

## Architecture
```
Subsystem Completion -> External Security Audits -> Testnet Burn-In (60 Days)
                     -> Genesis Ceremony -> Coordinated Validator Launch -> Mainnet Genesis
```

## Interfaces
- Genesis Tool: `tola-genesis-tool generate --config genesis-mainnet.toml`.

## Data Structures
```rust
pub struct MainnetLaunchChecklist {
    pub all_prompts_complete: bool,
    pub audits_cleared: bool,
    pub testnet_burn_in_days: u32,
    pub independent_validators_count: usize,
    pub genesis_hash: [u8; 32],
}
```

## Security Requirements
- Genesis block hash immutability: Hardcoded into node release binary as canonical trust anchor.
- Zero administrative backdoors or protocol god keys.

## Failure Modes
- Network failing to achieve consensus at genesis block due to validator miscoordination.
- Critical bug discovered in the first 24 hours of live trading.

## Testing Requirements
- Full dry-run simulation of the genesis launch ceremony on private shadow networks.

## Performance Requirements
- Seamless launch with 1-2 second block times from block height #1.

## Migration Requirements
- Post-genesis upgrades governed exclusively by on-chain governance and signaling.

## Acceptance Criteria
- Mainnet launch readiness checklist documented in `docs/deployment/mainnet-readiness.md`.
- Genesis specification reviewed and verified by all participating validators.

## Open Questions
- What external public entropy source (e.g. Drand beacon, Bitcoin block hash) should seed the Genesis timestamp?

## Implementation Tasks
1. Compile master Mainnet readiness checklist.
2. Author genesis block generation tool.
3. Draft official Mainnet Genesis Announcement and Validator Coordination Runbook.

## Completion Criteria
All launch criteria verified, genesis tool audited, and launch runbook approved.
