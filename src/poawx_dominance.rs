//! Phase 21A: PoAW-X anti-domination recent-reward state primitives.
//!
//! Bounded per-miner recent-reward tracker + a deterministic fixed-point fairness
//! weight so a miner that has recently captured a large reward share is
//! down-weighted in future role assignment. Data-only foundation (Phase 21B may
//! wire it into assignment). No floating point; saturating arithmetic; mainnet
//! hard-off. Does NOT touch chain difficulty / LWMA-144.
#![allow(dead_code)]

use std::collections::BTreeMap;

use crate::activation::network_id_byte;

/// Which role-reward bucket a payment belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoleRewardKind {
    Primary,
    Compute,
    Verify,
    Support,
}

/// Per-miner recent reward history (reset per window/epoch).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MinerRewardHistory {
    pub recent_primary: u64,
    pub recent_compute: u64,
    pub recent_verify: u64,
    pub recent_support: u64,
    pub total_recent: u64,
    pub window_id: u64,
    pub last_reward_height: u64,
}

impl MinerRewardHistory {
    /// Record a reward (saturating). A window change resets the buckets first.
    pub fn record(&mut self, kind: RoleRewardKind, amount: u64, height: u64, window_id: u64) {
        if window_id != self.window_id {
            *self = MinerRewardHistory {
                window_id,
                ..Default::default()
            };
        }
        match kind {
            RoleRewardKind::Primary => {
                self.recent_primary = self.recent_primary.saturating_add(amount)
            }
            RoleRewardKind::Compute => {
                self.recent_compute = self.recent_compute.saturating_add(amount)
            }
            RoleRewardKind::Verify => {
                self.recent_verify = self.recent_verify.saturating_add(amount)
            }
            RoleRewardKind::Support => {
                self.recent_support = self.recent_support.saturating_add(amount)
            }
        }
        self.total_recent = self.total_recent.saturating_add(amount);
        self.last_reward_height = height;
    }
}

/// Bounded recent-reward tracker keyed by miner pkh.
#[derive(Debug, Default)]
pub struct DominanceTracker {
    miners: BTreeMap<[u8; 20], MinerRewardHistory>,
    window_id: u64,
    network_recent_total: u64,
}

impl DominanceTracker {
    pub fn new(window_id: u64) -> Self {
        Self {
            miners: BTreeMap::new(),
            window_id,
            network_recent_total: 0,
        }
    }

    /// Advance to a new window: prune all per-miner histories + reset totals.
    pub fn set_window(&mut self, window_id: u64) {
        if window_id != self.window_id {
            self.miners.clear();
            self.network_recent_total = 0;
            self.window_id = window_id;
        }
    }

    pub fn record(&mut self, pkh: [u8; 20], kind: RoleRewardKind, amount: u64, height: u64) {
        let w = self.window_id;
        self.miners
            .entry(pkh)
            .or_default()
            .record(kind, amount, height, w);
        self.network_recent_total = self.network_recent_total.saturating_add(amount);
    }

    pub fn history(&self, pkh: &[u8; 20]) -> Option<&MinerRewardHistory> {
        self.miners.get(pkh)
    }

    pub fn network_recent_total(&self) -> u64 {
        self.network_recent_total
    }

    /// Miner's recent reward share in permille (0..=1000), relative to the network
    /// recent total. 0 when the network total is 0.
    pub fn recent_reward_share_permille(&self, pkh: &[u8; 20]) -> u32 {
        if self.network_recent_total == 0 {
            return 0;
        }
        let mine = self.miners.get(pkh).map(|h| h.total_recent).unwrap_or(0) as u128;
        let share = mine.saturating_mul(1000) / (self.network_recent_total as u128);
        share.min(1000) as u32
    }
}

/// Deterministic fixed-point fairness weight:
/// `weight = valid_work_score * 1000 / (1000 + recent_reward_share_permille)`.
/// A miner with no recent rewards keeps its full `valid_work_score`; a heavily
/// rewarded miner is reduced (e.g. 1000 permille share → halved). Saturating.
pub fn fairness_weight(valid_work_score: u64, recent_reward_share_permille: u32) -> u64 {
    let num = (valid_work_score as u128).saturating_mul(1000);
    let den = 1000u128 + recent_reward_share_permille as u128;
    (num / den) as u64
}

/// Activation height for anti-domination (env-gated; mainnet hard-off).
pub fn anti_domination_activation_height() -> Option<u64> {
    std::env::var("IRIUM_POAWX_ANTI_DOMINATION_ACTIVATION_HEIGHT")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
}

