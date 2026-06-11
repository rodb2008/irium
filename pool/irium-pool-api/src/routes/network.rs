use axum::{extract::State, Json};
use serde_json::{json, Value};
use crate::{AppState, upstream};

pub async fn handler(State(s): State<AppState>) -> Json<Value> {
    let cfg = &s.config;
    let (status, metrics) = tokio::join!(
        upstream::get_node_status(&s.client, &cfg.iriumd_rpc),
        upstream::get_mining_metrics(&s.client, &cfg.iriumd_rpc, &cfg.iriumd_token),
    );
    let synced = status.height > 0 && status.peer_count > 0;
    let supply_irm = format!("{:.2}", status.height as f64 * 50.0);
    Json(json!({
        "height":               status.height,
        "difficulty":           metrics.difficulty,
        "hashrate_hps":         metrics.hashrate as u64,
        "avg_block_time_secs":  metrics.avg_block_time,
        "peers":                status.peer_count,
        "supply_irm":           supply_irm,
        "block_time_target_secs": 120,
        "best_block_hash":      status.best_header_tip.hash,
        "synced":               synced,
    }))
}
