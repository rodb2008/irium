mod api;
mod btc;
mod irium;
mod model;
mod state_machine;
mod storage;

use std::{collections::HashSet, net::SocketAddr, sync::Arc, time::Duration};

use anyhow::Result;
use tokio::sync::RwLock;
use tracing::info;

use crate::{btc::BtcClient, irium::IriumClient, storage::Storage};

#[derive(Clone)]
pub struct AppConfig {
    pub operator_token: String,
    pub invite_codes: HashSet<String>,
    pub expected_amount_sats: u64,
    pub btc_min_confirmations: u32,
    pub auto_detect_btc: bool,
    pub auto_create_irium_htlc: bool,
    pub public_enabled: bool,
}

#[derive(Clone)]
pub struct AppCtx {
    pub storage: Storage,
    pub cfg: AppConfig,
    pub btc: BtcClient,
    pub irium: IriumClient,
    pub intake_paused: Arc<RwLock<bool>>,
}

fn flag(name: &str, default: bool) -> bool {
    std::env::var(name)
        .ok()
        .map(|v| matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"))
        .unwrap_or(default)
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let bind: SocketAddr = std::env::var("COORDINATOR_BIND")
        .unwrap_or_else(|_| "127.0.0.1:8088".to_string())
        .parse()?;
    let db_path =
        std::env::var("COORDINATOR_DB").unwrap_or_else(|_| "./swap-coordinator.db".to_string());

    let operator_token = std::env::var("COORDINATOR_OPERATOR_TOKEN")
        .unwrap_or_else(|_| "change-me-operator-token".to_string());
    let invite_codes = std::env::var("COORDINATOR_INVITE_CODES")
        .unwrap_or_default()
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect::<HashSet<_>>();

    let expected_amount_sats = std::env::var("COORDINATOR_EXPECTED_AMOUNT_SATS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(100_000);

    let btc_min_confirmations = std::env::var("COORDINATOR_BTC_MIN_CONFIRMATIONS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1);

    // mainnet-safe defaults: off unless explicitly enabled
    let public_enabled = flag("COORDINATOR_PUBLIC_ENABLED", false);
    let auto_detect_btc = flag("COORDINATOR_AUTO_DETECT_BTC", false);
    let auto_create_irium_htlc = flag("COORDINATOR_AUTO_CREATE_IRIUM_HTLC", false);

    let storage = Storage::open(&db_path)?;
    let paused = storage.intake_paused().unwrap_or(false);

    let btc = if let Ok(url) = std::env::var("BTC_RPC_URL") {
        BtcClient::enabled(
            url,
            std::env::var("BTC_RPC_USER").ok(),
            std::env::var("BTC_RPC_PASS")
                .ok()
                .or_else(|| std::env::var("BTC_RPC_PASSWORD").ok()),
            btc_min_confirmations,
        )
    } else {
        BtcClient::disabled(btc_min_confirmations)
    };

    // no implicit local fallback: must be explicitly configured
    let irium = if let Ok(url) = std::env::var("IRIUM_RPC_URL") {
        IriumClient {
            rpc_url: Some(url),
            rpc_token: std::env::var("IRIUM_RPC_TOKEN").ok(),
            recipient_address: std::env::var("IRIUM_RECIPIENT_ADDRESS").ok(),
            refund_address: std::env::var("IRIUM_REFUND_ADDRESS").ok(),
            amount_irm: std::env::var("IRIUM_AMOUNT").unwrap_or_else(|_| "1.0".to_string()),
            timeout_blocks: std::env::var("IRIUM_TIMEOUT_BLOCKS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(120),
        }
    } else {
        IriumClient::disabled()
    };

    let ctx = AppCtx {
        storage,
        cfg: AppConfig {
            operator_token,
            invite_codes,
            expected_amount_sats,
            btc_min_confirmations,
            auto_detect_btc,
            auto_create_irium_htlc,
            public_enabled,
        },
        btc,
        irium,
        intake_paused: Arc::new(RwLock::new(paused)),
    };

    let poll_ctx = ctx.clone();
    tokio::spawn(async move {
        loop {
            if let Err(e) = api::poll_progression(poll_ctx.clone()).await {
                tracing::warn!("poll_progression error: {e}");
            }
            tokio::time::sleep(Duration::from_secs(10)).await;
        }
    });

    let app = api::router(ctx);
    info!("atomic-swap-coordinator listening on {bind}");
    let listener = tokio::net::TcpListener::bind(bind).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

#[cfg(test)]
mod tests;
