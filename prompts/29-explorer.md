# SUBSYSTEM: Block Explorer & Indexing Architecture

## Objective
Design the decentralized block explorer, high-performance data indexer, relational query engine, and web interface architecture for TOLA Network.

## Why It Exists
Users, developers, and auditors require full visibility into block history, transaction lifecycles, token balances, contract interactions, and validator metrics. The indexer provides sub-second query performance without burdening consensus nodes.

## Dependencies
- `prompts/10-block-engine.md`
- `prompts/26-rpc.md`

## Non-Goals
- Running the indexer within the consensus node process (indexer is an external read-replica consumer).

## Requirements
- Decoupled Pipeline: Node emits block/receipt events -> Indexer ingests and parses -> Relational DB (PostgreSQL / ClickHouse) -> Explorer GraphQL/REST API -> Web UI.
- Comprehensive Indexing:
  - Blocks, transactions, internal contract calls, event logs.
  - TLS-20 token transfers and holder balances.
  - TLS-721 NFT mints, transfers, and metadata caching.
  - Validator performance, uptime, missed blocks, and slashing events.
- Search Functionality: Sub-second search across block heights, block hashes, transaction hashes, addresses, token symbols, and usernames.

## Architecture
```
TOLA Validator Node (RPC / Event Stream)
              │
      ┌───────▼───────┐
      │ TOLA Indexer  │ (Rust / Go Daemon)
      └───────┬───────┘
              │ Batch Write
      ┌───────▼───────┐
      │ PostgreSQL /  │
      │ ClickHouse DB │
      └───────┬───────┘
              │ GraphQL / REST
      ┌───────▼───────┐
      │ Explorer API  │
      └───────┬───────┘
              │
      ┌───────▼───────┐
      │ Explorer Web  │ (Next.js / Vercel-hosted)
      └───────────────┘
```

## Interfaces
```rust
pub trait BlockchainIndexer {
    fn index_block(&mut self, block: &Block, receipts: &[ExecutionReceipt]) -> Result<(), IndexerError>;
    fn reorg_to_height(&mut self, height: u64) -> Result<(), IndexerError>;
}
```

## Data Structures
```rust
pub struct IndexedTransaction {
    pub hash: [u8; 32],
    pub block_height: u64,
    pub from_address: Address,
    pub to_address: Option<Address>,
    pub value: u128,
    pub gas_used: u64,
    pub status: bool,
    pub timestamp: u64,
}
```

## Security Requirements
- Read-only isolation: The explorer and indexer have zero write access to validator nodes.
- Reorg resilience: Indexer must handle block rollbacks gracefully prior to BFT finality depth.

## Failure Modes
- Indexer lag during high-throughput block bursts (mitigated by bulk write pipelining).
- DB storage exhaustion on massive historical log collections.

## Testing Requirements
- Sync verification tests checking indexer state accuracy against live blockchain state.
- Query performance load testing under 1,000 requests/sec.

## Performance Requirements
- Query response time $< 50\text{ ms}$ for 95% of API requests.

## Migration Requirements
- Automated database schema migrations (using Flyway or Diesel migrations).

## Acceptance Criteria
- Explorer and indexer architecture documented in `docs/architecture/explorer.md`.
- Database relational schema defined and committed.

## Open Questions
- Is ClickHouse preferred over PostgreSQL for long-term historical transaction log analysis?

## Implementation Tasks
1. Design relational database schemas for blocks, transactions, logs, and tokens.
2. Specify event ingestion daemon architecture.
3. Design Explorer frontend UI wireframes and API endpoints.

## Completion Criteria
Explorer architecture specification complete, relational schemas reviewed, and API schema published.
