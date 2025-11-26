use crate::block::BlockHeader;
use crate::pow::{header_hash, Target};

/// A header-only view of the chain suitable for SPV-style clients.
#[derive(Debug)]
pub struct HeaderChain {
    pub headers: Vec<BlockHeader>,
}

impl HeaderChain {
    pub fn new(genesis: BlockHeader) -> HeaderChain {
        HeaderChain {
            headers: vec![genesis],
        }
    }

    pub fn height(&self) -> u64 {
        self.headers.len() as u64 - 1
    }

    /// Append a new header, verifying basic PoW and linkage.
    pub fn append(&mut self, header: BlockHeader, target: Target) -> Result<(), String> {
        let prev = self
            .headers
            .last()
            .ok_or_else(|| "header chain is empty".to_string())?;
        if header.prev_hash != prev.hash() {
            return Err("Header does not extend current tip".to_string());
        }
        let hash = header.hash();
        if !crate::pow::meets_target(&hash, target) {
            return Err("Header does not satisfy target".to_string());
        }
        self.headers.push(header);
        Ok(())
    }
}

/// Verify a merkle proof for a transaction ID against a known merkle root.
///
/// `txid` and all hashes are big-endian.
pub fn verify_merkle_proof(
    txid: &[u8; 32],
    merkle_root: &[u8; 32],
    mut proof: Vec<[u8; 32]>,
    mut index: usize,
) -> bool {
    let mut current = *txid;
    for sibling in proof.drain(..) {
        let (left, right) = if index % 2 == 0 {
            (current, sibling)
        } else {
            (sibling, current)
        };
        let h = header_hash(&[&left, &right]);
        current = h;
        index /= 2;
    }
    &current == merkle_root
}
