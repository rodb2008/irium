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
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::RwLock;
use tokio::time::Instant;

#[derive(Clone)]
struct AppState {
    client: Client,
    node_base: String,
    node_bases: Arc<Vec<String>>,
    active_node: Arc<RwLock<usize>>,
    limiter: Arc<Mutex<RateLimiter>>,
    api_token: Option<String>,
    rpc_token: Option<String>,
    miners_cache: Arc<RwLock<MinersCache>>,
}

#[derive(Debug, Clone, Default)]
struct MinersCache {
    active_miners: Option<u64>,
    window_blocks: u64,
    as_of_height: u64,
    updated_at_unix: u64,
    last_error: Option<String>,
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0))
        .as_secs()
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

fn build_client(node_base: &str) -> Result<Client, String> {
    let mut builder = Client::builder().timeout(Duration::from_secs(10));
    if let Ok(path) = env::var("IRIUM_RPC_CA") {
        let pem = std::fs::read(&path).map_err(|e| format!("read CA {path}: {e}"))?;
        let cert =
            reqwest::Certificate::from_pem(&pem).map_err(|e| format!("invalid CA {path}: {e}"))?;
        builder = builder.add_root_certificate(cert);
    }
    let insecure = env::var("IRIUM_RPC_INSECURE")
        .ok()
        .map(|v| {
            let v = v.to_lowercase();
            v == "1" || v == "true" || v == "yes"
        })
        .unwrap_or(false);
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


fn parse_node_bases() -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(list) = env::var("IRIUM_NODE_RPCS") {
        for raw in list.split(|c: char| c == ',' || c == ';' || c.is_whitespace()) {
            let v = raw.trim();
            if !v.is_empty() {
                out.push(v.trim_end_matches('/').to_string());
            }
        }
    }
    if let Ok(primary) = env::var("IRIUM_NODE_RPC") {
        let p = primary.trim();
        if !p.is_empty() {
            out.push(p.trim_end_matches('/').to_string());
        }
    }
    if out.is_empty() {
        out.push("https://127.0.0.1:38300".to_string());
    }
    let mut dedup = Vec::new();
    let mut seen = HashSet::new();
    for base in out {
        if seen.insert(base.clone()) {
            dedup.push(base);
        }
    }
    dedup
}

fn best_height_from_status(v: &Value) -> u64 {
    v.get("best_header_tip")
        .and_then(|b| b.get("height"))
        .and_then(|h| h.as_u64())
        .or_else(|| v.get("height").and_then(|h| h.as_u64()))
        .unwrap_or(0)
}

fn ordered_node_indexes(total: usize, active: usize) -> Vec<usize> {
    if total == 0 {
        return Vec::new();
    }
    let mut idxs = Vec::with_capacity(total);
    let start = active.min(total.saturating_sub(1));
    idxs.push(start);
    for i in 0..total {
        if i != start {
            idxs.push(i);
        }
    }
    idxs
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

    Some(MinedBlockEntry {
        miner,
        time,
        hash,
    })
}

async fn proxy_json(state: &AppState, path: &str) -> Result<Json<Value>, StatusCode> {
    let active = *state.active_node.read().await;
    let order = ordered_node_indexes(state.node_bases.len(), active);
    let mut last_status = StatusCode::BAD_GATEWAY;

    for idx in order {
        let base = &state.node_bases[idx];
        let url = node_url(base, path);
        let mut req = state.client.get(url);
        if let Some(token) = &state.rpc_token {
            if !token.is_empty() {
                req = req.bearer_auth(token);
            }
        }
        let resp = match req.send().await {
            Ok(r) => r,
            Err(_) => continue,
        };
        if !resp.status().is_success() {
            last_status = map_status(resp.status());
            continue;
        }
        let payload = match resp.json::<Value>().await {
            Ok(v) => v,
            Err(_) => continue,
        };
        {
            let mut w = state.active_node.write().await;
            *w = idx;
        }
        return Ok(Json(payload));
    }

    Err(last_status)
}

async fn proxy_value(state: &AppState, path: &str) -> Result<Value, StatusCode> {
    let Json(payload) = proxy_json(state, path).await?;
    Ok(payload)
}

