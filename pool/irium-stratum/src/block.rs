use crate::pow::sha256d;
use crate::template::PoawxPendingReceipt;
use anyhow::{anyhow, Result};
use sha2::{Digest, Sha256};

pub fn parse_address_to_pkh(addr: &str) -> Result<[u8; 20]> {
    let decoded = bs58::decode(addr).into_vec().map_err(|e| anyhow!("base58 decode: {e}"))?;
    if decoded.len() != 25 {
        return Err(anyhow!("invalid address length"));
    }
    let (payload, checksum) = decoded.split_at(21);
    let check = sha256d(payload);
    if checksum != &check[..4] {
        return Err(anyhow!("address checksum mismatch"));
    }
    let mut pkh = [0u8; 20];
    pkh.copy_from_slice(&payload[1..]);
    Ok(pkh)
}

fn put_varint(v: usize, out: &mut Vec<u8>) {
    if v < 0xfd {
        out.push(v as u8);
    } else if v <= 0xffff {
        out.push(0xfd);
        out.extend_from_slice(&(v as u16).to_le_bytes());
    } else if v <= 0xffff_ffff {
        out.push(0xfe);
        out.extend_from_slice(&(v as u32).to_le_bytes());
    } else {
        out.push(0xff);
        out.extend_from_slice(&(v as u64).to_le_bytes());
    }
}

fn p2pkh_script(pkh: &[u8; 20]) -> Vec<u8> {

    let mut s = Vec::with_capacity(25);
    s.push(0x76);
    s.push(0xa9);
    s.push(0x14);
    s.extend_from_slice(pkh);
    s.push(0x88);
    s.push(0xac);
    s
}


fn encode_bip34_height(height: u64) -> Vec<u8> {
    let mut n = height;
    let mut raw = Vec::new();
    while n > 0 {
        raw.push((n & 0xff) as u8);
        n >>= 8;
    }
    if raw.is_empty() {
        raw.push(0);
    }
    if raw.last().copied().unwrap_or(0) & 0x80 != 0 {
        raw.push(0);
    }
    let mut out = Vec::with_capacity(raw.len() + 1);
    out.push(raw.len() as u8);
    out.extend_from_slice(&raw);
    out
}

pub fn build_coinbase_tx(
    height: u64,
    reward: u64,
    pkh: &[u8; 20],
    extranonce: &[u8],
    bip34_height: bool,
    extras: &[(u64, Vec<u8>)],
) -> Vec<u8> {
    let mut tx = Vec::with_capacity(200 + extras.iter().map(|(_, s)| s.len() + 16).sum::<usize>());
    tx.extend_from_slice(&1u32.to_le_bytes());
    put_varint(1, &mut tx);
    // Fix F: iriumd's tx format prefixes prev_txid with a 1-byte length (=32),
    // unlike Bitcoin. Missing this byte caused submit_block to silent-400
    // (decode_full_tx_at: "invalid prev_txid length") for every pool block.
    tx.push(32u8);
    tx.extend_from_slice(&[0u8; 32]);
    tx.extend_from_slice(&0xffff_ffffu32.to_le_bytes());

    let mut script_sig = if bip34_height {
        let mut s = encode_bip34_height(height);
        s.extend_from_slice(b"Irium");
        s
    } else {
        format!("Irium {height}").into_bytes()
    };
    script_sig.extend_from_slice(extranonce);
    put_varint(script_sig.len(), &mut tx);
    tx.extend_from_slice(&script_sig);

    tx.extend_from_slice(&0xffff_ffffu32.to_le_bytes());
    // v1.9.62 issue #60: extras are zero-value BTC/LTC/DOGE header-batch
    // outputs that ride in the coinbase post-activation. They cost nothing
    // (coinbase has no inputs) and chain.rs accepts them at value=0 with a
    // one-per-chain cap.
    put_varint(1 + extras.len(), &mut tx);
    tx.extend_from_slice(&reward.to_le_bytes());

    let spk = p2pkh_script(pkh);
    put_varint(spk.len(), &mut tx);
    tx.extend_from_slice(&spk);
    for (value, script) in extras {
        tx.extend_from_slice(&value.to_le_bytes());
        put_varint(script.len(), &mut tx);
        tx.extend_from_slice(script);
    }
    tx.extend_from_slice(&0u32.to_le_bytes());
    tx
}

pub fn coinbase_prefix_suffix(
    height: u64,
    reward: u64,
    pkh: &[u8; 20],
    bip34_height: bool,
    extras: &[(u64, Vec<u8>)],
) -> (Vec<u8>, Vec<u8>) {
    // Use a unique non-zero marker so we only split at the extranonce location,
    // not at zero-filled fields like prevout hash/index in coinbase tx.
    // Marker length must match total extranonce payload length (4+4=8 bytes).
    let marker: [u8; 8] = [0xfa, 0xce, 0xb0, 0x0c, 0x1c, 0xab, 0xad, 0x1d];
    let full = build_coinbase_tx(height, reward, pkh, &marker, bip34_height, extras);
    let pos = full
        .windows(marker.len())
        .position(|w| w == marker)
        .unwrap_or(full.len());
    (full[..pos].to_vec(), full[pos + marker.len()..].to_vec())
}

