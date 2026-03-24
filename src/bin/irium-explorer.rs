use std::collections::{HashMap, HashSet};
use std::env;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::{
    extract::{ConnectInfo, Path, Query, State},
    http::{header::AUTHORIZATION, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use irium_node_rs::constants::{block_reward, COINBASE_MATURITY};
use irium_node_rs::rate_limiter::RateLimiter;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::RwLock;
use tokio::time::Instant;

#[derive(Clone)]
struct AppState {
    client: Client,
    status_client: Client,
    node_base: String,
    limiter: Arc<Mutex<RateLimiter>>,
    api_token: Option<String>,
    rpc_token: Option<String>,
    miners_cache: Arc<RwLock<MinersCache>>,
    network_cache: Arc<RwLock<NetworkStatusCache>>,
    network_config: NetworkStatusConfig,
    stratum_metrics_url: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct MinersCache {
    active_miners: Option<u64>,
    window_blocks: u64,
    as_of_height: u64,
    updated_at_unix: u64,
    last_error: Option<String>,
}

#[derive(Debug, Clone)]
struct NetworkStatusSourceConfig {
    name: String,
    url: String,
}

#[derive(Debug, Clone)]
struct NetworkStatusConfig {
    sources: Vec<NetworkStatusSourceConfig>,
    poll_interval: Duration,
    stale_secs: u64,
    outlier_blocks: u64,
}

#[derive(Debug, Clone, Serialize, Default)]
struct NetworkSourceSnapshot {
    name: String,
    url: String,
    healthy: bool,
    stale: bool,
    health: String,
    local_height: Option<u64>,
    raw_height: Option<u64>,
    persisted_contiguous_height: Option<u64>,
    best_peer_height: Option<u64>,
    best_observed_height: Option<u64>,
    peer_count: Option<u64>,
    latency_ms: Option<u64>,
    updated_at_unix: Option<u64>,
    last_success_at_unix: Option<u64>,
    error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct NetworkStatusAggregate {
    network_height: Option<u64>,
    explorer_indexed_height: u64,
    best_observed_height: Option<u64>,
    confidence: String,
    updated_at_unix: u64,
    last_updated_secs_ago: u64,
    healthy_sources: usize,
    total_sources: usize,
    sources: Vec<NetworkSourceSnapshot>,
    notes: Vec<String>,
}

impl Default for NetworkStatusAggregate {
    fn default() -> Self {
        Self {
            network_height: None,
            explorer_indexed_height: 0,
            best_observed_height: None,
            confidence: "low".to_string(),
            updated_at_unix: 0,
            last_updated_secs_ago: 0,
            healthy_sources: 0,
            total_sources: 0,
            sources: Vec::new(),
            notes: vec!["network status warming up".to_string()],
        }
    }
}

#[derive(Debug, Clone, Default)]
struct NetworkStatusCache {
    aggregate: NetworkStatusAggregate,
    sources: HashMap<String, NetworkSourceSnapshot>,
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0))
        .as_secs()
}

fn env_flag(name: &str) -> bool {
    env::var(name)
        .ok()
        .map(|v| matches!(v.to_lowercase().as_str(), "1" | "true" | "yes"))
        .unwrap_or(false)
}

fn env_u64(name: &str, default: u64) -> u64 {
    env::var(name)
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(default)
}

fn value_u64(v: Option<&Value>) -> Option<u64> {
    match v {
        Some(Value::Number(n)) => n.as_u64(),
        Some(Value::String(s)) => s.parse::<u64>().ok(),
        _ => None,
    }
}

fn default_status_source_name(url: &str) -> String {
    reqwest::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(|host| host.to_string()))
        .filter(|host| !host.is_empty())
        .unwrap_or_else(|| url.to_string())
}

fn normalize_status_source_url(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if let Ok(mut url) = reqwest::Url::parse(trimmed) {
        if url.path().is_empty() || url.path() == "/" {
            url.set_path("/status");
        }
        return url.to_string();
    }
    trimmed.to_string()
}

fn parse_status_sources(default_base: &str) -> Vec<NetworkStatusSourceConfig> {
    let raw = env::var("IRIUM_STATUS_SOURCES").unwrap_or_default();
    let mut sources = Vec::new();

    for item in raw.split(',') {
        let item = item.trim();
        if item.is_empty() {
            continue;
        }
        let (name, url) = if let Some((name, url)) = item.split_once('=') {
            (name.trim().to_string(), normalize_status_source_url(url))
        } else {
            let url = normalize_status_source_url(item);
            (default_status_source_name(&url), url)
        };
        if !name.is_empty() && !url.is_empty() {
            sources.push(NetworkStatusSourceConfig { name, url });
        }
    }

    if sources.is_empty() {
        sources.push(NetworkStatusSourceConfig {
            name: "local".to_string(),
            url: node_url(default_base, "/status"),
        });
    }

    sources
}

fn persisted_height(status: &Value) -> Option<u64> {
    value_u64(status.get("persisted_contiguous_height"))
}

fn raw_chain_height(status: &Value) -> Option<u64> {
    value_u64(status.get("height"))
}

fn local_validated_height(status: &Value) -> Option<u64> {
    persisted_height(status).or_else(|| raw_chain_height(status))
}

fn best_peer_height(status: &Value) -> Option<u64> {
    value_u64(status.get("best_peer_height"))
        .or_else(|| value_u64(status.get("best_observed_height")))
        .or_else(|| value_u64(status.get("best_header_tip").and_then(|v| v.get("height"))))
        .or_else(|| value_u64(status.get("best_header").and_then(|v| v.get("height"))))
}

fn peer_count_from_status(status: &Value) -> Option<u64> {
    value_u64(status.get("peer_count")).or_else(|| value_u64(status.get("peers_connected")))
}

fn median_height(values: &[u64]) -> Option<u64> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    Some(sorted[sorted.len() / 2])
}

#[derive(Deserialize)]
struct UtxoQuery {
    txid: String,
    index: u32,
}

