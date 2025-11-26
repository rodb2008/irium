use crate::tx::TxOutput;

/// Relay commitment describing a fee-sharing payout for a relay peer.
///
/// This mirrors `irium.miner.RelayCommitment` and is used when
/// constructing coinbase transactions that pay a portion of the fees
/// to relay nodes without changing consensus rules.
#[derive(Debug, Clone)]
pub struct RelayCommitment {
    pub address: String,
    pub amount: u64,
    pub memo: Option<String>,
}

impl RelayCommitment {
    /// Build the outputs corresponding to this relay commitment:
    /// - a standard P2PKH output paying `amount` to `address`
    /// - optional OP_RETURN metadata output for the memo, prefixed with `relay:`
    pub fn build_outputs<F>(&self, address_to_script: F) -> Result<Vec<TxOutput>, String>
    where
        F: Fn(&str) -> Result<Vec<u8>, String>,
    {
        let script = address_to_script(&self.address)?;
        let mut outputs = Vec::new();
        outputs.push(TxOutput {
            value: self.amount,
            script_pubkey: script,
        });

        if let Some(memo) = &self.memo {
            let memo_bytes = memo.as_bytes();
            if memo_bytes.len() > 64 {
                return Err("Relay memo exceeds 64 bytes".to_string());
            }
            let payload = {
                let mut v = Vec::with_capacity(6 + memo_bytes.len());
                v.extend_from_slice(b"relay:");
                v.extend_from_slice(memo_bytes);
                v
            };
            outputs.push(commitment_op_return(&payload)?);
        }

        Ok(outputs)
    }
}

/// Build an OP_RETURN output carrying a small metadata payload.
fn commitment_op_return(data: &[u8]) -> Result<TxOutput, String> {
    if data.len() > 75 {
        return Err("Commitment payload too large".to_string());
    }
    let mut script = Vec::with_capacity(2 + data.len());
    // OP_RETURN
    script.push(0x6a);
    // single-byte push
    script.push(data.len() as u8);
    script.extend_from_slice(data);
    Ok(TxOutput {
        value: 0,
        script_pubkey: script,
    })
}