// Solo-mode coinbase: two outputs in a single transaction.
//   output 0: worker reward = reward * (10_000 - fee_bps) / 10_000  to worker_pkh
//   output 1: pool fee      = reward - worker_reward                to pool_pkh
// fee_bps is capped at 10_000 (100%). A 0 fee still emits two outputs so the
// hash/wire format stays consistent across the solo path; operators who want
// zero fee should run a separate non-pool node, not solo mode with bps=0.
pub fn build_solo_coinbase_tx(
    height: u64,
    reward: u64,
    worker_pkh: &[u8; 20],
    pool_pkh: &[u8; 20],
    fee_bps: u64,
    extranonce: &[u8],
    bip34_height: bool,
) -> Vec<u8> {
    let fee_bps_capped = fee_bps.min(10_000);
    let pool_fee = reward * fee_bps_capped / 10_000;
    let worker_reward = reward.saturating_sub(pool_fee);

    let mut tx = Vec::with_capacity(260);
    tx.extend_from_slice(&1u32.to_le_bytes());
    put_varint(1, &mut tx);
    tx.push(32u8);
    tx.extend_from_slice(&[0u8; 32]);
    tx.extend_from_slice(&0xffff_ffffu32.to_le_bytes());

    let mut script_sig = if bip34_height {
        let mut s = encode_bip34_height(height);
        s.extend_from_slice(b"Irium");
        s
    } else {
        format!("Irium {height}").into_bytes()
    };
    script_sig.extend_from_slice(extranonce);
    put_varint(script_sig.len(), &mut tx);
    tx.extend_from_slice(&script_sig);
    tx.extend_from_slice(&0xffff_ffffu32.to_le_bytes());

    put_varint(2, &mut tx);

    tx.extend_from_slice(&worker_reward.to_le_bytes());
    let worker_spk = p2pkh_script(worker_pkh);
    put_varint(worker_spk.len(), &mut tx);
    tx.extend_from_slice(&worker_spk);

    tx.extend_from_slice(&pool_fee.to_le_bytes());
    let pool_spk = p2pkh_script(pool_pkh);
    put_varint(pool_spk.len(), &mut tx);
    tx.extend_from_slice(&pool_spk);

    tx.extend_from_slice(&0u32.to_le_bytes());
    tx
}

pub fn solo_coinbase_prefix_suffix(
    height: u64,
    reward: u64,
    worker_pkh: &[u8; 20],
    pool_pkh: &[u8; 20],
    fee_bps: u64,
    bip34_height: bool,
) -> (Vec<u8>, Vec<u8>) {
    let marker: [u8; 8] = [0xfa, 0xce, 0xb0, 0x0c, 0x1c, 0xab, 0xad, 0x1d];
    let full = build_solo_coinbase_tx(height, reward, worker_pkh, pool_pkh, fee_bps, &marker, bip34_height);
    let pos = full
        .windows(marker.len())
        .position(|w| w == marker)
        .unwrap_or(full.len());
    (full[..pos].to_vec(), full[pos + marker.len()..].to_vec())
}

pub fn build_merkle_branches(template_tx_hex: &[String]) -> Result<Vec<[u8; 32]>> {
    let mut level: Vec<[u8; 32]> = Vec::with_capacity(template_tx_hex.len() + 1);
    level.push([0u8; 32]);
    for h in template_tx_hex {
        let raw = hex::decode(h).map_err(|e| anyhow!("template tx decode: {e}"))?;
        level.push(sha256d(&raw));
    }
    let mut branches = Vec::new();
    let mut idx = 0usize;
    while level.len() > 1 {
        let sibling = if idx % 2 == 0 {
            if idx + 1 < level.len() { level[idx + 1] } else { level[idx] }
        } else {
            level[idx - 1]
        };
        branches.push(sibling);

        let mut next = Vec::with_capacity((level.len() + 1) / 2);
        for pair in level.chunks(2) {
            let left = pair[0];
            let right = if pair.len() == 2 { pair[1] } else { pair[0] };
            let mut data = Vec::with_capacity(64);
            data.extend_from_slice(&left);
            data.extend_from_slice(&right);
            next.push(sha256d(&data));
        }
        idx /= 2;
        level = next;
    }
    Ok(branches)
}

pub fn merkle_root_from_coinbase(coinbase_hash: [u8; 32], branches: &[[u8; 32]]) -> [u8; 32] {
    let mut root = coinbase_hash;
    for b in branches {
        let mut data = Vec::with_capacity(64);
        data.extend_from_slice(&root);
        data.extend_from_slice(b);
        root = sha256d(&data);
    }
    root
}

