
use axum::{extract::{Path, State}, Json, Router, routing::get};
use sqlx::PgPool;
use crate::{db, error::{ApiError, ApiResult}};
use axum::http::StatusCode;

pub fn router() -> Router<PgPool> {
    Router::new().route("/tx/:txid", get(get_tx))
}

async fn get_tx(
    State(pool): State<PgPool>,
    Path(txid): Path<String>,
) -> ApiResult<Json<crate::models::TxDetail>> {
    db::get_tx(&pool, &txid).await?
        .map(Json)
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "tx not found".into()))
}
