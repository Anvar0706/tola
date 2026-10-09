//! Pipelined BFT consensus state machine, pacemaker, threshold QC aggregation, and 2-chain commit finality.
//!
//! Complies with ADR-0009, Jolteon/HotStuff pipelined architecture, and Byzantine fault tolerance ($3f + 1$).

#![allow(clippy::redundant_closure, clippy::unnecessary_sort_by)]

pub mod committee;
pub mod engine;
pub mod governance;
pub mod pacemaker;
pub mod qc;
pub mod registry;
pub mod sharding;
pub mod slashing;

pub use committee::{is_quorum, quorum_threshold, CommitteeManager};
pub use engine::PipelinedBftEngine;
pub use governance::*;
pub use pacemaker::{Pacemaker, PacemakerConfig, DEFAULT_BASE_TIMEOUT_MS, DEFAULT_MAX_TIMEOUT_MS};
pub use qc::{compute_vote_hash, verify_qc, VoteAggregator, VoteTarget};
pub use registry::{
    RegistryConfig, ValidatorRecord, ValidatorRegistry, DEFAULT_JAILING_BLOCKS,
    DEFAULT_MAX_ACTIVE_VALIDATORS, DEFAULT_MIN_SELF_STAKE, DEFAULT_UNBONDING_BLOCKS,
};
pub use sharding::{CoordinationEngine, CrossShardRouter, InterRollupBridgeVerifier, ShardError};
pub use slashing::{SlashingEngine, SlashingEngineConfig, DEFAULT_EQUIVOCATION_PENALTY_BP};