async fn proxy_text(state: &AppState, path: &str) -> Result<Response, StatusCode> {
    let active = *state.active_node.read().await;
    let order = ordered_node_indexes(state.node_bases.len(), active);
    let mut last_status = StatusCode::BAD_GATEWAY;

    for idx in order {
        let base = &state.node_bases[idx];
        let url = node_url(base, path);
        let mut req = state.client.get(url);
        if let Some(token) = &state.rpc_token {
            if !token.is_empty() {
                req = req.bearer_auth(token);
            }
        }
        let resp = match req.send().await {
            Ok(r) => r,
            Err(_) => continue,
        };
        if !resp.status().is_success() {
            last_status = map_status(resp.status());
            continue;
        }
        let body = match resp.text().await {
            Ok(t) => t,
            Err(_) => continue,
        };
        {
            let mut w = state.active_node.write().await;
            *w = idx;
        }
        return Ok(body.into_response());
    }

    Err(last_status)
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


async fn node_selector_task(state: AppState, interval: Duration) {
    loop {
        let mut best_idx: Option<usize> = None;
        let mut best_height = 0u64;
        let mut best_latency_ms = u128::MAX;

        for (idx, base) in state.node_bases.iter().enumerate() {
            let t0 = Instant::now();
            let url = node_url(base, "/status");
            let mut req = state.client.get(url);
            if let Some(token) = &state.rpc_token {
                if !token.is_empty() {
                    req = req.bearer_auth(token);
                }
            }
            let resp = match req.send().await {
                Ok(r) => r,
                Err(_) => continue,
            };
            if !resp.status().is_success() {
                continue;
            }
            let payload = match resp.json::<Value>().await {
                Ok(v) => v,
                Err(_) => continue,
            };
            let latency = t0.elapsed().as_millis();
            let h = best_height_from_status(&payload);

            if h > best_height || (h == best_height && latency < best_latency_ms) {
                best_height = h;
                best_latency_ms = latency;
                best_idx = Some(idx);
            }
        }

        if let Some(idx) = best_idx {
            let mut w = state.active_node.write().await;
            *w = idx;
        }

        tokio::time::sleep(interval).await;
    }
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

    let payload = json!({
        "backend_connected": true,
        "source": "explorer-chain-derived",
        "payout_model": "solo",
        "workers_online": miners.active_miners,
        "active_miners_window_blocks": miners.window_blocks,
        "active_miners_as_of_height": miners.as_of_height,
        "active_miners_updated_at": miners.updated_at_unix,
        "accepted_shares": Value::Null,
        "rejected_shares": Value::Null,
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
    let healthy = backend_connected && freshness_secs.map(|s| s < 1800).unwrap_or(false);

    Ok(Json(json!({
        "healthy": healthy,
        "backend_connected": backend_connected,
        "height": chain_height,
        "peers_connected": peers,
        "difficulty": mining.get("difficulty"),
        "network_hashrate_hs": mining.get("hashrate"),
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
    let node_bases = parse_node_bases();
    let node_base = node_bases
        .first()
        .cloned()
        .unwrap_or_else(|| "https://127.0.0.1:38300".to_string());
    let client = match build_client(&node_base) {
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
        .unwrap_or(60);

    let node_probe_secs = env::var("IRIUM_EXPLORER_NODE_PROBE_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(15)
        .max(5);

    let state = AppState {
        client,
        node_base: node_base.trim_end_matches('/').to_string(),
        node_bases: Arc::new(node_bases.clone()),
        active_node: Arc::new(RwLock::new(0)),
        limiter: Arc::new(Mutex::new(RateLimiter::new(rate))),
        api_token,
        rpc_token,
        miners_cache: Arc::new(RwLock::new(MinersCache {
            window_blocks: miners_window_blocks,
            ..Default::default()
        })),
    };

    // Background refresh for "active miners" estimate.
    tokio::spawn(miners_refresher_task(
        state.clone(),
        miners_window_blocks,
        Duration::from_secs(miners_refresh_secs.max(10)),
    ));

    // Background probe that auto-selects the healthiest/highest node.
    tokio::spawn(node_selector_task(
        state.clone(),
        Duration::from_secs(node_probe_secs),
    ));

    let app = Router::new()
        .route("/stats", get(stats))
        .route("/blocks", get(blocks))
        .route("/status", get(status))
        .route("/peers", get(peers))
        .route("/metrics", get(metrics))
        .route("/block/:height", get(block))
        .route("/blockhash/:hash", get(blockhash))
        .route("/tx/:txid", get(tx))
        .route("/address/:address", get(address))
        .route("/utxo", get(utxo))
        .route("/mining", get(mining))
        .route("/pool/stats", get(pool_stats))
        .route("/pool/payouts", get(pool_payouts))
        .route("/pool/workers", get(pool_workers))
        .route("/pool/health", get(pool_health))
        .route("/pool/account/:address", get(pool_account))
        .with_state(state.clone())
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
        "Irium explorer API listening on http://{}:{} (node rpc primary {}, pool size {})",
        host,
        port,
        node_base,
        state.node_bases.len()
    );

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("bind failed");
    axum::serve(listener, app).await.expect("server error");
}
