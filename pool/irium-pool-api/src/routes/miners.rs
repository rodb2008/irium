use axum::{extract::{State, Path}, Json};
use serde_json::{json, Value};
use std::time::{SystemTime, UNIX_EPOCH};
use crate::{AppState, CachedMiner, db, upstream};

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
}

fn miner_to_json(m: &CachedMiner) -> Value {
    let ago = now_secs().saturating_sub(m.last_share_at);
    json!({
        "address":           m.address,
        "hashrate_hps":      m.hashrate_hps as u64,
        "accepted_shares":   m.accepted,
        "rejected_shares":   m.rejected,
        "reject_rate_pct":   (m.reject_rate_pct * 100.0).round() / 100.0,
        "reject_reasons":    m.reject_reasons,
        "last_share_ago_secs": ago,
        "port":              m.port,
        "profile":           m.profile,
        "active":            m.active,
    })
}

pub async fn list_handler(State(s): State<AppState>) -> Json<Value> {
    let cache = s.cache.lock().unwrap();
    let mut list: Vec<Value> = cache.miners.values().map(miner_to_json).collect();
    list.sort_by(|a, b| {
        let ha = a["hashrate_hps"].as_u64().unwrap_or(0);
        let hb = b["hashrate_hps"].as_u64().unwrap_or(0);
        hb.cmp(&ha)
    });
    Json(json!(list))
}

pub async fn single_handler(
    State(s): State<AppState>,
    Path(address): Path<String>,
) -> Json<Value> {
    let cached = {
        let cache = s.cache.lock().unwrap();
        cache.miners.get(&address).cloned()
    };

    let addr_info = upstream::get_address_info(
        &s.client, &s.config.explorer_url, &address,
    ).await;

    let (db_blocks, db_total) = {
        let conn = s.db.lock().unwrap();
        let blocks = db::blocks_for_miner(&conn, &address, 20).unwrap_or_default();
        let total  = db::count_blocks_for_miner(&conn, &address);
        (blocks, total)
    };

    let recent_blocks: Vec<Value> = db_blocks.iter().map(|b| json!({
        "height":     b.height,
        "block_time": b.block_time,
        "hash":       b.hash,
        "reward_irm": format!("{:.2}", b.reward_sats as f64 / 1e8),
    })).collect();

    let earned_irm = format!("{:.2}", addr_info.balance.mined_balance as f64 / 1e8);

    if let Some(m) = cached {
        let ago = now_secs().saturating_sub(m.last_share_at);
        Json(json!({
            "address":              m.address,
            "hashrate_hps":         m.hashrate_hps as u64,
            "accepted_shares":      m.accepted,
            "rejected_shares":      m.rejected,
            "reject_rate_pct":      (m.reject_rate_pct * 100.0).round() / 100.0,
            "reject_reasons":       m.reject_reasons,
            "last_share_ago_secs":  ago,
            "port":                 m.port,
            "profile":              m.profile,
            "active":               m.active,
            "blocks_found_total":   addr_info.balance.mined_blocks,
            "blocks_found_in_db":   db_total,
            "estimated_earnings_irm": earned_irm,
            "recent_blocks":        recent_blocks,
        }))
    } else {
        Json(json!({
            "address":              address,
            "hashrate_hps":         0u64,
            "accepted_shares":      0u64,
            "rejected_shares":      0u64,
            "reject_rate_pct":      0.0,
            "active":               false,
            "blocks_found_total":   addr_info.balance.mined_blocks,
            "blocks_found_in_db":   db_total,
            "estimated_earnings_irm": earned_irm,
            "recent_blocks":        recent_blocks,
        }))
    }
}
