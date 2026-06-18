//! Phase 21A: PoAW-X penalty / fraud state primitives (data-only, gated, mainnet hard-off).
//!
//! Tracks per-miner penalty state so high-trust role assignment (e.g. the
//! finality/SUPPORT role) can be withheld from misbehaving identities. This is a
//! FOUNDATION layer: it is not wired into live block acceptance yet (Phase 21B),
//! and is hard-off on mainnet and disabled unless the activation gate is set.
//! `SlashedPlaceholder` is a placeholder only — no economic slashing is performed.
#![allow(dead_code)]

use crate::activation::network_id_byte;

/// Penalty status for a miner identity / work ticket.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PenaltyStatus {
    Clean = 0,
    Warned = 1,
    TemporarilyReduced = 2,
    SuspendedForEpoch = 3,
    SlashedPlaceholder = 4,
}

impl PenaltyStatus {
    pub fn id(self) -> u8 {
        self as u8
    }
    pub fn from_id(b: u8) -> Option<Self> {
        Some(match b {
            0 => PenaltyStatus::Clean,
            1 => PenaltyStatus::Warned,
            2 => PenaltyStatus::TemporarilyReduced,
            3 => PenaltyStatus::SuspendedForEpoch,
            4 => PenaltyStatus::SlashedPlaceholder,
            _ => return None,
        })
    }

    /// Whether a miner in this state may be assigned a HIGH-TRUST role
    /// (VERIFY/SUPPORT/finality). Clean + Warned + (reduced) are eligible;
    /// Suspended/Slashed are not.
    pub fn eligible_for_high_trust_role(self) -> bool {
        matches!(
            self,
            PenaltyStatus::Clean | PenaltyStatus::Warned | PenaltyStatus::TemporarilyReduced
        )
    }

    /// Fixed-point eligibility weight multiplier in permille (1000 = full).
    /// Deterministic; no floats.
    pub fn weight_multiplier_permille(self) -> u32 {
        match self {
            PenaltyStatus::Clean => 1000,
            PenaltyStatus::Warned => 1000,
            PenaltyStatus::TemporarilyReduced => 500,
            PenaltyStatus::SuspendedForEpoch => 0,
            PenaltyStatus::SlashedPlaceholder => 0,
        }
    }
}

/// Escalation thresholds (configurable; deterministic).
#[derive(Debug, Clone, Copy)]
pub struct PenaltyThresholds {
    pub warn_at: u32,
    pub reduce_at: u32,
    pub suspend_at: u32,
    /// Number of epochs a suspension lasts.
    pub suspend_epochs: u64,
}

impl Default for PenaltyThresholds {
    fn default() -> Self {
        Self {
            warn_at: 1,
            reduce_at: 3,
            suspend_at: 5,
            suspend_epochs: 1,
        }
    }
}

/// Mutable per-miner penalty record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PenaltyRecord {
    pub status: PenaltyStatus,
    pub invalid_count: u32,
    pub valid_count: u32,
    pub suspended_until_epoch: u64,
    pub last_update_height: u64,
}

impl Default for PenaltyRecord {
    fn default() -> Self {
        Self {
            status: PenaltyStatus::Clean,
            invalid_count: 0,
            valid_count: 0,
            suspended_until_epoch: 0,
            last_update_height: 0,
        }
    }
}

impl PenaltyRecord {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record one unit of valid work (saturating).
    pub fn record_valid_work(&mut self, height: u64) {
        self.valid_count = self.valid_count.saturating_add(1);
        self.last_update_height = height;
    }

    /// Record invalid work; escalate the penalty status deterministically.
    pub fn record_invalid_work(&mut self, height: u64, epoch: u64, t: &PenaltyThresholds) {
        self.invalid_count = self.invalid_count.saturating_add(1);
        self.last_update_height = height;
        if self.invalid_count >= t.suspend_at {
            self.status = PenaltyStatus::SuspendedForEpoch;
            self.suspended_until_epoch = epoch.saturating_add(t.suspend_epochs);
        } else if self.invalid_count >= t.reduce_at {
            self.status = PenaltyStatus::TemporarilyReduced;
        } else if self.invalid_count >= t.warn_at {
            self.status = PenaltyStatus::Warned;
        }
    }

    /// Clear an expired suspension back to Warned once the epoch passes.
    pub fn expire_if_due(&mut self, current_epoch: u64) {
        if self.status == PenaltyStatus::SuspendedForEpoch
            && current_epoch >= self.suspended_until_epoch
        {
            self.status = PenaltyStatus::Warned;
        }
    }

    pub fn eligible_for_high_trust_role(&self) -> bool {
        self.status.eligible_for_high_trust_role()
    }
}

