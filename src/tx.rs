use crate::pow::sha256d;
use hex::FromHex;

pub const HTLC_V1_SCRIPT_TAG: u8 = 0xc0;
pub const HTLC_V1_VERSION: u8 = 1;
pub const HTLC_V1_HASHALG_SHA256: u8 = 1;
pub const HTLC_V1_SCRIPT_LEN: usize = 83;

pub const HTLC_WITNESS_CLAIM: u8 = 1;
pub const HTLC_WITNESS_REFUND: u8 = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TxInput {
    pub prev_txid: [u8; 32],
    pub prev_index: u32,
    pub script_sig: Vec<u8>,
    pub sequence: u32,
}

impl TxInput {
    pub fn serialize(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(1 + 32 + 4 + 1 + self.script_sig.len() + 4);
        out.push(self.prev_txid.len() as u8);
        out.extend_from_slice(&self.prev_txid);
        out.extend_from_slice(&self.prev_index.to_le_bytes());
        out.push(self.script_sig.len() as u8);
        out.extend_from_slice(&self.script_sig);
        out.extend_from_slice(&self.sequence.to_le_bytes());
        out
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TxOutput {
    pub value: u64,
    pub script_pubkey: Vec<u8>,
}

impl TxOutput {
    pub fn serialize(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(8 + 1 + self.script_pubkey.len());
        out.extend_from_slice(&self.value.to_le_bytes());
        out.push(self.script_pubkey.len() as u8);
        out.extend_from_slice(&self.script_pubkey);
        out
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transaction {
    pub version: u32,
    pub inputs: Vec<TxInput>,
    pub outputs: Vec<TxOutput>,
    pub locktime: u32,
}

impl Transaction {
    pub fn serialize(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&self.version.to_le_bytes());
        out.push(self.inputs.len() as u8);
        for inp in &self.inputs {
            out.extend_from_slice(&inp.serialize());
        }
        out.push(self.outputs.len() as u8);
        for outp in &self.outputs {
            out.extend_from_slice(&outp.serialize());
        }
        out.extend_from_slice(&self.locktime.to_le_bytes());
        out
    }

    pub fn txid(&self) -> [u8; 32] {
        let ser = self.serialize();
        let mut h = sha256d(&ser);
        h.reverse();
        h
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HtlcV1Output {
    pub expected_hash: [u8; 32],
    pub recipient_pkh: [u8; 20],
    pub refund_pkh: [u8; 20],
    pub timeout_height: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputEncumbrance {
    P2pkh([u8; 20]),
    HtlcV1(HtlcV1Output),
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputWitness {
    P2pkh { sig: Vec<u8>, pubkey: Vec<u8> },
    HtlcClaim {
        sig: Vec<u8>,
        pubkey: Vec<u8>,
        preimage: Vec<u8>,
    },
    HtlcRefund { sig: Vec<u8>, pubkey: Vec<u8> },
    Unknown,
}

pub fn p2pkh_script(pkh: &[u8; 20]) -> Vec<u8> {
    let mut script = Vec::with_capacity(25);
    script.push(0x76);
    script.push(0xa9);
    script.push(0x14);
    script.extend_from_slice(pkh);
    script.push(0x88);
    script.push(0xac);
    script
}

pub fn parse_p2pkh_script(script: &[u8]) -> Option<[u8; 20]> {
    if script.len() != 25 {
        return None;
    }
    if script[0] != 0x76 || script[1] != 0xa9 || script[2] != 0x14 {
        return None;
    }
    if script[23] != 0x88 || script[24] != 0xac {
        return None;
    }
    let mut out = [0u8; 20];
    out.copy_from_slice(&script[3..23]);
    Some(out)
}

pub fn encode_htlcv1_script(output: &HtlcV1Output) -> Vec<u8> {
    let mut script = Vec::with_capacity(HTLC_V1_SCRIPT_LEN);
    script.push(HTLC_V1_SCRIPT_TAG);
    script.push(HTLC_V1_VERSION);
    script.push(HTLC_V1_HASHALG_SHA256);
    script.extend_from_slice(&output.expected_hash);
    script.extend_from_slice(&output.recipient_pkh);
    script.extend_from_slice(&output.refund_pkh);
    script.extend_from_slice(&output.timeout_height.to_le_bytes());
    script
}

pub fn parse_htlcv1_script(script: &[u8]) -> Option<HtlcV1Output> {
    if script.len() != HTLC_V1_SCRIPT_LEN {
        return None;
    }
    if script[0] != HTLC_V1_SCRIPT_TAG {
        return None;
    }
    if script[1] != HTLC_V1_VERSION {
        return None;
    }
    if script[2] != HTLC_V1_HASHALG_SHA256 {
        return None;
    }

    let mut expected_hash = [0u8; 32];
    expected_hash.copy_from_slice(&script[3..35]);

    let mut recipient_pkh = [0u8; 20];
    recipient_pkh.copy_from_slice(&script[35..55]);

    let mut refund_pkh = [0u8; 20];
    refund_pkh.copy_from_slice(&script[55..75]);

    let mut timeout_bytes = [0u8; 8];
    timeout_bytes.copy_from_slice(&script[75..83]);
    let timeout_height = u64::from_le_bytes(timeout_bytes);

    Some(HtlcV1Output {
        expected_hash,
        recipient_pkh,
        refund_pkh,
        timeout_height,
    })
}

pub fn parse_output_encumbrance(script: &[u8]) -> OutputEncumbrance {
    if let Some(pkh) = parse_p2pkh_script(script) {
        return OutputEncumbrance::P2pkh(pkh);
    }
    if let Some(htlc) = parse_htlcv1_script(script) {
        return OutputEncumbrance::HtlcV1(htlc);
    }
    OutputEncumbrance::Unknown
}

fn parse_legacy_p2pkh_witness(script_sig: &[u8]) -> Option<InputWitness> {
    if script_sig.len() < 2 {
        return None;
    }
    let sig_len = script_sig[0] as usize;
    if sig_len == 0 || script_sig.len() < 1 + sig_len + 1 {
        return None;
    }
    let sig = script_sig[1..1 + sig_len].to_vec();
    let pk_len = script_sig[1 + sig_len] as usize;
    let pk_off = 1 + sig_len + 1;
    if pk_len == 0 || script_sig.len() != pk_off + pk_len {
        return None;
    }
    let pubkey = script_sig[pk_off..pk_off + pk_len].to_vec();
    Some(InputWitness::P2pkh { sig, pubkey })
}

pub fn encode_htlcv1_claim_witness(sig: &[u8], pubkey: &[u8], preimage: &[u8]) -> Option<Vec<u8>> {
    if sig.is_empty() || sig.len() > 255 || pubkey.is_empty() || pubkey.len() > 255 || preimage.is_empty() || preimage.len() > 255 {
        return None;
    }
    let mut out = Vec::with_capacity(4 + sig.len() + pubkey.len() + preimage.len());
    out.push(HTLC_WITNESS_CLAIM);
    out.push(sig.len() as u8);
    out.extend_from_slice(sig);
    out.push(pubkey.len() as u8);
    out.extend_from_slice(pubkey);
    out.push(preimage.len() as u8);
    out.extend_from_slice(preimage);
    Some(out)
}

pub fn encode_htlcv1_refund_witness(sig: &[u8], pubkey: &[u8]) -> Option<Vec<u8>> {
    if sig.is_empty() || sig.len() > 255 || pubkey.is_empty() || pubkey.len() > 255 {
        return None;
    }
    let mut out = Vec::with_capacity(3 + sig.len() + pubkey.len());
    out.push(HTLC_WITNESS_REFUND);
    out.push(sig.len() as u8);
    out.extend_from_slice(sig);
    out.push(pubkey.len() as u8);
    out.extend_from_slice(pubkey);
    Some(out)
}

pub fn parse_input_witness(script_sig: &[u8]) -> InputWitness {
    if script_sig.is_empty() {
        return InputWitness::Unknown;
    }

    match script_sig[0] {
        HTLC_WITNESS_CLAIM => {
            if script_sig.len() < 4 {
                return InputWitness::Unknown;
            }
            let sig_len = script_sig[1] as usize;
            let sig_off = 2;
            let pk_len_off = sig_off + sig_len;
            if sig_len == 0 || pk_len_off >= script_sig.len() {
                return InputWitness::Unknown;
            }
            let sig = script_sig[sig_off..pk_len_off].to_vec();
            let pk_len = script_sig[pk_len_off] as usize;
            let pk_off = pk_len_off + 1;
            let pre_len_off = pk_off + pk_len;
            if pk_len == 0 || pre_len_off >= script_sig.len() {
                return InputWitness::Unknown;
            }
            let pubkey = script_sig[pk_off..pre_len_off].to_vec();
            let pre_len = script_sig[pre_len_off] as usize;
            let pre_off = pre_len_off + 1;
            if pre_len == 0 || pre_off + pre_len != script_sig.len() {
                return InputWitness::Unknown;
            }
            let preimage = script_sig[pre_off..pre_off + pre_len].to_vec();
            InputWitness::HtlcClaim {
                sig,
                pubkey,
                preimage,
            }
        }
        HTLC_WITNESS_REFUND => {
            if script_sig.len() < 3 {
                return InputWitness::Unknown;
            }
            let sig_len = script_sig[1] as usize;
            let sig_off = 2;
            let pk_len_off = sig_off + sig_len;
            if sig_len == 0 || pk_len_off >= script_sig.len() {
                return InputWitness::Unknown;
            }
            let sig = script_sig[sig_off..pk_len_off].to_vec();
            let pk_len = script_sig[pk_len_off] as usize;
            let pk_off = pk_len_off + 1;
            if pk_len == 0 || pk_off + pk_len != script_sig.len() {
                return InputWitness::Unknown;
            }
            let pubkey = script_sig[pk_off..pk_off + pk_len].to_vec();
            InputWitness::HtlcRefund { sig, pubkey }
        }
        _ => parse_legacy_p2pkh_witness(script_sig).unwrap_or(InputWitness::Unknown),
    }
}

/// Helper used later to decode compact genesis tx hex.
pub fn decode_hex(s: &str) -> Result<Vec<u8>, hex::FromHexError> {
    Vec::from_hex(s)
}

#[allow(dead_code)]
/// Decode a full Transaction from its serialized bytes (the inverse of `serialize`).
/// This is used by tooling such as the Rust wallet and explorer.
pub fn decode_full_tx(raw: &[u8]) -> Result<Transaction, String> {
    let mut offset = 0usize;
    let tx = decode_full_tx_at(raw, &mut offset)?;
    if offset != raw.len() {
        return Err("trailing bytes after transaction".to_string());
    }
    Ok(tx)
}

/// Decode a full Transaction from its serialized bytes, updating the provided
/// offset as bytes are consumed. This mirrors the Python compact encoding.
pub fn decode_full_tx_at(raw: &[u8], offset: &mut usize) -> Result<Transaction, String> {
    let read_u8 = |buf: &[u8], off: &mut usize| -> Result<u8, String> {
        if *off >= buf.len() {
            return Err("unexpected EOF".to_string());
        }
        let v = buf[*off];
        *off += 1;
        Ok(v)
    };

    let read_u32 = |buf: &[u8], off: &mut usize| -> Result<u32, String> {
        if *off + 4 > buf.len() {
            return Err("unexpected EOF".to_string());
        }
        let mut bytes = [0u8; 4];
        bytes.copy_from_slice(&buf[*off..*off + 4]);
        *off += 4;
        Ok(u32::from_le_bytes(bytes))
    };

    let read_u64 = |buf: &[u8], off: &mut usize| -> Result<u64, String> {
        if *off + 8 > buf.len() {
            return Err("unexpected EOF".to_string());
        }
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(&buf[*off..*off + 8]);
        *off += 8;
        Ok(u64::from_le_bytes(bytes))
    };

    let read_bytes = |buf: &[u8], off: &mut usize, len: usize| -> Result<Vec<u8>, String> {
        if *off + len > buf.len() {
            return Err("unexpected EOF".to_string());
        }
        let out = buf[*off..*off + len].to_vec();
        *off += len;
        Ok(out)
    };

    let version = read_u32(raw, offset)?;
    let input_count = read_u8(raw, offset)? as usize;
    let mut inputs = Vec::with_capacity(input_count);
    for _ in 0..input_count {
        let txid_len = read_u8(raw, offset)? as usize;
        let prev_txid_bytes = read_bytes(raw, offset, txid_len)?;
        if prev_txid_bytes.len() != 32 {
            return Err("invalid prev_txid length".to_string());
        }
        let mut prev_txid = [0u8; 32];
        prev_txid.copy_from_slice(&prev_txid_bytes);
        let prev_index = read_u32(raw, offset)?;
        let script_len = read_u8(raw, offset)? as usize;
        let script_sig = read_bytes(raw, offset, script_len)?;
        let sequence = read_u32(raw, offset)?;
        inputs.push(TxInput {
            prev_txid,
            prev_index,
            script_sig,
            sequence,
        });
    }

    let output_count = read_u8(raw, offset)? as usize;
    let mut outputs = Vec::with_capacity(output_count);
    for _ in 0..output_count {
        let value = read_u64(raw, offset)?;
        let script_len = read_u8(raw, offset)? as usize;
        let script_pubkey = read_bytes(raw, offset, script_len)?;
        outputs.push(TxOutput {
            value,
            script_pubkey,
        });
    }

    let locktime = read_u32(raw, offset)?;

    Ok(Transaction {
        version,
        inputs,
        outputs,
        locktime,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn htlc_script_roundtrip() {
        let out = HtlcV1Output {
            expected_hash: [0x11; 32],
            recipient_pkh: [0x22; 20],
            refund_pkh: [0x33; 20],
            timeout_height: 123456,
        };
        let spk = encode_htlcv1_script(&out);
        assert_eq!(spk.len(), HTLC_V1_SCRIPT_LEN);
        let parsed = parse_htlcv1_script(&spk).expect("parse");
        assert_eq!(parsed, out);
    }

    #[test]
    fn htlc_claim_witness_roundtrip() {
        let w = encode_htlcv1_claim_witness(&[1, 2, 3], &[2; 33], &[9; 32]).expect("encode");
        match parse_input_witness(&w) {
            InputWitness::HtlcClaim {
                sig,
                pubkey,
                preimage,
            } => {
                assert_eq!(sig, vec![1, 2, 3]);
                assert_eq!(pubkey.len(), 33);
                assert_eq!(preimage.len(), 32);
            }
            _ => panic!("wrong witness"),
        }
    }

    #[test]
    fn htlc_refund_witness_roundtrip() {
        let w = encode_htlcv1_refund_witness(&[1, 2, 3], &[2; 33]).expect("encode");
        match parse_input_witness(&w) {
            InputWitness::HtlcRefund { sig, pubkey } => {
                assert_eq!(sig, vec![1, 2, 3]);
                assert_eq!(pubkey.len(), 33);
            }
            _ => panic!("wrong witness"),
        }
    }
}
