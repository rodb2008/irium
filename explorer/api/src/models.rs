
use chrono::{DateTime, Utc};
use serde::Serialize;

#[derive(Serialize)]
pub struct BlockSummary {
    pub height: i64,
    pub hash: String,
    pub timestamp: DateTime<Utc>,
    pub tx_count: i32,
    pub miner_address: Option<String>,
    pub total_reward: i64,
    pub coinbase_tag: Option<String>,
}

#[derive(Serialize)]
pub struct BlockDetail {
    pub height: i64,
    pub hash: String,
    pub prev_hash: String,
    pub merkle_root: String,
    pub timestamp: DateTime<Utc>,
    pub difficulty: String,
    pub nonce: String,
    pub tx_count: i32,
    pub miner_address: Option<String>,
    pub total_reward: i64,
    pub txids: Vec<String>,
    pub coinbase_tag: Option<String>,
}

#[derive(Serialize)]
pub struct TxInput {
    pub prev_txid: String,
    pub prev_vout: i64,
    pub script_sig_hex: String,
    pub is_coinbase: bool,
}

#[derive(Serialize)]
pub struct TxOutput {
    pub vout: i32,
    pub value: i64,
    pub script_type: String,
    pub address: Option<String>,
    pub spent_by_txid: Option<String>,
}

#[derive(Serialize)]
pub struct TxDetail {
    pub txid: String,
    pub block_height: i64,
    pub block_hash: String,
    pub tx_index: i32,
    pub is_coinbase: bool,
    pub input_count: i32,
    pub output_count: i32,
    pub total_out: i64,
    pub fee: i64,
    pub inputs: Vec<TxInput>,
    pub outputs: Vec<TxOutput>,
}

#[derive(Serialize)]
pub struct AddressStats {
    pub address: String,
    pub balance: i64,
    pub total_received: i64,
    pub total_sent: i64,
    pub tx_count: i32,
}

#[derive(Serialize)]
pub struct AddressTx {
    pub txid: String,
    pub block_height: i64,
    pub total_out: i64,
}

#[derive(Serialize)]
pub struct HtlcInfo {
    pub txid: String,
    pub vout: i32,
    pub block_height: i64,
    pub htlc_type: String,
    pub value: i64,
    pub recipient_addr: String,
    pub refund_addr: String,
    pub secret_hash: String,
    pub timeout_height: i64,
    pub state: String,
    pub spend_txid: Option<String>,
}

#[derive(Serialize)]
pub struct AgreementInfo {
    pub agreement_hash: String,
    pub anchor_type: String,
    pub txid: String,
    pub block_height: i64,
    pub milestone_id: Option<String>,
}

#[derive(Serialize)]
pub struct MinerStats {
    pub address: String,
    pub blocks_mined: i32,
    pub total_reward: i64,
    pub last_block_height: Option<i64>,
}

#[derive(Serialize)]
pub struct ExplorerStatus {
    pub synced_height: i64,
    pub synced_block_hash: String,
}
