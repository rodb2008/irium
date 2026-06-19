//! Phase 21E: mandatory PoAW-X candidate admission + gossip cache.
//!
//! Closes the Phase 21D gap from "best within the INCLUDED candidate set" toward
//! "best among the candidates ADMITTED to this node during the deterministic
//! admission window". A `CandidateAdmissionV1` is one canonical candidate bound to
//! a `(network, height, role, seed)` context; nodes gossip admissions, cache them
//! per `(target_height, role)`, and (when enforced) require a block's candidate set
//! to EQUAL the admitted set for that height/seed.
//!
//! HONEST LIMITATION: this proves "best among candidates admitted to THIS node in
//! the window", NOT "best among all unknowable offline/never-gossiped miners".
//! Equality against the local cache is propagation-sensitive and is testnet/devnet
//! only; public-network admission-window tuning is future work. Mainnet hard-off.
//!
//! Integer/fixed-point only; no floats; no LWMA/PoW interaction.
#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use sha2::{Digest, Sha256};

use crate::activation::network_id_byte;
use crate::poawx_candidate::{
    true_vrf_active, AssignmentProofV2, CandidateSet, RoleCandidate, ASSIGNMENT_PROOF_V2_WIRE,
};
use crate::poawx_gossip::GossipOutcome;

/// Domain tag for the admission digest.
pub const CANDIDATE_ADMISSION_DOMAIN: &[u8] = b"IRIUM_POAWX_CANDIDATE_ADMISSION_V1";
pub const CANDIDATE_ADMISSION_VERSION: u8 = 1;
/// Wire size: version(1)+net(1)+height(8)+seed(32)+candidate(175)+digest(32).
pub const CANDIDATE_ADMISSION_WIRE: usize = 1 + 1 + 8 + 32 + 175 + 32;
/// Phase 22E: wire size with a trailing true-VRF AssignmentProofV2 appended.
pub const CANDIDATE_ADMISSION_V2_WIRE: usize = CANDIDATE_ADMISSION_WIRE + ASSIGNMENT_PROOF_V2_WIRE;
/// Safety cap on a single admission payload (anti-oversize).
pub const CANDIDATE_ADMISSION_MAX_BYTES: usize = 4096;
const ADMISSION_SEEN_CAP: usize = 100_000;
const ADMISSION_PRUNE_KEEP: u64 = 64;

/// Default admission window (heights ahead of tip a candidate may be admitted for).
pub const DEFAULT_CANDIDATE_ADMISSION_WINDOW: u64 = 64;

pub fn candidate_admission_window() -> u64 {
    std::env::var("IRIUM_POAWX_CANDIDATE_ADMISSION_WINDOW")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|w| *w >= 1)
        .unwrap_or(DEFAULT_CANDIDATE_ADMISSION_WINDOW)
}

pub fn candidate_admission_activation_height() -> Option<u64> {
    std::env::var("IRIUM_POAWX_CANDIDATE_ADMISSION_ACTIVATION_HEIGHT")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
}
pub fn candidate_admission_required() -> bool {
    std::env::var("IRIUM_POAWX_CANDIDATE_ADMISSION_REQUIRED")
        .map(|v| v.trim() == "1")
        .unwrap_or(false)
}

/// Pure gate: network 0 (mainnet/unset) hard-off; else active at/after activation.
pub fn candidate_admission_gate(network_id: u8, activation: Option<u64>, height: u64) -> bool {
    if network_id == 0 {
        return false;
    }
    matches!(activation, Some(h) if height >= h)
}
pub fn candidate_admission_enforced_gate(
    network_id: u8,
    activation: Option<u64>,
    required: bool,
    height: u64,
) -> bool {
    candidate_admission_gate(network_id, activation, height) && required
}

pub fn candidate_admission_active(height: u64) -> bool {
    candidate_admission_gate(
        network_id_byte(),
        candidate_admission_activation_height(),
        height,
    )
}
pub fn candidate_admission_enforced(height: u64) -> bool {
    candidate_admission_enforced_gate(
        network_id_byte(),
        candidate_admission_activation_height(),
        candidate_admission_required(),
        height,
    )
}
/// Whether this node ingests/gossips admissions (testnet/devnet + gate configured).
pub fn candidate_admission_gossip_enabled() -> bool {
    network_id_byte() != 0 && candidate_admission_activation_height().is_some()
}

