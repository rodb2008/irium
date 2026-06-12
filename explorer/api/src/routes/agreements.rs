
use axum::{extract::{Path, State}, Json, Router, routing::get};
use sqlx::PgPool;
use crate::{db, error::{ApiError, ApiResult}};
use axum::http::StatusCode;

pub fn router() -> Router<PgPool> {
    Router::new().route("/agreement/{hash}", get(get_agreement))
}

async fn get_agreement(
    State(pool): State<PgPool>,
    Path(hash): Path<String>,
) -> ApiResult<Json<crate::models::AgreementInfo>> {
    db::get_agreement(&pool, &hash).await?
        .map(Json)
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "agreement not found".into()))
}
