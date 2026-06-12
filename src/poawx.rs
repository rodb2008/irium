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
}
