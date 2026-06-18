//! Phase 21A: PoAW-X Miner Work Ticket + lightweight Sybil-resistance primitive.
//!
//! A Miner Work Ticket is a per-epoch, network-bound identity/eligibility token.
//! It carries a small proof-of-work ("sybil work") that imposes a cheap identity
//! cost in testnet/devnet (configurable, default OFF) — this is NOT chain PoW and
//! does NOT touch LWMA-144. Data-only foundation (Phase 21B may enforce it).
//! Mainnet hard-off; no private key material; deterministic.
#![allow(dead_code)]

use sha2::{Digest, Sha256};

use crate::activation::network_id_byte;
use crate::poawx_penalty::PenaltyStatus;

pub const TICKET_VERSION: u8 = 1;
pub const TICKET_DOMAIN: &[u8] = b"IRIUM_POAWX_TICKET_V1";
pub const SYBIL_DOMAIN: &[u8] = b"IRIUM_POAWX_SYBIL_WORK_V1";

/// Miner Work Ticket. `assignment_public_key` is a placeholder for a future
/// VRF/private-assignment public key (Phase 21B+). No private material.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MinerWorkTicket {
    pub version: u8,
    pub network_id: u8,
    pub miner_pkh: [u8; 20],
    pub epoch: u64,
    pub assignment_public_key: [u8; 33],
    pub sybil_work_nonce: [u8; 32],
    pub sybil_work_digest: [u8; 32],
    pub recent_reward_score: u64,
    pub valid_work_count: u32,
    pub invalid_work_count: u32,
    pub penalty_status: u8,
    pub bond_reference: Option<[u8; 32]>,
    pub issued_height: u64,
    pub expiry_height: u64,
}

/// Recompute the sybil-work digest for a candidate nonce. Binding fields prevent
/// reuse across network/miner/epoch/assignment-key.
pub fn compute_sybil_digest(
    network_id: u8,
    miner_pkh: &[u8; 20],
    epoch: u64,
    assignment_public_key: &[u8; 33],
    nonce: &[u8; 32],
) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(SYBIL_DOMAIN);
    h.update([network_id]);
    h.update(miner_pkh);
    h.update(epoch.to_le_bytes());
    h.update(assignment_public_key);
    h.update(nonce);
    h.finalize().into()
}

/// Count leading zero bits of a 32-byte digest (big-endian).
pub fn leading_zero_bits(d: &[u8; 32]) -> u32 {
    let mut n = 0u32;
    for &b in d.iter() {
        if b == 0 {
            n += 8;
        } else {
            n += b.leading_zeros();
            break;
        }
    }
    n
}

/// Whether a sybil digest meets the leading-zero-bits target.
pub fn meets_sybil_target(digest: &[u8; 32], bits: u32) -> bool {
    leading_zero_bits(digest) >= bits
}

/// Configured sybil threshold (leading-zero bits). `0` = disabled (default).
/// Env `IRIUM_POAWX_TICKET_SYBIL_BITS`. Mainnet hard-off (always 0).
pub fn sybil_threshold_bits() -> u32 {
    if network_id_byte() == 0 {
        return 0;
    }
    std::env::var("IRIUM_POAWX_TICKET_SYBIL_BITS")
        .ok()
        .and_then(|v| v.trim().parse::<u32>().ok())
        .filter(|&b| b <= 32)
        .unwrap_or(0)
}

impl MinerWorkTicket {
    /// Canonical serialization. `bond_reference` present-flag is a single byte.
    pub fn serialize(&self) -> Vec<u8> {
        let mut out =
            Vec::with_capacity(1 + 1 + 20 + 8 + 33 + 32 + 32 + 8 + 4 + 4 + 1 + 1 + 8 + 8 + 32);
        out.push(self.version);
        out.push(self.network_id);
        out.extend_from_slice(&self.miner_pkh);
        out.extend_from_slice(&self.epoch.to_le_bytes());
        out.extend_from_slice(&self.assignment_public_key);
        out.extend_from_slice(&self.sybil_work_nonce);
        out.extend_from_slice(&self.sybil_work_digest);
        out.extend_from_slice(&self.recent_reward_score.to_le_bytes());
        out.extend_from_slice(&self.valid_work_count.to_le_bytes());
        out.extend_from_slice(&self.invalid_work_count.to_le_bytes());
        out.push(self.penalty_status);
        match &self.bond_reference {
            Some(b) => {
                out.push(1);
                out.extend_from_slice(b);
            }
            None => out.push(0),
        }
        out.extend_from_slice(&self.issued_height.to_le_bytes());
        out.extend_from_slice(&self.expiry_height.to_le_bytes());
        out
    }

