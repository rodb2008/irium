
mod blocks;
mod txs;
mod address;
mod agreements;
mod miners;
mod search;
mod status;

use axum::Router;
use sqlx::PgPool;

pub fn router() -> Router<PgPool> {
    Router::new()
        .merge(status::router())
        .merge(blocks::router())
        .merge(txs::router())
        .merge(address::router())
        .merge(agreements::router())
        .merge(miners::router())
        .merge(search::router())
}
