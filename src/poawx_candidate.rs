//! Phase 21D: PoAW-X candidate-set + assignment-proof foundation.
//!
//! This module adds the data + validation foundation for **global role selection**:
//! a canonical candidate set per block, a deterministic *effective score*, and a
//! **VRF-style assignment proof placeholder** (`AssignmentProofV1`).
//!
//! IMPORTANT — `AssignmentProofV1` is a **VRF-style placeholder, NOT a final
//! cryptographic VRF**. The repo ships no VRF library, so the proof is a
//! domain-separated, public-key-bound, hash-based digest that is deterministic and
//! independently *recomputable* by every node. It is NOT unpredictable-before-reveal
//! the way a true VRF output is. Replacing it with a real VRF is future work. No
//! miner private key is ever required to build it.
//!
//! Everything here is integer/fixed-point only (no floats), saturating arithmetic,
//! gated, and **mainnet hard-off** via `crate::activation::network_id_byte() == 0`.
//! It does NOT touch chain difficulty / LWMA-144.
#![allow(dead_code)]

use sha2::{Digest, Sha256};

use crate::activation::network_id_byte;
use crate::poawx_penalty::PenaltyStatus;

/// Domain tag for the assignment-proof digest.
pub const ASSIGNMENT_PROOF_DOMAIN: &[u8] = b"IRIUM_POAWX_ASSIGNMENT_PROOF_V1";
/// Domain tag for the candidate-set root.
pub const CANDIDATE_SET_DOMAIN: &[u8] = b"IRIUM_POAWX_CANDIDATE_SET_V1";
/// 4-byte magic for the trailing candidate-set ext section (fits the existing
/// magic-dispatch loop alongside TPK1/DOM1).
pub const CANDIDATE_SECTION_MAGIC: &[u8; 4] = b"CND1";
/// Wire size of one `RoleCandidate`.
pub const ROLE_CANDIDATE_WIRE: usize = 1 + 20 + 33 + 32 + 1 + 32 + 8 + 8 + 8 + 32; // 175
/// Upper bound on candidates in a set (deserialize safety / size bound).
pub const MAX_CANDIDATES: usize = 256;
/// Fixed-point scale for the effective-score weight product (permille × permille).
pub const EFFECTIVE_SCORE_SCALE: u128 = 1_000_000;

/// Deterministic, domain-separated, public-key-bound assignment-proof digest.
/// VRF-style placeholder (see module docs). No private key required.
pub fn compute_assignment_proof_digest(
    network_id: u8,
    target_height: u64,
    role_id: u8,
    solver_pkh: &[u8; 20],
    assignment_public_key: &[u8; 33],
    ticket_digest: &[u8; 32],
    seed: &[u8; 32],
) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(ASSIGNMENT_PROOF_DOMAIN);
    h.update([network_id]);
    h.update(target_height.to_le_bytes());
    h.update([role_id]);
    h.update(solver_pkh);
    h.update(assignment_public_key);
    h.update(ticket_digest);
    h.update(seed);
    h.finalize().into()
}

/// Derive the (deterministic) assignment score from a proof digest: the first 8
/// digest bytes as a little-endian u64. Higher score wins.
pub fn assignment_score_from_digest(proof_digest: &[u8; 32]) -> u64 {
    let mut b = [0u8; 8];
    b.copy_from_slice(&proof_digest[0..8]);
    u64::from_le_bytes(b)
}

/// Deterministic effective score (HIGHER WINS). Fixed-point: the assignment score
/// scaled by the dominance fairness weight (permille-scale) and the penalty weight
/// (permille). Suspended/slashed (penalty_weight 0) => score 0. Saturating.
pub fn effective_score(assignment_score: u64, dominance_weight: u64, penalty_weight: u64) -> u64 {
    let v = (assignment_score as u128)
        .saturating_mul(dominance_weight as u128)
        .saturating_mul(penalty_weight as u128)
        / EFFECTIVE_SCORE_SCALE;
    v.min(u64::MAX as u128) as u64
}

