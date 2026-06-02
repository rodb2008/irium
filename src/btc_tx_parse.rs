//! Light Bitcoin transaction parser for HtlcBtcSwapV1 proof verification.
//!
//! Parses just enough of a Bitcoin tx to (a) recognise P2PKH and canonical
//! `OP_RETURN` outputs and (b) compute the canonical txid (stripping
//! SegWit marker, flag, and witness data when present, per BIP141).
//!
//! Phase 2 consensus only inspects outputs. Inputs are walked solely to
//! advance the cursor past them; their content is otherwise ignored.

use crate::pow::sha256d;

/// Hard upper bound on parsed BTC tx length. Matches Bitcoin Core's
/// MAX_BLOCK_SERIALIZED_SIZE (4 MB) ceiling; any tx larger is non-standard
/// and meaningless for our use case (must fit in a Bitcoin block).
pub const BTC_TX_MAX_LEN: usize = 4_000_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BtcOutputScript {
    /// `OP_DUP OP_HASH160 <20> OP_EQUALVERIFY OP_CHECKSIG`, exactly 25 bytes.
    P2pkh([u8; 20]),
    /// `OP_0 <0x14> <20-byte pkh>`, exactly 22 bytes (BIP141 native SegWit
    /// version-0 program with a 20-byte witness program = P2WPKH). Stores
    /// the same 20-byte HASH160(pubkey) the P2PKH form encodes, so a
    /// claim consensus check can match a P2WPKH payment against the same
    /// `btc_recipient_pkh` field once the bech32-payment relaxation is
    /// active (see `ChainParams.btc_swap_bech32_payment_activation_height`).
    P2wpkh([u8; 20]),
    /// `OP_RETURN <OP_PUSHBYTES_N> <N bytes>`, 1 <= N <= 75. Stores the
    /// pushed payload bytes (without the opcode prefix).
    OpReturn(Vec<u8>),
    /// Anything else: P2WSH, P2TR, P2SH, multisig, non-standard, etc.
    Other,
}

#[derive(Debug, Clone)]
pub struct BtcTxOutputView {
    pub value: u64,
    pub script: BtcOutputScript,
    pub vout: u32,
}

fn read_varint(data: &[u8], offset: &mut usize) -> Result<u64, String> {
    if *offset >= data.len() {
        return Err("btc tx: EOF reading varint".to_string());
    }
    let first = data[*offset];
    *offset += 1;
    match first {
        0xff => {
            if *offset + 8 > data.len() {
                return Err("btc tx: EOF reading 8-byte varint".to_string());
            }
            let mut b = [0u8; 8];
            b.copy_from_slice(&data[*offset..*offset + 8]);
            *offset += 8;
            Ok(u64::from_le_bytes(b))
        }
        0xfe => {
            if *offset + 4 > data.len() {
                return Err("btc tx: EOF reading 4-byte varint".to_string());
            }
            let mut b = [0u8; 4];
            b.copy_from_slice(&data[*offset..*offset + 4]);
            *offset += 4;
            Ok(u32::from_le_bytes(b) as u64)
        }
        0xfd => {
            if *offset + 2 > data.len() {
                return Err("btc tx: EOF reading 2-byte varint".to_string());
            }
            let mut b = [0u8; 2];
            b.copy_from_slice(&data[*offset..*offset + 2]);
            *offset += 2;
            Ok(u16::from_le_bytes(b) as u64)
        }
        n => Ok(n as u64),
    }
}

fn skip_bytes(data: &[u8], offset: &mut usize, len: usize) -> Result<(), String> {
    let new = offset
        .checked_add(len)
        .ok_or_else(|| "btc tx: offset overflow".to_string())?;
    if new > data.len() {
        return Err(format!(
            "btc tx: truncated (need {} more bytes at offset {})",
            len, offset
        ));
    }
    *offset = new;
    Ok(())
}