/// Activation height for penalty-state enforcement (env-gated; mainnet hard-off).
pub fn penalty_state_activation_height() -> Option<u64> {
    std::env::var("IRIUM_POAWX_PENALTY_STATE_ACTIVATION_HEIGHT")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
}

/// Pure gate logic (network 0 = mainnet hard-off). Kept pure + param-driven so
/// tests need not mutate global env (avoids parallel-test env races).
pub fn penalty_gate(network_id: u8, activation: Option<u64>, height: u64) -> bool {
    if network_id == 0 {
        return false; // mainnet hard-off
    }
    matches!(activation, Some(h) if height >= h)
}

/// Whether penalty-state enforcement is active at `height`. Mainnet hard-off.
pub fn penalty_state_active(height: u64) -> bool {
    penalty_gate(network_id_byte(), penalty_state_activation_height(), height)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn penalty_status_roundtrip_and_eligibility() {
        for s in [
            PenaltyStatus::Clean,
            PenaltyStatus::Warned,
            PenaltyStatus::TemporarilyReduced,
            PenaltyStatus::SuspendedForEpoch,
            PenaltyStatus::SlashedPlaceholder,
        ] {
            assert_eq!(PenaltyStatus::from_id(s.id()), Some(s));
        }
        assert!(PenaltyStatus::from_id(9).is_none());
        // clean + warned eligible; suspended + slashed not.
        assert!(PenaltyStatus::Clean.eligible_for_high_trust_role());
        assert!(PenaltyStatus::Warned.eligible_for_high_trust_role());
        assert!(PenaltyStatus::TemporarilyReduced.eligible_for_high_trust_role());
        assert!(!PenaltyStatus::SuspendedForEpoch.eligible_for_high_trust_role());
        assert!(!PenaltyStatus::SlashedPlaceholder.eligible_for_high_trust_role());
    }

    #[test]
    fn penalty_weight_multipliers_fixed_point() {
        assert_eq!(PenaltyStatus::Clean.weight_multiplier_permille(), 1000);
        assert_eq!(PenaltyStatus::Warned.weight_multiplier_permille(), 1000);
        assert_eq!(
            PenaltyStatus::TemporarilyReduced.weight_multiplier_permille(),
            500
        );
        assert_eq!(
            PenaltyStatus::SuspendedForEpoch.weight_multiplier_permille(),
            0
        );
        assert_eq!(
            PenaltyStatus::SlashedPlaceholder.weight_multiplier_permille(),
            0
        );
    }

    #[test]
    fn penalty_escalation_and_expiry() {
        let t = PenaltyThresholds::default();
        let mut r = PenaltyRecord::new();
        assert!(r.eligible_for_high_trust_role());
        r.record_invalid_work(10, 5, &t); // 1 -> Warned
        assert_eq!(r.status, PenaltyStatus::Warned);
        assert!(r.eligible_for_high_trust_role());
        r.record_invalid_work(11, 5, &t); // 2
        r.record_invalid_work(12, 5, &t); // 3 -> TemporarilyReduced
        assert_eq!(r.status, PenaltyStatus::TemporarilyReduced);
        r.record_invalid_work(13, 5, &t); // 4
        r.record_invalid_work(14, 5, &t); // 5 -> Suspended (epoch 5 + 1 = 6)
        assert_eq!(r.status, PenaltyStatus::SuspendedForEpoch);
        assert_eq!(r.suspended_until_epoch, 6);
        assert!(
            !r.eligible_for_high_trust_role(),
            "suspended blocked for high-trust"
        );
        // not yet expired
        r.expire_if_due(5);
        assert_eq!(r.status, PenaltyStatus::SuspendedForEpoch);
        // expired
        r.expire_if_due(6);
        assert_eq!(r.status, PenaltyStatus::Warned);
        assert!(r.eligible_for_high_trust_role());
    }

    #[test]
    fn penalty_overflow_safe() {
        let t = PenaltyThresholds::default();
        let mut r = PenaltyRecord::new();
        r.invalid_count = u32::MAX;
        r.record_invalid_work(1, 1, &t); // saturating, no panic
        assert_eq!(r.invalid_count, u32::MAX);
        r.valid_count = u32::MAX;
        r.record_valid_work(2);
        assert_eq!(r.valid_count, u32::MAX);
    }

    #[test]
    fn penalty_gate_logic_pure() {
        assert!(!penalty_gate(0, Some(1), 100), "mainnet hard-off");
        assert!(
            penalty_gate(1, Some(1), 100),
            "testnet active at/after height"
        );
        assert!(!penalty_gate(1, None, 100), "no activation -> off");
        assert!(!penalty_gate(1, Some(50), 10), "below activation -> off");
    }
}
