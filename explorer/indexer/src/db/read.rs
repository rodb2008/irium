
use anyhow::Result;
use sqlx::{PgPool, Row};

/// Returns (synced_height, synced_block_hash).
/// synced_height = -1 means nothing indexed yet.
pub async fn get_indexer_state(pool: &PgPool) -> Result<(i64, String)> {
    let row = sqlx::query(
        "SELECT synced_height, synced_block_hash FROM indexer_state WHERE id = 1"
    )
    .fetch_one(pool)
    .await?;
    Ok((row.get("synced_height"), row.get("synced_block_hash")))
}
