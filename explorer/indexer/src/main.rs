
mod config;
mod db;
mod decoder;
mod indexer;
mod rpc;

use anyhow::Result;
use sqlx::postgres::PgPoolOptions;
use tracing::info;
use tracing_subscriber::{EnvFilter, fmt};

#[tokio::main]
async fn main() -> Result<()> {
    // Load .env if present (ignore if missing)
    let _ = dotenvy::dotenv();

    fmt()
        .with_env_filter(EnvFilter::from_default_env()
            .add_directive("irium_explorer_indexer=info".parse().unwrap()))
        .init();

    let cfg = config::Config::from_env()?;
    info!("connecting to database");

    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&cfg.database_url)
        .await?;

    // Run pending migrations
    sqlx::migrate!("./migrations").run(&pool).await?;
    info!("migrations applied");

    let rpc = rpc::RpcClient::new(&cfg.rpc_url);
    let status = rpc.get_status().await?;
    info!("iriumd at height {}", status.height);

    indexer::run(pool, rpc, cfg).await
}