/// One admitted candidate, bound to its `(network, height, role, seed)` context.
/// No private key material; the assignment-proof digest inside the candidate is the
/// VRF-style placeholder binding (recomputable).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateAdmissionV1 {
    pub version: u8,
    pub network_id: u8,
    pub target_height: u64,
    pub seed: [u8; 32],
    pub candidate: RoleCandidate,
    /// Phase 22E: optional true-VRF proof (absent when the true-VRF gate is off;
    /// required + validated when on). Bound into the admission digest when present.
    pub assignment_proof_v2: Option<AssignmentProofV2>,
    pub digest: [u8; 32],
}

fn admission_digest(
    network_id: u8,
    target_height: u64,
    seed: &[u8; 32],
    candidate: &RoleCandidate,
    v2: Option<&AssignmentProofV2>,
) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(CANDIDATE_ADMISSION_DOMAIN);
    h.update([CANDIDATE_ADMISSION_VERSION]);
    h.update([network_id]);
    h.update(target_height.to_le_bytes());
    h.update(seed);
    h.update(candidate.serialize());
    // Phase 22E: bind the true-VRF proof when present. Absent => byte-identical to
    // the pre-22E digest (backward compatible).
    if let Some(p) = v2 {
        h.update(b"IRIUM_POAWX_ADMISSION_V2");
        h.update(p.digest);
    }
    h.finalize().into()
}

impl CandidateAdmissionV1 {
    pub fn new(
        network_id: u8,
        target_height: u64,
        seed: [u8; 32],
        candidate: RoleCandidate,
    ) -> Self {
        Self::new_with_v2(network_id, target_height, seed, candidate, None)
    }

    /// Phase 22E: build an admission optionally carrying a true-VRF proof. When
    /// present, the proof is bound into the digest and validated when the gate is on.
    pub fn new_with_v2(
        network_id: u8,
        target_height: u64,
        seed: [u8; 32],
        candidate: RoleCandidate,
        assignment_proof_v2: Option<AssignmentProofV2>,
    ) -> Self {
        let digest = admission_digest(
            network_id,
            target_height,
            &seed,
            &candidate,
            assignment_proof_v2.as_ref(),
        );
        Self {
            version: CANDIDATE_ADMISSION_VERSION,
            network_id,
            target_height,
            seed,
            candidate,
            assignment_proof_v2,
            digest,
        }
    }

    pub fn serialize(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(CANDIDATE_ADMISSION_WIRE);
        out.push(self.version);
        out.push(self.network_id);
        out.extend_from_slice(&self.target_height.to_le_bytes());
        out.extend_from_slice(&self.seed);
        out.extend_from_slice(&self.candidate.serialize());
        out.extend_from_slice(&self.digest);
        // Phase 22E: trailing true-VRF proof (present-only); absent =>
        // byte-identical to a pre-22E admission wire.
        if let Some(p) = &self.assignment_proof_v2 {
            out.extend_from_slice(&p.serialize());
        }
        out
    }

    pub fn deserialize(raw: &[u8]) -> Result<Self, String> {
        if raw.len() != CANDIDATE_ADMISSION_WIRE && raw.len() != CANDIDATE_ADMISSION_V2_WIRE {
            return Err("candidate admission: bad length".to_string());
        }
        let version = raw[0];
        if version != CANDIDATE_ADMISSION_VERSION {
            return Err(format!("candidate admission: unknown version {version}"));
        }
        let network_id = raw[1];
        let mut hb = [0u8; 8];
        hb.copy_from_slice(&raw[2..10]);
        let target_height = u64::from_le_bytes(hb);
        let mut seed = [0u8; 32];
        seed.copy_from_slice(&raw[10..42]);
        let candidate = RoleCandidate::deserialize(&raw[42..42 + 175])?;
        let mut digest = [0u8; 32];
        digest.copy_from_slice(&raw[42 + 175..42 + 175 + 32]);
        let assignment_proof_v2 = if raw.len() == CANDIDATE_ADMISSION_V2_WIRE {
            Some(AssignmentProofV2::deserialize(
                &raw[CANDIDATE_ADMISSION_WIRE..CANDIDATE_ADMISSION_V2_WIRE],
            )?)
        } else {
            None
        };
        Ok(Self {
            version,
            network_id,
            target_height,
            seed,
            candidate,
            assignment_proof_v2,
            digest,
        })
    }

