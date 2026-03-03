mod block;
mod pow;
mod stratum;
mod template;

use anyhow::{anyhow, Result};
use std::env;
use stratum::{run, HashCmpMode, MinerFamilyMode, StratumConfig};
use tracing_subscriber::EnvFilter;

fn env_required(key: &str) -> Result<String> {
    env::var(key).map_err(|_| anyhow!("missing env {key}"))
}

#[tokio::main]
async fn main() -> Result<()> {
    let log_level = env::var("LOG_LEVEL").unwrap_or_else(|_| "info".to_string());
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::new(log_level))
        .with_target(false)
        .compact()
        .init();

    let bind = env::var("STRATUM_BIND").unwrap_or_else(|_| "0.0.0.0:3333".to_string());
    let default_diff = env::var("STRATUM_DEFAULT_DIFF")
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(16.0);
    let extranonce1_size = env::var("STRATUM_EXTRANONCE1_SIZE")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(4);
    let refresh_ms = env::var("TEMPLATE_REFRESH_MS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(1000);

    let metrics_bind = env::var("STRATUM_METRICS_BIND")
        .ok()
        .or_else(|| Some("127.0.0.1:3334".to_string()));

    let max_template_age_seconds = env::var("IRIUM_TEMPLATE_MAX_AGE_SECONDS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(60);

    let pow_limit = env::var("IRIUM_POW_LIMIT_HEX")
        .ok()
        .and_then(|v| pow::parse_pow_limit_hex(&v))
        .unwrap_or_else(pow::default_pow_limit);

    let hash_cmp_mode = HashCmpMode::from_env(env::var("IRIUM_HASH_CMP_MODE").ok());

    let soft_accept_invalid_shares = env::var("IRIUM_STRATUM_SOFT_ACCEPT_INVALID_SHARES")
        .ok()
        .map(|v| matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"))
        .unwrap_or(true);

    let miner_family_mode = MinerFamilyMode::from_env(env::var("IRIUM_STRATUM_MINER_FAMILY").ok());

    let sharecheck_samples = env::var("IRIUM_STRATUM_SHARECHECK_SAMPLES")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(3);

    let vardiff_enabled = env::var("IRIUM_STRATUM_VARDIFF_ENABLED")
        .ok()
        .map(|v| matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"))
        .unwrap_or(true);

    let vardiff_min_diff = env::var("IRIUM_STRATUM_VARDIFF_MIN_DIFF")
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(1.0);

    let vardiff_max_diff = env::var("IRIUM_STRATUM_VARDIFF_MAX_DIFF")
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(1024.0);

    let vardiff_target_share_secs = env::var("IRIUM_STRATUM_VARDIFF_TARGET_SHARE_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(15);

    let vardiff_retarget_secs = env::var("IRIUM_STRATUM_VARDIFF_RETARGET_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(30);

    let coinbase_bip34 = env::var("IRIUM_STRATUM_COINBASE_BIP34")
        .ok()
        .map(|v| matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"))
        .unwrap_or(true);

    let cfg = StratumConfig {
        bind,
        metrics_bind,
        default_diff,
        extranonce1_size,
        refresh_ms,
        rpc_base: env_required("IRIUM_RPC_BASE")?,
        rpc_token: env_required("IRIUM_RPC_TOKEN")?,
        pow_limit,
        hash_cmp_mode,
        soft_accept_invalid_shares,
        miner_family_mode,
        sharecheck_samples,
        vardiff_enabled,
        vardiff_min_diff,
        vardiff_max_diff,
        vardiff_target_share_secs,
        vardiff_retarget_secs,
        max_template_age_seconds,
        coinbase_bip34,
    };

    run(cfg).await
}