/// VRF-style assignment proof placeholder (see module docs). Binds network/height/
/// role/solver/assignment-key/ticket/seed; `proof_digest` is recomputable and
/// `score()` is derived from it. NOT a final cryptographic VRF.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssignmentProofV1 {
    pub network_id: u8,
    pub target_height: u64,
    pub role_id: u8,
    pub solver_pkh: [u8; 20],
    pub assignment_public_key: [u8; 33],
    pub ticket_digest: [u8; 32],
    pub seed: [u8; 32],
    pub proof_digest: [u8; 32],
}

impl AssignmentProofV1 {
    pub fn new(
        network_id: u8,
        target_height: u64,
        role_id: u8,
        solver_pkh: [u8; 20],
        assignment_public_key: [u8; 33],
        ticket_digest: [u8; 32],
        seed: [u8; 32],
    ) -> Self {
        let proof_digest = compute_assignment_proof_digest(
            network_id,
            target_height,
            role_id,
            &solver_pkh,
            &assignment_public_key,
            &ticket_digest,
            &seed,
        );
        Self {
            network_id,
            target_height,
            role_id,
            solver_pkh,
            assignment_public_key,
            ticket_digest,
            seed,
            proof_digest,
        }
    }

    pub fn score(&self) -> u64 {
        assignment_score_from_digest(&self.proof_digest)
    }

    /// Validate the proof against the expected binding context. Rejects wrong
    /// network/height/role/ticket/seed and any digest mutation. Mainnet hard-off
    /// is enforced by the caller's gate, not here.
    pub fn validate(
        &self,
        network_id: u8,
        target_height: u64,
        role_id: u8,
        ticket_digest: &[u8; 32],
        seed: &[u8; 32],
    ) -> Result<(), String> {
        if self.network_id != network_id {
            return Err("assignment proof: wrong network".to_string());
        }
        if self.target_height != target_height {
            return Err("assignment proof: wrong height".to_string());
        }
        if self.role_id != role_id {
            return Err("assignment proof: wrong role".to_string());
        }
        if &self.ticket_digest != ticket_digest {
            return Err("assignment proof: wrong ticket digest".to_string());
        }
        if &self.seed != seed {
            return Err("assignment proof: wrong seed".to_string());
        }
        let expect = compute_assignment_proof_digest(
            self.network_id,
            self.target_height,
            self.role_id,
            &self.solver_pkh,
            &self.assignment_public_key,
            &self.ticket_digest,
            &self.seed,
        );
        if expect != self.proof_digest {
            return Err("assignment proof: digest mismatch".to_string());
        }
        Ok(())
    }
}

/// One candidate for a role slot. Carries everything a node needs to recompute its
/// assignment-proof digest, penalty weight, and effective score, and to match it
/// against the selected role solver. `dominance_weight` is validated against the
/// node's persisted state externally (chain.rs) when dominance is active.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleCandidate {
    pub role_id: u8,
    pub solver_pkh: [u8; 20],
    pub assignment_public_key: [u8; 33],
    pub ticket_digest: [u8; 32],
    pub penalty_status: u8,
    pub assignment_proof_digest: [u8; 32],
    pub dominance_weight: u64,
    pub penalty_weight: u64,
    pub effective_score: u64,
    pub role_claim_digest: [u8; 32],
}

impl RoleCandidate {
    /// Build a candidate, computing the proof digest, penalty weight, and effective
    /// score deterministically. `dominance_weight` is supplied by the producer (the
    /// node validates it against persisted state when dominance is active).
    #[allow(clippy::too_many_arguments)]
    pub fn build(
        network_id: u8,
        target_height: u64,
        seed: &[u8; 32],
        role_id: u8,
        solver_pkh: [u8; 20],
        assignment_public_key: [u8; 33],
        ticket_digest: [u8; 32],
        penalty_status: u8,
        dominance_weight: u64,
        role_claim_digest: [u8; 32],
    ) -> Self {
        let assignment_proof_digest = compute_assignment_proof_digest(
            network_id,
            target_height,
            role_id,
            &solver_pkh,
            &assignment_public_key,
            &ticket_digest,
            seed,
        );
        let penalty_weight = PenaltyStatus::from_id(penalty_status)
            .map(|p| p.weight_multiplier_permille() as u64)
            .unwrap_or(0);
        let assignment_score = assignment_score_from_digest(&assignment_proof_digest);
        let effective_score = effective_score(assignment_score, dominance_weight, penalty_weight);
        Self {
            role_id,
            solver_pkh,
            assignment_public_key,
            ticket_digest,
            penalty_status,
            assignment_proof_digest,
            dominance_weight,
            penalty_weight,
            effective_score,
            role_claim_digest,
        }
    }

