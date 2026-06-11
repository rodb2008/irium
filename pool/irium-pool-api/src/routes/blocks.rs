use axum::{extract::{State, Path, Query}, Json};
use serde::Deserialize;
use serde_json::{json, Value};
use crate::{AppState, db, upstream};

fn block_to_json(b: &db::BlockRow) -> Value {
    json!({
        "height":         b.height,
        "miner_address":  b.miner_address,
        "block_time":     b.block_time,
        "difficulty":     b.difficulty,
        "reward_irm":     format!("{:.2}", b.reward_sats as f64 / 1e8),
        "hash":           b.hash,
        "found_at":       b.found_at_unix,
    })
}

#[derive(Deserialize)]
pub struct BlocksQuery {
    #[serde(default = "default_limit")]
    pub limit: u64,
    #[serde(default = "default_page")]
    pub page: u64,
}
fn default_limit() -> u64 { 50 }
fn default_page() -> u64 { 1 }

pub async fn list_handler(
    State(s): State<AppState>,
    Query(q): Query<BlocksQuery>,
) -> Json<Value> {
    let limit  = q.limit.min(200);
    let page   = q.page.max(1);
    let offset = (page - 1) * limit;
    let (blocks, total) = {
        let conn = s.db.lock().unwrap();
        (db::get_blocks(&conn, limit, offset).unwrap_or_default(),
         db::count_blocks(&conn))
    };
    let list: Vec<Value> = blocks.iter().map(block_to_json).collect();
    Json(json!({ "blocks": list, "total": total, "page": page, "limit": limit }))
}

pub async fn single_handler(
    State(s): State<AppState>,
    Path(height): Path<u64>,
) -> Json<Value> {
    let from_db = {
        let conn = s.db.lock().unwrap();
        db::get_block(&conn, height).unwrap_or(None)
    };
    if let Some(b) = from_db {
        return Json(block_to_json(&b));
    }
    let blocks = upstream::get_explorer_blocks(&s.client, &s.config.explorer_url, 200).await;
    if let Some(b) = blocks.iter().find(|b| b.height == height) {
        return Json(json!({
            "height":        b.height,
            "miner_address": b.miner_address,
            "block_time":    b.header.time,
            "difficulty":    null,
            "reward_irm":    "50.00",
            "hash":          b.header.hash,
            "found_at":      null,
        }));
    }
    Json(json!({ "error": "block not found", "height": height }))
}
