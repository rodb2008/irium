
use axum::{extract::State, Json, Router, routing::get};
use sqlx::PgPool;
use crate::{db, error::ApiResult};

pub fn router() -> Router<PgPool> {
    Router::new().route("/status", get(handler))
}

async fn handler(State(pool): State<PgPool>) -> ApiResult<Json<crate::models::ExplorerStatus>> {
    Ok(Json(db::get_status(&pool).await?))
}
