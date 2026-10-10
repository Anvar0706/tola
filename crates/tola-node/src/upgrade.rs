//! Protocol Versioning, On-Chain Validator Upgrade Signaling, and Fork-Free Coordination.
//!
//! Complies with on-chain protocol versioning and upgrade specifications.
//!
//! Mechanism:
//! 1. Validators signal readiness by including target `ProtocolVersion` in block proposals.
//! 2. Signaling Accumulator tracks votes across an evaluation window.
//! 3. When threshold (>= 80%) is satisfied, an activation block height is locked.
//! 4. Un-upgraded nodes halt safely on hard forks without disk corruption.

use std::collections::VecDeque;
use tola_types::{Address, BlockHeight};

/// Canonical SemVer protocol version
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProtocolVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl ProtocolVersion {
    pub const V1_0_0: Self = Self {
        major: 1,
        minor: 0,
        patch: 0,
    };

    pub fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    pub fn to_string(&self) -> String {
        format!("v{}.{}.{}", self.major, self.minor, self.patch)
    }

    /// Determines whether two versions are backward-compatible (same major version)
    pub fn is_compatible_with(&self, other: &ProtocolVersion) -> bool {
        self.major == other.major
    }

    /// Determines whether an upgrade from `self` to `other` constitutes a hard fork
    pub fn is_hard_fork_to(&self, other: &ProtocolVersion) -> bool {
        self.major < other.major
    }
}

/// A scheduled protocol upgrade plan
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpgradePlan {
    pub target_version: ProtocolVersion,
    pub activation_height: BlockHeight,
    pub description: String,
    pub is_hard_fork: bool,
}

/// Errors occurring during protocol upgrades and signaling
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpgradeError {
    HardForkHaltRequired {
        current_version: ProtocolVersion,
        required_version: ProtocolVersion,
        activation_height: BlockHeight,
    },
    PrematureActivationCheck,
    InvalidSignalWindow,
}

/// Validator signaling accumulator and activation engine
#[derive(Debug, Clone)]
pub struct UpgradeSignalingManager {
    running_version: ProtocolVersion,
    active_version: ProtocolVersion,
    window_size: u64,
    threshold_bp: u16, // e.g. 8000 = 80.0%
    signals: VecDeque<(BlockHeight, Address, ProtocolVersion)>,
    scheduled_upgrade: Option<UpgradePlan>,
}

impl UpgradeSignalingManager {
    pub fn new(running_version: ProtocolVersion, window_size: u64, threshold_bp: u16) -> Self {
        Self {
            running_version,
            active_version: running_version,
            window_size,
            threshold_bp,
            signals: VecDeque::new(),
            scheduled_upgrade: None,
        }
    }

    pub fn running_version(&self) -> ProtocolVersion {
        self.running_version
    }

    pub fn active_version(&self) -> ProtocolVersion {
        self.active_version
    }

    pub fn scheduled_upgrade(&self) -> Option<&UpgradePlan> {
        self.scheduled_upgrade.as_ref()
    }

    /// Records an upgrade signal proposed by a validator at a specific block height
    pub fn record_signal(
        &mut self,
        height: BlockHeight,
        proposer: Address,
        version: ProtocolVersion,
    ) {
        self.signals.push_back((height, proposer, version));

        // Evict signals older than the sliding window
        while let Some(&(h, _, _)) = self.signals.front() {
            if height.0 >= self.window_size && h.0 < height.0 - self.window_size + 1 {
                self.signals.pop_front();
            } else {
                break;
            }
        }
    }

    /// Evaluates whether the target version has reached the supermajority signaling threshold
    pub fn evaluate_signals(
        &mut self,
        target_version: ProtocolVersion,
        current_height: BlockHeight,
        lead_time_blocks: u64,
        description: String,
    ) -> Option<UpgradePlan> {
        if self.signals.len() < (self.window_size as usize) {
            return None;
        }

        let total_signals = self.signals.len();
        let target_signals = self
            .signals
            .iter()
            .filter(|(_, _, v)| *v == target_version)
            .count();

        // Check if target_signals / total_signals >= threshold_bp / 10,000
        let threshold_count = (total_signals * self.threshold_bp as usize + 9999) / 10_000;

        if target_signals >= threshold_count {
            let activation_height = BlockHeight(current_height.0 + lead_time_blocks);
            let is_hard_fork = self.active_version.is_hard_fork_to(&target_version);
            let plan = UpgradePlan {
                target_version,
                activation_height,
                description,
                is_hard_fork,
            };
            self.scheduled_upgrade = Some(plan.clone());
            Some(plan)
        } else {
            None
        }
    }

    /// Verifies compatibility before processing a block at `height`.
    /// Halts with error if node is incompatible with a hard fork.
    pub fn verify_and_advance(
        &mut self,
        height: BlockHeight,
    ) -> Result<ProtocolVersion, UpgradeError> {
        if let Some(plan) = &self.scheduled_upgrade {
            if height >= plan.activation_height {
                if plan.is_hard_fork
                    && !self
                        .running_version
                        .is_compatible_with(&plan.target_version)
                {
                    return Err(UpgradeError::HardForkHaltRequired {
                        current_version: self.running_version,
                        required_version: plan.target_version,
                        activation_height: plan.activation_height,
                    });
                }
                // Transition protocol version
                self.active_version = plan.target_version;
            }
        }

        Ok(self.active_version)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protocol_version_compatibility() {
        let v1_0_0 = ProtocolVersion::new(1, 0, 0);
        let v1_1_0 = ProtocolVersion::new(1, 1, 0);
        let v2_0_0 = ProtocolVersion::new(2, 0, 0);

        assert!(v1_0_0.is_compatible_with(&v1_1_0));
        assert!(!v1_0_0.is_compatible_with(&v2_0_0));
        assert!(v1_0_0.is_hard_fork_to(&v2_0_0));
    }

    #[test]
    fn test_signaling_threshold_and_activation() {
        let v1 = ProtocolVersion::new(1, 0, 0);
        let v2 = ProtocolVersion::new(2, 0, 0);
        let mut manager = UpgradeSignalingManager::new(v1, 10, 8000); // 10-block window, 80%

        let proposer = Address([1u8; 20]);

        // Propose 3 blocks with v1, 7 blocks with v2 (70% < 80%)
        for h in 1..=3 {
            manager.record_signal(BlockHeight(h), proposer, v1);
        }
        for h in 4..=10 {
            manager.record_signal(BlockHeight(h), proposer, v2);
        }

        let plan = manager.evaluate_signals(v2, BlockHeight(10), 5, "Upgrade to v2".into());
        assert!(plan.is_none());

        // Now propose 1 more block with v2 (height 11).
        // Evicts height 1 (which was v1). Window now has: 2 v1 + 8 v2 = 80%!
        manager.record_signal(BlockHeight(11), proposer, v2);

        let plan = manager.evaluate_signals(v2, BlockHeight(11), 5, "Upgrade to v2".into());
        assert!(plan.is_some());
        let plan = plan.unwrap();
        assert_eq!(plan.activation_height, BlockHeight(16));
        assert!(plan.is_hard_fork);

        // At height 15, still v1
        assert_eq!(manager.verify_and_advance(BlockHeight(15)).unwrap(), v1);

        // At height 16, node is running v1 but v2 hard-fork activates -> Safe Halt!
        assert!(manager.verify_and_advance(BlockHeight(16)).is_err());
    }
}
