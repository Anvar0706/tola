//! Peer identification, anti-eclipse subnet partitioning, and reputation scoring.
//!
//! Mitigates Sybil attacks and eclipse vulnerabilities by enforcing strict
//! per-subnet IP connection limits and continuous behavior scoring.

use std::collections::{HashMap, HashSet};
use std::net::IpAddr;
use tola_core::NetworkError;

/// 32-byte unique peer cryptographic identity.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct PeerId(pub [u8; 32]);

impl PeerId {
    pub fn random_for_test(seed: u8) -> Self {
        let mut bytes = [0u8; 32];
        bytes[0] = seed;
        PeerId(bytes)
    }
}

/// Coarse IP subnet representation for anti-eclipse connection bucketing.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum IpSubnet {
    /// IPv4 /24 subnet (first 3 octets).
    V4([u8; 3]),
    /// IPv6 /48 subnet (first 6 octets).
    V6([u8; 6]),
}

impl IpSubnet {
    pub fn from_ip(ip: IpAddr) -> Self {
        match ip {
            IpAddr::V4(v4) => {
                let octets = v4.octets();
                IpSubnet::V4([octets[0], octets[1], octets[2]])
            }
            IpAddr::V6(v6) => {
                let octets = v6.octets();
                IpSubnet::V6([
                    octets[0], octets[1], octets[2], octets[3], octets[4], octets[5],
                ])
            }
        }
    }
}

/// Anti-eclipse protection: limits concurrent connections from identical IP subnets.
#[derive(Debug)]
pub struct SubnetLimiter {
    max_per_subnet: usize,
    subnet_counts: HashMap<IpSubnet, usize>,
    peer_subnets: HashMap<PeerId, IpSubnet>,
}

impl SubnetLimiter {
    pub fn new(max_per_subnet: usize) -> Self {
        Self {
            max_per_subnet,
            subnet_counts: HashMap::new(),
            peer_subnets: HashMap::new(),
        }
    }

    /// Attempts to register an incoming or outgoing peer connection.
    pub fn register_peer(&mut self, peer_id: PeerId, ip: IpAddr) -> Result<(), NetworkError> {
        let subnet = IpSubnet::from_ip(ip);
        let count = self.subnet_counts.entry(subnet).or_insert(0);

        if *count >= self.max_per_subnet {
            return Err(NetworkError::SubnetLimitExceeded);
        }

        *count += 1;
        self.peer_subnets.insert(peer_id, subnet);
        Ok(())
    }

    /// Deregisters a disconnected peer and frees its subnet slot.
    pub fn unregister_peer(&mut self, peer_id: &PeerId) {
        if let Some(subnet) = self.peer_subnets.remove(peer_id) {
            if let Some(count) = self.subnet_counts.get_mut(&subnet) {
                *count = count.saturating_sub(1);
                if *count == 0 {
                    self.subnet_counts.remove(&subnet);
                }
            }
        }
    }

    pub fn active_peers_in_subnet(&self, subnet: &IpSubnet) -> usize {
        self.subnet_counts.get(subnet).copied().unwrap_or(0)
    }
}

/// Scoring parameters for GossipSub and peer behavior evaluation.
#[derive(Clone, Debug)]
pub struct PeerScoringParams {
    pub mesh_time_weight: f64,
    pub max_mesh_time_bonus: f64,
    pub first_delivery_weight: f64,
    pub invalid_message_weight: f64,
    pub rate_violation_weight: f64,
    pub disconnect_threshold: f64,
    pub ban_threshold: f64,
    pub decay_factor: f64,
}

impl Default for PeerScoringParams {
    fn default() -> Self {
        Self {
            mesh_time_weight: 0.1,
            max_mesh_time_bonus: 10.0,
            first_delivery_weight: 1.0,
            invalid_message_weight: 50.0, // High penalty for bad blocks/txs
            rate_violation_weight: 20.0,
            disconnect_threshold: -50.0,
            ban_threshold: -100.0,
            decay_factor: 0.90,
        }
    }
}

/// Tracks granular behavior metrics and composite score for a single peer.
#[derive(Clone, Debug)]
pub struct PeerScore {
    pub time_in_mesh_seconds: f64,
    pub first_deliveries: f64,
    pub invalid_messages: f64,
    pub rate_violations: f64,
    pub is_banned: bool,
    pub ban_until: u64,
}

impl PeerScore {
    pub fn new() -> Self {
        Self {
            time_in_mesh_seconds: 0.0,
            first_deliveries: 0.0,
            invalid_messages: 0.0,
            rate_violations: 0.0,
            is_banned: false,
            ban_until: 0,
        }
    }

    /// Computes the net peer reputation score based on configured weights.
    pub fn calculate_score(&self, params: &PeerScoringParams) -> f64 {
        let time_bonus =
            (self.time_in_mesh_seconds * params.mesh_time_weight).min(params.max_mesh_time_bonus);
        let delivery_bonus = self.first_deliveries * params.first_delivery_weight;
        let invalid_penalty = self.invalid_messages * params.invalid_message_weight;
        let rate_penalty = self.rate_violations * params.rate_violation_weight;

        time_bonus + delivery_bonus - invalid_penalty - rate_penalty
    }

    /// Decays delivery and penalty metrics over periodic intervals.
    pub fn apply_decay(&mut self, factor: f64) {
        self.first_deliveries *= factor;
        self.invalid_messages *= factor;
        self.rate_violations *= factor;
    }
}

