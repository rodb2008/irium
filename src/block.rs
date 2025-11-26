use crate::pow::{header_hash, Target};
use crate::tx::Transaction;

#[derive(Debug, Clone)]
pub struct BlockHeader {
    pub version: u32,
    pub prev_hash: [u8; 32],
    pub merkle_root: [u8; 32],
    pub time: u32,
    pub bits: u32,
    pub nonce: u32,
}

impl BlockHeader {
#[allow(dead_code)]
    pub fn serialize(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(80);
        out.extend_from_slice(&self.version.to_le_bytes());
        let mut prev = self.prev_hash;
        prev.reverse();
        out.extend_from_slice(&prev);
        let mut merkle = self.merkle_root;
        merkle.reverse();
        out.extend_from_slice(&merkle);
        out.extend_from_slice(&self.time.to_le_bytes());
        out.extend_from_slice(&self.bits.to_le_bytes());
        out.extend_from_slice(&self.nonce.to_le_bytes());
        out
    }

    pub fn hash(&self) -> [u8; 32] {
        let ser = self.serialize();
        let mut h = header_hash(&[&ser]);
        h.reverse();
        h
    }

    pub fn target(&self) -> Target {
        Target { bits: self.bits }
    }

    /// Deserialize a header from the 80-byte compact encoding.
#[allow(dead_code)]
#[allow(dead_code)]
    pub fn deserialize(raw: &[u8]) -> Result<(Self, usize), String> {
        if raw.len() < 80 {
            return Err("header too short".to_string());
        }
        let mut offset = 0usize;
        let read_u32 = |buf: &[u8], off: &mut usize| -> Result<u32, String> {
            if *off + 4 > buf.len() {
                return Err("unexpected EOF".to_string());
            }
            let mut bytes = [0u8; 4];
            bytes.copy_from_slice(&buf[*off..*off + 4]);
            *off += 4;
            Ok(u32::from_le_bytes(bytes))
        };

        let version = read_u32(raw, &mut offset)?;
        let mut prev_hash = [0u8; 32];
        prev_hash.copy_from_slice(&raw[offset..offset + 32]);
        prev_hash.reverse();
        offset += 32;
        let mut merkle_root = [0u8; 32];
        merkle_root.copy_from_slice(&raw[offset..offset + 32]);
        merkle_root.reverse();
        offset += 32;
        let time = read_u32(raw, &mut offset)?;
        let bits = read_u32(raw, &mut offset)?;
        let nonce = read_u32(raw, &mut offset)?;

        Ok((
            BlockHeader {
                version,
                prev_hash,
                merkle_root,
                time,
                bits,
                nonce,
            },
            offset,
        ))
    }
}

#[derive(Debug, Clone)]
pub struct Block {
    pub header: BlockHeader,
    pub transactions: Vec<Transaction>,
}

impl Block {
    pub fn merkle_root(&self) -> [u8; 32] {
        if self.transactions.is_empty() {
            return [0u8; 32];
        }
        let mut leaves: Vec<[u8; 32]> = self
            .transactions
            .iter()
            .map(|tx| tx.txid())
            .map(|mut h| {
                h.reverse();
                h
            })
            .collect();
        while leaves.len() > 1 {
            if leaves.len() % 2 == 1 {
                let last = *leaves.last().unwrap();
                leaves.push(last);
            }
            let mut next = Vec::with_capacity(leaves.len() / 2);
            for pair in leaves.chunks(2) {
                let h = header_hash(&[&pair[0], &pair[1]]);
                next.push(h);
            }
            leaves = next;
        }
        leaves[0]
    }

    /// Deserialize a block encoded as header + concatenated transactions.
    /// Returns the block and the number of bytes consumed from the input slice.
    #[allow(dead_code)]
    pub fn deserialize(raw: &[u8]) -> Result<(Self, usize), String> {
        let (header, mut offset) = BlockHeader::deserialize(raw)?;
        let mut txs = Vec::new();

        while offset < raw.len() {
            let tx = crate::tx::decode_full_tx_at(raw, &mut offset)?;
            txs.push(tx);
        }

        Ok((
            Block {
                header,
                transactions: txs,
            },
            offset,
        ))
    }

    /// Serialize the block as header + concatenated transactions.
    #[allow(dead_code)]
    pub fn serialize(&self) -> Vec<u8> {
        let mut out = self.header.serialize();
        for tx in &self.transactions {
            out.extend_from_slice(&tx.serialize());
        }
        out
    }
}
