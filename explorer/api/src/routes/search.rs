
use axum::{extract::{Query, State}, Json, Router, routing::get};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use crate::{db::{self, SearchResult}, error::ApiResult};

pub fn router() -> Router<PgPool> {
    Router::new().route("/search", get(search))
}

#[derive(Deserialize)]
struct SearchQuery { q: String }

#[derive(Serialize)]
struct SearchResponse {
    result_type: String,
    value: String,
}

async fn search(
    State(pool): State<PgPool>,
    Query(q): Query<SearchQuery>,
) -> ApiResult<Json<Option<SearchResponse>>> {
    let result = db::search(&pool, &q.q).await?;
    Ok(Json(result.map(|r| match r {
        SearchResult::Block(h)   => SearchResponse { result_type: "block".into(),   value: h.to_string() },
        SearchResult::Tx(txid)   => SearchResponse { result_type: "tx".into(),      value: txid },
        SearchResult::Address(a) => SearchResponse { result_type: "address".into(), value: a },
    })))
}