fn parse_output_script(script: &[u8]) -> BtcOutputScript {
    if script.len() == 25
        && script[0] == 0x76
        && script[1] == 0xa9
        && script[2] == 0x14
        && script[23] == 0x88
        && script[24] == 0xac
    {
        let mut pkh = [0u8; 20];
        pkh.copy_from_slice(&script[3..23]);
        return BtcOutputScript::P2pkh(pkh);
    }
    // P2WPKH (BIP141): OP_0 <0x14> <20-byte pkh>. Total exactly 22 bytes.
    // The 20-byte program is HASH160(pubkey), the same value the P2PKH
    // form embeds, so callers can compare against `btc_recipient_pkh`
    // without any per-form derivation. P2WSH is the same opcode shape
    // with a 32-byte program (34 bytes total) and is intentionally
    // classified as `Other` because the inner script hash cannot be
    // recovered from the on-chain bytes alone.
    if script.len() == 22 && script[0] == 0x00 && script[1] == 0x14 {
        let mut pkh = [0u8; 20];
        pkh.copy_from_slice(&script[2..22]);
        return BtcOutputScript::P2wpkh(pkh);
    }
    if script.len() >= 2 && script[0] == 0x6a {
        let push = script[1];
        if (1..=75).contains(&push) && (push as usize) + 2 == script.len() {
            return BtcOutputScript::OpReturn(script[2..].to_vec());
        }
    }
    BtcOutputScript::Other
}

/// Parse a Bitcoin transaction's outputs. Handles legacy and SegWit wire
/// formats transparently — for both, the returned vouts are 0..n-1 in wire
/// order.
pub fn parse_btc_tx_outputs(raw: &[u8]) -> Result<Vec<BtcTxOutputView>, String> {
    if raw.is_empty() {
        return Err("btc tx: empty".to_string());
    }
    if raw.len() > BTC_TX_MAX_LEN {
        return Err(format!("btc tx: too large ({} bytes)", raw.len()));
    }
    let mut off = 0usize;
    skip_bytes(raw, &mut off, 4)?; // version

    if off >= raw.len() {
        return Err("btc tx: truncated after version".to_string());
    }
    let segwit = raw[off] == 0x00;
    if segwit {
        if off + 2 > raw.len() {
            return Err("btc tx: truncated segwit marker".to_string());
        }
        if raw[off + 1] == 0x00 {
            return Err("btc tx: segwit flag cannot be zero".to_string());
        }
        off += 2;
    }

    let in_count = read_varint(raw, &mut off)?;
    if in_count == 0 {
        return Err("btc tx: zero inputs".to_string());
    }
    if in_count > 100_000 {
        return Err(format!("btc tx: absurd input count {}", in_count));
    }
    for _ in 0..in_count {
        skip_bytes(raw, &mut off, 36)?; // prev_txid(32) + prev_vout(4)
        let script_len = read_varint(raw, &mut off)? as usize;
        skip_bytes(raw, &mut off, script_len)?;
        skip_bytes(raw, &mut off, 4)?; // sequence
    }

    let out_count = read_varint(raw, &mut off)?;
    if out_count == 0 {
        return Err("btc tx: zero outputs".to_string());
    }
    if out_count > 100_000 {
        return Err(format!("btc tx: absurd output count {}", out_count));
    }
    let mut outputs: Vec<BtcTxOutputView> = Vec::with_capacity(out_count as usize);
    for vout in 0..out_count {
        if off + 8 > raw.len() {
            return Err("btc tx: truncated output value".to_string());
        }
        let mut vb = [0u8; 8];
        vb.copy_from_slice(&raw[off..off + 8]);
        let value = u64::from_le_bytes(vb);
        off += 8;
        let script_len = read_varint(raw, &mut off)? as usize;
        if off + script_len > raw.len() {
            return Err("btc tx: truncated output script".to_string());
        }
        let script = parse_output_script(&raw[off..off + script_len]);
        off += script_len;
        outputs.push(BtcTxOutputView {
            value,
            script,
            vout: vout as u32,
        });
    }

    if segwit {
        for _ in 0..in_count {
            let stack_count = read_varint(raw, &mut off)?;
            if stack_count > 100_000 {
                return Err("btc tx: absurd witness stack size".to_string());
            }
            for _ in 0..stack_count {
                let item_len = read_varint(raw, &mut off)? as usize;
                skip_bytes(raw, &mut off, item_len)?;
            }
        }
    }
    if off + 4 != raw.len() {
        return Err(format!(
            "btc tx: trailing bytes or truncated locktime (offset {} vs len {})",
            off,
            raw.len()
        ));
    }
    Ok(outputs)
}