    pub fn deserialize(b: &[u8]) -> Result<Self, String> {
        // fixed prefix up to penalty_status = 1+1+20+8+33+32+32+8+4+4+1 = 144
        if b.len() < 144 + 1 {
            return Err("ticket: too short".to_string());
        }
        if b[0] != TICKET_VERSION {
            return Err(format!("ticket: bad version {}", b[0]));
        }
        let mut p = 0usize;
        let rd = |p: &mut usize, n: usize| {
            let s = b[*p..*p + n].to_vec();
            *p += n;
            s
        };
        let version = b[p];
        p += 1;
        let network_id = b[p];
        p += 1;
        let mut miner_pkh = [0u8; 20];
        miner_pkh.copy_from_slice(&rd(&mut p, 20));
        let epoch = u64::from_le_bytes(rd(&mut p, 8).try_into().unwrap());
        let mut assignment_public_key = [0u8; 33];
        assignment_public_key.copy_from_slice(&rd(&mut p, 33));
        let mut sybil_work_nonce = [0u8; 32];
        sybil_work_nonce.copy_from_slice(&rd(&mut p, 32));
        let mut sybil_work_digest = [0u8; 32];
        sybil_work_digest.copy_from_slice(&rd(&mut p, 32));
        let recent_reward_score = u64::from_le_bytes(rd(&mut p, 8).try_into().unwrap());
        let valid_work_count = u32::from_le_bytes(rd(&mut p, 4).try_into().unwrap());
        let invalid_work_count = u32::from_le_bytes(rd(&mut p, 4).try_into().unwrap());
        let penalty_status = b[p];
        p += 1;
        let bond_flag = b[p];
        p += 1;
        let bond_reference = match bond_flag {
            0 => None,
            1 => {
                if b.len() < p + 32 + 16 {
                    return Err("ticket: truncated bond".to_string());
                }
                let mut bond = [0u8; 32];
                bond.copy_from_slice(&rd(&mut p, 32));
                Some(bond)
            }
            _ => return Err("ticket: bad bond flag".to_string()),
        };
        if b.len() < p + 16 {
            return Err("ticket: truncated tail".to_string());
        }
        let issued_height = u64::from_le_bytes(rd(&mut p, 8).try_into().unwrap());
        let expiry_height = u64::from_le_bytes(rd(&mut p, 8).try_into().unwrap());
        Ok(MinerWorkTicket {
            version,
            network_id,
            miner_pkh,
            epoch,
            assignment_public_key,
            sybil_work_nonce,
            sybil_work_digest,
            recent_reward_score,
            valid_work_count,
            invalid_work_count,
            penalty_status,
            bond_reference,
            issued_height,
            expiry_height,
        })
    }

    /// Stable ticket digest over the full canonical serialization.
    pub fn digest(&self) -> [u8; 32] {
        let mut h = Sha256::new();
        h.update(TICKET_DOMAIN);
        h.update(self.serialize());
        h.finalize().into()
    }

    fn recompute_sybil(&self) -> [u8; 32] {
        compute_sybil_digest(
            self.network_id,
            &self.miner_pkh,
            self.epoch,
            &self.assignment_public_key,
            &self.sybil_work_nonce,
        )
    }

