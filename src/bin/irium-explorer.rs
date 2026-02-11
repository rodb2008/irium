use std::env;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::{
    extract::{ConnectInfo, Path, Query, State},
    http::{header::AUTHORIZATION, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use irium_node_rs::constants::block_reward;
use irium_node_rs::rate_limiter::RateLimiter;
use reqwest::Client;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Clone)]
struct AppState {
    client: Client,
    node_base: String,
    limiter: Arc<Mutex<RateLimiter>>,
    api_token: Option<String>,
    rpc_token: Option<String>,
}

#[derive(Deserialize)]
struct UtxoQuery {
    txid: String,
    index: u32,
}

#[derive(Deserialize)]
struct BlocksQuery {
    limit: Option<usize>,
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
    let mut total = 0u64;
    for h in 1..=height {
        total = total.saturating_add(block_reward(h));
    }
    let payload = json!({
        "height": height,
        "total_blocks": height,
        "total": height,
        "supply_irm": (total as f64) / 100_000_000.0,
        "genesis_hash": status.get("genesis_hash"),
        "peer_count": status.get("peer_count"),
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
    let mut blocks = Vec::new();
    let mut h = height as i64;
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

#[tokio::main]
async fn main() {
    let node_base =
        env::var("IRIUM_NODE_RPC").unwrap_or_else(|_| "https://127.0.0.1:38300".to_string());
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

    let state = AppState {
        client,
        node_base: node_base.trim_end_matches('/').to_string(),
        limiter: Arc::new(Mutex::new(RateLimiter::new(rate))),
        api_token,
        rpc_token,
    };

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
