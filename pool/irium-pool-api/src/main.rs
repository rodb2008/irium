mod config;
mod db;
mod upstream;
mod poller;
mod routes;

use axum::Router;
use config::Config;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tower_http::cors::{CorsLayer, Any};
use tracing::info;

#[derive(Default, Clone)]
pub struct CachedMiner {
    pub address:         String,
    pub hashrate_hps:    f64,
    pub accepted:        u64,
    pub rejected:        u64,
    pub reject_rate_pct: f64,
    pub reject_reasons:  HashMap<String, u64>,
    pub last_share_at:   u64,
    pub current_diff:    f64,
    pub port:            u16,
    pub profile:         String,
    pub active:          bool,
}

#[derive(Default, Clone)]
pub struct LiveCache {
    pub miners:             HashMap<String, CachedMiner>,
    pub total_hashrate_hps: f64,
    pub active_miners:      u64,
    pub asic_sessions:      u64,
    pub cpu_sessions:       u64,
    pub solo_sessions:      u64,
    pub p443_sessions:      u64,
    pub asic_hashrate:      f64,
    pub cpu_hashrate:       f64,
    pub solo_hashrate:      f64,
    pub p443_hashrate:      f64,
    pub asic_accepted:      u64,
    pub asic_rejected:      u64,
    pub cpu_accepted:       u64,
    pub cpu_rejected:       u64,
    pub solo_accepted:      u64,
    pub solo_rejected:      u64,
    pub p443_accepted:      u64,
    pub p443_rejected:      u64,
    pub blocks_found_today: u64,
    pub blocks_found_total: u64,
    pub updated_at:         u64,
}

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub db:     Arc<Mutex<rusqlite::Connection>>,
    pub client: reqwest::Client,
    pub cache:  Arc<Mutex<LiveCache>>,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_env("RUST_LOG")
                .add_directive("irium_pool_api=info".parse().unwrap()),
        )
        .init();

    let cfg = Arc::new(Config::from_env());
    let conn = db::init(&cfg.db_path).expect("failed to open SQLite DB");

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .expect("failed to build HTTP client");

    let state = AppState {
        config: cfg.clone(),
        db:     Arc::new(Mutex::new(conn)),
        client,
        cache:  Arc::new(Mutex::new(LiveCache::default())),
    };

    {
        let s = state.clone();
        tokio::spawn(async move { poller::run(s).await });
    }

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .merge(routes::router())
        .with_state(state)
        .layer(cors);

    let addr = SocketAddr::from(([0, 0, 0, 0], cfg.port));
    info!("irium-pool-api listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("failed to bind port");
    axum::serve(listener, app).await.expect("server error");
}
