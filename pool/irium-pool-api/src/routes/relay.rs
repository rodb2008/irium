use axum::{extract::State, Json};
use serde_json::{json, Value};
use crate::{AppState, upstream};

pub async fn handler(State(s): State<AppState>) -> Json<Value> {
    let cfg = &s.config;
    let (btc, ltc) = tokio::join!(
        upstream::get_btc_relay(&s.client, &cfg.iriumd_rpc, &cfg.iriumd_token),
        upstream::get_ltc_relay(&s.client, &cfg.iriumd_rpc, &cfg.iriumd_token),
    );
    Json(json!({
        "btc": {
            "active":     btc.active,
            "tip_height": btc.tip_height,
            "tip_hash":   btc.tip_hash,
            "tip_time":   btc.tip_time,
        },
        "ltc": {
            "active":     ltc.active,
            "tip_height": ltc.tip_height,
            "tip_hash":   ltc.tip_hash,
            "tip_time":   ltc.tip_time,
        },
    }))
}