#[derive(Deserialize)]
struct BlocksQuery {
    limit: Option<usize>,
    start: Option<u64>,
}

#[derive(Deserialize)]
struct MiningQuery {
    window: Option<usize>,
    series: Option<usize>,
}

#[derive(Deserialize)]
struct PoolQuery {
    limit: Option<usize>,
    window: Option<u64>,
}

fn is_loopback_host(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "::1")
}

fn build_client(node_base: &str, timeout: Duration) -> Result<Client, String> {
    let mut builder = Client::builder().timeout(timeout);
    if let Ok(path) = env::var("IRIUM_RPC_CA") {
        let pem = std::fs::read(&path).map_err(|e| format!("read CA {path}: {e}"))?;
        let cert =
            reqwest::Certificate::from_pem(&pem).map_err(|e| format!("invalid CA {path}: {e}"))?;
        builder = builder.add_root_certificate(cert);
    }
    let insecure = env_flag("IRIUM_RPC_INSECURE");
    if insecure {
        let url = reqwest::Url::parse(node_base)
            .map_err(|e| format!("invalid RPC URL {node_base}: {e}"))?;
        if url.scheme() != "https" {
            eprintln!("[warn] IRIUM_RPC_INSECURE=1 has no effect on non-HTTPS RPC URL");
        } else {
            let host = url
                .host_str()
                .ok_or_else(|| "RPC URL missing host".to_string())?;
            if !is_loopback_host(host) {
                return Err(format!(
                    "Refusing to disable TLS verification for non-local RPC host {host}; set IRIUM_RPC_CA instead"
                ));
            }
            eprintln!("[warn] IRIUM_RPC_INSECURE=1: TLS verification disabled for https://{host}");
            builder = builder.danger_accept_invalid_certs(true);
        }
    }
    builder.build().map_err(|e| format!("build client: {e}"))
}

fn api_authorized(headers: &HeaderMap, token: &Option<String>) -> bool {
    let token = match token {
        Some(t) if !t.is_empty() => t,
        _ => return true,
    };
    let expected = format!("Bearer {}", token);
    let header = headers.get(AUTHORIZATION).and_then(|v| v.to_str().ok());
    header == Some(expected.as_str())
}

fn check_rate(state: &AppState, addr: &SocketAddr, headers: &HeaderMap) -> Result<(), StatusCode> {
    if api_authorized(headers, &state.api_token) {
        return Ok(());
    }
    let mut limiter = state.limiter.lock().unwrap_or_else(|e| e.into_inner());
    if limiter.is_allowed(&addr.ip().to_string()) {
        Ok(())
    } else {
        Err(StatusCode::TOO_MANY_REQUESTS)
    }
}

fn map_status(status: reqwest::StatusCode) -> StatusCode {
    StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY)
}

fn node_url(base: &str, path: &str) -> String {
    format!(
        "{}/{}",
        base.trim_end_matches('/'),
        path.trim_start_matches('/')
    )
}

fn value_f64(v: Option<&Value>) -> Option<f64> {
    match v {
        Some(Value::Number(n)) => n.as_f64(),
        Some(Value::String(s)) => s.parse::<f64>().ok(),
        _ => None,
    }
}

fn reward_irm_for_height(height: u64) -> f64 {
    (block_reward(height) as f64) / 100_000_000.0
}

#[derive(Debug, Clone)]
struct MinedBlockEntry {
    miner: String,
    time: u64,
    hash: String,
    source: Option<String>,
}

