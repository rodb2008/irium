
use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone)]
pub struct ParsedTx {
    /// Hex-encoded txid, display format (SHA256d, byte-reversed).
    pub txid: String,
    pub version: i32,
    pub inputs: Vec<TxInput>,
    pub outputs: Vec<TxOutput>,
    pub locktime: u32,
}

#[derive(Debug, Clone)]
pub struct TxInput {
    pub prev_txid: String,
    pub prev_vout: u32,
    pub script_sig: Vec<u8>,
    pub sequence: u32,
}

impl TxInput {
    pub fn is_coinbase(&self) -> bool {
        self.prev_vout == 0xFFFF_FFFF
            && self.prev_txid == "0000000000000000000000000000000000000000000000000000000000000000"
    }
}

#[derive(Debug, Clone)]
pub struct TxOutput {
    pub value: i64,
    pub script_pubkey: Vec<u8>,
}

pub fn decode_tx(hex_str: &str) -> Result<ParsedTx> {
    let bytes = hex::decode(hex_str.trim()).context("invalid hex in tx")?;
    let mut cur = Cursor::new(&bytes);

    let version = cur.read_i32_le()?;
    let in_count = cur.read_varint()?;
    let mut inputs = Vec::with_capacity(in_count as usize);
    for _ in 0..in_count {
        // Irium tx format: 1-byte length prefix before prev_txid bytes.
        // The prefix is always 0x20 (= 32) for a valid 32-byte txid.
        let txid_len = cur.read_u8()? as usize;
        if txid_len != 32 {
            bail!("unexpected prev_txid length prefix {txid_len}, expected 32");
        }
        let prev_txid_bytes = cur.read_bytes(32)?;
        // Display txid = byte-reversed hex
        let mut rev = prev_txid_bytes.clone();
        rev.reverse();
        let prev_txid = hex::encode(rev);
        let prev_vout  = cur.read_u32_le()?;
        let script_len = cur.read_varint()? as usize;
        let script_sig = cur.read_bytes(script_len)?;
        let sequence   = cur.read_u32_le()?;
        inputs.push(TxInput { prev_txid, prev_vout, script_sig, sequence });
    }

    let out_count = cur.read_varint()?;
    let mut outputs = Vec::with_capacity(out_count as usize);
    for _ in 0..out_count {
        let value      = cur.read_i64_le()?;
        let script_len = cur.read_varint()? as usize;
        let script_pubkey = cur.read_bytes(script_len)?;
        outputs.push(TxOutput { value, script_pubkey });
    }

    let locktime = cur.read_u32_le()?;

    // TXID = SHA256d(raw tx bytes), then byte-reversed for display
    let hash1 = Sha256::digest(&bytes);
    let hash2 = Sha256::digest(hash1);
    let mut txid_bytes: [u8; 32] = hash2.into();
    txid_bytes.reverse();
    let txid = hex::encode(txid_bytes);

    Ok(ParsedTx { txid, version, inputs, outputs, locktime })
}

// ─── Cursor helper ───────────────────────────────────────────────────────────

struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(data: &'a [u8]) -> Self { Self { data, pos: 0 } }

    fn remaining(&self) -> usize { self.data.len().saturating_sub(self.pos) }

    fn read_bytes(&mut self, n: usize) -> Result<Vec<u8>> {
        if self.remaining() < n {
            bail!("unexpected end of tx bytes at pos={}, need {n}", self.pos);
        }
        let slice = self.data[self.pos..self.pos + n].to_vec();
        self.pos += n;
        Ok(slice)
    }

    fn read_u8(&mut self) -> Result<u8> {
        if self.remaining() == 0 {
            bail!("unexpected end of tx bytes reading u8 at pos={}", self.pos);
        }
        let b = self.data[self.pos];
        self.pos += 1;
        Ok(b)
    }

    fn read_u32_le(&mut self) -> Result<u32> {
        let b = self.read_bytes(4)?;
        Ok(u32::from_le_bytes(b.try_into().unwrap()))
    }

    fn read_i32_le(&mut self) -> Result<i32> {
        let b = self.read_bytes(4)?;
        Ok(i32::from_le_bytes(b.try_into().unwrap()))
    }

    fn read_i64_le(&mut self) -> Result<i64> {
        let b = self.read_bytes(8)?;
        Ok(i64::from_le_bytes(b.try_into().unwrap()))
    }

    // Bitcoin / Irium variable-length integer encoding
    fn read_varint(&mut self) -> Result<u64> {
        let tag = self.read_u8()?;
        match tag {
            0xfd => {
                let b = self.read_bytes(2)?;
                Ok(u16::from_le_bytes(b.try_into().unwrap()) as u64)
            }
            0xfe => {
                let b = self.read_bytes(4)?;
                Ok(u32::from_le_bytes(b.try_into().unwrap()) as u64)
            }
            0xff => {
                let b = self.read_bytes(8)?;
                Ok(u64::from_le_bytes(b.try_into().unwrap()))
            }
            n => Ok(n as u64),
        }
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    // Real coinbase tx from block 30220 (fetched from iriumd RPC).
    // Display txid is SHA256d(bytes) byte-reversed = 3e5f34dd17bfa4f1a1c041ed7c65e71ba60a79855270667b7a5661428874077d
    // iriumd merkle_root = 7d0774884261567a7b66705285790aa61be7657ced41c0a1f1a4bf17dd345f3e (internal byte order)
    const COINBASE_HEX: &str = "0100000001200000000000000000000000000000000000000000000000000000000000000000ffffffff10020c76497269756d000000100c2f0000ffffffff0100f2052a010000001976a914950fa3d54aea13c0502e42cdc4d02c714c810d9b88ac00000000";

    #[test]
    fn decodes_coinbase_txid() {
        let tx = decode_tx(COINBASE_HEX).expect("decode failed");
        assert_eq!(
            tx.txid,
            "3e5f34dd17bfa4f1a1c041ed7c65e71ba60a79855270667b7a5661428874077d"
        );
    }

    #[test]
    fn coinbase_input_detected() {
        let tx = decode_tx(COINBASE_HEX).expect("decode failed");
        assert_eq!(tx.inputs.len(), 1);
        assert!(tx.inputs[0].is_coinbase(), "expected coinbase input");
    }

    #[test]
    fn coinbase_version() {
        let tx = decode_tx(COINBASE_HEX).expect("decode failed");
        assert_eq!(tx.version, 1);
    }

    #[test]
    fn coinbase_outputs_count() {
        let tx = decode_tx(COINBASE_HEX).expect("decode failed");
        assert_eq!(tx.outputs.len(), 1);
    }

    #[test]
    fn miner_output_value() {
        let tx = decode_tx(COINBASE_HEX).expect("decode failed");
        // 50 IRM = 5_000_000_000 satoshis (block 30220 reward)
        assert_eq!(tx.outputs[0].value, 5_000_000_000);
    }

    #[test]
    fn coinbase_script_is_p2pkh() {
        let tx = decode_tx(COINBASE_HEX).expect("decode failed");
        let spk = &tx.outputs[0].script_pubkey;
        // P2PKH: 76 a9 14 <20 bytes> 88 ac
        assert_eq!(spk.len(), 25);
        assert_eq!(spk[0], 0x76);
        assert_eq!(spk[1], 0xa9);
        assert_eq!(spk[24], 0xac);
    }

    #[test]
    fn invalid_hex_returns_error() {
        assert!(decode_tx("not hex").is_err());
    }

    #[test]
    fn truncated_bytes_returns_error() {
        // Chop 30 bytes off the end — guaranteed to cut into required fields
        let hex_len = COINBASE_HEX.len() - 60; // 30 bytes = 60 hex chars
        let hex = &COINBASE_HEX[..hex_len];
        assert!(decode_tx(hex).is_err());
    }

    fn make_spend_tx_hex(prev_txid_display: &str, prev_vout: u32) -> String {
        // Build a minimal non-coinbase tx with the Irium length-prefixed format.
        let mut raw = Vec::new();
        // version
        raw.extend_from_slice(&1i32.to_le_bytes());
        // vin_count
        raw.push(1);
        // txid length prefix (0x20 = 32)
        raw.push(0x20);
        // prev_txid: decode display txid (byte-reversed) back to internal
        let mut txid_bytes = hex::decode(prev_txid_display).unwrap();
        txid_bytes.reverse(); // internal = reversed display
        raw.extend_from_slice(&txid_bytes);
        // prev_vout
        raw.extend_from_slice(&prev_vout.to_le_bytes());
        // script_sig (empty)
        raw.push(0);
        // sequence
        raw.extend_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
        // vout_count
        raw.push(0);
        // locktime
        raw.extend_from_slice(&0u32.to_le_bytes());
        hex::encode(raw)
    }

    #[test]
    fn non_coinbase_input_detected() {
        let spend_hex = make_spend_tx_hex(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            0,
        );
        let tx = decode_tx(&spend_hex).expect("decode failed");
        assert!(!tx.inputs[0].is_coinbase());
    }

    #[test]
    fn non_coinbase_zero_vout_is_not_coinbase() {
        // vout=0 with non-null txid must NOT be treated as coinbase
        let spend_hex = make_spend_tx_hex(
            "deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
            0,
        );
        let tx = decode_tx(&spend_hex).expect("decode failed");
        assert!(!tx.inputs[0].is_coinbase());
    }

    #[test]
    fn varint_1byte() {
        let mut c = Cursor::new(&[0x05]);
        assert_eq!(c.read_varint().unwrap(), 5);
    }

    #[test]
    fn varint_fd_2bytes() {
        let mut c = Cursor::new(&[0xfd, 0x00, 0x01]);
        assert_eq!(c.read_varint().unwrap(), 256);
    }

    #[test]
    fn varint_fe_4bytes() {
        let mut c = Cursor::new(&[0xfe, 0x01, 0x00, 0x00, 0x01]);
        assert_eq!(c.read_varint().unwrap(), 0x01000001);
    }
}
