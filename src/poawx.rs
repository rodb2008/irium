//! PoAW-X consensus helpers shared between the daemon, P2P layer, and tests.
//!
//! Env-var reads are intentionally absent here — callers own activation gating.

use crate::block::Block;
use sha2::{Digest, Sha256};

const IRX1_TAG: &[u8] = b"irx1";
/// 0x6a 0x24 b"irx1" <32-byte root> = 38 bytes
const IRX1_SCRIPT_LEN: usize = 38;

/// Returns `true` when the block coinbase contains a well-formed `irx1`
/// OP_RETURN commitment with a non-zero 32-byte root.
///
/// Wire format: `0x6a 0x24 "irx1" <32-byte root>` (38 bytes); root must not be all-zeros.
pub fn block_has_irx1_commitment(block: &Block) -> bool {
    let coinbase = match block.transactions.first() {
        Some(tx) => tx,
        None => return false,
    };
    coinbase.outputs.iter().any(|out| {
        let s = &out.script_pubkey;
        s.len() == IRX1_SCRIPT_LEN
            && s[0] == 0x6a
            && s[1] == 0x24
            && &s[2..6] == IRX1_TAG
            && s[6..38] != [0u8; 32]
    })
}

/// Eight-byte magic that precedes the PoAW-X receipt section appended after
/// all transactions in the block wire encoding. Chosen to be unambiguous
/// as a transaction start (version `0x575041AF` is not a real tx version).
pub const POAWX_RECEIPT_SECTION_MAGIC: &[u8; 8] = b"POAWXR\x01\x00";

/// Worker reward per receipt as permille (1/1000) of the block subsidy.
pub const POAWX_WORKER_REWARD_PERMILLE: u64 = 100;

/// Phase 18B: eight-byte magic for the v2 receipt section. A block uses this
/// (instead of `POAWX_RECEIPT_SECTION_MAGIC`) only when it carries at least one
/// mode-1 (delegated) receipt. Pure mode-0 blocks keep the v1 magic and are
/// byte-for-byte identical to the Phase 13-A encoding.
pub const POAWX_RECEIPT_SECTION_MAGIC_V2: &[u8; 8] = b"POAWXR\x02\x00";

/// Phase 18B: domain separator for the one-time miner delegation signature.
pub const DOMAIN_DELEG: &[u8] = b"irium.poawx.delegation.v1";

/// Receipt mode discriminator used in the v2 receipt section.
pub const RECEIPT_MODE_DIRECT: u8 = 0;
pub const RECEIPT_MODE_DELEGATED: u8 = 1;

// ── Phase 20: multi-role reward split (testnet/devnet-gated) ──────────────────
//
// Owner-supplied spec. Basis points of the block subsidy per role; total = 10000.
// PRIMARY_MINER is the miner/block-producing identity = the receipt `worker_pkh`
// (never the pool delegate key). COMPUTE/VERIFY/SUPPORT are consensus-bound payout
// roles only here; their eligibility/fairness assignment is a SEPARATE task (the
// CPU/GPU/ASIC fairness matrix), and this code invents no assignment rules.
//
// Activation is gated by `IRIUM_POAWX_MULTI_ROLE_REWARD_ACTIVATION_HEIGHT`
// (testnet/devnet only); mainnet is hard-off until an explicit future governance
// activation. Before activation, nothing here is used and existing Phase 18/19
// behavior is byte-identical.
pub const MULTI_ROLE_PRIMARY_BPS: u64 = 5500;
pub const MULTI_ROLE_COMPUTE_BPS: u64 = 2200;
pub const MULTI_ROLE_VERIFY_BPS: u64 = 1300;
pub const MULTI_ROLE_SUPPORT_BPS: u64 = 1000;
pub const MULTI_ROLE_TOTAL_BPS: u64 = 10000;

/// The three non-primary role payout identities for a multi-role PoAW-X block.
/// The PRIMARY role pkh is the receipt `worker_pkh` (the miner = payout identity)
/// and is intentionally NOT stored here so it can never be replaced by a pool key.
/// Canonical 60-byte wire encoding: compute || verify || support (20 bytes each).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleReward {
    pub compute_contributor_pkh: [u8; 20],
    pub verify_contributor_pkh: [u8; 20],
    pub support_contributor_pkh: [u8; 20],
}

impl RoleReward {
    /// Fixed wire size: 3 × 20 = 60 bytes.
    pub const WIRE_SIZE: usize = 60;

    pub fn serialize(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(Self::WIRE_SIZE);
        out.extend_from_slice(&self.compute_contributor_pkh);
        out.extend_from_slice(&self.verify_contributor_pkh);
        out.extend_from_slice(&self.support_contributor_pkh);
        out
    }