pub fn parse_hex32(s: &str) -> Result<[u8; 32]> {
    let b = hex::decode(s).map_err(|e| anyhow!("hex decode: {e}"))?;
    if b.len() != 32 {
        return Err(anyhow!("expected 32 bytes"));
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&b);
    Ok(out)
}

pub fn parse_u32_hex(s: &str) -> Result<u32> {
    let t = s.trim_start_matches("0x");
    Ok(u32::from_str_radix(t, 16).map_err(|e| anyhow!("hex parse: {e}"))?)
}

/// Phase 10-D: compute receipts root for PoAW-X irx1 commitment.
/// Algorithm: SHA256(concat(SHA256(receipt_fields) for each receipt)).
/// Phase 11-B: sort canonically by (height, lane, worker_pkh, commitment_nonce)
/// so the root is deterministic regardless of receipt insertion order.
pub fn compute_receipts_root_from_pending(receipts: &[PoawxPendingReceipt]) -> [u8; 32] {
    let mut sorted: Vec<&PoawxPendingReceipt> = receipts.iter().collect();
    sorted.sort_unstable_by(|a, b| {
        a.height.cmp(&b.height)
            .then_with(|| a.lane.as_bytes().cmp(b.lane.as_bytes()))
            .then_with(|| a.worker_pkh.as_bytes().cmp(b.worker_pkh.as_bytes()))
            .then_with(|| a.commitment_nonce.as_bytes().cmp(b.commitment_nonce.as_bytes()))
    });
    let mut outer = Sha256::new();
    for r in sorted {
        let mut inner = Sha256::new();
        inner.update(r.height.to_le_bytes());
        inner.update(r.lane.as_bytes());
        inner.update(hex::decode(&r.worker_pkh).unwrap_or_default());
        inner.update(hex::decode(&r.solution).unwrap_or_default());
        inner.update(hex::decode(&r.commitment_nonce).unwrap_or_default());
        outer.update(inner.finalize());
    }
    outer.finalize().into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::template::PoawxPendingReceipt;

    fn mkr(height: u64, lane: &str, pkh: &str, sol: &str, nonce: &str) -> PoawxPendingReceipt {
        PoawxPendingReceipt {
            height,
            lane: lane.to_string(),
            worker_pkh: pkh.to_string(),
            solution: sol.to_string(),
            commitment_nonce: nonce.to_string(),
        }
    }

    #[test]
    fn single_receipt_stable() {
        let r = mkr(1, "cpu", "aabb", "dead", "cafe");
        assert_eq!(
            compute_receipts_root_from_pending(&[r.clone()]),
            compute_receipts_root_from_pending(&[r])
        );
    }

    #[test]
    fn two_receipts_order_independent() {
        let r1 = mkr(1, "cpu", "aaaa", "0001", "0011");
        let r2 = mkr(1, "cpu", "bbbb", "0002", "0022");
        assert_eq!(
            compute_receipts_root_from_pending(&[r1.clone(), r2.clone()]),
            compute_receipts_root_from_pending(&[r2, r1]),
            "root must not depend on insertion order"
        );
    }

    #[test]
    fn many_receipts_shuffled_same_root() {
        let receipts: Vec<PoawxPendingReceipt> = (0u64..5)
            .map(|i| mkr(1, "cpu", &format!("{:04x}", i * 17), &format!("{:04x}", i), &format!("{:04x}", i + 100)))
            .collect();
        let mut rev = receipts.clone();
        rev.reverse();
        assert_eq!(
            compute_receipts_root_from_pending(&receipts),
            compute_receipts_root_from_pending(&rev)
        );
    }

    #[test]
    fn different_heights_different_root() {
        let r1 = mkr(1, "cpu", "aaaa", "0001", "0011");
        let r2 = mkr(2, "cpu", "aaaa", "0001", "0011");
        assert_ne!(
            compute_receipts_root_from_pending(&[r1]),
            compute_receipts_root_from_pending(&[r2])
        );
    }
}

/// Phase 10-D: build irx1 OP_RETURN script for coinbase.
/// Format: 0x6a 0x24 "irx1" <32-byte receipts_root> = 38 bytes.
pub fn build_irx1_commitment_script(receipts_root: &[u8; 32]) -> Vec<u8> {
    let mut s = Vec::with_capacity(38);
    s.push(0x6a); // OP_RETURN
    s.push(0x24); // PUSH 36 bytes
    s.extend_from_slice(b"irx1");
    s.extend_from_slice(receipts_root);
    s
}

pub fn header_bytes(version: u32, prev_hash: [u8; 32], merkle_root: [u8; 32], ntime: u32, nbits: u32, nonce: u32) -> [u8; 80] {
    let mut h = [0u8; 80];
    h[0..4].copy_from_slice(&version.to_le_bytes());
    h[4..36].copy_from_slice(&prev_hash);
    h[36..68].copy_from_slice(&merkle_root);
    h[68..72].copy_from_slice(&ntime.to_le_bytes());
    h[72..76].copy_from_slice(&nbits.to_le_bytes());
    h[76..80].copy_from_slice(&nonce.to_le_bytes());
    h
}