    pub fn serialize(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(ROLE_CANDIDATE_WIRE);
        out.push(self.role_id);
        out.extend_from_slice(&self.solver_pkh);
        out.extend_from_slice(&self.assignment_public_key);
        out.extend_from_slice(&self.ticket_digest);
        out.push(self.penalty_status);
        out.extend_from_slice(&self.assignment_proof_digest);
        out.extend_from_slice(&self.dominance_weight.to_le_bytes());
        out.extend_from_slice(&self.penalty_weight.to_le_bytes());
        out.extend_from_slice(&self.effective_score.to_le_bytes());
        out.extend_from_slice(&self.role_claim_digest);
        out
    }

    pub fn deserialize(raw: &[u8]) -> Result<Self, String> {
        if raw.len() != ROLE_CANDIDATE_WIRE {
            return Err("role candidate: bad length".to_string());
        }
        let mut p = 0usize;
        let rd = |p: &mut usize, n: usize| -> Vec<u8> {
            let s = raw[*p..*p + n].to_vec();
            *p += n;
            s
        };
        let role_id = raw[p];
        p += 1;
        let mut solver_pkh = [0u8; 20];
        solver_pkh.copy_from_slice(&rd(&mut p, 20));
        let mut assignment_public_key = [0u8; 33];
        assignment_public_key.copy_from_slice(&rd(&mut p, 33));
        let mut ticket_digest = [0u8; 32];
        ticket_digest.copy_from_slice(&rd(&mut p, 32));
        let penalty_status = raw[p];
        p += 1;
        let mut assignment_proof_digest = [0u8; 32];
        assignment_proof_digest.copy_from_slice(&rd(&mut p, 32));
        let mut w = [0u8; 8];
        w.copy_from_slice(&rd(&mut p, 8));
        let dominance_weight = u64::from_le_bytes(w);
        w.copy_from_slice(&rd(&mut p, 8));
        let penalty_weight = u64::from_le_bytes(w);
        w.copy_from_slice(&rd(&mut p, 8));
        let effective_score = u64::from_le_bytes(w);
        let mut role_claim_digest = [0u8; 32];
        role_claim_digest.copy_from_slice(&rd(&mut p, 32));
        Ok(Self {
            role_id,
            solver_pkh,
            assignment_public_key,
            ticket_digest,
            penalty_status,
            assignment_proof_digest,
            dominance_weight,
            penalty_weight,
            effective_score,
            role_claim_digest,
        })
    }

    /// Total canonical sort key: (role_id, solver_pkh, ticket_digest,
    /// assignment_proof_digest). Defines the stable candidate-set ordering.
    fn sort_key(&self) -> ([u8; 1], [u8; 20], [u8; 32], [u8; 32]) {
        (
            [self.role_id],
            self.solver_pkh,
            self.ticket_digest,
            self.assignment_proof_digest,
        )
    }