    pub fn deserialize(raw: &[u8]) -> Result<Self, String> {
        if raw.len() < Self::WIRE_SIZE {
            return Err(format!(
                "role reward too short: {} < {}",
                raw.len(),
                Self::WIRE_SIZE
            ));
        }
        let mut compute_contributor_pkh = [0u8; 20];
        compute_contributor_pkh.copy_from_slice(&raw[0..20]);
        let mut verify_contributor_pkh = [0u8; 20];
        verify_contributor_pkh.copy_from_slice(&raw[20..40]);
        let mut support_contributor_pkh = [0u8; 20];
        support_contributor_pkh.copy_from_slice(&raw[40..60]);
        Ok(Self {
            compute_contributor_pkh,
            verify_contributor_pkh,
            support_contributor_pkh,
        })
    }

    pub fn digest(&self) -> [u8; 32] {
        let mut h = Sha256::new();
        h.update(self.serialize());
        h.finalize().into()
    }
}

/// Split `total_reward` (atomic units) into the four canonical role amounts in
/// fixed order `[primary, compute, verify, support]`. Each non-primary role gets
/// `floor(total * bps / 10000)`; any integer-division remainder goes to PRIMARY.
/// The returned amounts always sum to exactly `total_reward` (no over/underpay).
/// Uses u128 intermediates to avoid overflow.
pub fn multi_role_amounts(total_reward: u64) -> [u64; 4] {
    let bps = |b: u64| -> u64 { ((total_reward as u128 * b as u128) / 10000u128) as u64 };
    let compute = bps(MULTI_ROLE_COMPUTE_BPS);
    let verify = bps(MULTI_ROLE_VERIFY_BPS);
    let support = bps(MULTI_ROLE_SUPPORT_BPS);
    let primary_floor = bps(MULTI_ROLE_PRIMARY_BPS);
    // remainder (from all four floors) goes to PRIMARY so the sum is exact.
    let remainder = total_reward - primary_floor - compute - verify - support;
    let primary = primary_floor + remainder;
    [primary, compute, verify, support]
}

/// Per-receipt data embedded in the block wire/storage format so that every
/// node can validate PoAW-X receipts from block-contained data (Phase 13-A).
/// All multi-byte integers are little-endian.
#[derive(Debug, Clone, PartialEq)]
pub struct PoawxBlockReceipt {
    pub height: u64,
    /// Raw lane byte (e.g. `b'A'`).
    pub lane: u8,
    pub worker_pkh: [u8; 20],
    pub worker_pubkey: [u8; 33],
    pub worker_sig: [u8; 64],
    pub solution: [u8; 8],
    pub commitment_nonce: [u8; 32],
    /// Phase 18B: `None` => mode-0 (direct miner-signed; `worker_pubkey`/
    /// `worker_sig` are the miner's own). `Some` => mode-1 (delegated;
    /// `worker_pubkey`/`worker_sig` are the pool delegate's signer key, and
    /// `worker_pkh` still belongs to the miner = payout identity).
    pub delegation: Option<Delegation>,
}

impl PoawxBlockReceipt {
    /// Fixed wire size of the legacy mode-0 element: 8 + 1 + 20 + 33 + 64 + 8 + 32 = 166 bytes.
    pub const WIRE_SIZE: usize = 8 + 1 + 20 + 33 + 64 + 8 + 32;

    /// Mode discriminator for this receipt.
    pub fn mode(&self) -> u8 {
        if self.delegation.is_some() {
            RECEIPT_MODE_DELEGATED
        } else {
            RECEIPT_MODE_DIRECT
        }
    }

    /// Phase 18B v2 element encoding: `mode(1)` followed by the legacy 166-byte
    /// payload, and (mode-1 only) the 226-byte delegation blob. Mode-0 here is
    /// the legacy payload prefixed with a single `0x00` byte.
    pub fn serialize_v2(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(1 + Self::WIRE_SIZE + Delegation::WIRE_SIZE);
        out.push(self.mode());
        out.extend_from_slice(&self.serialize());
        if let Some(d) = &self.delegation {
            out.extend_from_slice(&d.serialize());
        }
        out
    }

    /// Parse a single v2 element, returning the receipt and the number of bytes
    /// consumed. Errors on unknown mode or truncation.
    pub fn deserialize_v2(raw: &[u8]) -> Result<(Self, usize), String> {
        if raw.is_empty() {
            return Err("poawx v2 receipt: missing mode byte".to_string());
        }
        let mode = raw[0];
        let body = &raw[1..];
        if body.len() < Self::WIRE_SIZE {
            return Err("poawx v2 receipt: legacy body truncated".to_string());
        }
        let mut receipt = Self::deserialize(body)?;
        match mode {
            RECEIPT_MODE_DIRECT => {
                receipt.delegation = None;
                Ok((receipt, 1 + Self::WIRE_SIZE))
            }
            RECEIPT_MODE_DELEGATED => {
                let deleg_start = Self::WIRE_SIZE;
                if body.len() < deleg_start + Delegation::WIRE_SIZE {
                    return Err("poawx v2 receipt: delegation truncated".to_string());
                }
                let d = Delegation::deserialize(&body[deleg_start..])?;
                receipt.delegation = Some(d);
                Ok((receipt, 1 + Self::WIRE_SIZE + Delegation::WIRE_SIZE))
            }
            other => Err(format!("poawx v2 receipt: unknown mode {}", other)),
        }
    }

