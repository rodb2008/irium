use crate::pow::sha256d;
use hex::FromHex;

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
