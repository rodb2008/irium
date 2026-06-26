//! PoAW-X VRF-assigned proposer sortition (Phase 31). Devnet/testnet only;
//! mainnet hard-off (`network_id == 0`). See `docs/poawx-proposer-vrf-design.md`.
//!
//! The chain decides who may propose each height via a VRF lottery on the
//! committee-controlled epoch seed: hashrate gives zero advantage. A backup
//! cascade keyed to the block time keeps the chain live if the primary is offline:
//!   round 0 = top 1 (lowest VRF score), round 1 = top 4 (+3), round 2 = top 14
//!   (+10), round 3+ = all eligible.
//!
//! This module is the PURE math + gate layer (no chain state). The eligibility
//! registry, validator gate, fork-choice rank, and wiring live in `chain.rs` /
//! `bin/iriumd.rs` / `bin/irium-miner.rs`.

use crate::activation::network_id_byte;

/// The PRIMARY proposer role id for the proposer VRF (distinct from the
/// compute/verify/support sub-roles 1/2/3). The block's `worker_pkh` IS the
/// proposer, so the proposer proof is bound to `ROLE_PROPOSER`.
pub const ROLE_PROPOSER: u8 = 0;

/// Default freeze depth (blocks): eligibility for height `H` uses the registry
/// state at `H - FREEZE_DEPTH`, so the seed `S_H` (revealed at H-1) cannot be used
/// to register a favorable key after the fact. Env-tunable; clamped `>= 2`.
pub const DEFAULT_PROPOSER_FREEZE_DEPTH: u64 = 16;

/// Default round interval (seconds) = target block time. Mainnet 120; devnet 30.
pub const DEFAULT_PROPOSER_ROUND_INTERVAL_SECS: u64 = 120;

/// Cumulative admitted proposer-slot count by `round`: round 0 = top 1,
/// round 1 = top 4 (next 3), round 2 = top 14 (next 10), round 3+ = all. Capped at
/// `eligible_count` (>= 1). Realizes the ordered cascade via thresholds.
pub fn cumulative_slots(round: u32, eligible_count: u64) -> u64 {
    let cum: u64 = match round {
        0 => 1,
        1 => 4,
        2 => 14,
        _ => u64::MAX, // round 3+ => all eligible
    };
    cum.min(eligible_count.max(1))
}

/// Proposer-lottery priority from a VRF output: lower = higher priority (closer to
/// slot 1). Reuses the V2 score (first 8 bytes of the VRF output, LE).
pub fn proposer_priority(vrf_output: &[u8; 32]) -> u64 {
    crate::poawx_candidate::assignment_v2_score_from_output(vrf_output)
}

/// Selection threshold at `round` for `eligible_count` registered keys. A miner is
/// admitted iff `proposer_priority < tau`. `tau = (U64_MAX / n) * slots` (saturating);
/// at round 3+ (`slots == n`) `tau == U64_MAX` so ALL eligible are admitted =>
/// liveness. With an empty registry (`n == 0`) treated as `n == 1` => permissive
/// bootstrap (everyone admitted) until keys register.
pub fn proposer_threshold(eligible_count: u64, round: u32) -> u64 {
    let n = eligible_count.max(1);
    let slots = cumulative_slots(round, n);
    if slots >= n {
        return u64::MAX;
    }
    (u64::MAX / n).saturating_mul(slots)
}

/// Whether `priority` is admitted (selected) at `round` for `eligible_count` keys.
pub fn is_selected(priority: u64, eligible_count: u64, round: u32) -> bool {
    priority < proposer_threshold(eligible_count, round)
}

/// Earliest header timestamp allowed for a `round`-r block: `parent_time + r*interval`.
/// The validator rejects a round-r block whose timestamp is earlier (anti round-grind).
pub fn min_time_for_round(parent_time: u32, round: u32, round_interval_secs: u64) -> u32 {
    let add = (round as u64).saturating_mul(round_interval_secs);
    parent_time.saturating_add(add.min(u32::MAX as u64) as u32)
}

/// Highest round a miner may attempt given `elapsed_secs` since the parent block.
/// Round 0 is open immediately; round r opens after `r * interval` seconds.
pub fn max_round_for_elapsed(elapsed_secs: u64, round_interval_secs: u64) -> u32 {
    let iv = round_interval_secs.max(1);
    (elapsed_secs / iv).min(u32::MAX as u64) as u32
}

// ── env-configurable params (devnet/testnet) ─────────────────────────────────

pub fn proposer_freeze_depth() -> u64 {
    std::env::var("IRIUM_POAWX_PROPOSER_FREEZE_DEPTH")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(DEFAULT_PROPOSER_FREEZE_DEPTH)
        .max(2)
}

pub fn proposer_round_interval_secs() -> u64 {
    std::env::var("IRIUM_POAWX_PROPOSER_ROUND_INTERVAL_SECS")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(DEFAULT_PROPOSER_ROUND_INTERVAL_SECS)
        .max(1)
}