    pub fn serialize(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(Self::WIRE_SIZE);
        out.extend_from_slice(&self.height.to_le_bytes());
        out.push(self.lane);
        out.extend_from_slice(&self.worker_pkh);
        out.extend_from_slice(&self.worker_pubkey);
        out.extend_from_slice(&self.worker_sig);
        out.extend_from_slice(&self.solution);
        out.extend_from_slice(&self.commitment_nonce);
        out
    }

    pub fn deserialize(raw: &[u8]) -> Result<Self, String> {
        if raw.len() < Self::WIRE_SIZE {
            return Err(format!(
                "poawx receipt too short: {} < {}",
                raw.len(),
                Self::WIRE_SIZE
            ));
        }
        let mut off = 0usize;
        let height = u64::from_le_bytes(raw[off..off + 8].try_into().expect("slice len checked"));
        off += 8;
        let lane = raw[off];
        off += 1;
        let mut worker_pkh = [0u8; 20];
        worker_pkh.copy_from_slice(&raw[off..off + 20]);
        off += 20;
        let mut worker_pubkey = [0u8; 33];
        worker_pubkey.copy_from_slice(&raw[off..off + 33]);
        off += 33;
        let mut worker_sig = [0u8; 64];
        worker_sig.copy_from_slice(&raw[off..off + 64]);
        off += 64;
        let mut solution = [0u8; 8];
        solution.copy_from_slice(&raw[off..off + 8]);
        off += 8;
        let mut commitment_nonce = [0u8; 32];
        commitment_nonce.copy_from_slice(&raw[off..off + 32]);
        Ok(Self {
            height,
            lane,
            worker_pkh,
            worker_pubkey,
            worker_sig,
            solution,
            commitment_nonce,
            delegation: None,
        })
    }
}

/// Phase 18B: a one-time miner delegation authorizing a specific pool delegate
/// key (`pool_pubkey`) to create PoAW-X receipts paying the miner (`miner_pubkey`,
/// whose HASH160 is the receipt `worker_pkh`). Signed by the miner key over
/// `message_hash()`; the signature never contains a private key. All multi-byte
/// integers are little-endian.
#[derive(Debug, Clone, PartialEq)]
pub struct Delegation {
    pub deleg_version: u8,
    pub network_id: u8,
    pub miner_pubkey: [u8; 33],
    pub pool_pubkey: [u8; 33],
    pub worker_tag: [u8; 32],
    pub expiry_height: u64,
    pub fee_bps: u16,
    pub fee_pkh: [u8; 20],
    pub deleg_nonce: [u8; 32],
    pub delegation_sig: [u8; 64],
}

impl Delegation {
    /// Current delegation format version.
    pub const VERSION: u8 = 1;
    /// Fixed wire size: 1 + 1 + 33 + 33 + 32 + 8 + 2 + 20 + 32 + 64 = 226 bytes.
    pub const WIRE_SIZE: usize = 1 + 1 + 33 + 33 + 32 + 8 + 2 + 20 + 32 + 64;

    pub fn serialize(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(Self::WIRE_SIZE);
        out.push(self.deleg_version);
        out.push(self.network_id);
        out.extend_from_slice(&self.miner_pubkey);
        out.extend_from_slice(&self.pool_pubkey);
        out.extend_from_slice(&self.worker_tag);
        out.extend_from_slice(&self.expiry_height.to_le_bytes());
        out.extend_from_slice(&self.fee_bps.to_le_bytes());
        out.extend_from_slice(&self.fee_pkh);
        out.extend_from_slice(&self.deleg_nonce);
        out.extend_from_slice(&self.delegation_sig);
        out
    }

    pub fn deserialize(raw: &[u8]) -> Result<Self, String> {
        if raw.len() < Self::WIRE_SIZE {
            return Err(format!(
                "delegation too short: {} < {}",
                raw.len(),
                Self::WIRE_SIZE
            ));
        }
        let mut off = 0usize;
        let deleg_version = raw[off];
        off += 1;
        let network_id = raw[off];
        off += 1;
        let mut miner_pubkey = [0u8; 33];
        miner_pubkey.copy_from_slice(&raw[off..off + 33]);
        off += 33;
        let mut pool_pubkey = [0u8; 33];
        pool_pubkey.copy_from_slice(&raw[off..off + 33]);
        off += 33;
        let mut worker_tag = [0u8; 32];
        worker_tag.copy_from_slice(&raw[off..off + 32]);
        off += 32;
        let expiry_height =
            u64::from_le_bytes(raw[off..off + 8].try_into().expect("slice len checked"));
        off += 8;
        let fee_bps = u16::from_le_bytes(raw[off..off + 2].try_into().expect("slice len checked"));
        off += 2;
        let mut fee_pkh = [0u8; 20];
        fee_pkh.copy_from_slice(&raw[off..off + 20]);
        off += 20;
        let mut deleg_nonce = [0u8; 32];
        deleg_nonce.copy_from_slice(&raw[off..off + 32]);
        off += 32;
        let mut delegation_sig = [0u8; 64];
        delegation_sig.copy_from_slice(&raw[off..off + 64]);
        Ok(Self {
            deleg_version,
            network_id,
            miner_pubkey,
            pool_pubkey,
            worker_tag,
            expiry_height,
            fee_bps,
            fee_pkh,
            deleg_nonce,
            delegation_sig,
        })
    }

