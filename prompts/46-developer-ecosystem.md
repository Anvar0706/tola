# SUBSYSTEM: Developer Ecosystem, Tooling & Documentation

## Objective
Design the developer documentation portal, CLI tooling, local sandbox environments, smart contract starter templates, and grant frameworks to foster a thriving TOLA developer ecosystem.

## Why It Exists
A blockchain's long-term survival depends entirely on the developers building upon it. If developing on TOLA is confusing, poorly documented, or frustrating, developers will build on competing networks.

## Dependencies
- `prompts/26-rpc.md`
- `prompts/27-sdk.md`
- `prompts/39-node-deployment.md`

## Non-Goals
- Proprietary closed-source development environments.

## Requirements
- Developer CLI Tool (`tola-cli` / `create-tola-app`):
  - One-command scaffolding for dApps, Mini Apps, smart contracts, and indexers.
  - Instant local private testnet initialization (`tola devnet start`).
- Starter Templates:
  1. `tola-mini-app-starter`: React/Next.js template integrated with TOLA Connect and Telegram webview SDK.
  2. `tola-token-starter`: Foundry/Hardhat template for TLS-20 tokens with gasless permits.
  3. `tola-nft-starter`: TLS-721 digital gift and collectible showcase contract.
- Comprehensive Documentation Portal:
  - Interactive tutorials, RPC references, SDK guides, and architectural diagrams.
  - Hosted cleanly via standard documentation frameworks (Docusaurus/Nextra).

## Architecture
```
Developer Terminal -> `npx create-tola-app` -> Scaffolded dApp / Mini App
                  -> Local Devnet Container -> Instant Hot-Reload & Deploy
                  -> Production Deployment via Vercel / IPFS
```

## Interfaces (CLI)
```bash
# Scaffolding and local testing
npx create-tola-app my-mini-app --template messenger-gift
cd my-mini-app
tola devnet start
tola deploy --network local
```

## Data Structures
```json
{
  "name": "my-tola-app",
  "version": "1.0.0",
  "dependencies": {
    "@tola/sdk": "^1.0.0",
    "@tola/connect": "^1.0.0"
  }
}
```

## Security Requirements
- Audited contract templates: All starter contracts must be pre-audited and strictly follow security best practices.
- Zero tracking or telemetry without explicit developer opt-in.

## Failure Modes
- Outdated documentation leading to compilation errors for new developers.
- Broken local devnet setup frustrating developers during initial onboarding.

## Testing Requirements
- Automated CI testing of all starter templates against the latest node and SDK releases.

## Performance Requirements
- Local devnet initialization time $< 10\text{ seconds}$.

## Migration Requirements
- Versioned documentation ensuring developers on older releases can access relevant API specs.

## Acceptance Criteria
- Developer ecosystem strategy documented in `docs/operations/developer-ecosystem.md`.
- Scaffolding templates and CLI specifications defined.

## Open Questions
- What documentation generator framework provides the best zero-cost static hosting compatibility?

## Implementation Tasks
1. Draft developer onboarding roadmap.
2. Design `create-tola-app` CLI specification.
3. Author starter templates for TLS-20 and Messenger Mini Apps.

## Completion Criteria
Developer documentation blueprint complete, CLI templates specified, and ecosystem roadmap approved.