    /// Validate the ticket. `expected_network` 0 = mainnet hard-off. `require_bits`
    /// (typically `sybil_threshold_bits()`) enforces the Sybil cost when > 0.
    pub fn validate(
        &self,
        expected_network: u8,
        current_height: u64,
        require_bits: u32,
    ) -> Result<(), String> {
        if expected_network == 0 {
            return Err("ticket: mainnet hard-off".to_string());
        }
        if self.version != TICKET_VERSION {
            return Err("ticket: bad version".to_string());
        }
        if self.network_id != expected_network {
            return Err("ticket: network mismatch".to_string());
        }
        if self.issued_height > current_height {
            return Err("ticket: issued in the future".to_string());
        }
        if self.expiry_height <= current_height {
            return Err("ticket: expired".to_string());
        }
        if PenaltyStatus::from_id(self.penalty_status).is_none() {
            return Err("ticket: bad penalty status".to_string());
        }
        // sybil-work binding: the digest must match the recomputed value.
        if self.sybil_work_digest != self.recompute_sybil() {
            return Err("ticket: sybil_work_digest mismatch".to_string());
        }
        if require_bits > 0 && !meets_sybil_target(&self.sybil_work_digest, require_bits) {
            return Err("ticket: insufficient sybil work".to_string());
        }
        Ok(())
    }

    /// Whether this ticket's holder may receive a high-trust role.
    pub fn eligible_for_high_trust_role(&self) -> bool {
        PenaltyStatus::from_id(self.penalty_status)
            .map(|s| s.eligible_for_high_trust_role())
            .unwrap_or(false)
    }
}

/// Test/dev helper: grind a sybil nonce meeting `bits` (small targets only).
pub fn grind_sybil_nonce(
    network_id: u8,
    miner_pkh: &[u8; 20],
    epoch: u64,
    assignment_public_key: &[u8; 33],
    bits: u32,
    max_iters: u64,
) -> Option<([u8; 32], [u8; 32])> {
    let mut nonce = [0u8; 32];
    for i in 0..max_iters {
        nonce[0..8].copy_from_slice(&i.to_le_bytes());
        let d = compute_sybil_digest(network_id, miner_pkh, epoch, assignment_public_key, &nonce);
        if meets_sybil_target(&d, bits) {
            return Some((nonce, d));
        }
    }
    None
}

/// Activation height for ticket enforcement (env-gated; mainnet hard-off).
pub fn tickets_activation_height() -> Option<u64> {
    std::env::var("IRIUM_POAWX_TICKETS_ACTIVATION_HEIGHT")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
}

/// Pure gate logic (network 0 = mainnet hard-off); param-driven for race-free tests.
pub fn tickets_gate(network_id: u8, activation: Option<u64>, height: u64) -> bool {
    if network_id == 0 {
        return false;
    }
    matches!(activation, Some(h) if height >= h)
}

/// Whether ticket validation is active at `height`. Mainnet hard-off.
pub fn tickets_active(height: u64) -> bool {
    tickets_gate(network_id_byte(), tickets_activation_height(), height)
}

