# SUBSYSTEM: Disaster Recovery, Backups & State Rollback

## Objective
Design the node backup protocols, state snapshotting, disaster recovery playbooks, and network emergency coordination procedures for TOLA Network.

## Why It Exists
Hardware failures, catastrophic disk corruptions, datacenter outages, and protocol emergency halts require bulletproof recovery mechanisms to restore validator operation without risking double-signing or data loss.

## Dependencies
- `prompts/09-storage.md`
- `prompts/14-validator-system.md`
- `prompts/16-finality.md`

## Non-Goals
- Manual ad-hoc state edits without cryptographic consensus.

## Requirements
- Point-in-Time Snapshots: Consistent RocksDB database snapshots created without halting node execution.
- Fast State Sync: Enable newly spawned or recovered nodes to sync via recent state snapshots + light client headers without replaying every block from genesis.
- Anti-Double-Signing Guard on Restore:
  - When a validator restores from backup, the signing daemon MUST refuse to sign any block or vote at or below the highest height recorded in the persistent local guard.
  - Fail-safe prevents accidental equivocation when migrating to secondary hardware.
- Disaster Recovery Playbooks: Step-by-step procedures for datacenter migration, disk failure recovery, and chain halt coordination.

## Architecture
```
Live Node State DB -> Live RocksDB Checkpoint -> Tarball / Zstandard Compression
                   -> Encrypted Storage Bucket -> Fast Sync Node Restore
```

## Interfaces
```rust
pub trait DisasterRecoveryService {
    fn create_state_snapshot(&self, target_path: &Path) -> Result<SnapshotMeta, StorageError>;
    fn restore_from_snapshot(&mut self, snapshot_path: &Path) -> Result<(), StorageError>;
    fn verify_signing_guard(&self, target_height: u64) -> Result<(), SafetyError>;
}
```

## Data Structures
```rust
pub struct SnapshotMeta {
    pub height: u64,
    pub block_hash: [u8; 32],
    pub state_root: [u8; 32],
    pub timestamp: u64,
    pub file_size_bytes: u64,
}
```

## Security Requirements
- Anti-Equivocation Invariant: Restoring from a backup must NEVER overwrite or rewind the validator's local anti-double-signing watermark.
- Snapshot authenticity: Snapshots must be cryptographically signed by known validator sets or verified against finalized commit certificates.

## Failure Modes
- Validator restored on secondary machine while primary machine is still running (triggers local guard or remote HSM lock to prevent double-signing).
- Corrupted snapshot tarball causing incomplete state restoration.

## Testing Requirements
- Disaster recovery simulation: Node forcefully destroyed; restored from snapshot on clean VM; verified for correct sync resumption and consensus participation.

## Performance Requirements
- Snapshot creation time $< 30\text{ seconds}$ without stalling active block processing.

## Migration Requirements
- Snapshot formats versioned to ensure backward compatibility across minor releases.

## Acceptance Criteria
- Disaster recovery guide documented in `docs/operations/disaster-recovery.md`.
- Automated snapshot and restore CLI commands implemented.

## Open Questions
- What decentralized storage solution (IPFS/BitTorrent) should be used for public state snapshot distribution?

## Implementation Tasks
1. Implement RocksDB checkpoint API wrapper.
2. Author validator migration safety script with signing lock checks.
3. Write official emergency recovery runbook.

## Completion Criteria
Disaster recovery procedures tested, documented, and integrated into node CLI tooling.