    /// Validate self-consistency against the expected network/height: the embedded
    /// candidate must be self-consistent (recomputed proof/penalty/score) for this
    /// (network, height, seed), and the admission digest must recompute. Rejects
    /// wrong network/height and any mutation. No state/dominance check here.
    pub fn validate(&self, network_id: u8, target_height: u64) -> Result<(), String> {
        if self.version != CANDIDATE_ADMISSION_VERSION {
            return Err("candidate admission: bad version".to_string());
        }
        if self.network_id != network_id {
            return Err("candidate admission: wrong network".to_string());
        }
        if self.target_height != target_height {
            return Err("candidate admission: wrong height".to_string());
        }
        // Phase 22E: candidate self-consistency -- under the true-VRF gate the digest
        // is the VRF output (verified below), so check scoring only.
        if true_vrf_active(self.target_height) {
            self.candidate.validate_scoring()?;
        } else {
            self.candidate
                .validate_self(self.network_id, self.target_height, &self.seed)?;
        }
        // Phase 22E: under the true-VRF gate the admission MUST carry a valid V2
        // proof bound to the candidate (the V1 placeholder is not accepted).
        if true_vrf_active(self.target_height) {
            let p = self
                .assignment_proof_v2
                .as_ref()
                .ok_or("candidate admission: true-VRF proof required")?;
            p.validate(self.network_id, self.target_height)?;
            if p.role_id != self.candidate.role_id {
                return Err("candidate admission: v2 role mismatch".to_string());
            }
            if p.solver_pkh != self.candidate.solver_pkh {
                return Err("candidate admission: v2 solver mismatch".to_string());
            }
            if p.ticket_digest != self.candidate.ticket_digest {
                return Err("candidate admission: v2 ticket mismatch".to_string());
            }
            if p.assignment_public_key != self.candidate.assignment_public_key {
                return Err("candidate admission: v2 assignment key mismatch".to_string());
            }
            if p.seed != self.seed {
                return Err("candidate admission: v2 seed mismatch".to_string());
            }
            if p.vrf_output != self.candidate.assignment_proof_digest {
                return Err("candidate admission: v2 output != candidate digest".to_string());
            }
        }
        let expect = admission_digest(
            self.network_id,
            self.target_height,
            &self.seed,
            &self.candidate,
            self.assignment_proof_v2.as_ref(),
        );
        if expect != self.digest {
            return Err("candidate admission: digest mismatch".to_string());
        }
        Ok(())
    }
}

/// Process-global node candidate-admission cache (one per node process).
/// Keyed by `(target_height, role_id, solver_pkh)`; deduped by admission digest.
pub struct NodeCandidateAdmissionCache {
    admissions: Mutex<BTreeMap<(u64, u8, [u8; 20]), CandidateAdmissionV1>>,
    seen: Mutex<BTreeSet<[u8; 32]>>,
    tip: AtomicU64,
}

impl Default for NodeCandidateAdmissionCache {
    fn default() -> Self {
        Self {
            admissions: Mutex::new(BTreeMap::new()),
            seen: Mutex::new(BTreeSet::new()),
            tip: AtomicU64::new(0),
        }
    }
}

impl NodeCandidateAdmissionCache {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn set_tip(&self, tip: u64) {
        self.tip.store(tip, Ordering::Relaxed);
    }
    pub fn tip(&self) -> u64 {
        self.tip.load(Ordering::Relaxed)
    }
    fn in_window(&self, target: u64) -> bool {
        let tip = self.tip();
        target >= tip && target <= tip.saturating_add(candidate_admission_window())
    }
    fn already_seen(&self, d: &[u8; 32]) -> bool {
        self.seen
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains(d)
    }
    fn mark_seen(&self, d: [u8; 32]) {
        let mut s = self.seen.lock().unwrap_or_else(|e| e.into_inner());
        if s.len() >= ADMISSION_SEEN_CAP {
            s.clear();
        }
        s.insert(d);
    }

