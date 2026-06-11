use axum::{extract::{State, Query}, Json};
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::{SystemTime, UNIX_EPOCH};
use crate::{AppState, db};

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
}

pub async fn pool_handler(State(s): State<AppState>) -> Json<Value> {
    let c = s.cache.lock().unwrap().clone();
    let total_accepted = c.asic_accepted + c.cpu_accepted + c.solo_accepted + c.p443_accepted;
    let total_rejected = c.asic_rejected + c.cpu_rejected + c.solo_rejected + c.p443_rejected;
    let reject_rate = if total_accepted + total_rejected > 0 {
        (total_rejected as f64 / (total_accepted + total_rejected) as f64) * 100.0
    } else {
        0.0
    };
    Json(json!({
        "total_hashrate_hps":  c.total_hashrate_hps as u64,
        "active_miners":       c.active_miners,
        "blocks_found_today":  c.blocks_found_today,
        "blocks_found_total":  c.blocks_found_total,
        "accepted_shares":     total_accepted,
        "rejected_shares":     total_rejected,
        "reject_rate_pct":     (reject_rate * 1000.0).round() / 1000.0,
        "updated_at":          c.updated_at,
        "ports": {
            "asic":            { "port": 3333, "sessions": c.asic_sessions, "hashrate_hps": c.asic_hashrate as u64, "accepted": c.asic_accepted, "rejected": c.asic_rejected },
            "cpu_gpu":         { "port": 3335, "sessions": c.cpu_sessions,  "hashrate_hps": c.cpu_hashrate as u64,  "accepted": c.cpu_accepted,  "rejected": c.cpu_rejected  },
            "solo":            { "port": 3336, "sessions": c.solo_sessions, "hashrate_hps": c.solo_hashrate as u64, "accepted": c.solo_accepted, "rejected": c.solo_rejected },
            "firewall_bypass": { "port": 443,  "sessions": c.p443_sessions, "hashrate_hps": c.p443_hashrate as u64, "accepted": c.p443_accepted, "rejected": c.p443_rejected },
        },
    }))
}

#[derive(Deserialize)]
pub struct HashratePeriod {
    #[serde(default)]
    pub period: String,
}

pub async fn hashrate_handler(
    State(s): State<AppState>,
    Query(q): Query<HashratePeriod>,
) -> Json<Value> {
    let now  = now_secs();
    let since = match q.period.as_str() {
        "6h"  => now - 6 * 3600,
        "24h" => now - 86400,
        "7d"  => now - 7 * 86400,
        _     => now - 3600,
    };
    let rows = {
        let conn = s.db.lock().unwrap();
        db::get_snapshots(&conn, since).unwrap_or_default()
    };
    let data: Vec<Value> = rows.iter()
        .map(|(t, h)| json!({ "unix_time": t, "hashrate_hps": *h as u64 }))
        .collect();
    Json(json!({ "period": q.period, "data": data }))
}
