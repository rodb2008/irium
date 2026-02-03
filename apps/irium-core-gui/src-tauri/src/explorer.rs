use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::rpc_client::RpcClient;

#[derive(Debug, Serialize, Deserialize)]
pub struct BlockSummary {
    pub height: u64,
    pub hash: String,
    pub time: String,
    pub tx_count: usize,
}

pub async fn fetch_block(client: &RpcClient, height: u64) -> Result<Value, String> {
    client.get_json(&format!("/rpc/block?height={}", height)).await
}

pub async fn fetch_tx(client: &RpcClient, txid: &str) -> Result<Value, String> {
    client.get_json(&format!("/rpc/tx?txid={}", txid)).await
}

pub async fn latest_blocks(client: &RpcClient, limit: usize) -> Result<Vec<BlockSummary>, String> {
    let status: Value = client.get_json("/status").await?;
    let height = status.get("height").and_then(|v| v.as_u64()).unwrap_or(0);
    let mut out = Vec::new();
    let mut h = height as i64;
    while h >= 0 && out.len() < limit {
        if let Ok(block) = fetch_block(client, h as u64).await {
            let header = block.get("header").unwrap_or(&block);
            let hash = header
                .get("hash")
                .and_then(|v| v.as_str())
                .unwrap_or("N/A")
                .to_string();
            let time = header
                .get("time")
                .or_else(|| header.get("timestamp"))
                .and_then(|v| v.as_u64())
                .map(|t| format!("{}", t))
                .unwrap_or_else(|| "0".to_string());
            let txs = block
                .get("tx_hex")
                .and_then(|v| v.as_array())
                .map(|v| v.len())
                .unwrap_or(0);
            out.push(BlockSummary {
                height: h as u64,
                hash,
                time,
                tx_count: txs,
            });
        }
        h -= 1;
    }
    Ok(out)
}
