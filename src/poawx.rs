//! PoAW-X consensus helpers shared between the daemon, P2P layer, and tests.
//!
//! Env-var reads are intentionally absent here — callers own activation gating.

use crate::block::Block;

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
}

impl PoawxBlockReceipt {
    /// Fixed wire size: 8 + 1 + 20 + 33 + 64 + 8 + 32 = 166 bytes.
    pub const WIRE_SIZE: usize = 8 + 1 + 20 + 33 + 64 + 8 + 32;

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
        })
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
}
