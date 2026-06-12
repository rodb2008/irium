
use axum::{extract::{Path, Query, State}, Json, Router, routing::get};
use serde::Deserialize;
use sqlx::PgPool;
use crate::{db, error::{ApiError, ApiResult}};
use axum::http::StatusCode;

pub fn router() -> Router<PgPool> {
    Router::new()
        .route("/address/:address", get(get_address))
        .route("/address/:address/txs", get(get_address_txs))
        .route("/address/:address/htlcs", get(get_address_htlcs))
}

#[derive(Deserialize)]
struct LimitParam { limit: Option<i64> }

async fn get_address(
    State(pool): State<PgPool>,
    Path(address): Path<String>,
) -> ApiResult<Json<crate::models::AddressStats>> {
    db::get_address(&pool, &address).await?
        .map(Json)
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "address not found".into()))
}

async fn get_address_txs(
    State(pool): State<PgPool>,
    Path(address): Path<String>,
    Query(q): Query<LimitParam>,
) -> ApiResult<Json<Vec<crate::models::AddressTx>>> {
    let limit = q.limit.unwrap_or(50).min(200);
    Ok(Json(db::get_address_txs(&pool, &address, limit).await?))
}

async fn get_address_htlcs(
    State(pool): State<PgPool>,
    Path(address): Path<String>,
    Query(q): Query<LimitParam>,
) -> ApiResult<Json<Vec<crate::models::HtlcInfo>>> {
    let limit = q.limit.unwrap_or(50).min(200);
    Ok(Json(db::get_htlcs(&pool, &address, limit).await?))
}
