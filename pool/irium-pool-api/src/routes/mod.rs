pub mod health;
pub mod network;
pub mod relay;
pub mod pool;
pub mod miners;
pub mod blocks;

use axum::{Router, routing::get};
use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/health",              get(health::handler))
        .route("/api/v1/network",             get(network::handler))
        .route("/api/v1/relay",               get(relay::handler))
        .route("/api/v1/pool",                get(pool::pool_handler))
        .route("/api/v1/pool/hashrate",       get(pool::hashrate_handler))
        .route("/api/v1/miners",              get(miners::list_handler))
        .route("/api/v1/miner/:address",      get(miners::single_handler))
        .route("/api/v1/blocks",              get(blocks::list_handler))
        .route("/api/v1/block/:height",       get(blocks::single_handler))
}