    /// Self-consistency: recompute the assignment-proof digest, penalty weight, and
    /// effective score from the bound fields and confirm they match the stored
    /// values. Does NOT check dominance_weight vs persisted state (external).
    pub fn validate_self(
        &self,
        network_id: u8,
        target_height: u64,
        seed: &[u8; 32],
    ) -> Result<(), String> {
        let expect_digest = compute_assignment_proof_digest(
            network_id,
            target_height,
            self.role_id,
            &self.solver_pkh,
            &self.assignment_public_key,
            &self.ticket_digest,
            seed,
        );
        if expect_digest != self.assignment_proof_digest {
            return Err("candidate: assignment proof digest mismatch".to_string());
        }
        let ps =
            PenaltyStatus::from_id(self.penalty_status).ok_or("candidate: bad penalty status")?;
        if self.penalty_weight != ps.weight_multiplier_permille() as u64 {
            return Err("candidate: penalty weight mismatch".to_string());
        }
        let score = assignment_score_from_digest(&self.assignment_proof_digest);
        if self.effective_score
            != effective_score(score, self.dominance_weight, self.penalty_weight)
        {
            return Err("candidate: effective score mismatch".to_string());
        }
        Ok(())
    }
}

/// "Is candidate `a` strictly better than `b`?" Higher effective_score wins; ties
/// break by SMALLER assignment_proof_digest, then solver_pkh, then ticket_digest.
fn candidate_better(a: &RoleCandidate, b: &RoleCandidate) -> bool {
    if a.effective_score != b.effective_score {
        return a.effective_score > b.effective_score;
    }
    if a.assignment_proof_digest != b.assignment_proof_digest {
        return a.assignment_proof_digest < b.assignment_proof_digest;
    }
    if a.solver_pkh != b.solver_pkh {
        return a.solver_pkh < b.solver_pkh;
    }
    a.ticket_digest < b.ticket_digest
}

/// Canonical candidate set for a block: header (network/height/seed) + a sorted,
/// duplicate-free candidate list. The root binds all of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateSet {
    pub network_id: u8,
    pub target_height: u64,
    pub seed: [u8; 32],
    pub candidates: Vec<RoleCandidate>,
}

impl CandidateSet {
    pub fn new(network_id: u8, target_height: u64, seed: [u8; 32]) -> Self {
        Self {
            network_id,
            target_height,
            seed,
            candidates: Vec::new(),
        }
    }

    pub fn push(&mut self, c: RoleCandidate) {
        self.candidates.push(c);
    }

    /// Sort into canonical order (stable, total). Call before computing the root.
    pub fn sort_canonical(&mut self) {
        self.candidates
            .sort_by(|a, b| a.sort_key().cmp(&b.sort_key()));
    }

    /// Whether the candidate list is in canonical order with no duplicate keys.
    pub fn is_canonical(&self) -> bool {
        for w in self.candidates.windows(2) {
            if w[0].sort_key() >= w[1].sort_key() {
                return false;
            }
        }
        true
    }

    pub fn serialize(&self) -> Vec<u8> {
        let mut out =
            Vec::with_capacity(1 + 8 + 32 + 2 + self.candidates.len() * ROLE_CANDIDATE_WIRE);
        out.push(self.network_id);
        out.extend_from_slice(&self.target_height.to_le_bytes());
        out.extend_from_slice(&self.seed);
        out.extend_from_slice(&(self.candidates.len() as u16).to_le_bytes());
        for c in &self.candidates {
            out.extend_from_slice(&c.serialize());
        }
        out
    }

    pub fn deserialize(raw: &[u8]) -> Result<Self, String> {
        if raw.len() < 1 + 8 + 32 + 2 {
            return Err("candidate set: truncated header".to_string());
        }
        let network_id = raw[0];
        let mut p = 1usize;
        let mut hb = [0u8; 8];
        hb.copy_from_slice(&raw[p..p + 8]);
        let target_height = u64::from_le_bytes(hb);
        p += 8;
        let mut seed = [0u8; 32];
        seed.copy_from_slice(&raw[p..p + 32]);
        p += 32;
        let count = u16::from_le_bytes([raw[p], raw[p + 1]]) as usize;
        p += 2;
        if count > MAX_CANDIDATES {
            return Err("candidate set: too many candidates".to_string());
        }
        if raw.len() != p + count * ROLE_CANDIDATE_WIRE {
            return Err("candidate set: bad length".to_string());
        }
        let mut candidates = Vec::with_capacity(count);
        for _ in 0..count {
            candidates.push(RoleCandidate::deserialize(
                &raw[p..p + ROLE_CANDIDATE_WIRE],
            )?);
            p += ROLE_CANDIDATE_WIRE;
        }
        Ok(Self {
            network_id,
            target_height,
            seed,
            candidates,
        })
    }