async fn load_block_entry(state: &AppState, height: u64) -> Option<MinedBlockEntry> {
    let path = format!("/rpc/block?height={}", height);
    let block = proxy_value(state, &path).await.ok()?;

    let miner = block
        .get("miner_address")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .unwrap_or("N/A")
        .to_string();
    let time = block
        .get("header")
        .and_then(|hh| hh.get("time"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let hash = block
        .get("header")
        .and_then(|hh| hh.get("hash"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let source = block
        .get("submit_source")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    Some(MinedBlockEntry {
        miner,
        time,
        hash,
        source,
    })
}

async fn proxy_json(state: &AppState, path: &str) -> Result<Json<Value>, StatusCode> {
    let url = node_url(&state.node_base, path);
    let mut req = state.client.get(url);
    if let Some(token) = &state.rpc_token {
        if !token.is_empty() {
            req = req.bearer_auth(token);
        }
    }
    let resp = req.send().await.map_err(|_| StatusCode::BAD_GATEWAY)?;
    if !resp.status().is_success() {
        return Err(map_status(resp.status()));
    }
    let payload = resp
        .json::<Value>()
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;
    Ok(Json(payload))
}

async fn proxy_value(state: &AppState, path: &str) -> Result<Value, StatusCode> {
    let Json(payload) = proxy_json(state, path).await?;
    Ok(payload)
}



async fn fetch_stratum_metrics(state: &AppState) -> Option<Value> {
    let base = state.stratum_metrics_url.as_ref()?;
    let url = node_url(base, "/metrics");
    let resp = state.client.get(url).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    resp.json::<Value>().await.ok()
}

async fn proxy_text(state: &AppState, path: &str) -> Result<Response, StatusCode> {
    let url = node_url(&state.node_base, path);
    let mut req = state.client.get(url);
    if let Some(token) = &state.rpc_token {
        if !token.is_empty() {
            req = req.bearer_auth(token);
        }
    }
    let resp = req.send().await.map_err(|_| StatusCode::BAD_GATEWAY)?;
    if !resp.status().is_success() {
        return Err(map_status(resp.status()));
    }
    let body = resp.text().await.map_err(|_| StatusCode::BAD_GATEWAY)?;
    Ok(body.into_response())
}

async fn refresh_active_miners_once(
    state: &AppState,
    window_blocks: u64,
) -> Result<MinersCache, String> {
    let status = proxy_value(state, "/status")
        .await
        .map_err(|e| format!("status: {e}"))?;
    let height = status.get("height").and_then(|v| v.as_u64()).unwrap_or(0);

    let window_blocks = window_blocks.max(1);
    let start = height.saturating_sub(window_blocks.saturating_sub(1));

    let mut miners = HashSet::new();
    let mut ok_blocks = 0u64;

    for h in start..=height {
        let path = format!("/rpc/block?height={}", h);
        match proxy_value(state, &path).await {
            Ok(block) => {
                ok_blocks += 1;
                let addr = block
                    .get("miner_address")
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .unwrap_or("");
                if !addr.is_empty() && addr != "N/A" {
                    miners.insert(addr.to_string());
                }
            }
            Err(_) => {
                // Keep going; this is best-effort.
                continue;
            }
        }
    }

    Ok(MinersCache {
        active_miners: Some(miners.len() as u64),
        window_blocks: ok_blocks.max(1),
        as_of_height: height,
        updated_at_unix: now_unix(),
        last_error: None,
    })
}

async fn miners_refresher_task(state: AppState, window_blocks: u64, interval: Duration) {
    loop {
        match refresh_active_miners_once(&state, window_blocks).await {
            Ok(cache) => {
                let mut w = state.miners_cache.write().await;
                *w = cache;
            }
            Err(e) => {
                let mut w = state.miners_cache.write().await;
                w.updated_at_unix = now_unix();
                w.last_error = Some(e);
            }
        }
        tokio::time::sleep(interval).await;
    }
}

async fn fetch_network_source_snapshot(
    client: &Client,
    source: &NetworkStatusSourceConfig,
    rpc_token: &Option<String>,
) -> Result<NetworkSourceSnapshot, String> {
    let started = Instant::now();
    let mut req = client.get(&source.url);
    if let Some(token) = rpc_token {
        if !token.is_empty() {
            req = req.bearer_auth(token);
        }
    }

    let resp = req
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?;
    let status_code = resp.status();
    if !status_code.is_success() {
        return Err(format!("HTTP {}", status_code));
    }
    let payload = resp
        .json::<Value>()
        .await
        .map_err(|e| format!("invalid JSON: {e}"))?;

    let fetched_at = now_unix();
    let local_height = local_validated_height(&payload)
        .ok_or_else(|| "missing local validated height".to_string())?;
    let raw_height = raw_chain_height(&payload);
    let persisted = persisted_height(&payload);
    let best_peer = best_peer_height(&payload);
    let best_observed = best_peer.map(|h| h.max(local_height)).or(Some(local_height));
    let peer_count = peer_count_from_status(&payload);
    let latency_ms = started.elapsed().as_millis() as u64;

    println!(
        "[network-status] fetched source={} local_height={} best_peer_height={:?} peers={:?} latency_ms={}",
        source.name, local_height, best_peer, peer_count, latency_ms
    );

    Ok(NetworkSourceSnapshot {
        name: source.name.clone(),
        url: source.url.clone(),
        healthy: true,
        stale: false,
        health: "healthy".to_string(),
        local_height: Some(local_height),
        raw_height,
        persisted_contiguous_height: persisted,
        best_peer_height: best_peer,
        best_observed_height: best_observed,
        peer_count,
        latency_ms: Some(latency_ms),
        updated_at_unix: Some(fetched_at),
        last_success_at_unix: Some(fetched_at),
        error: None,
    })
}

fn fallback_source_snapshot(
    source: &NetworkStatusSourceConfig,
    previous: Option<&NetworkSourceSnapshot>,
    error: String,
    stale_secs: u64,
) -> NetworkSourceSnapshot {
    let now = now_unix();
    if let Some(prev) = previous {
        let mut snapshot = prev.clone();
        let age = prev
            .last_success_at_unix
            .map(|ts| now.saturating_sub(ts))
            .unwrap_or(u64::MAX);
        let stale = age <= stale_secs;
        snapshot.name = source.name.clone();
        snapshot.url = source.url.clone();
        snapshot.healthy = false;
        snapshot.stale = stale;
        snapshot.health = if stale { "stale" } else { "failed" }.to_string();
        snapshot.updated_at_unix = Some(now);
        snapshot.error = Some(error);
        return snapshot;
    }

    NetworkSourceSnapshot {
        name: source.name.clone(),
        url: source.url.clone(),
        healthy: false,
        stale: false,
        health: "failed".to_string(),
        local_height: None,
        raw_height: None,
        persisted_contiguous_height: None,
        best_peer_height: None,
        best_observed_height: None,
        peer_count: None,
        latency_ms: None,
        updated_at_unix: Some(now),
        last_success_at_unix: None,
        error: Some(error),
    }
}

fn compute_network_status_aggregate(
    explorer_status: Option<&Value>,
    sources: &[NetworkSourceSnapshot],
    config: &NetworkStatusConfig,
) -> NetworkStatusAggregate {
    let now = now_unix();
    let explorer_indexed_height = explorer_status
        .and_then(local_validated_height)
        .unwrap_or(0);

    let mut notes = Vec::new();
    let healthy: Vec<&NetworkSourceSnapshot> = sources
        .iter()
        .filter(|s| s.healthy && s.local_height.is_some())
        .collect();
    let local_heights: Vec<u64> = healthy.iter().filter_map(|s| s.local_height).collect();

    let mut agreed_locals: Vec<u64> = Vec::new();
    let mut rejected_names: Vec<String> = Vec::new();
    let mut confidence = "low".to_string();
    let mut network_height = None;

    if local_heights.is_empty() {
        notes.push("no healthy trusted sources available".to_string());
    } else if local_heights.len() == 1 {
        network_height = local_heights.first().copied();
        agreed_locals = local_heights.clone();
        confidence = "medium".to_string();
        notes.push("using single healthy trusted source".to_string());
    } else if let Some(median) = median_height(&local_heights) {
        for source in &healthy {
            if let Some(height) = source.local_height {
                let delta = height.max(median) - height.min(median);
                if delta <= config.outlier_blocks {
                    agreed_locals.push(height);
                } else {
                    rejected_names.push(source.name.clone());
                }
            }
        }
        if !rejected_names.is_empty() {
            println!(
                "[network-status] rejected outlier sources around median {}: {}",
                median,
                rejected_names.join(", ")
            );
            notes.push(format!(
                "rejected outlier sources: {}",
                rejected_names.join(", ")
            ));
        }
        if !agreed_locals.is_empty() {
            network_height = agreed_locals.iter().copied().max();
            confidence = if agreed_locals.len() >= 2 {
                "high".to_string()
            } else {
                notes.push("sources disagree beyond threshold".to_string());
                "low".to_string()
            };
        }
    }

    let baseline = network_height.or_else(|| local_heights.iter().copied().max());
    let mut best_observed_candidates = agreed_locals.clone();
    if let Some(base) = baseline {
        let ceiling = base.saturating_add(config.outlier_blocks);
        for source in &healthy {
            if let Some(observed) = source.best_observed_height.or(source.best_peer_height) {
                if observed <= ceiling {
                    best_observed_candidates.push(observed.max(base));
                }
            }
        }
    }
    let best_observed_height = best_observed_candidates.iter().copied().max().or(baseline);

    let latest_update = sources
        .iter()
        .filter_map(|s| s.last_success_at_unix)
        .max()
        .unwrap_or(now);

    println!(
        "[network-status] aggregate network_height={:?} explorer_indexed_height={} best_observed_height={:?} confidence={} healthy_sources={}/{}",
        network_height,
        explorer_indexed_height,
        best_observed_height,
        confidence,
        healthy.len(),
        sources.len()
    );

    NetworkStatusAggregate {
        network_height,
        explorer_indexed_height,
        best_observed_height,
        confidence,
        updated_at_unix: latest_update,
        last_updated_secs_ago: now.saturating_sub(latest_update),
        healthy_sources: healthy.len(),
        total_sources: sources.len(),
        sources: sources.to_vec(),
        notes,
    }
}

async fn network_status_refresher_task(state: AppState) {
    loop {
        let previous_sources = {
            let cache = state.network_cache.read().await;
            cache.sources.clone()
        };

        let explorer_status = match proxy_value(&state, "/status").await {
            Ok(value) => Some(value),
            Err(err) => {
                println!("[network-status] local explorer status fetch failed: {err}");
                None
            }
        };

        let mut snapshots = Vec::new();
        let mut snapshot_map = HashMap::new();
        for source in &state.network_config.sources {
            let snapshot = match fetch_network_source_snapshot(
                &state.status_client,
                source,
                &state.rpc_token,
            )
            .await
            {
                Ok(snapshot) => snapshot,
                Err(err) => {
                    println!("[network-status] failed source={} error={}", source.name, err);
                    fallback_source_snapshot(
                        source,
                        previous_sources.get(&source.name),
                        err,
                        state.network_config.stale_secs,
                    )
                }
            };
            snapshot_map.insert(source.name.clone(), snapshot.clone());
            snapshots.push(snapshot);
        }

        let aggregate = compute_network_status_aggregate(
            explorer_status.as_ref(),
            &snapshots,
            &state.network_config,
        );

        let mut cache = state.network_cache.write().await;
        cache.aggregate = aggregate;
        cache.sources = snapshot_map;

        tokio::time::sleep(state.network_config.poll_interval).await;
    }
}

async fn network_status(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, StatusCode> {
    check_rate(&state, &addr, &headers)?;
    let aggregate = state.network_cache.read().await.aggregate.clone();
    let payload = serde_json::to_value(&aggregate).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(payload))
}


async fn status(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, StatusCode> {
    check_rate(&state, &addr, &headers)?;
    proxy_json(&state, "/status").await
}

async fn peers(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, StatusCode> {
    check_rate(&state, &addr, &headers)?;
    proxy_json(&state, "/peers").await
}

async fn metrics(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, StatusCode> {
    check_rate(&state, &addr, &headers)?;
    proxy_text(&state, "/metrics").await
}

async fn stats(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, StatusCode> {
    check_rate(&state, &addr, &headers)?;

    let status = proxy_value(&state, "/status").await?;
    let height = status.get("height").and_then(|v| v.as_u64()).unwrap_or(0);
    let peer_count = status.get("peer_count").and_then(|v| v.as_u64());

    // Supply calculation is deterministic; keep existing behavior.
    let mut total = 0u64;
    for h in 1..=height {
        total = total.saturating_add(block_reward(h));
    }

    let miners = state.miners_cache.read().await.clone();

    let payload = json!({
        "height": height,
        "total_blocks": height,
        "total": height,
        "supply_irm": (total as f64) / 100_000_000.0,
        "genesis_hash": status.get("genesis_hash"),

        // Live peers
        "peer_count": peer_count,
        "peers_connected": peer_count,

        // Approx miners: unique miner addresses observed in a rolling recent window.
        "active_miners": miners.active_miners,
        "active_miners_window_blocks": miners.window_blocks,
        "active_miners_as_of_height": miners.as_of_height,
        "active_miners_updated_at": miners.updated_at_unix,
        "active_miners_last_error": miners.last_error,
    });

    Ok(Json(payload))
}

async fn blocks(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<BlocksQuery>,
) -> Result<Json<Value>, StatusCode> {
    check_rate(&state, &addr, &headers)?;
    let status = proxy_value(&state, "/status").await?;
    let height = status.get("height").and_then(|v| v.as_u64()).unwrap_or(0);
    let limit = q.limit.unwrap_or(50).min(200);
    let start = q.start.unwrap_or(height).min(height);
    let mut blocks = Vec::new();
    let mut h = start as i64;
    while h >= 0 && blocks.len() < limit {
        let path = format!("/rpc/block?height={}", h);
        if let Ok(block) = proxy_value(&state, &path).await {
            blocks.push(block);
        }
        h -= 1;
    }
    let payload = json!({
        "height": height,
        "total_blocks": height,
        "total": height,
        "blocks": blocks,
    });
    Ok(Json(payload))
}

async fn block(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(height): Path<u64>,
) -> Result<Json<Value>, StatusCode> {
    check_rate(&state, &addr, &headers)?;
    proxy_json(&state, &format!("/rpc/block?height={}", height)).await
}

async fn blockhash(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(hash): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    check_rate(&state, &addr, &headers)?;
    proxy_json(&state, &format!("/rpc/block_by_hash?hash={}", hash)).await
}

async fn tx(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(txid): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    check_rate(&state, &addr, &headers)?;
    proxy_json(&state, &format!("/rpc/tx?txid={}", txid)).await
}

async fn address(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(address): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    check_rate(&state, &addr, &headers)?;
    let balance = proxy_value(&state, &format!("/rpc/balance?address={}", address)).await?;
    let utxos = proxy_value(&state, &format!("/rpc/utxos?address={}", address)).await?;
    let history = proxy_value(&state, &format!("/rpc/history?address={}", address)).await?;
    let payload = json!({
        "address": address,
        "balance": balance,
        "utxos": utxos,
        "history": history,
    });
    Ok(Json(payload))
}

async fn utxo(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<UtxoQuery>,
) -> Result<Json<Value>, StatusCode> {
    check_rate(&state, &addr, &headers)?;
    proxy_json(
        &state,
        &format!("/rpc/utxo?txid={}&index={}", q.txid, q.index),
    )
    .await
}

async fn mining(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<MiningQuery>,
) -> Result<Json<Value>, StatusCode> {
    check_rate(&state, &addr, &headers)?;
    let mut path = String::from("/rpc/mining_metrics");
    let mut first = true;
    if let Some(w) = q.window {
        path.push_str(if first { "?window=" } else { "&window=" });
        path.push_str(&w.to_string());
        first = false;
    }
    if let Some(n) = q.series {
        path.push_str(if first { "?series=" } else { "&series=" });
        path.push_str(&n.to_string());
    }
    proxy_json(&state, &path).await
}


async fn pool_stats(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<PoolQuery>,
) -> Result<Json<Value>, StatusCode> {
    check_rate(&state, &addr, &headers)?;

    let status = proxy_value(&state, "/status").await?;
    let mining = proxy_value(&state, "/rpc/mining_metrics").await?;
    let miners = state.miners_cache.read().await.clone();

    let sample_window = q.window.unwrap_or(miners.window_blocks.max(1));
    let stratum_metrics = fetch_stratum_metrics(&state).await;

    let active_tcp_sessions = stratum_metrics
        .as_ref()
        .and_then(|m| m.get("active_tcp_sessions"))
        .cloned()
        .unwrap_or(Value::Null);
    let accepted_shares = stratum_metrics
        .as_ref()
        .and_then(|m| m.get("accepted_shares"))
        .cloned()
        .unwrap_or(Value::Null);
    let rejected_shares = stratum_metrics
        .as_ref()
        .and_then(|m| m.get("rejected_shares"))
        .cloned()
        .unwrap_or(Value::Null);
    let mut chain_pool_blocks = 0u64;
    let mut h = status.get("height").and_then(|v| v.as_u64()).unwrap_or(0) as i64;
    let mut scanned = 0u64;
    while h >= 0 && scanned < sample_window {
        let height = h as u64;
        if let Some(entry) = load_block_entry(&state, height).await {
            scanned += 1;
            if entry.source.as_deref() == Some("pool_stratum") {
                chain_pool_blocks = chain_pool_blocks.saturating_add(1);
            }
        }
        h -= 1;
    }

    let blocks_accepted = stratum_metrics
        .as_ref()
        .and_then(|m| m.get("blocks_accepted"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let blocks_accepted = if blocks_accepted > 0 {
        blocks_accepted
    } else {
        chain_pool_blocks
    };
    let candidates_detected = stratum_metrics
        .as_ref()
        .and_then(|m| m.get("candidates_detected"))
        .cloned()
        .unwrap_or(Value::Null);
    let candidates_submitted = stratum_metrics
        .as_ref()
        .and_then(|m| m.get("candidates_submitted"))
        .cloned()
        .unwrap_or(Value::Null);
    let rejected_stale = stratum_metrics
        .as_ref()
        .and_then(|m| m.get("rejected_stale"))
        .cloned()
        .unwrap_or(Value::Null);
    let rejected_low_difficulty = stratum_metrics
        .as_ref()
        .and_then(|m| m.get("rejected_low_difficulty"))
        .cloned()
        .unwrap_or(Value::Null);
    let rejected_invalid = stratum_metrics
        .as_ref()
        .and_then(|m| m.get("rejected_invalid"))
        .cloned()
        .unwrap_or(Value::Null);
    let rejected_duplicate = stratum_metrics
        .as_ref()
        .and_then(|m| m.get("rejected_duplicate"))
        .cloned()
        .unwrap_or(Value::Null);
    let last_share_accepted_at = stratum_metrics
        .as_ref()
        .and_then(|m| m.get("last_share_accepted_at"))
        .cloned()
        .unwrap_or(Value::Null);
    let last_share_rejected_at = stratum_metrics
        .as_ref()
        .and_then(|m| m.get("last_share_rejected_at"))
        .cloned()
        .unwrap_or(Value::Null);

    let payload = json!({
        "backend_connected": true,
        "stratum_metrics_connected": stratum_metrics.is_some(),
        "source": "explorer-chain-derived+stratum",
        "payout_model": "solo",
        "workers_online": active_tcp_sessions,
        "active_tcp_sessions": active_tcp_sessions,
        "chain_active_miners_window": miners.active_miners,
        "chain_active_miners_window_blocks": miners.window_blocks,
        "chain_active_miners_as_of_height": miners.as_of_height,
        "chain_active_miners_updated_at": miners.updated_at_unix,
        "accepted_shares": accepted_shares,
        "rejected_shares": rejected_shares,
        "blocks_accepted": blocks_accepted,
        "pool_blocks_accepted_chain_attributed": chain_pool_blocks,
        "candidates_detected": candidates_detected,
        "candidates_submitted": candidates_submitted,
        "rejected_stale": rejected_stale,
        "rejected_low_difficulty": rejected_low_difficulty,
        "rejected_invalid": rejected_invalid,
        "rejected_duplicate": rejected_duplicate,
        "last_share_accepted_at": last_share_accepted_at,
        "last_share_rejected_at": last_share_rejected_at,
        "stale_shares": Value::Null,
        "round_luck": Value::Null,
        "round_effort": Value::Null,
        "pool_hashrate": mining.get("hashrate"),
        "difficulty": mining.get("difficulty"),
        "network_height": status.get("height"),
        "sample_window_blocks": sample_window,
    });

    Ok(Json(payload))
}

async fn pool_payouts(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<PoolQuery>,
) -> Result<Json<Value>, StatusCode> {
    check_rate(&state, &addr, &headers)?;

    let status = proxy_value(&state, "/status").await?;
    let chain_height = status.get("height").and_then(|v| v.as_u64()).unwrap_or(0);
    let limit = q.limit.unwrap_or(100).min(500);

    let mut payouts = Vec::new();
    let mut h = chain_height as i64;
    while h >= 0 && payouts.len() < limit {
        let height = h as u64;
        if let Some(entry) = load_block_entry(&state, height).await {
            let confirmations = chain_height.saturating_sub(height).saturating_add(1);
            let mature = confirmations >= COINBASE_MATURITY;
            let maturity_remaining = COINBASE_MATURITY.saturating_sub(confirmations);

            payouts.push(json!({
                "height": height,
                "address": entry.miner,
                "reward_irm": reward_irm_for_height(height),
                "time": entry.time,
                "hash": entry.hash,
                "status": "on_chain",
                "source": entry.source.clone().unwrap_or_else(|| "unknown".to_string()),
                "confirmations": confirmations,
                "coinbase_maturity": COINBASE_MATURITY,
                "mature": mature,
                "maturity_remaining": maturity_remaining
            }));
        }
        h -= 1;
    }

    Ok(Json(json!({
        "height": chain_height,
        "count": payouts.len(),
        "coinbase_maturity": COINBASE_MATURITY,
        "payouts": payouts
    })))
}

async fn pool_workers(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<PoolQuery>,
) -> Result<Json<Value>, StatusCode> {
    check_rate(&state, &addr, &headers)?;

    let status = proxy_value(&state, "/status").await?;
    let chain_height = status.get("height").and_then(|v| v.as_u64()).unwrap_or(0);

    let window = q.window.unwrap_or(288).clamp(32, 5000);
    let limit = q.limit.unwrap_or(50).min(500);
    let start = chain_height.saturating_sub(window.saturating_sub(1));

    let mining = proxy_value(&state, "/rpc/mining_metrics").await.ok();
    let network_hashrate = value_f64(mining.as_ref().and_then(|m| m.get("hashrate")));

    let mut by_addr: HashMap<String, u64> = HashMap::new();
    let mut scanned_blocks = 0u64;
    for h in (start..=chain_height).rev() {
        let Some(entry) = load_block_entry(&state, h).await else {
            continue;
        };
        scanned_blocks = scanned_blocks.saturating_add(1);
        if entry.miner.is_empty() || entry.miner == "N/A" {
            continue;
        }
        *by_addr.entry(entry.miner).or_insert(0) += 1;
    }

    let total_found: u64 = by_addr.values().copied().sum();
    let mut rows: Vec<(String, u64)> = by_addr.into_iter().collect();
    rows.sort_by(|a, b| b.1.cmp(&a.1));

    let workers: Vec<Value> = rows
        .into_iter()
        .take(limit)
        .map(|(address, blocks_found)| {
            let share = if total_found > 0 {
                (blocks_found as f64) / (total_found as f64)
            } else {
                0.0
            };
            let est_hashrate = network_hashrate.map(|h| h * share);
            json!({
                "address": address,
                "blocks_found": blocks_found,
                "share_pct": share * 100.0,
                "estimated_hashrate_hs": est_hashrate,
            })
        })
        .collect();

    Ok(Json(json!({
        "height": chain_height,
        "window_scanned": window,
        "scanned_blocks": scanned_blocks,
        "workers_online": workers.len(),
        "total_found_blocks": total_found,
        "network_hashrate_hs": network_hashrate,
        "workers": workers
    })))
}

async fn pool_distribution(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<PoolQuery>,
) -> Result<Json<Value>, StatusCode> {
    check_rate(&state, &addr, &headers)?;

    let status = proxy_value(&state, "/status").await?;
    let chain_height = status.get("height").and_then(|v| v.as_u64()).unwrap_or(0);

    let window = q.window.unwrap_or(4000).clamp(200, 20000);
    let limit = q.limit.unwrap_or(100).min(1000);
    let start = chain_height.saturating_sub(window.saturating_sub(1));

    let mining = proxy_value(&state, "/rpc/mining_metrics").await.ok();
    let network_hashrate = value_f64(mining.as_ref().and_then(|m| m.get("hashrate")));

    let mut by_addr: HashMap<String, u64> = HashMap::new();
    let mut scanned_blocks = 0u64;
    for h in (start..=chain_height).rev() {
        let Some(entry) = load_block_entry(&state, h).await else {
            continue;
        };
        scanned_blocks = scanned_blocks.saturating_add(1);
        if entry.miner.is_empty() || entry.miner == "N/A" {
            continue;
        }
        *by_addr.entry(entry.miner).or_insert(0) += 1;
    }

    let total_found: u64 = by_addr.values().copied().sum();
    let mut rows: Vec<(String, u64)> = by_addr.into_iter().collect();
    rows.sort_by(|a, b| b.1.cmp(&a.1));

    let unique_addresses = rows.len() as u64;

    let top1_share_pct = rows
        .first()
        .map(|(_, c)| if total_found > 0 { (*c as f64) * 100.0 / (total_found as f64) } else { 0.0 })
        .unwrap_or(0.0);
    let top5_total: u64 = rows.iter().take(5).map(|(_, c)| *c).sum();
    let top5_share_pct = if total_found > 0 {
        (top5_total as f64) * 100.0 / (total_found as f64)
    } else {
        0.0
    };

    let distribution: Vec<Value> = rows
        .into_iter()
        .take(limit)
        .enumerate()
        .map(|(i, (address, blocks_found))| {
            let share = if total_found > 0 {
                (blocks_found as f64) / (total_found as f64)
            } else {
                0.0
            };
            let est_hashrate = network_hashrate.map(|h| h * share);
            json!({
                "rank": i + 1,
                "address": address,
                "blocks_found": blocks_found,
                "share_pct": share * 100.0,
                "estimated_hashrate_hs": est_hashrate,
            })
        })
        .collect();

    Ok(Json(json!({
        "height": chain_height,
        "window_scanned": window,
        "scanned_blocks": scanned_blocks,
        "total_found_blocks": total_found,
        "unique_addresses": unique_addresses,
        "network_hashrate_hs": network_hashrate,
        "top1_share_pct": top1_share_pct,
        "top5_share_pct": top5_share_pct,
        "distribution": distribution,
    })))
}

async fn pool_health(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, StatusCode> {
    check_rate(&state, &addr, &headers)?;

    let mut issues: Vec<String> = Vec::new();
    let mut backend_connected = true;

    let t_status = Instant::now();
    let status = match proxy_value(&state, "/status").await {
        Ok(v) => v,
        Err(e) => {
            backend_connected = false;
            issues.push(format!("status fetch failed: {e}"));
            Value::Null
        }
    };
    let status_latency_ms = t_status.elapsed().as_millis() as u64;

    let t_mining = Instant::now();
    let mining = match proxy_value(&state, "/rpc/mining_metrics").await {
        Ok(v) => v,
        Err(e) => {
            backend_connected = false;
            issues.push(format!("mining_metrics fetch failed: {e}"));
            Value::Null
        }
    };
    let mining_latency_ms = t_mining.elapsed().as_millis() as u64;

    let chain_height = status.get("height").and_then(|v| v.as_u64()).unwrap_or(0);
    let peers = status
        .get("peer_count")
        .and_then(|v| v.as_u64())
        .or_else(|| status.get("peers_connected").and_then(|v| v.as_u64()));

    let tip_time = if chain_height > 0 {
        load_block_entry(&state, chain_height).await.map(|b| b.time)
    } else {
        None
    };
    let freshness_secs = tip_time.map(|t| now_unix().saturating_sub(t));

    // Dynamic freshness policy: based on observed block cadence, clamped to sane bounds.
    let avg_block_time = value_f64(mining.get("avg_block_time")).unwrap_or(600.0);
    let freshness_threshold_secs = ((avg_block_time * 6.0).round() as u64).clamp(1800, 21600);
    let chain_fresh = freshness_secs
        .map(|s| s <= freshness_threshold_secs)
        .unwrap_or(false);
    let freshness_state = if chain_fresh { "fresh" } else { "stale" };

    // `healthy` now reflects API/backend availability. Chain freshness is reported separately.
    let api_healthy = backend_connected;
    let healthy = api_healthy;

    Ok(Json(json!({
        "healthy": healthy,
        "api_healthy": api_healthy,
        "chain_fresh": chain_fresh,
        "freshness_state": freshness_state,
        "freshness_threshold_secs": freshness_threshold_secs,
        "backend_connected": backend_connected,
        "height": chain_height,
        "peers_connected": peers,
        "difficulty": mining.get("difficulty"),
        "network_hashrate_hs": mining.get("hashrate"),
        "avg_block_time": mining.get("avg_block_time"),
        "tip_time": tip_time,
        "freshness_secs": freshness_secs,
        "latency_ms": {
            "status": status_latency_ms,
            "mining": mining_latency_ms,
            "total": status_latency_ms.saturating_add(mining_latency_ms)
        },
        "issues": issues,
        "updated_at": now_unix()
    })))
}

async fn pool_account(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(address): Path<String>,
    Query(q): Query<PoolQuery>,
) -> Result<Json<Value>, StatusCode> {
    check_rate(&state, &addr, &headers)?;

    let status = proxy_value(&state, "/status").await?;
    let chain_height = status.get("height").and_then(|v| v.as_u64()).unwrap_or(0);

    let window = q.window.unwrap_or(4000).clamp(100, 20000);
    let limit = q.limit.unwrap_or(200).min(1000);

    let mining = proxy_value(&state, "/rpc/mining_metrics").await.ok();
    let network_hashrate_hs = value_f64(mining.as_ref().and_then(|m| m.get("hashrate")));

    let start = chain_height.saturating_sub(window.saturating_sub(1));
    let mut found = Vec::new();
    let mut total = 0.0f64;
    let mut mature_total = 0.0f64;

    for h in (start..=chain_height).rev() {
        if found.len() >= limit {
            break;
        }
        let Some(entry) = load_block_entry(&state, h).await else {
            continue;
        };
        if entry.miner != address {
            continue;
        }

        let reward = reward_irm_for_height(h);
        total += reward;

        let confirmations = chain_height.saturating_sub(h).saturating_add(1);
        let mature = confirmations >= COINBASE_MATURITY;
        if mature {
            mature_total += reward;
        }

        found.push(json!({
            "height": h,
            "time": entry.time,
            "hash": entry.hash,
            "reward_irm": reward,
            "status": "on_chain",
            "source": entry.source.clone().unwrap_or_else(|| "unknown".to_string()),
            "confirmations": confirmations,
            "coinbase_maturity": COINBASE_MATURITY,
            "mature": mature,
            "maturity_remaining": COINBASE_MATURITY.saturating_sub(confirmations)
        }));
    }

    let pending_total = (total - mature_total).max(0.0);
    let found_count = found.len() as u64;
    let share_window = if window > 0 {
        (found_count as f64) / (window as f64)
    } else {
        0.0
    };
    let estimated_hashrate_hs = network_hashrate_hs.map(|h| h * share_window);

    let last = found.first().cloned();

    Ok(Json(json!({
        "address": address,
        "window_scanned": window,
        "blocks_found": found_count,
        "total_rewards_irm": total,
        "pending_balance_irm": pending_total,
        "paid_total_irm": mature_total,
        "payout_model": "solo",
        "coinbase_maturity": COINBASE_MATURITY,
        "estimated_hashrate_hs": estimated_hashrate_hs,
        "network_hashrate_hs": network_hashrate_hs,
        "window_share_pct": share_window * 100.0,
        "last_found": last,
        "records": found
    })))
}


#[tokio::main]
async fn main() {
    let node_base =
        env::var("IRIUM_NODE_RPC").unwrap_or_else(|_| "https://127.0.0.1:38300".to_string());
    let client = match build_client(&node_base, Duration::from_secs(10)) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to init HTTP client: {e}");
            std::process::exit(1);
        }
    };
    let api_token = env::var("IRIUM_EXPLORER_TOKEN").ok();
    let rpc_token = env::var("IRIUM_RPC_TOKEN").ok();
    let rate = env::var("IRIUM_EXPLORER_RATE_LIMIT_PER_MIN")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(120);

    let miners_window_blocks = env::var("IRIUM_EXPLORER_MINERS_WINDOW_BLOCKS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(144);
    let miners_refresh_secs = env::var("IRIUM_EXPLORER_MINERS_REFRESH_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(15);

    let stratum_metrics_url = env::var("IRIUM_STRATUM_TELEMETRY_URL")
        .ok()
        .map(|v| v.trim_end_matches('/').to_string())
        .filter(|v| !v.is_empty());

    let status_sources = parse_status_sources(&node_base);
    let status_timeout_secs = env_u64("IRIUM_STATUS_TIMEOUT_SECS", 4).clamp(2, 15);
    let status_client = match build_client(
        &status_sources
            .first()
            .map(|s| s.url.as_str())
            .unwrap_or(node_base.as_str()),
        Duration::from_secs(status_timeout_secs),
    ) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to init network status client: {e}");
            std::process::exit(1);
        }
    };
    let network_config = NetworkStatusConfig {
        sources: status_sources,
        poll_interval: Duration::from_secs(env_u64("IRIUM_STATUS_POLL_SECS", 15).clamp(10, 30)),
        stale_secs: env_u64("IRIUM_STATUS_STALE_SECS", 60).max(15),
        outlier_blocks: env_u64("IRIUM_STATUS_OUTLIER_BLOCKS", 3).max(1),
    };

    let state = AppState {
        client,
        status_client,
        node_base: node_base.trim_end_matches('/').to_string(),
        limiter: Arc::new(Mutex::new(RateLimiter::new(rate))),
        api_token,
        rpc_token,
        miners_cache: Arc::new(RwLock::new(MinersCache {
            window_blocks: miners_window_blocks,
            ..Default::default()
        })),
        network_cache: Arc::new(RwLock::new(NetworkStatusCache::default())),
        network_config,
        stratum_metrics_url,
    };

    // Background refresh for "active miners" estimate.
    tokio::spawn(miners_refresher_task(
        state.clone(),
        miners_window_blocks,
        Duration::from_secs(miners_refresh_secs.max(3)),
    ));
    tokio::spawn(network_status_refresher_task(state.clone()));

    let app = Router::new()
        .route("/health", get(status))
        .route("/status", get(status))
        .route("/api/status", get(status))
        .route("/stats", get(stats))
        .route("/api/stats", get(stats))
        .route("/network/status", get(network_status))
        .route("/api/network/status", get(network_status))
        .route("/blocks", get(blocks))
        .route("/api/blocks", get(blocks))
        .route("/peers", get(peers))
        .route("/api/peers", get(peers))
        .route("/metrics", get(metrics))
        .route("/api/metrics", get(metrics))
        .route("/block/:height", get(block))
        .route("/api/block/:height", get(block))
        .route("/blockhash/:hash", get(blockhash))
        .route("/api/blockhash/:hash", get(blockhash))
        .route("/tx/:txid", get(tx))
        .route("/api/tx/:txid", get(tx))
        .route("/address/:address", get(address))
        .route("/api/address/:address", get(address))
        .route("/utxo", get(utxo))
        .route("/api/utxo", get(utxo))
        .route("/mining", get(mining))
        .route("/api/mining", get(mining))
        .route("/pool/stats", get(pool_stats))
        .route("/api/pool/stats", get(pool_stats))
        .route("/pool/payouts", get(pool_payouts))
        .route("/api/pool/payouts", get(pool_payouts))
        .route("/pool/workers", get(pool_workers))
        .route("/api/pool/workers", get(pool_workers))
        .route("/pool/distribution", get(pool_distribution))
        .route("/api/pool/distribution", get(pool_distribution))
        .route("/pool/health", get(pool_health))
        .route("/api/pool/health", get(pool_health))
        .route("/pool/account/:address", get(pool_account))
        .route("/api/pool/account/:address", get(pool_account))
        .with_state(state)
        .into_make_service_with_connect_info::<SocketAddr>();

    let host = env::var("IRIUM_EXPLORER_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let port: u16 = env::var("IRIUM_EXPLORER_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(38310);
    let addr: SocketAddr = format!("{}:{}", host, port)
        .parse()
        .expect("valid bind address");

    println!(
        "Irium explorer API listening on http://{}:{} (node rpc {})",
        host, port, node_base
    );

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("bind failed");
    axum::serve(listener, app).await.expect("server error");
}
