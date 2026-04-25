use std::env;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::{
    extract::{ConnectInfo, Query, State},
    http::{
        header::{AUTHORIZATION, CONTENT_TYPE},
        HeaderMap, StatusCode,
    },
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use axum_server::tls_rustls::RustlsConfig;
use irium_node_rs::qr::{render_ascii, render_svg};
use irium_node_rs::rate_limiter::RateLimiter;
use irium_node_rs::settlement::{
    build_otc_agreement, settlement_proof_payload_bytes, AgreementParty, ProofSignatureEnvelope,
    SettlementProof, TypedProofPayload, AGREEMENT_SIGNATURE_TYPE_SECP256K1,
    SETTLEMENT_PROOF_SCHEMA_ID,
};
use k256::ecdsa::signature::hazmat::PrehashSigner;
use k256::ecdsa::{Signature, SigningKey};
use k256::SecretKey;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

#[derive(Clone)]
struct AppState {
    client: Client,
    node_base: String,
    limiter: Arc<Mutex<RateLimiter>>,
    api_token: Option<String>,
    rpc_token: Option<String>,
}

#[derive(Deserialize)]
struct BalanceQuery {
    address: String,
}

#[derive(Deserialize)]
struct QrQuery {
    address: String,
    format: Option<String>,
    scale: Option<u32>,
    margin: Option<u32>,
}

#[derive(Deserialize, Serialize)]
struct SubmitTxRequest {
    tx_hex: String,
}

#[derive(Deserialize)]
struct CreateOtcAgreementRequest {
    agreement_id: String,
    creation_time: Option<u64>,
    buyer_party_id: String,
    buyer_display_name: String,
    buyer_address: String,
    seller_party_id: String,
    seller_display_name: String,
    seller_address: String,
    total_amount: u64,
    asset_reference: String,
    payment_reference: String,
    refund_timeout_height: u64,
    secret_hash_hex: String,
    document_hash: String,
    metadata_hash: Option<String>,
    notes: Option<String>,
}

#[derive(Deserialize)]
struct ProofCreateRequest {
    agreement_hash: String,
    proof_type: String,
    attested_by: String,
    signing_key_hex: String,
    pubkey_hex: String,
    milestone_id: Option<String>,
    evidence_summary: Option<String>,
    evidence_hash: Option<String>,
    proof_id: Option<String>,
    timestamp: Option<u64>,
    expires_at_height: Option<u64>,
    proof_kind: Option<String>,
    reference_id: Option<String>,
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

fn json_err(status: StatusCode, code: &str, message: &str) -> (StatusCode, Json<Value>) {
    (status, Json(json!({ "error": message, "code": code })))
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

async fn post_json(
    state: &AppState,
    path: &str,
    body: &SubmitTxRequest,
) -> Result<Json<Value>, StatusCode> {
    let url = node_url(&state.node_base, path);
    let mut req = state.client.post(url).json(body);
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

async fn proxy_post_value(
    state: &AppState,
    path: &str,
    body: &Value,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let url = node_url(&state.node_base, path);
    let mut req = state.client.post(url).json(body);
    if let Some(token) = &state.rpc_token {
        if !token.is_empty() {
            req = req.bearer_auth(token);
        }
    }
    let resp = req.send().await.map_err(|_| {
        json_err(
            StatusCode::BAD_GATEWAY,
            "network_error",
            "upstream unreachable",
        )
    })?;
    let upstream_status = resp.status();
    let payload = resp.json::<Value>().await.map_err(|_| {
        json_err(
            StatusCode::BAD_GATEWAY,
            "parse_error",
            "upstream response not JSON",
        )
    })?;
    if !upstream_status.is_success() {
        let code = map_status(upstream_status);
        return Err((code, Json(payload)));
    }
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

async fn balance(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<BalanceQuery>,
) -> Result<Json<Value>, StatusCode> {
    check_rate(&state, &addr, &headers)?;
    proxy_json(&state, &format!("/rpc/balance?address={}", q.address)).await
}

async fn utxos(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<BalanceQuery>,
) -> Result<Json<Value>, StatusCode> {
    check_rate(&state, &addr, &headers)?;
    proxy_json(&state, &format!("/rpc/utxos?address={}", q.address)).await
}

async fn history(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<BalanceQuery>,
) -> Result<Json<Value>, StatusCode> {
    check_rate(&state, &addr, &headers)?;
    proxy_json(&state, &format!("/rpc/history?address={}", q.address)).await
}

async fn fee_estimate(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, StatusCode> {
    check_rate(&state, &addr, &headers)?;
    proxy_json(&state, "/rpc/fee_estimate").await
}

async fn qr(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<QrQuery>,
) -> Result<Response, StatusCode> {
    check_rate(&state, &addr, &headers)?;
    let address = q.address.trim();
    if address.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let format = q.format.as_deref().unwrap_or("svg").to_lowercase();
    match format.as_str() {
        "svg" => {
            let scale = q.scale.unwrap_or(8);
            let margin = q.margin.unwrap_or(2);
            let body = render_svg(address, scale, margin).map_err(|_| StatusCode::BAD_REQUEST)?;
            Ok(([(CONTENT_TYPE, "image/svg+xml; charset=utf-8")], body).into_response())
        }
        "ascii" => {
            let body = render_ascii(address).map_err(|_| StatusCode::BAD_REQUEST)?;
            Ok(([(CONTENT_TYPE, "text/plain; charset=utf-8")], body).into_response())
        }
        _ => Err(StatusCode::BAD_REQUEST),
    }
}

async fn submit_tx(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<SubmitTxRequest>,
) -> Result<Json<Value>, StatusCode> {
    if !api_authorized(&headers, &state.api_token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    check_rate(&state, &addr, &headers)?;
    post_json(&state, "/rpc/submit_tx", &body).await
}

async fn agreement_create(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    check_rate(&state, &addr, &headers)
        .map_err(|s| json_err(s, "rate_limit", "rate limit exceeded"))?;
    proxy_post_value(&state, "/rpc/createagreement", &body).await
}

async fn agreement_create_otc(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<CreateOtcAgreementRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    check_rate(&state, &addr, &headers)
        .map_err(|s| json_err(s, "rate_limit", "rate limit exceeded"))?;
    let creation_time = body.creation_time.unwrap_or_else(|| {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    });
    let buyer = AgreementParty {
        party_id: body.buyer_party_id,
        display_name: body.buyer_display_name,
        address: body.buyer_address,
        role: Some("buyer".to_string()),
    };
    let seller = AgreementParty {
        party_id: body.seller_party_id,
        display_name: body.seller_display_name,
        address: body.seller_address,
        role: Some("seller".to_string()),
    };
    let agreement = build_otc_agreement(
        body.agreement_id,
        creation_time,
        buyer,
        seller,
        body.total_amount,
        body.asset_reference,
        body.payment_reference,
        body.refund_timeout_height,
        body.secret_hash_hex,
        body.document_hash,
        body.metadata_hash,
        body.notes,
    )
    .map_err(|e| json_err(StatusCode::UNPROCESSABLE_ENTITY, "build_error", &e))?;
    let val = serde_json::to_value(&agreement).map_err(|e| {
        json_err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "serialize_error",
            &e.to_string(),
        )
    })?;
    Ok(Json(val))
}

async fn agreement_hash(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    check_rate(&state, &addr, &headers)
        .map_err(|s| json_err(s, "rate_limit", "rate limit exceeded"))?;
    proxy_post_value(&state, "/rpc/computeagreementhash", &body).await
}

async fn agreement_settle_status(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    check_rate(&state, &addr, &headers)
        .map_err(|s| json_err(s, "rate_limit", "rate limit exceeded"))?;
    proxy_post_value(&state, "/rpc/agreementstatus", &body).await
}

async fn policy_build_otc(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    check_rate(&state, &addr, &headers)
        .map_err(|s| json_err(s, "rate_limit", "rate limit exceeded"))?;
    proxy_post_value(&state, "/rpc/buildotctemplate", &body).await
}

async fn policy_set(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    check_rate(&state, &addr, &headers)
        .map_err(|s| json_err(s, "rate_limit", "rate limit exceeded"))?;
    proxy_post_value(&state, "/rpc/storepolicy", &body).await
}

async fn policy_get(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    check_rate(&state, &addr, &headers)
        .map_err(|s| json_err(s, "rate_limit", "rate limit exceeded"))?;
    proxy_post_value(&state, "/rpc/getpolicy", &body).await
}

async fn policy_evaluate(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    check_rate(&state, &addr, &headers)
        .map_err(|s| json_err(s, "rate_limit", "rate limit exceeded"))?;
    proxy_post_value(&state, "/rpc/evaluatepolicy", &body).await
}

async fn proof_create(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<ProofCreateRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    check_rate(&state, &addr, &headers)
        .map_err(|s| json_err(s, "rate_limit", "rate limit exceeded"))?;
    let attestation_time = body.timestamp.unwrap_or_else(|| {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    });
    let key_bytes = hex::decode(&body.signing_key_hex).map_err(|_| {
        json_err(
            StatusCode::BAD_REQUEST,
            "invalid_key",
            "signing_key_hex must be 32-byte hex",
        )
    })?;
    let secret = SecretKey::from_slice(&key_bytes).map_err(|e| {
        json_err(
            StatusCode::BAD_REQUEST,
            "invalid_key",
            &format!("invalid signing key: {e}"),
        )
    })?;
    let signing_key = SigningKey::from(secret);

    let proof_id = match &body.proof_id {
        Some(id) => id.clone(),
        None => {
            let mut seed = body.proof_type.clone();
            seed.push_str(&body.agreement_hash);
            seed.push_str(&attestation_time.to_string());
            let digest = Sha256::digest(seed.as_bytes());
            format!("prf-{}", hex::encode(&digest[..8]))
        }
    };

    let mut proof = SettlementProof {
        proof_id,
        schema_id: SETTLEMENT_PROOF_SCHEMA_ID.to_string(),
        proof_type: body.proof_type.clone(),
        agreement_hash: body.agreement_hash.clone(),
        milestone_id: body.milestone_id.clone(),
        attested_by: body.attested_by.clone(),
        attestation_time,
        evidence_hash: body.evidence_hash.clone(),
        evidence_summary: body.evidence_summary.clone(),
        signature: ProofSignatureEnvelope {
            signature_type: AGREEMENT_SIGNATURE_TYPE_SECP256K1.to_string(),
            pubkey_hex: body.pubkey_hex.clone(),
            signature_hex: String::new(),
            payload_hash: String::new(),
        },
        expires_at_height: body.expires_at_height,
        typed_payload: body.proof_kind.as_ref().map(|kind| TypedProofPayload {
            proof_kind: kind.clone(),
            content_hash: None,
            reference_id: body.reference_id.clone(),
            attributes: None,
        }),
    };

    let payload_bytes = settlement_proof_payload_bytes(&proof)
        .map_err(|e| json_err(StatusCode::INTERNAL_SERVER_ERROR, "payload_error", &e))?;
    let payload_digest = Sha256::digest(&payload_bytes);
    let payload_hash_hex = hex::encode(&payload_digest);
    let sig: Signature = signing_key.sign_prehash(&payload_digest).map_err(|e| {
        json_err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "sign_error",
            &format!("sign proof: {e}"),
        )
    })?;
    proof.signature.signature_hex = hex::encode(sig.to_bytes());
    proof.signature.payload_hash = payload_hash_hex;

    let val = serde_json::to_value(&proof).map_err(|e| {
        json_err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "serialize_error",
            &e.to_string(),
        )
    })?;
    Ok(Json(val))
}

async fn proof_submit(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    check_rate(&state, &addr, &headers)
        .map_err(|s| json_err(s, "rate_limit", "rate limit exceeded"))?;
    proxy_post_value(&state, "/rpc/submitproof", &body).await
}

async fn proof_list(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    check_rate(&state, &addr, &headers)
        .map_err(|s| json_err(s, "rate_limit", "rate limit exceeded"))?;
    proxy_post_value(&state, "/rpc/listproofs", &body).await
}

async fn proof_get(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    check_rate(&state, &addr, &headers)
        .map_err(|s| json_err(s, "rate_limit", "rate limit exceeded"))?;
    proxy_post_value(&state, "/rpc/getproof", &body).await
}

async fn settlement_build(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    check_rate(&state, &addr, &headers)
        .map_err(|s| json_err(s, "rate_limit", "rate limit exceeded"))?;
    proxy_post_value(&state, "/rpc/buildsettlementtx", &body).await
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
    let api_token = env::var("IRIUM_WALLET_API_TOKEN").ok();
    let rpc_token = env::var("IRIUM_RPC_TOKEN").ok();
    let rate = env::var("IRIUM_WALLET_API_RATE_LIMIT_PER_MIN")
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
        .route("/status", get(status))
        .route("/balance", get(balance))
        .route("/utxos", get(utxos))
        .route("/history", get(history))
        .route("/fee_estimate", get(fee_estimate))
        .route("/qr", get(qr))
        .route("/submit_tx", post(submit_tx))
        .route("/agreement/create", post(agreement_create))
        .route("/agreement/create/otc", post(agreement_create_otc))
        .route("/agreement/hash", post(agreement_hash))
        .route("/agreement/settle-status", post(agreement_settle_status))
        .route("/policy/build/otc", post(policy_build_otc))
        .route("/policy/set", post(policy_set))
        .route("/policy/get", post(policy_get))
        .route("/policy/evaluate", post(policy_evaluate))
        .route("/proof/create", post(proof_create))
        .route("/proof/submit", post(proof_submit))
        .route("/proof/list", post(proof_list))
        .route("/proof/get", post(proof_get))
        .route("/settlement/build", post(settlement_build))
        .with_state(state)
        .into_make_service_with_connect_info::<SocketAddr>();

    let host = env::var("IRIUM_WALLET_API_HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
    let port: u16 = env::var("IRIUM_WALLET_API_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(38320);
    let addr: SocketAddr = format!("{}:{}", host, port)
        .parse()
        .expect("valid bind address");

    let tls_cert = env::var("IRIUM_WALLET_API_TLS_CERT").ok();
    let tls_key = env::var("IRIUM_WALLET_API_TLS_KEY").ok();
    if let (Some(cert_path), Some(key_path)) = (tls_cert, tls_key) {
        let config = RustlsConfig::from_pem_file(cert_path, key_path)
            .await
            .expect("failed to load TLS cert/key");
        println!(
            "Irium wallet API listening on https://{}:{} (node rpc {})",
            host, port, node_base
        );
        axum_server::bind_rustls(addr, config)
            .serve(app)
            .await
            .expect("server error");
    } else {
        println!(
            "Irium wallet API listening on http://{}:{} (node rpc {})",
            host, port, node_base
        );
        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .expect("bind failed");
        axum::serve(listener, app).await.expect("server error");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::extract::ConnectInfo;
    use axum::http::{Method, Request};
    use std::net::SocketAddr;
    use tower::ServiceExt;

    fn test_state() -> AppState {
        AppState {
            client: Client::new(),
            node_base: "http://127.0.0.1:19999".to_string(),
            limiter: Arc::new(Mutex::new(RateLimiter::new(120))),
            api_token: None,
            rpc_token: None,
        }
    }

    fn test_router() -> Router {
        Router::new()
            .route("/agreement/create", post(agreement_create))
            .route("/agreement/create/otc", post(agreement_create_otc))
            .route("/agreement/hash", post(agreement_hash))
            .route("/agreement/settle-status", post(agreement_settle_status))
            .route("/policy/build/otc", post(policy_build_otc))
            .route("/policy/set", post(policy_set))
            .route("/policy/get", post(policy_get))
            .route("/policy/evaluate", post(policy_evaluate))
            .route("/proof/create", post(proof_create))
            .route("/proof/submit", post(proof_submit))
            .route("/proof/list", post(proof_list))
            .route("/proof/get", post(proof_get))
            .route("/settlement/build", post(settlement_build))
            .with_state(test_state())
    }

    fn fake_addr() -> SocketAddr {
        "127.0.0.1:12345".parse().unwrap()
    }

    #[tokio::test]
    async fn test_agreement_create_otc_valid() {
        let body = json!({
            "agreement_id": "agr-test-001",
            "buyer_party_id": "buyer-1",
            "buyer_display_name": "Alice",
            "buyer_address": "ir1qtest_buyer",
            "seller_party_id": "seller-1",
            "seller_display_name": "Bob",
            "seller_address": "ir1qtest_seller",
            "total_amount": 1000000,
            "asset_reference": "IRIUM",
            "payment_reference": "ref-001",
            "refund_timeout_height": 5000,
            "secret_hash_hex": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "document_hash": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
        });
        let req = Request::builder()
            .method(Method::POST)
            .uri("/agreement/create/otc")
            .header("content-type", "application/json")
            .extension(ConnectInfo(fake_addr()))
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        let resp = test_router().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_agreement_create_otc_missing_field() {
        let body = json!({ "agreement_id": "agr-test-002" });
        let req = Request::builder()
            .method(Method::POST)
            .uri("/agreement/create/otc")
            .header("content-type", "application/json")
            .extension(ConnectInfo(fake_addr()))
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        let resp = test_router().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn test_proof_create_valid() {
        let sk = SigningKey::random(&mut rand_core::OsRng);
        let pk_hex = hex::encode(sk.verifying_key().to_sec1_bytes());
        let sk_hex = hex::encode(sk.to_bytes());
        let body = json!({
            "agreement_hash": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "proof_type": "delivery_confirmation",
            "attested_by": "ir1qtest_attestor",
            "signing_key_hex": sk_hex,
            "pubkey_hex": pk_hex,
            "evidence_summary": "Goods delivered"
        });
        let req = Request::builder()
            .method(Method::POST)
            .uri("/proof/create")
            .header("content-type", "application/json")
            .extension(ConnectInfo(fake_addr()))
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        let resp = test_router().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20)
            .await
            .unwrap();
        let val: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(val["proof_type"], "delivery_confirmation");
        assert_eq!(val["schema_id"], SETTLEMENT_PROOF_SCHEMA_ID);
        assert!(!val["signature"]["signature_hex"]
            .as_str()
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn test_proof_create_bad_key() {
        let body = json!({
            "agreement_hash": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "proof_type": "delivery_confirmation",
            "attested_by": "ir1qtest_attestor",
            "signing_key_hex": "not_hex_at_all",
            "pubkey_hex": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        });
        let req = Request::builder()
            .method(Method::POST)
            .uri("/proof/create")
            .header("content-type", "application/json")
            .extension(ConnectInfo(fake_addr()))
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        let resp = test_router().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20)
            .await
            .unwrap();
        let val: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(val["code"], "invalid_key");
    }

    #[tokio::test]
    async fn test_proxy_endpoints_return_bad_gateway_when_node_down() {
        let endpoints = [
            "/agreement/create",
            "/agreement/hash",
            "/agreement/settle-status",
            "/policy/build/otc",
            "/policy/set",
            "/policy/get",
            "/policy/evaluate",
            "/proof/submit",
            "/proof/list",
            "/proof/get",
            "/settlement/build",
        ];
        for path in &endpoints {
            let req = Request::builder()
                .method(Method::POST)
                .uri(*path)
                .header("content-type", "application/json")
                .extension(ConnectInfo(fake_addr()))
                .body(Body::from(b"{}" as &[u8]))
                .unwrap();
            let resp = test_router().oneshot(req).await.unwrap();
            assert_eq!(
                resp.status(),
                StatusCode::BAD_GATEWAY,
                "expected BAD_GATEWAY for {path}"
            );
        }
    }
}