    /// Canonical root over the (canonically serialized) set. Any mutation or
    /// reorder changes the root.
    pub fn root(&self) -> [u8; 32] {
        let mut h = Sha256::new();
        h.update(CANDIDATE_SET_DOMAIN);
        h.update(self.serialize());
        h.finalize().into()
    }

    /// The best candidate for `role_id` under the deterministic ordering, or None.
    pub fn best_for_role(&self, role_id: u8) -> Option<&RoleCandidate> {
        let mut best: Option<&RoleCandidate> = None;
        for c in self.candidates.iter().filter(|c| c.role_id == role_id) {
            match best {
                None => best = Some(c),
                Some(b) if candidate_better(c, b) => best = Some(c),
                _ => {}
            }
        }
        best
    }
}

// ── Gates (param-driven pure logic; mainnet hard-off) ────────────────────────

pub fn candidate_set_activation_height() -> Option<u64> {
    std::env::var("IRIUM_POAWX_CANDIDATE_SET_ACTIVATION_HEIGHT")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
}
pub fn candidate_set_required() -> bool {
    std::env::var("IRIUM_POAWX_CANDIDATE_SET_REQUIRED")
        .map(|v| v.trim() == "1")
        .unwrap_or(false)
}
pub fn assignment_proof_activation_height() -> Option<u64> {
    std::env::var("IRIUM_POAWX_ASSIGNMENT_PROOF_ACTIVATION_HEIGHT")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
}
pub fn assignment_proof_required() -> bool {
    std::env::var("IRIUM_POAWX_ASSIGNMENT_PROOF_REQUIRED")
        .map(|v| v.trim() == "1")
        .unwrap_or(false)
}

/// Pure gate: network 0 (mainnet/unset) hard-off; else active at/after activation.
pub fn poawx_phase21d_gate(network_id: u8, activation: Option<u64>, height: u64) -> bool {
    if network_id == 0 {
        return false;
    }
    matches!(activation, Some(h) if height >= h)
}
/// Pure enforcement gate: active AND required.
pub fn poawx_phase21d_enforced_gate(
    network_id: u8,
    activation: Option<u64>,
    required: bool,
    height: u64,
) -> bool {
    poawx_phase21d_gate(network_id, activation, height) && required
}