    /// Ingest one admission (raw wire bytes). Validate → window → dedupe → store.
    /// Returns AcceptedNew (rebroadcast), Duplicate (don't), or Rejected (drop).
    pub fn ingest_bytes(&self, bytes: &[u8]) -> GossipOutcome {
        if !candidate_admission_gossip_enabled() {
            return GossipOutcome::Rejected("candidate admission disabled".to_string());
        }
        if bytes.len() > CANDIDATE_ADMISSION_MAX_BYTES {
            return GossipOutcome::Rejected("candidate admission oversize".to_string());
        }
        let adm = match CandidateAdmissionV1::deserialize(bytes) {
            Ok(a) => a,
            Err(e) => return GossipOutcome::Rejected(e),
        };
        if adm.network_id != network_id_byte() {
            return GossipOutcome::Rejected("wrong network".to_string());
        }
        if let Err(e) = adm.validate(adm.network_id, adm.target_height) {
            return GossipOutcome::Rejected(e);
        }
        if !self.in_window(adm.target_height) {
            return GossipOutcome::Rejected("out of admission window".to_string());
        }
        if self.already_seen(&adm.digest) {
            return GossipOutcome::Duplicate;
        }
        let key = (
            adm.target_height,
            adm.candidate.role_id,
            adm.candidate.solver_pkh,
        );
        let mut map = self.admissions.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(existing) = map.get(&key) {
            if existing.digest != adm.digest {
                return GossipOutcome::Rejected(
                    "conflicting admission for (height,role,solver)".to_string(),
                );
            }
            return GossipOutcome::Duplicate;
        }
        map.insert(key, adm.clone());
        drop(map);
        self.mark_seen(adm.digest);
        GossipOutcome::AcceptedNew
    }

    /// Admitted candidates for `(target_height, seed)`, canonically sorted.
    pub fn candidates_for(&self, target_height: u64, seed: &[u8; 32]) -> Vec<RoleCandidate> {
        let map = self.admissions.lock().unwrap_or_else(|e| e.into_inner());
        let mut cands: Vec<RoleCandidate> = map
            .iter()
            .filter(|((h, _, _), a)| *h == target_height && &a.seed == seed)
            .map(|(_, a)| a.candidate.clone())
            .collect();
        // canonical order via CandidateSet sort logic.
        let mut cs = CandidateSet::new(network_id_byte(), target_height, *seed);
        cs.candidates.append(&mut cands);
        cs.sort_canonical();
        cs.candidates
    }

    /// Admitted candidate set for `(network, target_height, seed)` (canonical).
    pub fn admitted_candidate_set(
        &self,
        network_id: u8,
        target_height: u64,
        seed: &[u8; 32],
    ) -> CandidateSet {
        let mut cs = CandidateSet::new(network_id, target_height, *seed);
        cs.candidates = self.candidates_for(target_height, seed);
        cs
    }

    /// All admissions for a target height (any seed), for RPC export.
    pub fn admissions_for_height(&self, target_height: u64) -> Vec<CandidateAdmissionV1> {
        self.admissions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .filter(|((h, _, _), _)| *h == target_height)
            .map(|(_, a)| a.clone())
            .collect()
    }

    pub fn admission_count(&self, target_height: u64) -> usize {
        self.admissions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .filter(|((h, _, _), _)| *h == target_height)
            .count()
    }

    /// Drop admissions for heights strictly below `tip - ADMISSION_PRUNE_KEEP`.
    pub fn prune(&self, tip: u64) {
        self.set_tip(tip);
        let floor = tip.saturating_sub(ADMISSION_PRUNE_KEEP);
        if floor == 0 {
            return;
        }
        let mut map = self.admissions.lock().unwrap_or_else(|e| e.into_inner());
        map.retain(|(h, _, _), _| *h >= floor);
    }

    pub fn clear(&self) {
        self.admissions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
        self.seen.lock().unwrap_or_else(|e| e.into_inner()).clear();
    }
}

static GLOBAL_ADMISSION_CACHE: OnceLock<NodeCandidateAdmissionCache> = OnceLock::new();

