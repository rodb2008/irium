use axum::{extract::State, Json, Router, routing::get};
use serde::Deserialize;
use sqlx::PgPool;
use crate::{db, error::ApiResult, models::ChainStats};

pub fn router() -> Router<PgPool> {
    Router::new().route("/stats", get(handler))
}

#[derive(Deserialize)]
struct RpcHashrateResp {
    difficulty: f64,
    hashrate: f64,
}

async fn fetch_rpc_stats(base_url: &str) -> Option<(f64, f64)> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(2))
        .build()
        .ok()?;
    let url = format!("{}/rpc/network_hashrate", base_url);
    let resp = client.get(&url).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let data: RpcHashrateResp = resp.json().await.ok()?;
    Some((data.difficulty, data.hashrate))
}

async fn handler(State(pool): State<PgPool>) -> ApiResult<Json<ChainStats>> {
    let (height, total_txs, total_addresses, circulating_supply) =
        db::get_db_stats(&pool).await?;

    let rpc_url = std::env::var("IRIUMD_RPC_URL")
        .unwrap_or_else(|_| "http://host.docker.internal:38300".to_string());

    let rpc = fetch_rpc_stats(&rpc_url).await;
    let (difficulty, network_hashrate) = match rpc {
        Some((d, h)) => (Some(d), Some(h)),
        None => (None, None),
    };

    Ok(Json(ChainStats {
        height,
        total_txs,
        total_addresses,
        difficulty,
        network_hashrate,
        peer_count: 0,
        circulating_supply,
    }))
}