/// Pure gate logic (network 0 = mainnet hard-off); param-driven for race-free tests.
pub fn anti_domination_gate(network_id: u8, activation: Option<u64>, height: u64) -> bool {
    if network_id == 0 {
        return false;
    }
    matches!(activation, Some(h) if height >= h)
}

/// Whether anti-domination weighting is active at `height`. Mainnet hard-off.
pub fn anti_domination_active(height: u64) -> bool {
    anti_domination_gate(
        network_id_byte(),
        anti_domination_activation_height(),
        height,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_recent_reward_keeps_full_weight() {
        let t = DominanceTracker::new(1);
        let pkh = [0x01u8; 20];
        assert_eq!(t.recent_reward_share_permille(&pkh), 0);
        assert_eq!(
            fairness_weight(1000, 0),
            1000,
            "full weight with no recent reward"
        );
    }

    #[test]
    fn high_recent_reward_reduces_weight_deterministic_ordering() {
        let mut t = DominanceTracker::new(1);
        let a = [0xAAu8; 20];
        let b = [0xBBu8; 20];
        // A captures 3/4 of recent rewards, B captures 1/4.
        t.record(a, RoleRewardKind::Primary, 3_000, 10);
        t.record(b, RoleRewardKind::Compute, 1_000, 10);
        let sa = t.recent_reward_share_permille(&a);
        let sb = t.recent_reward_share_permille(&b);
        assert_eq!(sa, 750);
        assert_eq!(sb, 250);
        let wa = fairness_weight(1000, sa); // 1000*1000/1750 = 571
        let wb = fairness_weight(1000, sb); // 1000*1000/1250 = 800
        assert!(wb > wa, "less-rewarded miner keeps more weight");
        assert_eq!(wa, 571);
        assert_eq!(wb, 800);
    }

    #[test]
    fn overflow_safe() {
        let mut t = DominanceTracker::new(1);
        let pkh = [0x09u8; 20];
        t.record(pkh, RoleRewardKind::Primary, u64::MAX, 1);
        t.record(pkh, RoleRewardKind::Compute, u64::MAX, 2); // saturates
        assert_eq!(t.history(&pkh).unwrap().total_recent, u64::MAX);
        assert_eq!(t.recent_reward_share_permille(&pkh), 1000);
        assert_eq!(
            fairness_weight(u64::MAX, 1000),
            (u64::MAX as u128 * 1000 / 2000) as u64
        );
    }

    #[test]
    fn window_reset_prunes() {
        let mut t = DominanceTracker::new(1);
        let pkh = [0x05u8; 20];
        t.record(pkh, RoleRewardKind::Primary, 500, 10);
        assert!(t.history(&pkh).is_some());
        assert_eq!(t.network_recent_total(), 500);
        t.set_window(2);
        assert!(t.history(&pkh).is_none(), "window change prunes histories");
        assert_eq!(t.network_recent_total(), 0);
        assert_eq!(t.recent_reward_share_permille(&pkh), 0);
        // recording under the new window starts fresh.
        t.record(pkh, RoleRewardKind::Verify, 100, 20);
        assert_eq!(t.history(&pkh).unwrap().recent_verify, 100);
    }

    #[test]
    fn phase20_role_rewards_feed_tracker() {
        // Prove a Phase 20 multi-role reward event (RoleReward pkhs + the canonical
        // 55/22/13/10 split) can feed the dominance tracker.
        use crate::poawx::multi_role_amounts;
        let amts = multi_role_amounts(5_000_000_000); // [primary, compute, verify, support]
        let primary = [0x11u8; 20];
        let compute = [0x22u8; 20];
        let verify = [0x33u8; 20];
        let support = [0x44u8; 20];
        let mut t = DominanceTracker::new(7);
        t.record(primary, RoleRewardKind::Primary, amts[0], 2);
        t.record(compute, RoleRewardKind::Compute, amts[1], 2);
        t.record(verify, RoleRewardKind::Verify, amts[2], 2);
        t.record(support, RoleRewardKind::Support, amts[3], 2);
        assert_eq!(t.network_recent_total(), 5_000_000_000);
        // PRIMARY (55%) has the largest recent share.
        assert!(
            t.recent_reward_share_permille(&primary) > t.recent_reward_share_permille(&support)
        );
        assert_eq!(t.recent_reward_share_permille(&primary), 550);
        assert_eq!(t.recent_reward_share_permille(&support), 100);
    }

    #[test]
    fn gate_logic_pure() {
        assert!(!anti_domination_gate(0, Some(1), 100), "mainnet hard-off");
        assert!(anti_domination_gate(1, Some(1), 100));
        assert!(!anti_domination_gate(1, None, 100));
        assert!(!anti_domination_gate(1, Some(50), 10));
    }
}
