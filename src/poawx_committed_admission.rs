//! Phase 22A: chain-committed candidate admission.
//!
//! Strengthens Phase 21E (which validated a block's candidate set against the
//! validating node's *local* admission cache) by **chain-committing** the admitted
//! candidate-set root in a prior block. Block `H-1` carries an
//! `AdmissionCommitmentV1` for target height `H` (root over the admitted candidate
//! set, bound to the freeze seed = `H-1`'s parent hash, known when `H-1` is
//! produced — no circularity). Block `H`'s candidate set must reproduce that exact
//! committed root, so the producer of `H` cannot silently add/omit candidates
//! relative to the `H-1` commitment.
//!
//! HONEST LIMITATION: this does NOT prove offline / never-gossiped miners existed.
//! It makes the admitted set chain-committed before selection (removing the
//! per-node-cache divergence at selection time and the silent-omission attack at
//! `H`). Deterministic, bounded, no floats, no wall-clock. Gated + mainnet hard-off.
#![allow(dead_code)]

use sha2::{Digest, Sha256};

use crate::activation::network_id_byte;
use crate::poawx_candidate::CandidateSet;

const COMMITMENT_DOMAIN: &[u8] = b"IRIUM_POAWX_ADMISSION_COMMITMENT_V1";
/// 4-byte trailing-section magic for the committed admission in the Phase 20 ext.
pub const COMMITTED_ADMISSION_SECTION_MAGIC: &[u8; 4] = b"CAC1";
pub const ADMISSION_COMMITMENT_VERSION: u8 = 1;
/// Wire: version(1)+net(1)+target(8)+commit(8)+seed(32)+root(32)+count(2)+window(8)+digest(32).
pub const ADMISSION_COMMITMENT_WIRE: usize = 1 + 1 + 8 + 8 + 32 + 32 + 2 + 8 + 32; // 124
/// Upper bound on committed candidate count (matches the candidate-set bound).
pub const MAX_COMMITTED_CANDIDATES: u16 = 4096;
/// Default admission-freeze window length (blocks).
pub const DEFAULT_COMMITTED_ADMISSION_WINDOW: u64 = 64;

/// Chain-committed admission root for a target height. Carried in the COMMIT block
/// (`commit_height = target_height - 1`) and matched by the candidate set at the
/// target block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmissionCommitmentV1 {
    pub version: u8,
    pub network_id: u8,
    pub target_height: u64,
    pub commit_height: u64,
    pub seed: [u8; 32],
    pub candidate_admission_root: [u8; 32],
    pub candidate_count: u16,
    pub window_id: u64,
    pub digest: [u8; 32],
}

#[allow(clippy::too_many_arguments)]
fn commitment_digest(
    network_id: u8,
    target_height: u64,
    commit_height: u64,
    seed: &[u8; 32],
    candidate_admission_root: &[u8; 32],
    candidate_count: u16,
    window_id: u64,
) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(COMMITMENT_DOMAIN);
    h.update([ADMISSION_COMMITMENT_VERSION]);
    h.update([network_id]);
    h.update(target_height.to_le_bytes());
    h.update(commit_height.to_le_bytes());
    h.update(seed);
    h.update(candidate_admission_root);
    h.update(candidate_count.to_le_bytes());
    h.update(window_id.to_le_bytes());
    h.finalize().into()
}

