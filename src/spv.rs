use crate::block::BlockHeader;
use crate::pow::{header_hash, meets_target, Target};
use num_bigint::BigUint;
use num_traits::Zero;


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


/// Compute the NiPoPoW level of a header (mu), based on its PoW target.
/// Level mu is the largest integer such that hash <= target / 2^mu.
pub fn header_level(header: &BlockHeader) -> u32 {
    let target = Target { bits: header.bits }.to_target();
    let hash = BigUint::from_bytes_be(&header.hash());
    if hash > target {
        return 0;
    }
    let mut level: u32 = 0;
    let mut cur = target;
    while !cur.is_zero() && hash <= cur {
        level = level.saturating_add(1);
        cur >>= 1u32;
    }
    level.saturating_sub(1)
}

/// Verify a contiguous header chain (prev_hash linkage + PoW).
pub fn verify_header_chain(headers: &[BlockHeader]) -> Result<(), String> {
    if headers.is_empty() {
        return Err("header chain is empty".to_string());
    }
    for (idx, header) in headers.iter().enumerate() {
        let target = Target { bits: header.bits };
        if !meets_target(&header.hash(), target) {
            return Err(format!("header {} fails PoW", idx));
        }
        if idx > 0 {
            let prev = &headers[idx - 1];
            if header.prev_hash != prev.hash() {
                return Err(format!("header {} does not link to previous", idx));
            }
        }
    }
    Ok(())
}

/// Compute superchain counts |C_mu| for all mu.
pub fn nipopow_counts(levels: &[u32]) -> Vec<usize> {
    let max = levels.iter().copied().max().unwrap_or(0) as usize;
    let mut exact = vec![0usize; max + 1];
    for lvl in levels {
        let idx = *lvl as usize;
        exact[idx] = exact[idx].saturating_add(1);
    }
    let mut counts = vec![0usize; max + 1];
    let mut running = 0usize;
    for mu in (0..=max).rev() {
        running = running.saturating_add(exact[mu]);
        counts[mu] = running;
    }
    counts
}

/// Return the highest mu such that |C_mu| >= m.
pub fn nipopow_best_level(counts: &[usize], m: usize) -> u32 {
    let mut best = 0u32;
    for (mu, cnt) in counts.iter().enumerate() {
        if *cnt >= m {
            best = mu as u32;
        }
    }
    best
}

/// Compare two header chains using NiPoPoW superchain counts.
pub fn nipopow_compare_counts(a: &[usize], b: &[usize], m: usize) -> std::cmp::Ordering {
    let max = std::cmp::max(a.len(), b.len()).saturating_sub(1);
    for mu in (0..=max).rev() {
        let ac = a.get(mu).copied().unwrap_or(0);
        let bc = b.get(mu).copied().unwrap_or(0);
        if ac >= m || bc >= m {
            if ac != bc {
                return ac.cmp(&bc);
            }
        }
    }
    std::cmp::Ordering::Equal
}