impl Default for PeerScore {
    fn default() -> Self {
        Self::new()
    }
}

/// Reputation and scoring registry managing all connected and banned peers.
pub struct PeerScoreManager {
    params: PeerScoringParams,
    scores: HashMap<PeerId, PeerScore>,
    banned_peers: HashSet<PeerId>,
}

impl PeerScoreManager {
    pub fn new(params: PeerScoringParams) -> Self {
        Self {
            params,
            scores: HashMap::new(),
            banned_peers: HashSet::new(),
        }
    }

    pub fn is_banned(&self, peer_id: &PeerId, current_time: u64) -> bool {
        if self.banned_peers.contains(peer_id) {
            if let Some(score) = self.scores.get(peer_id) {
                if score.ban_until == 0 || score.ban_until > current_time {
                    return true;
                }
            }
        }
        false
    }

    pub fn record_first_delivery(&mut self, peer_id: &PeerId) {
        let entry = self.scores.entry(*peer_id).or_default();
        entry.first_deliveries += 1.0;
    }

    pub fn record_invalid_message(
        &mut self,
        peer_id: &PeerId,
        current_time: u64,
    ) -> Result<(), NetworkError> {
        let entry = self.scores.entry(*peer_id).or_default();
        entry.invalid_messages += 1.0;
        self.evaluate_peer_state(peer_id, current_time)
    }

    pub fn record_rate_violation(
        &mut self,
        peer_id: &PeerId,
        current_time: u64,
    ) -> Result<(), NetworkError> {
        let entry = self.scores.entry(*peer_id).or_default();
        entry.rate_violations += 1.0;
        self.evaluate_peer_state(peer_id, current_time)
    }

    pub fn update_uptime(&mut self, peer_id: &PeerId, delta_seconds: f64) {
        let entry = self.scores.entry(*peer_id).or_default();
        entry.time_in_mesh_seconds += delta_seconds;
    }

    pub fn apply_periodic_decay(&mut self) {
        for score in self.scores.values_mut() {
            score.apply_decay(self.params.decay_factor);
        }
    }

    pub fn get_score(&self, peer_id: &PeerId) -> f64 {
        if let Some(entry) = self.scores.get(peer_id) {
            entry.calculate_score(&self.params)
        } else {
            0.0
        }
    }

    fn evaluate_peer_state(
        &mut self,
        peer_id: &PeerId,
        current_time: u64,
    ) -> Result<(), NetworkError> {
        if let Some(score) = self.scores.get_mut(peer_id) {
            let current_score = score.calculate_score(&self.params);

            if current_score <= self.params.ban_threshold {
                score.is_banned = true;
                score.ban_until = current_time + 86400; // 24-hour ban
                self.banned_peers.insert(*peer_id);
                return Err(NetworkError::PeerBanned);
            } else if current_score <= self.params.disconnect_threshold {
                return Err(NetworkError::ScoreTooLow);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_subnet_limiter_enforces_anti_eclipse_quota() {
        let mut limiter = SubnetLimiter::new(2); // Max 2 per /24 subnet

        let p1 = PeerId::random_for_test(1);
        let p2 = PeerId::random_for_test(2);
        let p3 = PeerId::random_for_test(3);

        let ip_same1 = "192.168.1.10".parse::<IpAddr>().unwrap();
        let ip_same2 = "192.168.1.20".parse::<IpAddr>().unwrap();
        let ip_same3 = "192.168.1.30".parse::<IpAddr>().unwrap();
        let ip_other = "10.0.0.5".parse::<IpAddr>().unwrap();

        assert!(limiter.register_peer(p1, ip_same1).is_ok());
        assert!(limiter.register_peer(p2, ip_same2).is_ok());

        // 3rd peer in 192.168.1.0/24 exceeds quota
        let err = limiter.register_peer(p3, ip_same3).unwrap_err();
        assert_eq!(err, NetworkError::SubnetLimitExceeded);

        // Different subnet allowed
        assert!(limiter.register_peer(p3, ip_other).is_ok());

        // Disconnect p1 allows another peer in 192.168.1.0/24
        limiter.unregister_peer(&p1);
        let p4 = PeerId::random_for_test(4);
        assert!(limiter.register_peer(p4, ip_same3).is_ok());
    }

    #[test]
    fn test_peer_scoring_penalties_and_automatic_ban() {
        let params = PeerScoringParams::default();
        let mut manager = PeerScoreManager::new(params);
        let peer = PeerId::random_for_test(10);

        // Peer delivers 5 valid messages first
        for _ in 0..5 {
            manager.record_first_delivery(&peer);
        }
        assert_eq!(manager.get_score(&peer), 5.0);

        // Peer sends 1 invalid message (-50 pts -> score -45)
        assert!(manager.record_invalid_message(&peer, 1000).is_ok());
        assert_eq!(manager.get_score(&peer), -45.0);

        // Peer sends a rate violation (-20 pts -> score -65, triggers disconnect <= -50)
        let err = manager.record_rate_violation(&peer, 1000).unwrap_err();
        assert_eq!(err, NetworkError::ScoreTooLow);

        // Another invalid message drops score below -100, triggering ban
        let ban_err = manager.record_invalid_message(&peer, 1000).unwrap_err();
        assert_eq!(ban_err, NetworkError::PeerBanned);
        assert!(manager.is_banned(&peer, 1000));
    }
}