/// Canonical Bitcoin txid: sha256d of the transaction serialization with
/// SegWit marker, flag, and witness data stripped (BIP141). Returns the
/// natural (non-reversed) byte order matching what is stored in BTC block
/// merkle trees. User-display BTC hashes are this value reversed.
pub fn btc_txid(raw: &[u8]) -> Result<[u8; 32], String> {
    if raw.len() < 10 || raw.len() > BTC_TX_MAX_LEN {
        return Err(format!("btc tx: length {} out of range", raw.len()));
    }
    if raw[4] != 0x00 {
        return Ok(sha256d(raw));
    }
    if raw.len() < 6 {
        return Err("btc tx: truncated segwit marker".to_string());
    }
    if raw[5] == 0x00 {
        return Err("btc tx: segwit flag cannot be zero".to_string());
    }

    let mut off = 6usize;
    let inputs_start = off;
    let in_count = read_varint(raw, &mut off)?;
    if in_count == 0 {
        return Err("btc tx: zero inputs".to_string());
    }
    if in_count > 100_000 {
        return Err(format!("btc tx: absurd input count {}", in_count));
    }
    for _ in 0..in_count {
        skip_bytes(raw, &mut off, 36)?;
        let script_len = read_varint(raw, &mut off)? as usize;
        skip_bytes(raw, &mut off, script_len)?;
        skip_bytes(raw, &mut off, 4)?;
    }

    let out_count = read_varint(raw, &mut off)?;
    if out_count == 0 {
        return Err("btc tx: zero outputs".to_string());
    }
    if out_count > 100_000 {
        return Err(format!("btc tx: absurd output count {}", out_count));
    }
    for _ in 0..out_count {
        skip_bytes(raw, &mut off, 8)?;
        let script_len = read_varint(raw, &mut off)? as usize;
        skip_bytes(raw, &mut off, script_len)?;
    }
    let outputs_end = off;

    for _ in 0..in_count {
        let stack_count = read_varint(raw, &mut off)?;
        if stack_count > 100_000 {
            return Err("btc tx: absurd witness stack size".to_string());
        }
        for _ in 0..stack_count {
            let item_len = read_varint(raw, &mut off)? as usize;
            skip_bytes(raw, &mut off, item_len)?;
        }
    }

    if off + 4 > raw.len() {
        return Err("btc tx: truncated locktime".to_string());
    }
    let locktime_off = off;

    let body_len = outputs_end - inputs_start;
    let mut stripped = Vec::with_capacity(4 + body_len + 4);
    stripped.extend_from_slice(&raw[0..4]);
    stripped.extend_from_slice(&raw[inputs_start..outputs_end]);
    stripped.extend_from_slice(&raw[locktime_off..locktime_off + 4]);
    Ok(sha256d(&stripped))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p2pkh_script_bytes(pkh: &[u8; 20]) -> Vec<u8> {
        let mut s = Vec::with_capacity(25);
        s.extend_from_slice(&[0x76, 0xa9, 0x14]);
        s.extend_from_slice(pkh);
        s.extend_from_slice(&[0x88, 0xac]);
        s
    }

    fn op_return_script_bytes(payload: &[u8]) -> Vec<u8> {
        assert!(!payload.is_empty() && payload.len() <= 75);
        let mut s = Vec::with_capacity(2 + payload.len());
        s.push(0x6a);
        s.push(payload.len() as u8);
        s.extend_from_slice(payload);
        s
    }

    fn build_legacy_tx(outputs: &[(u64, Vec<u8>)]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&1u32.to_le_bytes());
        out.push(1); // in_count = 1
        out.extend_from_slice(&[0u8; 32]); // prev_txid
        out.extend_from_slice(&0u32.to_le_bytes()); // prev_vout
        out.push(0); // empty scriptSig
        out.extend_from_slice(&0xffff_ffffu32.to_le_bytes()); // sequence
        out.push(outputs.len() as u8); // out_count
        for (value, script) in outputs {
            out.extend_from_slice(&value.to_le_bytes());
            out.push(script.len() as u8);
            out.extend_from_slice(script);
        }
        out.extend_from_slice(&0u32.to_le_bytes()); // locktime
        out
    }

    fn build_segwit_tx_minimal(outputs: &[(u64, Vec<u8>)]) -> Vec<u8> {
        // Insert marker+flag after version, append one empty witness stack per input.
        let legacy = build_legacy_tx(outputs);
        let mut out = Vec::with_capacity(legacy.len() + 3);
        out.extend_from_slice(&legacy[0..4]);
        out.extend_from_slice(&[0x00, 0x01]);
        out.extend_from_slice(&legacy[4..legacy.len() - 4]);
        out.push(0x00); // one input, witness stack count = 0
        out.extend_from_slice(&legacy[legacy.len() - 4..]);
        out
    }

    #[test]
    fn parse_simple_p2pkh_tx() {
        let pkh = [0x11u8; 20];
        let tx = build_legacy_tx(&[(50_000, p2pkh_script_bytes(&pkh))]);
        let outs = parse_btc_tx_outputs(&tx).expect("parse");
        assert_eq!(outs.len(), 1);
        assert_eq!(outs[0].value, 50_000);
        assert_eq!(outs[0].vout, 0);
        match &outs[0].script {
            BtcOutputScript::P2pkh(p) => assert_eq!(p, &pkh),
            _ => panic!("expected P2pkh"),
        }
    }

    #[test]
    fn parse_op_return_extracts_payload() {
        let payload = b"irmswp\x01\x02\x03\x04\x05\x06\x07\x08".to_vec();
        assert_eq!(payload.len(), 14);
        let tx = build_legacy_tx(&[(0, op_return_script_bytes(&payload))]);
        let outs = parse_btc_tx_outputs(&tx).expect("parse");
        assert_eq!(outs.len(), 1);
        assert_eq!(outs[0].value, 0);
        match &outs[0].script {
            BtcOutputScript::OpReturn(p) => assert_eq!(p, &payload),
            _ => panic!("expected OpReturn"),
        }
    }

    #[test]
    fn parse_multi_output_preserves_vout_order() {
        let p1 = [0x22u8; 20];
        let p2 = [0x33u8; 20];
        let payload = vec![0xaa; 14];
        let tx = build_legacy_tx(&[
            (100, p2pkh_script_bytes(&p1)),
            (200, p2pkh_script_bytes(&p2)),
            (0, op_return_script_bytes(&payload)),
        ]);
        let outs = parse_btc_tx_outputs(&tx).expect("parse");
        assert_eq!(outs.len(), 3);
        assert_eq!(outs[0].vout, 0);
        assert_eq!(outs[1].vout, 1);
        assert_eq!(outs[2].vout, 2);
        match &outs[2].script {
            BtcOutputScript::OpReturn(p) => assert_eq!(p, &payload),
            _ => panic!("expected OpReturn at vout 2"),
        }
    }

    #[test]
    fn parse_rejects_zero_inputs() {
        let mut tx = build_legacy_tx(&[(100, p2pkh_script_bytes(&[0u8; 20]))]);
        tx[4] = 0; // zero inputs varint
        assert!(parse_btc_tx_outputs(&tx).is_err());
    }

    #[test]
    fn parse_rejects_truncated_tx() {
        let tx = build_legacy_tx(&[(100, p2pkh_script_bytes(&[0u8; 20]))]);
        for cut in 1..tx.len() {
            assert!(parse_btc_tx_outputs(&tx[..cut]).is_err());
        }
    }

    #[test]
    fn parse_recognises_p2wpkh() {
        // P2WPKH script: OP_0 <0x14> <20 bytes>, total exactly 22 bytes.
        let pkh = [0x44u8; 20];
        let mut script = vec![0x00, 0x14];
        script.extend_from_slice(&pkh);
        let tx = build_legacy_tx(&[(100, script)]);
        let outs = parse_btc_tx_outputs(&tx).expect("parse");
        match &outs[0].script {
            BtcOutputScript::P2wpkh(p) => assert_eq!(p, &pkh),
            other => panic!("expected P2wpkh, got {:?}", other),
        }
    }

    #[test]
    fn parse_p2wpkh_wrong_length_is_other() {
        // 21-byte form: OP_0 <0x13> <19 bytes>. Not a valid P2WPKH program length.
        let mut s21 = vec![0x00, 0x13];
        s21.extend_from_slice(&[0x55u8; 19]);
        let tx21 = build_legacy_tx(&[(100, s21)]);
        let outs21 = parse_btc_tx_outputs(&tx21).expect("parse");
        assert!(matches!(outs21[0].script, BtcOutputScript::Other));

        // 34-byte P2WSH form: OP_0 <0x20> <32 bytes>. Different script type;
        // we deliberately do NOT support claiming via P2WSH because the
        // inner script hash cannot be matched against a 20-byte pkh.
        let mut s34 = vec![0x00, 0x20];
        s34.extend_from_slice(&[0x66u8; 32]);
        let tx34 = build_legacy_tx(&[(100, s34)]);
        let outs34 = parse_btc_tx_outputs(&tx34).expect("parse");
        assert!(matches!(outs34[0].script, BtcOutputScript::Other));
    }

    #[test]
    fn parse_p2wpkh_wrong_opcode_is_other() {
        // OP_1 <0x14> <20 bytes> — would be Taproot (v1) shape, but
        // Taproot uses a 32-byte program, not 20. Either way, witness
        // version 1 is intentionally not recognised here; classified as
        // Other.
        let mut script = vec![0x51, 0x14]; // OP_1 (0x51), push 20
        script.extend_from_slice(&[0x77u8; 20]);
        let tx = build_legacy_tx(&[(100, script)]);
        let outs = parse_btc_tx_outputs(&tx).expect("parse");
        assert!(matches!(outs[0].script, BtcOutputScript::Other));
    }

    #[test]
    fn parse_rejects_non_canonical_op_return_long_pushdata1() {
        // 0x6a 0x4c 0x05 <5 bytes> — semantically equivalent to canonical
        // 0x6a 0x05 <5 bytes>, but we accept only the minimal canonical form.
        let script = vec![0x6a, 0x4c, 0x05, 0xaa, 0xbb, 0xcc, 0xdd, 0xee];
        let tx = build_legacy_tx(&[(0, script)]);
        let outs = parse_btc_tx_outputs(&tx).expect("parse");
        assert!(matches!(outs[0].script, BtcOutputScript::Other));
    }

    #[test]
    fn parse_op_return_zero_length_push_is_other() {
        // 0x6a 0x00 — push 0 bytes. Spec requires N >= 1 for our binding marker.
        let tx = build_legacy_tx(&[(0, vec![0x6a, 0x00])]);
        let outs = parse_btc_tx_outputs(&tx).expect("parse");
        assert!(matches!(outs[0].script, BtcOutputScript::Other));
    }

    #[test]
    fn txid_legacy_is_sha256d_of_raw() {
        let pkh = [0x55u8; 20];
        let tx = build_legacy_tx(&[(100, p2pkh_script_bytes(&pkh))]);
        let expected = sha256d(&tx);
        let actual = btc_txid(&tx).expect("txid");
        assert_eq!(actual, expected);
    }

    #[test]
    fn txid_segwit_matches_stripped_form() {
        let pkh = [0x66u8; 20];
        let legacy = build_legacy_tx(&[(100, p2pkh_script_bytes(&pkh))]);
        let segwit = build_segwit_tx_minimal(&[(100, p2pkh_script_bytes(&pkh))]);
        assert_ne!(legacy, segwit, "wire form differs");
        let legacy_txid = btc_txid(&legacy).expect("legacy txid");
        let segwit_txid = btc_txid(&segwit).expect("segwit txid");
        assert_eq!(
            legacy_txid, segwit_txid,
            "SegWit txid must equal stripped (legacy-equivalent) txid"
        );
    }

    #[test]
    fn segwit_tx_outputs_parse_normally() {
        let pkh = [0x77u8; 20];
        let tx = build_segwit_tx_minimal(&[(123, p2pkh_script_bytes(&pkh))]);
        let outs = parse_btc_tx_outputs(&tx).expect("parse segwit outputs");
        assert_eq!(outs.len(), 1);
        assert_eq!(outs[0].value, 123);
        match &outs[0].script {
            BtcOutputScript::P2pkh(p) => assert_eq!(p, &pkh),
            _ => panic!(),
        }
    }

    #[test]
    fn txid_rejects_segwit_flag_zero() {
        let pkh = [0x88u8; 20];
        let mut tx = build_segwit_tx_minimal(&[(100, p2pkh_script_bytes(&pkh))]);
        tx[5] = 0; // zero flag
        assert!(btc_txid(&tx).is_err());
    }
}