    /// The 32-byte message the miner signs. Excludes `delegation_sig`.
    pub fn message_hash(&self) -> [u8; 32] {
        let mut h = Sha256::new();
        h.update(DOMAIN_DELEG);
        h.update([self.deleg_version]);
        h.update([self.network_id]);
        h.update(self.miner_pubkey);
        h.update(self.pool_pubkey);
        h.update(self.worker_tag);
        h.update(self.expiry_height.to_le_bytes());
        h.update(self.fee_bps.to_le_bytes());
        h.update(self.fee_pkh);
        h.update(self.deleg_nonce);
        h.finalize().into()
    }

    /// Tamper-evident digest over the full delegation bytes (including the
    /// signature). Bound into the mode-1 irx1 inner hash so any alteration of
    /// the embedded delegation changes the receipts root.
    pub fn digest(&self) -> [u8; 32] {
        let mut h = Sha256::new();
        h.update(self.serialize());
        h.finalize().into()
    }

    /// HASH160(miner_pubkey) — the miner pkh that must equal the receipt
    /// `worker_pkh` (payout identity).
    pub fn miner_pkh(&self) -> [u8; 20] {
        let sha = Sha256::digest(self.miner_pubkey);
        let rip = ripemd::Ripemd160::digest(sha);
        let mut pkh = [0u8; 20];
        pkh.copy_from_slice(&rip);
        pkh
    }

    /// Verify the miner's delegation signature over `message_hash()`.
    pub fn verify_signature(&self) -> Result<(), &'static str> {
        use k256::ecdsa::signature::hazmat::PrehashVerifier;
        use k256::ecdsa::{Signature, VerifyingKey};
        let vk = VerifyingKey::from_sec1_bytes(&self.miner_pubkey)
            .map_err(|_| "delegation: invalid miner_pubkey")?;
        let sig = Signature::from_slice(&self.delegation_sig)
            .map_err(|_| "delegation: malformed delegation_sig")?;
        vk.verify_prehash(&self.message_hash(), &sig)
            .map_err(|_| "delegation: signature verification failed")
    }
}

/// Extracts the 32-byte `irx1` root from the block coinbase if a well-formed
/// `irx1` OP_RETURN output is present. Root may be all-zeros.
pub fn irx1_root_from_block_bytes(block: &Block) -> Option<[u8; 32]> {
    let coinbase = block.transactions.first()?;
    for out in &coinbase.outputs {
        let s = &out.script_pubkey;
        if s.len() == IRX1_SCRIPT_LEN && s[0] == 0x6a && s[1] == 0x24 && &s[2..6] == IRX1_TAG {
            let mut root = [0u8; 32];
            root.copy_from_slice(&s[6..38]);
            return Some(root);
        }
    }
    None
}

/// Counts leading zero bits in a 32-byte hash.
/// Used by connect_block (Phase 13-B) and submit_block_extended for puzzle PoW checks.
pub fn count_leading_zero_bits(hash: &[u8; 32]) -> u32 {
    let mut bits = 0u32;
    for &b in hash.iter() {
        let z = b.leading_zeros();
        bits += z;
        if z < 8 {
            break;
        }
    }
    bits
}

