
use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Clone)]
pub struct RpcClient {
    base_url: String,
    client: reqwest::Client,
}

// ─── Wire types ──────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct BlocksResponse {
    pub blocks: Vec<RpcBlock>,
    pub count: u64,
    pub from: u64,
}

#[derive(Debug, Deserialize)]
pub struct RpcBlock {
    pub height: i64,
    pub miner_address: Option<String>,
    pub tx_hex: Vec<String>,
    pub header: RpcHeader,
    pub auxpow_hex: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RpcHeader {
    pub hash: String,
    pub prev_hash: String,
    pub merkle_root: String,
    pub time: i64,
    pub bits: String,
    pub nonce: u64,
    pub version: i64,
}

#[derive(Debug, Deserialize)]
pub struct StatusResponse {
    pub height: i64,
    pub best_hash: String,
}

// ─── Client ──────────────────────────────────────────────────────────────────

impl RpcClient {
    pub fn new(base_url: &str) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("failed to build HTTP client");
        Self { base_url: base_url.trim_end_matches('/').to_string(), client }
    }

    pub async fn get_status(&self) -> Result<StatusResponse> {
        let url = format!("{}/status", self.base_url);
        let resp = self.client.get(&url).send().await
            .context("GET /status failed")?;
        resp.json::<StatusResponse>().await
            .context("failed to deserialise /status response")
    }

    /// Fetch up to `count` blocks starting at height `from`.
    /// iriumd accepts a maximum of 500 blocks per request.
    pub async fn get_blocks(&self, from: i64, count: u64) -> Result<BlocksResponse> {
        let url = format!("{}/rpc/blocks?from={from}&count={count}", self.base_url);
        let resp = self.client.get(&url).send().await
            .context("GET /rpc/blocks failed")?;
        let text = resp.text().await.context("reading /rpc/blocks body")?;
        serde_json::from_str::<BlocksResponse>(&text)
            .with_context(|| format!("deserialising /rpc/blocks: {text}"))
    }
}