pub fn candidate_set_active(height: u64) -> bool {
    poawx_phase21d_gate(network_id_byte(), candidate_set_activation_height(), height)
}
pub fn candidate_set_enforced(height: u64) -> bool {
    poawx_phase21d_enforced_gate(
        network_id_byte(),
        candidate_set_activation_height(),
        candidate_set_required(),
        height,
    )
}
pub fn assignment_proof_active(height: u64) -> bool {
    poawx_phase21d_gate(
        network_id_byte(),
        assignment_proof_activation_height(),
        height,
    )
}
pub fn assignment_proof_enforced(height: u64) -> bool {
    poawx_phase21d_enforced_gate(
        network_id_byte(),
        assignment_proof_activation_height(),
        assignment_proof_required(),
        height,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apk() -> [u8; 33] {
        [0x02u8; 33]
    }

    #[test]
    fn assignment_proof_digest_deterministic_and_sensitive() {
        let d0 = compute_assignment_proof_digest(
            1,
            10,
            1,
            &[0xAAu8; 20],
            &apk(),
            &[0x11u8; 32],
            &[0x22u8; 32],
        );
        let same = compute_assignment_proof_digest(
            1,
            10,
            1,
            &[0xAAu8; 20],
            &apk(),
            &[0x11u8; 32],
            &[0x22u8; 32],
        );
        assert_eq!(d0, same, "deterministic");
        // each input changes the digest.
        assert_ne!(
            d0,
            compute_assignment_proof_digest(
                2,
                10,
                1,
                &[0xAAu8; 20],
                &apk(),
                &[0x11u8; 32],
                &[0x22u8; 32]
            )
        );
        assert_ne!(
            d0,
            compute_assignment_proof_digest(
                1,
                11,
                1,
                &[0xAAu8; 20],
                &apk(),
                &[0x11u8; 32],
                &[0x22u8; 32]
            )
        );
        assert_ne!(
            d0,
            compute_assignment_proof_digest(
                1,
                10,
                2,
                &[0xAAu8; 20],
                &apk(),
                &[0x11u8; 32],
                &[0x22u8; 32]
            )
        );
        assert_ne!(
            d0,
            compute_assignment_proof_digest(
                1,
                10,
                1,
                &[0xABu8; 20],
                &apk(),
                &[0x11u8; 32],
                &[0x22u8; 32]
            )
        );
        assert_ne!(
            d0,
            compute_assignment_proof_digest(
                1,
                10,
                1,
                &[0xAAu8; 20],
                &[0x03u8; 33],
                &[0x11u8; 32],
                &[0x22u8; 32]
            )
        );
        assert_ne!(
            d0,
            compute_assignment_proof_digest(
                1,
                10,
                1,
                &[0xAAu8; 20],
                &apk(),
                &[0x12u8; 32],
                &[0x22u8; 32]
            )
        );
        assert_ne!(
            d0,
            compute_assignment_proof_digest(
                1,
                10,
                1,
                &[0xAAu8; 20],
                &apk(),
                &[0x11u8; 32],
                &[0x23u8; 32]
            )
        );
    }

    #[test]
    fn assignment_proof_validate_accept_reject() {
        let p = AssignmentProofV1::new(1, 10, 1, [0xAAu8; 20], apk(), [0x11u8; 32], [0x22u8; 32]);
        assert!(p.validate(1, 10, 1, &[0x11u8; 32], &[0x22u8; 32]).is_ok());
        assert!(
            p.validate(2, 10, 1, &[0x11u8; 32], &[0x22u8; 32]).is_err(),
            "wrong net"
        );
        assert!(
            p.validate(1, 11, 1, &[0x11u8; 32], &[0x22u8; 32]).is_err(),
            "wrong height"
        );
        assert!(
            p.validate(1, 10, 2, &[0x11u8; 32], &[0x22u8; 32]).is_err(),
            "wrong role"
        );
        assert!(
            p.validate(1, 10, 1, &[0x99u8; 32], &[0x22u8; 32]).is_err(),
            "wrong ticket"
        );
        assert!(
            p.validate(1, 10, 1, &[0x11u8; 32], &[0x99u8; 32]).is_err(),
            "wrong seed"
        );
        // digest mutation rejects.
        let mut m = p.clone();
        m.proof_digest[0] ^= 1;
        assert!(
            m.validate(1, 10, 1, &[0x11u8; 32], &[0x22u8; 32]).is_err(),
            "mutated digest"
        );
        // score derived from digest changes when digest changes.
        assert_ne!(p.score(), assignment_score_from_digest(&m.proof_digest));
    }

    #[test]
    fn effective_score_rules() {
        let s = 1_000_000u64;
        // penalty 0 => score 0.
        assert_eq!(effective_score(s, 1000, 0), 0);
        // higher dominance weight => higher score.
        assert!(effective_score(s, 1000, 1000) > effective_score(s, 500, 1000));
        // full weights => assignment score preserved (1000*1000/1e6 = 1).
        assert_eq!(effective_score(s, 1000, 1000), s);
        // saturating: no panic on large inputs.
        let _ = effective_score(u64::MAX, 1000, 1000);
    }

    #[test]
    fn candidate_wire_roundtrip() {
        let c = RoleCandidate::build(
            1,
            10,
            &[0x22u8; 32],
            1,
            [0xAAu8; 20],
            apk(),
            [0x11u8; 32],
            PenaltyStatus::Clean.id(),
            800,
            [0x44u8; 32],
        );
        let b = c.serialize();
        assert_eq!(b.len(), ROLE_CANDIDATE_WIRE);
        assert_eq!(RoleCandidate::deserialize(&b).unwrap(), c);
        assert!(c.validate_self(1, 10, &[0x22u8; 32]).is_ok());
        // wrong seed/height fail self-consistency (digest recompute).
        assert!(c.validate_self(1, 11, &[0x22u8; 32]).is_err());
        assert!(c.validate_self(1, 10, &[0x23u8; 32]).is_err());
    }

    #[test]
    fn candidate_set_root_and_mutation() {
        let mut cs = CandidateSet::new(1, 10, [0x22u8; 32]);
        let c1 = RoleCandidate::build(
            1,
            10,
            &cs.seed,
            1,
            [0x01u8; 20],
            apk(),
            [0x11u8; 32],
            PenaltyStatus::Clean.id(),
            1000,
            [0x44u8; 32],
        );
        let c2 = RoleCandidate::build(
            1,
            10,
            &cs.seed,
            1,
            [0x02u8; 20],
            apk(),
            [0x12u8; 32],
            PenaltyStatus::Clean.id(),
            1000,
            [0x45u8; 32],
        );
        cs.push(c1.clone());
        cs.push(c2.clone());
        cs.sort_canonical();
        assert!(cs.is_canonical());
        let r = cs.root();
        // round-trip stable.
        let rt = CandidateSet::deserialize(&cs.serialize()).unwrap();
        assert_eq!(rt, cs);
        assert_eq!(rt.root(), r);
        // mutate a candidate weight => different root.
        let mut cs2 = cs.clone();
        cs2.candidates[0].dominance_weight ^= 1;
        assert_ne!(cs2.root(), r, "mutation changes root");
    }

    #[test]
    fn best_for_role_and_tiebreak() {
        let mut cs = CandidateSet::new(1, 10, [0x22u8; 32]);
        // two candidates same role; pick higher effective_score.
        let mut hi = RoleCandidate::build(
            1,
            10,
            &cs.seed,
            1,
            [0x01u8; 20],
            apk(),
            [0x11u8; 32],
            PenaltyStatus::Clean.id(),
            1000,
            [0x44u8; 32],
        );
        let mut lo = RoleCandidate::build(
            1,
            10,
            &cs.seed,
            1,
            [0x02u8; 20],
            apk(),
            [0x12u8; 32],
            PenaltyStatus::Clean.id(),
            1000,
            [0x45u8; 32],
        );
        hi.effective_score = 5000;
        lo.effective_score = 100;
        cs.push(lo.clone());
        cs.push(hi.clone());
        assert_eq!(cs.best_for_role(1).unwrap().solver_pkh, hi.solver_pkh);
        // tie on score => smaller assignment_proof_digest wins.
        let mut a = hi.clone();
        let mut b = hi.clone();
        a.effective_score = 5000;
        b.effective_score = 5000;
        a.assignment_proof_digest = [0x01u8; 32];
        a.solver_pkh = [0xA1u8; 20];
        b.assignment_proof_digest = [0x02u8; 32];
        b.solver_pkh = [0xB2u8; 20];
        let mut cs2 = CandidateSet::new(1, 10, [0x22u8; 32]);
        cs2.push(b);
        cs2.push(a.clone());
        assert_eq!(
            cs2.best_for_role(1).unwrap().assignment_proof_digest,
            a.assignment_proof_digest
        );
    }

    #[test]
    fn gate_logic_pure() {
        assert!(!poawx_phase21d_gate(0, Some(1), 100), "mainnet hard-off");
        assert!(poawx_phase21d_gate(1, Some(1), 100));
        assert!(!poawx_phase21d_gate(1, None, 100));
        assert!(!poawx_phase21d_gate(1, Some(200), 100));
        assert!(poawx_phase21d_enforced_gate(1, Some(1), true, 100));
        assert!(!poawx_phase21d_enforced_gate(1, Some(1), false, 100));
        assert!(
            !poawx_phase21d_enforced_gate(0, Some(1), true, 100),
            "mainnet hard-off"
        );
    }
}
