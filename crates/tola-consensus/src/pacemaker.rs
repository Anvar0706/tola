//! Consensus Pacemaker coordinating round synchronization, view timeouts, and exponential backoff.
//!
//! Guarantees liveness under partial synchrony by advancing rounds when a leader
//! fails to produce a block or gather a Quorum Certificate within the view duration.

use tola_types::{BlockHeight, Round};

pub const DEFAULT_BASE_TIMEOUT_MS: u64 = 2000; // 2.0s target round cadence
pub const DEFAULT_MAX_TIMEOUT_MS: u64 = 30000; // 30s maximum round backoff cap

#[derive(Clone, Debug)]
pub struct PacemakerConfig {
    pub base_timeout_ms: u64,
    pub max_timeout_ms: u64,
    pub max_backoff_exponent: u32,
}

impl Default for PacemakerConfig {
    fn default() -> Self {
        Self {
            base_timeout_ms: DEFAULT_BASE_TIMEOUT_MS,
            max_timeout_ms: DEFAULT_MAX_TIMEOUT_MS,
            max_backoff_exponent: 4, // 2^4 = 16x multiplier max (32s capped at 30s)
        }
    }
}

pub struct Pacemaker {
    config: PacemakerConfig,
    current_height: BlockHeight,
    current_round: Round,
    round_start_time_ms: u64,
}

impl Pacemaker {
    pub fn new(config: PacemakerConfig, start_height: BlockHeight, start_time_ms: u64) -> Self {
        Self {
            config,
            current_height: start_height,
            current_round: Round(0),
            round_start_time_ms: start_time_ms,
        }
    }

    pub fn current_height(&self) -> BlockHeight {
        self.current_height
    }

    pub fn current_round(&self) -> Round {
        self.current_round
    }

    pub fn round_start_time_ms(&self) -> u64 {
        self.round_start_time_ms
    }

    /// Computes the timeout duration for the current round using exponential backoff:
    /// timeout = min(base * 2^round, max)
    pub fn current_timeout_ms(&self) -> u64 {
        let exp = self.current_round.0.min(self.config.max_backoff_exponent);
        let multiplier = 1u64.checked_shl(exp).unwrap_or(u64::MAX);
        let calculated = self.config.base_timeout_ms.saturating_mul(multiplier);
        calculated.min(self.config.max_timeout_ms)
    }

    /// Evaluates if the current view has expired at `current_time_ms`.
    pub fn is_timed_out(&self, current_time_ms: u64) -> bool {
        let elapsed = current_time_ms.saturating_sub(self.round_start_time_ms);
        elapsed >= self.current_timeout_ms()
    }

    /// Advances the consensus round (view change) upon timeout or failed leader proposal.
    pub fn advance_round(&mut self, current_time_ms: u64) -> Round {
        self.current_round = Round(self.current_round.0.saturating_add(1));
        self.round_start_time_ms = current_time_ms;
        self.current_round
    }

    /// Advances the consensus height to a new block height, resetting the round to 0.
    pub fn advance_height(&mut self, next_height: BlockHeight, current_time_ms: u64) {
        if next_height.0 > self.current_height.0 {
            self.current_height = next_height;
            self.current_round = Round(0);
            self.round_start_time_ms = current_time_ms;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pacemaker_exponential_backoff_and_advance() {
        let config = PacemakerConfig {
            base_timeout_ms: 2000,
            max_timeout_ms: 20000,
            max_backoff_exponent: 4,
        };

        let mut pm = Pacemaker::new(config, BlockHeight(1), 1000);

        // Round 0: 2000ms
        assert_eq!(pm.current_timeout_ms(), 2000);
        assert!(!pm.is_timed_out(2999));
        assert!(pm.is_timed_out(3000));

        // Advance to Round 1: 4000ms
        pm.advance_round(3000);
        assert_eq!(pm.current_round(), Round(1));
        assert_eq!(pm.current_timeout_ms(), 4000);

        // Advance to Round 2: 8000ms
        pm.advance_round(7000);
        assert_eq!(pm.current_round(), Round(2));
        assert_eq!(pm.current_timeout_ms(), 8000);

        // Advance to Round 4: 32000ms capped at 20000ms
        pm.advance_round(15000); // Round 3
        pm.advance_round(25000); // Round 4
        assert_eq!(pm.current_round(), Round(4));
        assert_eq!(pm.current_timeout_ms(), 20000);

        // Advance Height: resets round to 0
        pm.advance_height(BlockHeight(2), 50000);
        assert_eq!(pm.current_height(), BlockHeight(2));
        assert_eq!(pm.current_round(), Round(0));
        assert_eq!(pm.current_timeout_ms(), 2000);
    }
}