impl AdmissionCommitmentV1 {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        network_id: u8,
        target_height: u64,
        commit_height: u64,
        seed: [u8; 32],
        candidate_admission_root: [u8; 32],
        candidate_count: u16,
        window_id: u64,
    ) -> Self {
        let digest = commitment_digest(
            network_id,
            target_height,
            commit_height,
            &seed,
            &candidate_admission_root,
            candidate_count,
            window_id,
        );
        Self {
            version: ADMISSION_COMMITMENT_VERSION,
            network_id,
            target_height,
            commit_height,
            seed,
            candidate_admission_root,
            candidate_count,
            window_id,
            digest,
        }
    }

    /// Build the commitment for `target_height` from the admitted candidate set
    /// (which carries the freeze seed + the candidates). `commit_height` is the
    /// producing block's height (`target_height - 1`).
    pub fn from_candidate_set(cs: &CandidateSet, commit_height: u64) -> Self {
        Self::new(
            cs.network_id,
            cs.target_height,
            commit_height,
            cs.seed,
            cs.root(),
            cs.candidates.len().min(u16::MAX as usize) as u16,
            committed_admission_window_id(cs.target_height),
        )
    }

    pub fn serialize(&self) -> Vec<u8> {
        let mut o = Vec::with_capacity(ADMISSION_COMMITMENT_WIRE);
        o.push(self.version);
        o.push(self.network_id);
        o.extend_from_slice(&self.target_height.to_le_bytes());
        o.extend_from_slice(&self.commit_height.to_le_bytes());
        o.extend_from_slice(&self.seed);
        o.extend_from_slice(&self.candidate_admission_root);
        o.extend_from_slice(&self.candidate_count.to_le_bytes());
        o.extend_from_slice(&self.window_id.to_le_bytes());
        o.extend_from_slice(&self.digest);
        o
    }

    pub fn deserialize(raw: &[u8]) -> Result<Self, String> {
        if raw.len() != ADMISSION_COMMITMENT_WIRE {
            return Err("admission commitment: bad length".to_string());
        }
        let version = raw[0];
        if version != ADMISSION_COMMITMENT_VERSION {
            return Err(format!("admission commitment: unknown version {version}"));
        }
        let network_id = raw[1];
        let mut p = 2usize;
        let mut h8 = [0u8; 8];
        h8.copy_from_slice(&raw[p..p + 8]);
        let target_height = u64::from_le_bytes(h8);
        p += 8;
        h8.copy_from_slice(&raw[p..p + 8]);
        let commit_height = u64::from_le_bytes(h8);
        p += 8;
        let mut seed = [0u8; 32];
        seed.copy_from_slice(&raw[p..p + 32]);
        p += 32;
        let mut candidate_admission_root = [0u8; 32];
        candidate_admission_root.copy_from_slice(&raw[p..p + 32]);
        p += 32;
        let candidate_count = u16::from_le_bytes([raw[p], raw[p + 1]]);
        p += 2;
        h8.copy_from_slice(&raw[p..p + 8]);
        let window_id = u64::from_le_bytes(h8);
        p += 8;
        let mut digest = [0u8; 32];
        digest.copy_from_slice(&raw[p..p + 32]);
        if candidate_count > MAX_COMMITTED_CANDIDATES {
            return Err("admission commitment: candidate count over cap".to_string());
        }
        Ok(Self {
            version,
            network_id,
            target_height,
            commit_height,
            seed,
            candidate_admission_root,
            candidate_count,
            window_id,
            digest,
        })
    }

    /// Validate self-consistency + binding: recompute the digest, check version,
    /// network, target height, commit_height == target_height - 1, and count cap.
    pub fn validate(&self, network_id: u8, target_height: u64) -> Result<(), String> {
        if self.version != ADMISSION_COMMITMENT_VERSION {
            return Err("admission commitment: bad version".to_string());
        }
        if self.network_id != network_id {
            return Err("admission commitment: wrong network".to_string());
        }
        if self.target_height != target_height {
            return Err("admission commitment: wrong target height".to_string());
        }
        if self.commit_height + 1 != self.target_height {
            return Err("admission commitment: commit_height must be target-1".to_string());
        }
        if self.candidate_count > MAX_COMMITTED_CANDIDATES {
            return Err("admission commitment: candidate count over cap".to_string());
        }
        let expect = commitment_digest(
            self.network_id,
            self.target_height,
            self.commit_height,
            &self.seed,
            &self.candidate_admission_root,
            self.candidate_count,
            self.window_id,
        );
        if expect != self.digest {
            return Err("admission commitment: digest mismatch".to_string());
        }
        Ok(())
    }

    /// Whether a candidate set exactly matches this commitment (root + count +
    /// seed). The candidate set's `root()` binds network/height/seed/candidates.
    pub fn matches_candidate_set(&self, cs: &CandidateSet) -> bool {
        cs.root() == self.candidate_admission_root
            && (cs.candidates.len().min(u16::MAX as usize) as u16) == self.candidate_count
            && cs.seed == self.seed
            && cs.network_id == self.network_id
            && cs.target_height == self.target_height
    }
}

