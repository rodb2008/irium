
use axum::{extract::{Query, State}, Json, Router, routing::get};
use serde::Deserialize;
use sqlx::PgPool;
use crate::{db, error::ApiResult};

pub fn router() -> Router<PgPool> {
    Router::new().route("/miners", get(top_miners))
}

#[derive(Deserialize)]
struct LimitParam { limit: Option<i64> }

async fn top_miners(
    State(pool): State<PgPool>,
    Query(q): Query<LimitParam>,
) -> ApiResult<Json<Vec<crate::models::MinerStats>>> {
    let limit = q.limit.unwrap_or(50).min(200);
    Ok(Json(db::get_top_miners(&pool, limit).await?))
}
