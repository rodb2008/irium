
mod config;
mod db;
mod error;
mod models;
mod rate_limit;
mod routes;

use anyhow::Result;
use axum::Router;
use sqlx::postgres::PgPoolOptions;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing::info;
use tracing_subscriber::{EnvFilter, fmt};

#[tokio::main]
async fn main() -> Result<()> {
    let _ = dotenvy::dotenv();

    fmt()
        .with_env_filter(EnvFilter::from_default_env()
            .add_directive("irium_explorer_api=info".parse().unwrap()))
        .init();

    let cfg = config::Config::from_env()?;
    info!("connecting to database");

    let pool = PgPoolOptions::new()
        .max_connections(20)
        .connect(&cfg.database_url)
        .await?;

    let rl = rate_limit::build(cfg.rate_limit_rps);
    let trusted = cfg.trusted_ips.clone();

    let app = Router::new()
        .merge(routes::router())
        .layer(axum::middleware::from_fn_with_state(
            (rl, trusted),
            rate_limit::middleware,
        ))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(pool)
        .into_make_service_with_connect_info::<std::net::SocketAddr>();

    let addr = format!("{}:{}", cfg.bind_host, cfg.bind_port);
    info!("API listening on {addr}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