/// Computes the irx1 root from block-contained receipt data deterministically.
///
/// Sort order and inner hash algorithm match `compute_poawx_receipts_root` in
/// iriumd.rs (which operates on `PoawxPendingReceipt` hex fields):
///   inner = SHA256(height_le8 || lane_byte || worker_pkh_bytes ||
///                  solution_bytes || commitment_nonce_bytes)
///   root  = SHA256(concat inner hashes; receipts sorted by
///                  (height, lane, worker_pkh, commitment_nonce))
pub fn irx1_root_from_block_receipts(receipts: &[PoawxBlockReceipt]) -> [u8; 32] {
    let mut sorted: Vec<&PoawxBlockReceipt> = receipts.iter().collect();
    sorted.sort_unstable_by(|a, b| {
        a.height
            .cmp(&b.height)
            .then_with(|| a.lane.cmp(&b.lane))
            .then_with(|| a.worker_pkh.cmp(&b.worker_pkh))
            .then_with(|| a.commitment_nonce.cmp(&b.commitment_nonce))
    });
    let mut outer = Sha256::new();
    for r in &sorted {
        let mut inner = Sha256::new();
        inner.update(r.height.to_le_bytes());
        inner.update([r.lane]);
        inner.update(r.worker_pkh);
        inner.update(r.solution);
        inner.update(r.commitment_nonce);
        // Phase 18B: mode-1 binds the delegation digest into the inner hash so
        // the embedded delegation is tamper-evident. Mode-0 (delegation None)
        // is byte-identical to the Phase 13-A inner hash.
        if let Some(d) = &r.delegation {
            inner.update(d.digest());
        }
        outer.update(inner.finalize());
    }
    outer.finalize().into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::Block;
    use crate::tx::{Transaction, TxInput, TxOutput};

    fn make_block_with_coinbase_script(script: Vec<u8>) -> Block {
        use crate::block::BlockHeader;
        Block {
            header: BlockHeader {
                version: 1,
                prev_hash: [0u8; 32],
                merkle_root: [0u8; 32],
                time: 0,
                bits: 0x207fffff,
                nonce: 0,
            },
            transactions: vec![Transaction {
                version: 1,
                inputs: vec![TxInput {
                    prev_txid: [0u8; 32],
                    prev_index: 0xffff_ffff,
                    script_sig: vec![0x01, 0x00],
                    sequence: 0xffff_ffff,
                }],
                outputs: vec![
                    TxOutput {
                        value: 50_0000_0000,
                        script_pubkey: vec![0x51],
                    },
                    TxOutput {
                        value: 0,
                        script_pubkey: script,
                    },
                ],
                locktime: 0,
            }],
            auxpow: None,
            poawx_receipts: None,
        }
    }

    fn valid_irx1_script(root: [u8; 32]) -> Vec<u8> {
        let mut s = vec![0x6a, 0x24];
        s.extend_from_slice(b"irx1");
        s.extend_from_slice(&root);
        s
    }

    #[test]
    fn no_coinbase_returns_false() {
        use crate::block::BlockHeader;
        let block = Block {
            header: BlockHeader {
                version: 1,
                prev_hash: [0u8; 32],
                merkle_root: [0u8; 32],
                time: 0,
                bits: 0,
                nonce: 0,
            },
            transactions: vec![],
            auxpow: None,
            poawx_receipts: None,
        };
        assert!(!block_has_irx1_commitment(&block));
    }

    #[test]
    fn no_irx1_output_returns_false() {
        let block = make_block_with_coinbase_script(vec![0x51]);
        assert!(!block_has_irx1_commitment(&block));
    }

    #[test]
    fn irx1_with_nonzero_root_returns_true() {
        let mut root = [0u8; 32];
        root[0] = 0xde;
        root[31] = 0xad;
        let block = make_block_with_coinbase_script(valid_irx1_script(root));
        assert!(block_has_irx1_commitment(&block));
    }

    #[test]
    fn irx1_with_zero_root_returns_false() {
        let block = make_block_with_coinbase_script(valid_irx1_script([0u8; 32]));
        assert!(!block_has_irx1_commitment(&block));
    }

    #[test]
    fn wrong_tag_returns_false() {
        let mut s = vec![0x6a, 0x24];
        s.extend_from_slice(b"irx2");
        s.extend_from_slice(&[0xab; 32]);
        let block = make_block_with_coinbase_script(s);
        assert!(!block_has_irx1_commitment(&block));
    }

    #[test]
    fn too_short_script_returns_false() {
        let block = make_block_with_coinbase_script(vec![0x6a, 0x24, b'i', b'r', b'x', b'1']);
        assert!(!block_has_irx1_commitment(&block));
    }

    #[test]
    fn irx1_root_extraction_works() {
        let mut expected = [0u8; 32];
        expected[0] = 0x3a;
        expected[31] = 0xfe;
        let block = make_block_with_coinbase_script(valid_irx1_script(expected));
        assert_eq!(irx1_root_from_block_bytes(&block), Some(expected));
    }

    #[test]
    fn irx1_root_absent_returns_none() {
        let block = make_block_with_coinbase_script(vec![0x51]);
        assert_eq!(irx1_root_from_block_bytes(&block), None);
    }

    // ── Phase 13-A receipt wire format tests ─────────────────────────────

    fn make_test_receipt(height: u64) -> PoawxBlockReceipt {
        PoawxBlockReceipt {
            height,
            lane: b'A',
            worker_pkh: [0x11u8; 20],
            worker_pubkey: [0x22u8; 33],
            worker_sig: [0x33u8; 64],
            solution: [0x44u8; 8],
            commitment_nonce: [0x55u8; 32],
            delegation: None,
        }
    }

    #[test]
    fn phase13a_receipt_serialize_deserialize_roundtrip() {
        let r = make_test_receipt(42);
        let bytes = r.serialize();
        assert_eq!(bytes.len(), PoawxBlockReceipt::WIRE_SIZE);
        let r2 = PoawxBlockReceipt::deserialize(&bytes).expect("deserialize");
        assert_eq!(r, r2);
        assert_eq!(r2.height, 42);
        assert_eq!(r2.lane, b'A');
        assert_eq!(r2.worker_pkh, [0x11u8; 20]);
        assert_eq!(r2.worker_pubkey, [0x22u8; 33]);
        assert_eq!(r2.worker_sig, [0x33u8; 64]);
        assert_eq!(r2.solution, [0x44u8; 8]);
        assert_eq!(r2.commitment_nonce, [0x55u8; 32]);
    }

    // ── Phase 20: multi-role reward split primitives ─────────────────────────

    #[test]
    fn phase20_multi_role_amounts_exact_split_and_remainder() {
        // 55/22/13/10 of a clean total divides exactly.
        let amts = multi_role_amounts(10_000);
        assert_eq!(amts, [5500, 2200, 1300, 1000]);
        assert_eq!(amts.iter().sum::<u64>(), 10_000);
        // A reward that does not divide evenly: remainder goes to PRIMARY, sum exact.
        let total = 5_000_000_001u64; // odd, forces a remainder
        let a = multi_role_amounts(total);
        assert_eq!(a[1], (total as u128 * 2200 / 10000) as u64);
        assert_eq!(a[2], (total as u128 * 1300 / 10000) as u64);
        assert_eq!(a[3], (total as u128 * 1000 / 10000) as u64);
        assert_eq!(a.iter().sum::<u64>(), total, "sum equals total exactly");
        // remainder lands in PRIMARY: primary >= its floor.
        assert!(a[0] >= (total as u128 * 5500 / 10000) as u64);
        // zero reward → all zero.
        assert_eq!(multi_role_amounts(0), [0, 0, 0, 0]);
    }

    #[test]
    fn phase20_role_reward_wire_roundtrip() {
        let r = RoleReward {
            compute_contributor_pkh: [0xC0u8; 20],
            verify_contributor_pkh: [0x7Eu8; 20],
            support_contributor_pkh: [0x5Au8; 20],
        };
        let bytes = r.serialize();
        assert_eq!(bytes.len(), RoleReward::WIRE_SIZE);
        assert_eq!(RoleReward::WIRE_SIZE, 60);
        let r2 = RoleReward::deserialize(&bytes).expect("deserialize");
        assert_eq!(r, r2);
        // digest is deterministic + sensitive to content.
        assert_eq!(r.digest(), r2.digest());
        let mut r3 = r.clone();
        r3.support_contributor_pkh = [0x5Bu8; 20];
        assert_ne!(r.digest(), r3.digest());
        // truncated input rejects.
        assert!(RoleReward::deserialize(&bytes[..59]).is_err());
    }

    #[test]
    fn phase20_role_reward_json_roundtrip() {
        // serde round-trip of the role pkhs (hex) — the persistence carrier shape.
        let r = RoleReward {
            compute_contributor_pkh: [1u8; 20],
            verify_contributor_pkh: [2u8; 20],
            support_contributor_pkh: [3u8; 20],
        };
        let j = serde_json::json!({
            "compute_contributor_pkh": hex::encode(r.compute_contributor_pkh),
            "verify_contributor_pkh": hex::encode(r.verify_contributor_pkh),
            "support_contributor_pkh": hex::encode(r.support_contributor_pkh),
        });
        let s = serde_json::to_string(&j).unwrap();
        let v: serde_json::Value = serde_json::from_str(&s).unwrap();
        let back = RoleReward {
            compute_contributor_pkh: {
                let b = hex::decode(v["compute_contributor_pkh"].as_str().unwrap()).unwrap();
                let mut a = [0u8; 20];
                a.copy_from_slice(&b);
                a
            },
            verify_contributor_pkh: {
                let b = hex::decode(v["verify_contributor_pkh"].as_str().unwrap()).unwrap();
                let mut a = [0u8; 20];
                a.copy_from_slice(&b);
                a
            },
            support_contributor_pkh: {
                let b = hex::decode(v["support_contributor_pkh"].as_str().unwrap()).unwrap();
                let mut a = [0u8; 20];
                a.copy_from_slice(&b);
                a
            },
        };
        assert_eq!(r, back);
    }

    #[test]
    fn phase20_bps_constants_total_10000() {
        assert_eq!(
            MULTI_ROLE_PRIMARY_BPS
                + MULTI_ROLE_COMPUTE_BPS
                + MULTI_ROLE_VERIFY_BPS
                + MULTI_ROLE_SUPPORT_BPS,
            MULTI_ROLE_TOTAL_BPS
        );
        assert_eq!(MULTI_ROLE_TOTAL_BPS, 10_000);
    }

    #[test]
    fn phase20_v1_v2_receipt_encoding_unchanged() {
        // Adding the multi-role primitives must NOT change the existing receipt
        // wire size or mode-0 encoding (pre-activation byte-identical guarantee).
        assert_eq!(PoawxBlockReceipt::WIRE_SIZE, 166);
        let r = make_test_receipt(7);
        assert_eq!(r.serialize().len(), 166);
        assert_eq!(r.mode(), RECEIPT_MODE_DIRECT);
        // mode-0 v2 element = 1 (mode) + 166 (legacy), no delegation/role bytes.
        assert_eq!(r.serialize_v2().len(), 1 + 166);
    }

    #[test]
    fn phase13a_receipt_wire_size_is_166() {
        assert_eq!(PoawxBlockReceipt::WIRE_SIZE, 166);
        let r = make_test_receipt(1);
        assert_eq!(r.serialize().len(), 166);
    }

    #[test]
    fn phase13a_receipt_truncated_deserialize_fails() {
        let r = make_test_receipt(1);
        let bytes = r.serialize();
        // Truncate by 1 byte — must error.
        assert!(
            PoawxBlockReceipt::deserialize(&bytes[..165]).is_err(),
            "truncated bytes must fail to deserialize"
        );
        // Empty slice — must also error.
        assert!(PoawxBlockReceipt::deserialize(&[]).is_err());
    }

    #[test]
    fn phase13a_receipt_section_magic_length() {
        assert_eq!(POAWX_RECEIPT_SECTION_MAGIC.len(), 8);
    }

    // ── Phase 18B delegated-receipt primitive tests ──────────────────────

    fn test_sk() -> k256::ecdsa::SigningKey {
        k256::ecdsa::SigningKey::from_slice(&[7u8; 32]).expect("valid sk")
    }

    fn miner_pubkey_from(sk: &k256::ecdsa::SigningKey) -> [u8; 33] {
        let vk = k256::ecdsa::VerifyingKey::from(sk);
        let enc = vk.to_encoded_point(true);
        let mut pk = [0u8; 33];
        pk.copy_from_slice(enc.as_bytes());
        pk
    }

    fn make_signed_delegation(sk: &k256::ecdsa::SigningKey, fee_bps: u16) -> Delegation {
        use k256::ecdsa::signature::hazmat::PrehashSigner;
        let mut d = Delegation {
            deleg_version: Delegation::VERSION,
            network_id: 1,
            miner_pubkey: miner_pubkey_from(sk),
            pool_pubkey: [0x02u8; 33],
            worker_tag: [0xaau8; 32],
            expiry_height: 1000,
            fee_bps,
            fee_pkh: [0u8; 20],
            deleg_nonce: [0xcdu8; 32],
            delegation_sig: [0u8; 64],
        };
        let sig: k256::ecdsa::Signature = sk.sign_prehash(&d.message_hash()).unwrap();
        d.delegation_sig.copy_from_slice(&sig.to_bytes());
        d
    }

    fn mode1_receipt(height: u64, d: Delegation) -> PoawxBlockReceipt {
        let mut r = make_test_receipt(height);
        r.delegation = Some(d);
        r
    }

    #[test]
    fn phase18b_delegation_roundtrip() {
        let sk = test_sk();
        let d = make_signed_delegation(&sk, 0);
        let bytes = d.serialize();
        assert_eq!(Delegation::WIRE_SIZE, 226);
        assert_eq!(bytes.len(), Delegation::WIRE_SIZE);
        let d2 = Delegation::deserialize(&bytes).expect("deserialize");
        assert_eq!(d, d2);
    }

    #[test]
    fn phase18b_delegation_truncated_fails() {
        let sk = test_sk();
        let d = make_signed_delegation(&sk, 0);
        let bytes = d.serialize();
        assert!(Delegation::deserialize(&bytes[..225]).is_err());
        assert!(Delegation::deserialize(&[]).is_err());
    }

    #[test]
    fn phase18b_delegation_signature_verifies_and_tamper_fails() {
        let sk = test_sk();
        let d = make_signed_delegation(&sk, 0);
        assert!(d.verify_signature().is_ok());
        let mut t = d.clone();
        t.expiry_height ^= 1;
        assert!(t.verify_signature().is_err(), "tampered expiry must fail");
        let mut t2 = d.clone();
        t2.pool_pubkey[0] ^= 0xff;
        assert!(
            t2.verify_signature().is_err(),
            "tampered pool_pubkey must fail"
        );
    }

    #[test]
    fn phase18b_delegation_miner_pkh_matches_pubkey() {
        let sk = test_sk();
        let d = make_signed_delegation(&sk, 0);
        let pkh = d.miner_pkh();
        let sha = Sha256::digest(d.miner_pubkey);
        let rip = ripemd::Ripemd160::digest(sha);
        assert_eq!(&pkh[..], &rip[..]);
    }

    #[test]
    fn phase18b_message_hash_excludes_sig() {
        let sk = test_sk();
        let d = make_signed_delegation(&sk, 0);
        let h1 = d.message_hash();
        let mut d2 = d.clone();
        d2.delegation_sig = [0xffu8; 64];
        assert_eq!(h1, d2.message_hash(), "sig must not affect message_hash");
    }

    #[test]
    fn phase18b_digest_changes_with_any_field() {
        let sk = test_sk();
        let d = make_signed_delegation(&sk, 0);
        let base = d.digest();
        let mut a = d.clone();
        a.network_id ^= 1;
        assert_ne!(base, a.digest());
        let mut b = d.clone();
        b.fee_bps ^= 1;
        assert_ne!(base, b.digest());
        let mut c = d.clone();
        c.deleg_nonce[0] ^= 1;
        assert_ne!(base, c.digest());
        let mut e = d.clone();
        e.delegation_sig[0] ^= 1;
        assert_ne!(base, e.digest(), "digest must cover the signature too");
    }

    #[test]
    fn phase18b_mode0_root_unchanged() {
        let r = make_test_receipt(5);
        let got = irx1_root_from_block_receipts(&[r.clone()]);
        let expected: [u8; 32] = {
            let mut inner = Sha256::new();
            inner.update(r.height.to_le_bytes());
            inner.update([r.lane]);
            inner.update(r.worker_pkh);
            inner.update(r.solution);
            inner.update(r.commitment_nonce);
            let mut outer = Sha256::new();
            outer.update(inner.finalize());
            outer.finalize().into()
        };
        assert_eq!(
            got, expected,
            "mode-0 root must be byte-identical to Phase 13-A"
        );
    }

    #[test]
    fn phase18b_mode1_root_includes_delegation_digest() {
        let sk = test_sk();
        let d = make_signed_delegation(&sk, 0);
        let r0 = make_test_receipt(5);
        let r1 = mode1_receipt(5, d);
        assert_ne!(
            irx1_root_from_block_receipts(&[r0]),
            irx1_root_from_block_receipts(&[r1.clone()]),
            "mode-1 root must differ from mode-0 with same base fields"
        );
        let expected: [u8; 32] = {
            let mut inner = Sha256::new();
            inner.update(r1.height.to_le_bytes());
            inner.update([r1.lane]);
            inner.update(r1.worker_pkh);
            inner.update(r1.solution);
            inner.update(r1.commitment_nonce);
            inner.update(r1.delegation.as_ref().unwrap().digest());
            let mut outer = Sha256::new();
            outer.update(inner.finalize());
            outer.finalize().into()
        };
        assert_eq!(irx1_root_from_block_receipts(&[r1]), expected);
    }

    #[test]
    fn phase18b_v2_mode0_roundtrip() {
        let r = make_test_receipt(9);
        let bytes = r.serialize_v2();
        assert_eq!(bytes.len(), 1 + PoawxBlockReceipt::WIRE_SIZE);
        assert_eq!(bytes[0], RECEIPT_MODE_DIRECT);
        let (r2, used) = PoawxBlockReceipt::deserialize_v2(&bytes).expect("v2 de");
        assert_eq!(used, bytes.len());
        assert_eq!(r, r2);
        assert!(r2.delegation.is_none());
    }

    #[test]
    fn phase18b_v2_mode1_roundtrip() {
        let sk = test_sk();
        let d = make_signed_delegation(&sk, 0);
        let r = mode1_receipt(9, d);
        let bytes = r.serialize_v2();
        assert_eq!(
            bytes.len(),
            1 + PoawxBlockReceipt::WIRE_SIZE + Delegation::WIRE_SIZE
        );
        assert_eq!(bytes[0], RECEIPT_MODE_DELEGATED);
        let (r2, used) = PoawxBlockReceipt::deserialize_v2(&bytes).expect("v2 de");
        assert_eq!(used, bytes.len());
        assert_eq!(r, r2);
        assert!(r2.delegation.is_some());
    }

    #[test]
    fn phase18b_v2_mixed_stream_roundtrip() {
        let sk = test_sk();
        let d = make_signed_delegation(&sk, 0);
        let r0 = make_test_receipt(1);
        let r1 = mode1_receipt(2, d);
        let mut stream = Vec::new();
        stream.extend_from_slice(&r0.serialize_v2());
        stream.extend_from_slice(&r1.serialize_v2());
        let (a, ua) = PoawxBlockReceipt::deserialize_v2(&stream).unwrap();
        let (b, _ub) = PoawxBlockReceipt::deserialize_v2(&stream[ua..]).unwrap();
        assert_eq!(a, r0);
        assert_eq!(b, r1);
    }

    #[test]
    fn phase18b_v2_unknown_mode_and_truncation_fail() {
        let sk = test_sk();
        let d = make_signed_delegation(&sk, 0);
        let r = mode1_receipt(3, d);
        let mut bytes = r.serialize_v2();
        assert!(PoawxBlockReceipt::deserialize_v2(&bytes[..bytes.len() - 1]).is_err());
        bytes[0] = 0x09;
        assert!(PoawxBlockReceipt::deserialize_v2(&bytes).is_err());
    }
}