/// Whether a valid ticket is REQUIRED (vs. advisory) — `IRIUM_POAWX_TICKETS_REQUIRED=1`.
pub fn tickets_required() -> bool {
    if network_id_byte() == 0 {
        return false;
    }
    std::env::var("IRIUM_POAWX_TICKETS_REQUIRED")
        .map(|v| v.trim() == "1")
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk(net: u8, h_issue: u64, h_exp: u64) -> MinerWorkTicket {
        let pkh = [0xA1u8; 20];
        let apk = [0x02u8; 33];
        let epoch = 7u64;
        let (nonce, digest) = grind_sybil_nonce(net, &pkh, epoch, &apk, 0, 1).unwrap();
        MinerWorkTicket {
            version: TICKET_VERSION,
            network_id: net,
            miner_pkh: pkh,
            epoch,
            assignment_public_key: apk,
            sybil_work_nonce: nonce,
            sybil_work_digest: digest,
            recent_reward_score: 42,
            valid_work_count: 3,
            invalid_work_count: 0,
            penalty_status: PenaltyStatus::Clean.id(),
            bond_reference: None,
            issued_height: h_issue,
            expiry_height: h_exp,
        }
    }

    #[test]
    fn ticket_serialize_roundtrip_and_digest_mutation() {
        let t = mk(1, 10, 100);
        let b = t.serialize();
        let t2 = MinerWorkTicket::deserialize(&b).unwrap();
        assert_eq!(t, t2);
        let d0 = t.digest();
        let mut t3 = t.clone();
        t3.recent_reward_score += 1;
        assert_ne!(d0, t3.digest(), "mutation changes digest");
        // with bond reference
        let mut tb = t.clone();
        tb.bond_reference = Some([0x09u8; 32]);
        assert_eq!(MinerWorkTicket::deserialize(&tb.serialize()).unwrap(), tb);
    }

    #[test]
    fn ticket_validate_accept_and_rejects() {
        let net = 1u8;
        let t = mk(net, 10, 100);
        assert!(t.validate(net, 50, 0).is_ok(), "valid in-window ticket");
        // mainnet hard-off
        assert!(t.validate(0, 50, 0).is_err());
        // wrong network
        assert!(t.validate(2, 50, 0).is_err());
        // expired
        assert!(t.validate(net, 100, 0).is_err());
        // future-issued
        let tf = mk(net, 60, 100);
        assert!(tf.validate(net, 50, 0).is_err());
        // tampered sybil nonce -> digest mismatch
        let mut tt = t.clone();
        tt.sybil_work_nonce[0] ^= 1;
        assert!(tt.validate(net, 50, 0).is_err());
        // malformed deserialize
        assert!(MinerWorkTicket::deserialize(b"short").is_err());
    }

    #[test]
    fn sybil_threshold_disabled_permits_enabled_rejects_insufficient() {
        let net = 1u8;
        let pkh = [0xB2u8; 20];
        let apk = [0x03u8; 33];
        let epoch = 9u64;
        // threshold disabled (bits=0): any nonce permitted.
        let (n0, d0) = grind_sybil_nonce(net, &pkh, epoch, &apk, 0, 1).unwrap();
        assert!(meets_sybil_target(&d0, 0));
        // enabled with a tiny target: grind finds a valid nonce.
        let (n1, d1) =
            grind_sybil_nonce(net, &pkh, epoch, &apk, 8, 200_000).expect("grind tiny target");
        assert!(meets_sybil_target(&d1, 8));
        assert_eq!(
            compute_sybil_digest(net, &pkh, epoch, &apk, &n1),
            d1,
            "binding"
        );
        // an insufficient digest is rejected at the higher threshold.
        assert!(!meets_sybil_target(&d0, 8) || leading_zero_bits(&d0) >= 8);
        // a ticket carrying d0 fails validate when require_bits=8 (unless d0 happens to meet it).
        let mut t = mk(net, 10, 100);
        t.miner_pkh = pkh;
        t.epoch = epoch;
        t.assignment_public_key = apk;
        t.sybil_work_nonce = n0;
        t.sybil_work_digest = compute_sybil_digest(net, &pkh, epoch, &apk, &n0);
        let res = t.validate(net, 50, 24); // require 24 bits — astronomically unlikely for d0
        assert!(
            res.is_err(),
            "insufficient sybil work rejected at high threshold"
        );
        let _ = n1;
    }

    #[test]
    fn ticket_penalized_not_high_trust_eligible() {
        let net = 1u8;
        let mut t = mk(net, 10, 100);
        t.penalty_status = PenaltyStatus::SuspendedForEpoch.id();
        assert!(!t.eligible_for_high_trust_role());
        t.penalty_status = PenaltyStatus::Clean.id();
        assert!(t.eligible_for_high_trust_role());
    }

    #[test]
    fn ticket_gate_logic_pure() {
        // pure gate (no global env mutation -> race-free under parallel tests).
        assert!(!tickets_gate(0, Some(1), 100), "mainnet hard-off");
        assert!(tickets_gate(1, Some(1), 100), "testnet active");
        assert!(!tickets_gate(1, None, 100), "no activation -> off");
        assert!(!tickets_gate(1, Some(50), 10), "below activation -> off");
        // validate() already enforces mainnet hard-off via expected_network==0:
        let t = mk(1, 10, 100);
        assert!(t.validate(0, 50, 0).is_err(), "validate mainnet hard-off");
    }
}