// ── activation gate (mainnet hard-off) ───────────────────────────────────────

pub fn proposer_vrf_activation_height() -> Option<u64> {
    std::env::var("IRIUM_POAWX_PROPOSER_VRF_ACTIVATION_HEIGHT")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
}

pub fn proposer_vrf_required() -> bool {
    std::env::var("IRIUM_POAWX_PROPOSER_VRF_REQUIRED")
        .map(|v| v.trim() == "1")
        .unwrap_or(false)
}

/// Pure gate (param-driven for race-free tests). `network_id == 0` (mainnet) hard-off.
pub fn proposer_vrf_gate(network_id: u8, activation: Option<u64>, height: u64) -> bool {
    if network_id == 0 {
        return false;
    }
    matches!(activation, Some(h) if height >= h)
}

pub fn proposer_vrf_active(height: u64) -> bool {
    proposer_vrf_gate(
        network_id_byte(),
        proposer_vrf_activation_height(),
        height,
    )
}

pub fn proposer_vrf_enforced(height: u64) -> bool {
    proposer_vrf_active(height) && proposer_vrf_required()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cumulative_slots_cascade() {
        // counts: round0=1, round1=4, round2=14, round3+=all, capped at n.
        assert_eq!(cumulative_slots(0, 100), 1);
        assert_eq!(cumulative_slots(1, 100), 4);
        assert_eq!(cumulative_slots(2, 100), 14);
        assert_eq!(cumulative_slots(3, 100), 100);
        assert_eq!(cumulative_slots(9, 100), 100);
        // capped at eligible_count
        assert_eq!(cumulative_slots(2, 5), 5);
        assert_eq!(cumulative_slots(0, 5), 1);
        // empty registry treated as 1
        assert_eq!(cumulative_slots(0, 0), 1);
    }

    #[test]
    fn threshold_widens_and_saturates() {
        // n=100: round0 admits ~1/100 of the space, round2 ~14/100, round3 all.
        assert_eq!(proposer_threshold(100, 0), u64::MAX / 100);
        assert_eq!(proposer_threshold(100, 1), (u64::MAX / 100) * 4);
        assert_eq!(proposer_threshold(100, 2), (u64::MAX / 100) * 14);
        assert_eq!(proposer_threshold(100, 3), u64::MAX); // all
        // n=1 (single eligible): always saturates -> that one is always selected.
        assert_eq!(proposer_threshold(1, 0), u64::MAX);
        // n=4 round1: slots(4)==n -> saturates.
        assert_eq!(proposer_threshold(4, 1), u64::MAX);
        // monotonic non-decreasing in round
        let n = 50;
        let mut prev = 0u64;
        for r in 0..6u32 {
            let t = proposer_threshold(n, r);
            assert!(t >= prev, "threshold must be non-decreasing in round");
            prev = t;
        }
        assert_eq!(prev, u64::MAX);
    }

    #[test]
    fn selection_by_priority() {
        // lowest priority (0) always selected at round 0; priority at/above tau not.
        let n = 100;
        let tau0 = proposer_threshold(n, 0);
        assert!(is_selected(0, n, 0));
        assert!(is_selected(tau0 - 1, n, 0));
        assert!(!is_selected(tau0, n, 0)); // strictly < tau
        assert!(!is_selected(u64::MAX, n, 0));
        // a miner not selected at round 0 may be selected at a later (wider) round.
        let p = tau0 + 1; // above round-0 cut
        assert!(!is_selected(p, n, 0));
        assert!(is_selected(p, n, 3)); // round 3 admits all
    }

    #[test]
    fn round_timing() {
        // round r opens at parent + r*interval; validator floor is the same.
        assert_eq!(min_time_for_round(1000, 0, 30), 1000);
        assert_eq!(min_time_for_round(1000, 1, 30), 1030);
        assert_eq!(min_time_for_round(1000, 2, 30), 1060);
        // elapsed -> max allowed round
        assert_eq!(max_round_for_elapsed(0, 30), 0);
        assert_eq!(max_round_for_elapsed(29, 30), 0);
        assert_eq!(max_round_for_elapsed(30, 30), 1);
        assert_eq!(max_round_for_elapsed(95, 30), 3);
    }

    #[test]
    fn gate_mainnet_hard_off() {
        // network 0 (mainnet) is always off, even with activation + height set.
        assert!(!proposer_vrf_gate(0, Some(1), 1_000_000));
        // devnet/testnet: active at/after activation height.
        assert!(proposer_vrf_gate(2, Some(1), 1));
        assert!(proposer_vrf_gate(1, Some(100), 100));
        assert!(!proposer_vrf_gate(2, Some(100), 99));
        assert!(!proposer_vrf_gate(2, None, 1)); // unset => off
    }

    #[test]
    fn priority_from_output_le() {
        let mut out = [0u8; 32];
        out[0..8].copy_from_slice(&7u64.to_le_bytes());
        assert_eq!(proposer_priority(&out), 7);
    }
}