// ── Admission epoch seed (Phase 26B seed reconciliation) ─────────────────────

/// The candidate-admission EPOCH seed for a block at height `H`.
///
/// The candidate set / admissions for `H` are FROZEN one block ahead by the
/// parent's outgoing committed admission, whose freeze seed is the parent block's
/// `prev_hash` (= the grandparent hash, `hash(H-2)`). So the epoch seed for `H` is
/// the parent's `prev_hash`. At the activation boundary — when the parent is the
/// genesis block, whose `prev_hash` is all-zero — the epoch seed is THIS block's
/// `prev_hash` (the genesis hash), and the incoming committed-admission check is
/// graced. This reconciles the phase21d candidate-set gate (which validates this
/// seed) with the phase22a committed-admission gate (whose parent commitment
/// carries exactly this seed), making multi-block chains satisfiable WITHOUT
/// weakening either gate. Pure; no wire-format change. Mainnet behavior is
/// unaffected (the PoAW-X gates remain hard-off for `network_id == 0`).
///
/// `parent_prev_hash` is the parent (tip) block's own `prev_hash` (`None` if there
/// is no parent). `block_prev_hash` is the current block's `prev_hash`.
pub fn admission_epoch_seed(parent_prev_hash: Option<[u8; 32]>, block_prev_hash: [u8; 32]) -> [u8; 32] {
    match parent_prev_hash {
        // Parent is a real (non-genesis) block: its prev_hash is the grandparent
        // hash and equals the seed it froze for this height.
        Some(p) if p != [0u8; 32] => p,
        // Activation boundary (parent is genesis, or no parent): use this block's
        // prev_hash (the genesis hash); incoming committed admission is graced.
        _ => block_prev_hash,
    }
}

// ── Gates (param-driven; mainnet hard-off) ───────────────────────────────────

