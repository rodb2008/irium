use axum::{extract::State, Json};
use serde_json::{json, Value};
use crate::{AppState, upstream};

pub async fn handler(State(s): State<AppState>) -> Json<Value> {
    let cfg = &s.config;
    let status_url  = format!("{}/status", cfg.iriumd_rpc);
    let explorer_url = format!("{}/api/status", cfg.explorer_url);
    let (iriumd, asic, cpu, solo, p443, explorer) = tokio::join!(
        upstream::check_reachable(&s.client, &status_url,   None),
        upstream::check_reachable(&s.client, &cfg.stratum_asic, None),
        upstream::check_reachable(&s.client, &cfg.stratum_cpu,  None),
        upstream::check_reachable(&s.client, &cfg.stratum_solo, None),
        upstream::check_reachable(&s.client, &cfg.stratum_443,  None),
        upstream::check_reachable(&s.client, &explorer_url, None),
    );
    let db_ok = s.db.lock().is_ok();
    let all_ok = iriumd && asic && explorer && db_ok;
    Json(json!({
        "status":       if all_ok { "ok" } else { "degraded" },
        "iriumd":       if iriumd   { "connected" } else { "error" },
        "stratum_asic": if asic     { "connected" } else { "error" },
        "stratum_cpu":  if cpu      { "connected" } else { "error" },
        "stratum_solo": if solo     { "connected" } else { "error" },
        "stratum_443":  if p443     { "connected" } else { "error" },
        "explorer":     if explorer { "connected" } else { "error" },
        "db":           if db_ok    { "ok" } else { "error" },
    }))
}