pub fn global_admission_cache() -> &'static NodeCandidateAdmissionCache {
    GLOBAL_ADMISSION_CACHE.get_or_init(NodeCandidateAdmissionCache::new)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::poawx_penalty::PenaltyStatus;

    fn cand(role: u8, solver: [u8; 20], tag: u8, seed: &[u8; 32]) -> RoleCandidate {
        RoleCandidate::build(
            1,
            10,
            seed,
            role,
            solver,
            [0x02u8; 33],
            [tag; 32],
            PenaltyStatus::Clean.id(),
            1000,
            [tag.wrapping_add(1); 32],
        )
    }

    #[test]
    fn admission_wire_roundtrip_and_digest_sensitivity() {
        // Gate-off path: serialize vs the V2 tests and ensure the true-VRF gate is
        // off so a V1 admission validates deterministically.
        let _g = crate::poawx::poawx_test_env_lock()
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        std::env::remove_var("IRIUM_POAWX_TRUE_VRF_ACTIVATION_HEIGHT");
        std::env::remove_var("IRIUM_POAWX_TRUE_VRF_REQUIRED");
        let seed = [0x22u8; 32];
        let a = CandidateAdmissionV1::new(1, 10, seed, cand(1, [0xC1u8; 20], 0x11, &seed));
        let b = a.serialize();
        assert_eq!(b.len(), CANDIDATE_ADMISSION_WIRE);
        assert_eq!(CandidateAdmissionV1::deserialize(&b).unwrap(), a);
        assert!(a.validate(1, 10).is_ok());
        assert!(a.validate(2, 10).is_err(), "wrong network");
        assert!(a.validate(1, 11).is_err(), "wrong height");
        // mutation changes digest -> validate rejects.
        let mut m = a.clone();
        m.candidate.effective_score ^= 1;
        assert!(m.validate(1, 10).is_err(), "mutation rejects");
        assert_ne!(
            admission_digest(1, 10, &seed, &m.candidate, None),
            a.digest,
            "mutation changes digest"
        );
    }

    #[test]
    fn cache_ingest_dedupe_window_and_root() {
        let _g = crate::poawx::poawx_test_env_lock()
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        std::env::set_var("IRIUM_NETWORK", "testnet");
        std::env::set_var("IRIUM_POAWX_CANDIDATE_ADMISSION_ACTIVATION_HEIGHT", "1");
        let net = network_id_byte();
        let seed = [0x22u8; 32];
        let cache = NodeCandidateAdmissionCache::new();
        cache.set_tip(10);
        let a1 = CandidateAdmissionV1::new(net, 10, seed, cand(1, [0xC1u8; 20], 0x11, &seed));
        let a2 = CandidateAdmissionV1::new(net, 10, seed, cand(2, [0xC2u8; 20], 0x12, &seed));
        assert_eq!(
            cache.ingest_bytes(&a1.serialize()),
            GossipOutcome::AcceptedNew
        );
        assert_eq!(
            cache.ingest_bytes(&a1.serialize()),
            GossipOutcome::Duplicate
        );
        assert_eq!(
            cache.ingest_bytes(&a2.serialize()),
            GossipOutcome::AcceptedNew
        );
        assert_eq!(cache.admission_count(10), 2);
        // out of window rejects.
        let far = CandidateAdmissionV1::new(net, 10_000, seed, cand(1, [0xC9u8; 20], 0x33, &seed));
        assert!(matches!(
            cache.ingest_bytes(&far.serialize()),
            GossipOutcome::Rejected(_)
        ));
        // malformed rejects, no panic.
        assert!(matches!(
            cache.ingest_bytes(&[0u8; 10]),
            GossipOutcome::Rejected(_)
        ));
        // deterministic admitted set root.
        let cs = cache.admitted_candidate_set(net, 10, &seed);
        assert_eq!(cs.candidates.len(), 2);
        let root1 = cs.root();
        let cs2 = cache.admitted_candidate_set(net, 10, &seed);
        assert_eq!(cs2.root(), root1, "admitted set root deterministic");
        // prune drops old heights.
        cache.prune(10_000);
        assert_eq!(cache.admission_count(10), 0);
        std::env::remove_var("IRIUM_NETWORK");
        std::env::remove_var("IRIUM_POAWX_CANDIDATE_ADMISSION_ACTIVATION_HEIGHT");
    }

    fn v2_admission(
        net: u8,
        height: u64,
        seed: [u8; 32],
        secret: u8,
        role: u8,
        solver: [u8; 20],
        ticket: [u8; 32],
    ) -> CandidateAdmissionV1 {
        let proof =
            AssignmentProofV2::prove(&[secret; 32], net, height, role, solver, ticket, seed)
                .expect("v2 prove");
        let cand =
            RoleCandidate::from_assignment_v2(&proof, PenaltyStatus::Clean.id(), 1000, [role; 32]);
        CandidateAdmissionV1::new_with_v2(net, height, seed, cand, Some(proof))
    }

    fn restamp(a: &mut CandidateAdmissionV1) {
        a.digest = admission_digest(
            a.network_id,
            a.target_height,
            &a.seed,
            &a.candidate,
            a.assignment_proof_v2.as_ref(),
        );
    }

    #[test]
    fn phase22e_admission_v2_accept_and_reject() {
        let _g = crate::poawx::poawx_test_env_lock()
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        std::env::set_var("IRIUM_NETWORK", "testnet");
        std::env::set_var("IRIUM_POAWX_TRUE_VRF_ACTIVATION_HEIGHT", "1");
        std::env::set_var("IRIUM_POAWX_TRUE_VRF_REQUIRED", "1");
        let net = network_id_byte();
        let seed = [0x44u8; 32];
        let a = v2_admission(net, 10, seed, 7, 1, [0xC1u8; 20], [0x11u8; 32]);
        // (6) valid V2 admission accepts + wire round-trips.
        assert!(a.validate(net, 10).is_ok(), "valid V2 admission");
        let wire = a.serialize();
        assert_eq!(wire.len(), CANDIDATE_ADMISSION_V2_WIRE);
        assert_eq!(CandidateAdmissionV1::deserialize(&wire).unwrap(), a);
        // (7) wrong network, (8) wrong height.
        assert!(a.validate(net + 1, 10).is_err(), "wrong network");
        assert!(a.validate(net, 11).is_err(), "wrong height");
        // (9) wrong role, (10) wrong solver, (11) wrong ticket (binding mismatch).
        let mut m = a.clone();
        m.candidate.role_id ^= 1;
        restamp(&mut m);
        assert!(m.validate(net, 10).is_err(), "wrong role");
        let mut m = a.clone();
        m.candidate.solver_pkh[0] ^= 1;
        restamp(&mut m);
        assert!(m.validate(net, 10).is_err(), "wrong solver");
        let mut m = a.clone();
        m.candidate.ticket_digest[0] ^= 1;
        restamp(&mut m);
        assert!(m.validate(net, 10).is_err(), "wrong ticket");
        // (12) wrong seed (proof seed != admission seed).
        let p2 = AssignmentProofV2::prove(
            &[7u8; 32],
            net,
            10,
            1,
            [0xC1u8; 20],
            [0x11u8; 32],
            [0x55u8; 32],
        )
        .unwrap();
        let cand2 =
            RoleCandidate::from_assignment_v2(&p2, PenaltyStatus::Clean.id(), 1000, [1u8; 32]);
        let mut ws = CandidateAdmissionV1::new_with_v2(net, 10, seed, cand2, Some(p2));
        restamp(&mut ws);
        assert!(ws.validate(net, 10).is_err(), "wrong seed");
        // (13) mutated proof + mutated output.
        let mut m = a.clone();
        m.assignment_proof_v2.as_mut().unwrap().vrf_proof[0] ^= 1;
        restamp(&mut m);
        assert!(m.validate(net, 10).is_err(), "mutated proof");
        let mut m = a.clone();
        m.assignment_proof_v2.as_mut().unwrap().vrf_output[0] ^= 1;
        restamp(&mut m);
        assert!(m.validate(net, 10).is_err(), "mutated output");
        // (14) V2 required rejects a V1-only admission.
        let v1cand = RoleCandidate::from_assignment_v2(
            a.assignment_proof_v2.as_ref().unwrap(),
            PenaltyStatus::Clean.id(),
            1000,
            [1u8; 32],
        );
        let v1only = CandidateAdmissionV1::new(net, 10, seed, v1cand);
        assert!(
            v1only.validate(net, 10).is_err(),
            "V1-only rejected when V2 required"
        );
        std::env::remove_var("IRIUM_NETWORK");
        std::env::remove_var("IRIUM_POAWX_TRUE_VRF_ACTIVATION_HEIGHT");
        std::env::remove_var("IRIUM_POAWX_TRUE_VRF_REQUIRED");
    }

    #[test]
    fn phase22e_gate_off_accepts_v1_admission() {
        // (15) with the true-VRF gate off, an old V1 admission still validates and is
        // byte-identical on the wire.
        let _g = crate::poawx::poawx_test_env_lock()
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        std::env::set_var("IRIUM_NETWORK", "testnet");
        std::env::remove_var("IRIUM_POAWX_TRUE_VRF_ACTIVATION_HEIGHT");
        std::env::remove_var("IRIUM_POAWX_TRUE_VRF_REQUIRED");
        let net = network_id_byte();
        let seed = [0x22u8; 32];
        let cand = cand(1, [0xC1u8; 20], 0x11, &seed);
        let a = CandidateAdmissionV1::new(net, 10, seed, cand);
        assert!(a.assignment_proof_v2.is_none());
        assert_eq!(
            a.serialize().len(),
            CANDIDATE_ADMISSION_WIRE,
            "byte-identical pre-22E wire"
        );
        assert!(
            a.validate(net, 10).is_ok(),
            "V1 admission accepts when gate off"
        );
        std::env::remove_var("IRIUM_NETWORK");
    }

    #[test]
    fn phase22e_committed_root_binds_v2() {
        // (16) the committed-admission root changes when the V2 proof (output) changes,
        // because the candidate digest = the VRF output.
        use crate::poawx_committed_admission::AdmissionCommitmentV1;
        let _g = crate::poawx::poawx_test_env_lock()
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        std::env::set_var("IRIUM_NETWORK", "testnet");
        std::env::set_var("IRIUM_POAWX_TRUE_VRF_ACTIVATION_HEIGHT", "1");
        std::env::set_var("IRIUM_POAWX_TRUE_VRF_REQUIRED", "1");
        let net = network_id_byte();
        let seed = [0x44u8; 32];
        let mk_root = |secret: u8| -> [u8; 32] {
            let a = v2_admission(net, 10, seed, secret, 1, [0xC1u8; 20], [0x11u8; 32]);
            let mut cs = CandidateSet::new(net, 10, seed);
            cs.push(a.candidate.clone());
            cs.sort_canonical();
            AdmissionCommitmentV1::from_candidate_set(&cs, 9).candidate_admission_root
        };
        assert_ne!(
            mk_root(7),
            mk_root(9),
            "different VRF output => different committed root"
        );
        std::env::remove_var("IRIUM_NETWORK");
        std::env::remove_var("IRIUM_POAWX_TRUE_VRF_ACTIVATION_HEIGHT");
        std::env::remove_var("IRIUM_POAWX_TRUE_VRF_REQUIRED");
    }

    #[test]
    fn phase23a_admission_deserialize_rejects_bad_trailing_length() {
        let seed = [0x22u8; 32];
        let a = CandidateAdmissionV1::new(1, 10, seed, cand(1, [0xC1u8; 20], 0x11, &seed));
        // base (V1) length parses; base + junk that is neither V1 nor V2 length rejects.
        assert!(CandidateAdmissionV1::deserialize(&a.serialize()).is_ok());
        let mut junk = a.serialize();
        junk.extend_from_slice(&[0u8; 100]);
        assert!(
            CandidateAdmissionV1::deserialize(&junk).is_err(),
            "+100 junk"
        );
        let mut partial = a.serialize();
        partial.extend_from_slice(&[0u8; ASSIGNMENT_PROOF_V2_WIRE - 1]);
        assert!(
            CandidateAdmissionV1::deserialize(&partial).is_err(),
            "partial v2"
        );
        assert!(CandidateAdmissionV1::deserialize(&[]).is_err(), "empty");
    }

    #[test]
    fn gate_logic_pure_and_mainnet_off() {
        assert!(
            !candidate_admission_gate(0, Some(1), 100),
            "mainnet hard-off"
        );
        assert!(candidate_admission_gate(1, Some(1), 100));
        assert!(!candidate_admission_gate(1, None, 100));
        assert!(candidate_admission_enforced_gate(1, Some(1), true, 100));
        assert!(!candidate_admission_enforced_gate(1, Some(1), false, 100));
        assert!(
            !candidate_admission_enforced_gate(0, Some(1), true, 100),
            "mainnet hard-off"
        );
    }
}