pub fn committed_admission_window() -> u64 {
    std::env::var("IRIUM_POAWX_COMMITTED_ADMISSION_WINDOW")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|w| *w >= 1)
        .unwrap_or(DEFAULT_COMMITTED_ADMISSION_WINDOW)
}
pub fn committed_admission_window_id(target_height: u64) -> u64 {
    target_height / committed_admission_window()
}
pub fn committed_admission_activation_height() -> Option<u64> {
    std::env::var("IRIUM_POAWX_COMMITTED_ADMISSION_ACTIVATION_HEIGHT")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
}
pub fn committed_admission_required() -> bool {
    std::env::var("IRIUM_POAWX_COMMITTED_ADMISSION_REQUIRED")
        .map(|v| v.trim() == "1")
        .unwrap_or(false)
}
pub fn committed_admission_gate(network_id: u8, activation: Option<u64>, height: u64) -> bool {
    if network_id == 0 {
        return false;
    }
    matches!(activation, Some(h) if height >= h)
}
pub fn committed_admission_enforced_gate(
    network_id: u8,
    activation: Option<u64>,
    required: bool,
    height: u64,
) -> bool {
    committed_admission_gate(network_id, activation, height) && required
}
pub fn committed_admission_active(height: u64) -> bool {
    committed_admission_gate(
        network_id_byte(),
        committed_admission_activation_height(),
        height,
    )
}
pub fn committed_admission_enforced(height: u64) -> bool {
    committed_admission_enforced_gate(
        network_id_byte(),
        committed_admission_activation_height(),
        committed_admission_required(),
        height,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::poawx_candidate::{CandidateSet, RoleCandidate};
    use crate::poawx_penalty::PenaltyStatus;

    fn sample_set(net: u8, h: u64, seed: [u8; 32]) -> CandidateSet {
        let mut cs = CandidateSet::new(net, h, seed);
        cs.push(RoleCandidate::build(
            net,
            h,
            &seed,
            1,
            [0xC1u8; 20],
            [0x02u8; 33],
            [0x11u8; 32],
            PenaltyStatus::Clean.id(),
            1000,
            [0x21u8; 32],
        ));
        cs.push(RoleCandidate::build(
            net,
            h,
            &seed,
            2,
            [0xC2u8; 20],
            [0x02u8; 33],
            [0x12u8; 32],
            PenaltyStatus::Clean.id(),
            1000,
            [0x22u8; 32],
        ));
        cs.sort_canonical();
        cs
    }

    #[test]
    fn phase24a_committed_admission_wire_malformed_rejected() {
        assert!(AdmissionCommitmentV1::deserialize(&[0u8; ADMISSION_COMMITMENT_WIRE - 1]).is_err());
        assert!(AdmissionCommitmentV1::deserialize(&[0u8; ADMISSION_COMMITMENT_WIRE + 1]).is_err());
        assert!(AdmissionCommitmentV1::deserialize(&[]).is_err());
        // all-zeros at exact length still rejects (version byte guard).
        assert!(AdmissionCommitmentV1::deserialize(&[0u8; ADMISSION_COMMITMENT_WIRE]).is_err());
    }

    #[test]
    fn commitment_wire_roundtrip_and_validate() {
        let seed = [0x44u8; 32];
        let cs = sample_set(1, 11, seed);
        let c = AdmissionCommitmentV1::from_candidate_set(&cs, 10);
        let b = c.serialize();
        assert_eq!(b.len(), ADMISSION_COMMITMENT_WIRE);
        assert_eq!(AdmissionCommitmentV1::deserialize(&b).unwrap(), c);
        assert!(c.validate(1, 11).is_ok());
        assert!(c.validate(2, 11).is_err(), "wrong network");
        assert!(c.validate(1, 12).is_err(), "wrong target height");
        assert!(c.matches_candidate_set(&cs), "matches its source set");
    }

    #[test]
    fn mutation_changes_digest_and_breaks_match() {
        let seed = [0x44u8; 32];
        let cs = sample_set(1, 11, seed);
        let c = AdmissionCommitmentV1::from_candidate_set(&cs, 10);
        // mutate the committed root -> validate (digest) fails + match fails.
        let mut m = c.clone();
        m.candidate_admission_root[0] ^= 1;
        assert!(m.validate(1, 11).is_err(), "root mutation breaks digest");
        // a different candidate set (mutated) no longer matches the commitment.
        let mut cs2 = cs.clone();
        cs2.candidates[0].dominance_weight ^= 1;
        assert!(!c.matches_candidate_set(&cs2), "mutated set fails match");
    }

    #[test]
    fn commit_height_must_be_target_minus_one() {
        let seed = [0x44u8; 32];
        let cs = sample_set(1, 11, seed);
        // commit_height 9 (not 10) for target 11 -> invalid.
        let bad = AdmissionCommitmentV1::new(1, 11, 9, seed, cs.root(), 2, 0);
        assert!(
            bad.validate(1, 11).is_err(),
            "commit_height must be target-1"
        );
        let good = AdmissionCommitmentV1::new(1, 11, 10, seed, cs.root(), 2, 0);
        assert!(good.validate(1, 11).is_ok());
    }

    #[test]
    fn count_cap_enforced_on_deserialize() {
        let seed = [0x44u8; 32];
        let mut c = AdmissionCommitmentV1::new(1, 11, 10, seed, [0u8; 32], 2, 0);
        c.candidate_count = MAX_COMMITTED_CANDIDATES + 1;
        // re-stamp digest so only the cap check fires.
        c.digest = commitment_digest(1, 11, 10, &seed, &[0u8; 32], c.candidate_count, 0);
        assert!(AdmissionCommitmentV1::deserialize(&c.serialize()).is_err());
    }

    #[test]
    fn gate_logic_pure_and_mainnet_off() {
        assert!(
            !committed_admission_gate(0, Some(1), 100),
            "mainnet hard-off"
        );
        assert!(committed_admission_gate(1, Some(1), 100));
        assert!(!committed_admission_gate(1, None, 100));
        assert!(committed_admission_enforced_gate(1, Some(1), true, 100));
        assert!(!committed_admission_enforced_gate(1, Some(1), false, 100));
        assert!(
            !committed_admission_enforced_gate(0, Some(1), true, 100),
            "mainnet hard-off"
        );
    }
}
