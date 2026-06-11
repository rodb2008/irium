use reqwest::Client;
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Deserialize, Default, Clone)]
pub struct StratumMetrics {
    pub accepted_shares:        u64,
    pub rejected_shares:        u64,
    pub active_tcp_sessions:    u64,
    pub submit_accepted:        u64,
    pub submit_rejected:        u64,
    pub last_share_accepted_at: u64,
    #[serde(default)]
    pub global_reject_reasons:  HashMap<String, u64>,
    #[serde(default)]
    pub miners:                 HashMap<String, WorkerMetrics>,
}

#[derive(Deserialize, Default, Clone)]
pub struct WorkerMetrics {
    pub accepted:       u64,
    pub rejected:       u64,
    pub current_diff:   f64,
    pub last_share_at:  u64,
    #[serde(default)]
    pub reject_reasons: HashMap<String, u64>,
}

#[derive(Deserialize, Default, Clone)]
pub struct NodeStatus {
    pub height:           u64,
    pub peer_count:       u64,
    #[serde(default)]
    pub best_header_tip:  BestHeaderTip,
    #[serde(default)]
    pub persisted_height: u64,
}

#[derive(Deserialize, Default, Clone)]
pub struct BestHeaderTip {
    pub height: u64,
    #[serde(default)]
    pub hash: String,
}

#[derive(Deserialize, Default, Clone)]
pub struct MiningMetrics {
    pub difficulty:     f64,
    pub hashrate:       f64,
    pub avg_block_time: f64,
    pub tip_height:     u64,
    #[serde(default)]
    pub tip_time:       u64,
}

#[derive(Deserialize, Default, Clone)]
pub struct RelayTip {
    pub active:        bool,
    #[serde(default)]
    pub tip_height:    u64,
    #[serde(default)]
    pub tip_hash:      String,
    #[serde(default)]
    pub tip_time:      u64,
    #[serde(default)]
    pub anchor_height: u64,
}

#[derive(Deserialize, Clone)]
pub struct ExplorerBlock {
    pub height:        u64,
    #[serde(default)]
    pub miner_address: String,
    pub header:        ExplorerHeader,
}

#[derive(Deserialize, Default, Clone)]
pub struct ExplorerHeader {
    pub time: u64,
    #[serde(default)]
    pub hash: String,
    #[serde(default)]
    pub bits: String,
}

#[derive(Deserialize, Default, Clone)]
pub struct ExplorerBlocksResponse {
    pub blocks: Vec<ExplorerBlock>,
    #[serde(default)]
    pub total_blocks: u64,
}

#[derive(Deserialize, Default, Clone)]
pub struct ExplorerAddress {
    #[serde(default)]
    pub balance: AddressBalance,
}

#[derive(Deserialize, Default, Clone)]
pub struct AddressBalance {
    pub mined_blocks:  u64,
    pub mined_balance: u64,
    pub balance:       u64,
}

async fn fetch_json<T: for<'de> Deserialize<'de>>(
    client: &Client,
    url: &str,
    token: Option<&str>,
) -> Option<T> {
    let mut req = client.get(url);
    if let Some(t) = token {
        req = req.bearer_auth(t);
    }
    req.send().await.ok()?.json().await.ok()
}

pub async fn get_stratum(client: &Client, url: &str) -> StratumMetrics {
    fetch_json::<StratumMetrics>(client, url, None)
        .await
        .unwrap_or_default()
}

pub async fn get_node_status(client: &Client, base: &str) -> NodeStatus {
    let url = format!("{}/status", base);
    fetch_json::<NodeStatus>(client, &url, None)
        .await
        .unwrap_or_default()
}

pub async fn get_mining_metrics(client: &Client, base: &str, token: &str) -> MiningMetrics {
    let url = format!("{}/rpc/mining_metrics", base);
    fetch_json::<MiningMetrics>(client, &url, Some(token))
        .await
        .unwrap_or_default()
}

pub async fn get_btc_relay(client: &Client, base: &str, token: &str) -> RelayTip {
    let url = format!("{}/rpc/btcrelaytip", base);
    fetch_json::<RelayTip>(client, &url, Some(token))
        .await
        .unwrap_or_default()
}

pub async fn get_ltc_relay(client: &Client, base: &str, token: &str) -> RelayTip {
    let url = format!("{}/rpc/ltcrelaytip", base);
    fetch_json::<RelayTip>(client, &url, Some(token))
        .await
        .unwrap_or_default()
}

pub async fn get_explorer_blocks(client: &Client, base: &str, limit: u64) -> Vec<ExplorerBlock> {
    let url = format!("{}/api/blocks?limit={}", base, limit);
    fetch_json::<ExplorerBlocksResponse>(client, &url, None)
        .await
        .unwrap_or_default()
        .blocks
}

pub async fn get_address_info(client: &Client, base: &str, address: &str) -> ExplorerAddress {
    let url = format!("{}/api/address/{}", base, address);
    fetch_json::<ExplorerAddress>(client, &url, None)
        .await
        .unwrap_or_default()
}

pub async fn check_reachable(client: &Client, url: &str, token: Option<&str>) -> bool {
    let mut req = client.get(url);
    if let Some(t) = token {
        req = req.bearer_auth(t);
    }
    req.send().await
        .map(|r| r.status().as_u16() < 500)
        .unwrap_or(false)
}
