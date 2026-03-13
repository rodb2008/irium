use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use std::{collections::HashSet, sync::Arc};
use tokio::sync::RwLock;
use tower::ServiceExt;

use crate::{api, btc::BtcClient, irium::IriumClient, storage::Storage, AppConfig, AppCtx};

fn test_ctx(public_enabled: bool) -> AppCtx {
    let storage = Storage::open(":memory:").expect("sqlite");
    let mut invites = HashSet::new();
    invites.insert("invite-1".to_string());
    AppCtx {
        storage,
        cfg: AppConfig {
            operator_token: "op-token".to_string(),
            invite_codes: invites,
            expected_amount_sats: 1000,
            btc_min_confirmations: 1,
            auto_detect_btc: false,
            auto_create_irium_htlc: false,
            public_enabled,
        },
        btc: BtcClient::disabled(1),
        irium: IriumClient::disabled(),
        intake_paused: Arc::new(RwLock::new(false)),
    }
}

async fn create_swap(app: &axum::Router) -> serde_json::Value {
    let req = Request::builder()
        .method("POST")
        .uri("/v1/public-swaps")
        .header("content-type", "application/json")
        .body(Body::from(
            r#"{"tester_handle":"u","btc_testnet_receive_address":"tb1qabc","invite_code":"invite-1"}"#,
        ))
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = res.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&body).unwrap()
}

#[tokio::test]
async fn public_swap_requires_invite() {
    let app = api::router(test_ctx(true));
    let req = Request::builder()
        .method("POST")
        .uri("/v1/public-swaps")
        .header("content-type", "application/json")
        .body(Body::from(
            r#"{"tester_handle":"u","btc_testnet_receive_address":"tb1qabc"}"#,
        ))
        .unwrap();
    let res = app.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn public_swap_create_and_read() {
    let app = api::router(test_ctx(true));
    let v = create_swap(&app).await;
    let id = v["swap_id"].as_str().unwrap();
    let token = v["session_token"].as_str().unwrap();

    let req2 = Request::builder()
        .method("GET")
        .uri(format!("/v1/public-swaps/{id}?token={token}"))
        .body(Body::empty())
        .unwrap();
    let res2 = app.oneshot(req2).await.unwrap();
    assert_eq!(res2.status(), StatusCode::OK);
}

#[tokio::test]
async fn operator_pause_works() {
    let app = api::router(test_ctx(true));
    let req = Request::builder()
        .method("POST")
        .uri("/v1/admin/intake")
        .header("x-operator-token", "op-token")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"paused":true}"#))
        .unwrap();
    let res = app.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
}

#[tokio::test]
async fn public_flow_disabled_by_default() {
    let app = api::router(test_ctx(false));
    let req = Request::builder()
        .method("POST")
        .uri("/v1/public-swaps")
        .header("content-type", "application/json")
        .body(Body::from(
            r#"{"tester_handle":"u","btc_testnet_receive_address":"tb1qabc","invite_code":"invite-1"}"#,
        ))
        .unwrap();
    let res = app.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn claimed_requires_real_proof_and_rejects_fake_txid() {
    let app = api::router(test_ctx(true));
    let v = create_swap(&app).await;
    let id = v["swap_id"].as_str().unwrap();

    let req = Request::builder()
        .method("POST")
        .uri(format!("/v1/admin/swaps/{id}/submit-terminal-proof"))
        .header("x-operator-token", "op-token")
        .header("content-type", "application/json")
        .body(Body::from(
            r#"{"side":"btc","outcome":"claim","txid":"auto-claim:123"}"#,
        ))
        .unwrap();
    let res = app.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    let body = res.into_body().collect().await.unwrap().to_bytes();
    let msg = String::from_utf8_lossy(&body);
    assert!(msg.contains("bad_txid_format"));
}

#[tokio::test]
async fn refund_requires_proof_and_does_not_terminalize_without_it() {
    let app = api::router(test_ctx(true));
    let v = create_swap(&app).await;
    let id = v["swap_id"].as_str().unwrap();
    let token = v["session_token"].as_str().unwrap();

    let req = Request::builder()
        .method("POST")
        .uri(format!("/v1/admin/swaps/{id}/submit-terminal-proof"))
        .header("x-operator-token", "op-token")
        .header("content-type", "application/json")
        .body(Body::from(
            r#"{"side":"btc","outcome":"refund","txid":"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"}"#,
        ))
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);

    let req2 = Request::builder()
        .method("GET")
        .uri(format!("/v1/public-swaps/{id}/status?token={token}"))
        .body(Body::empty())
        .unwrap();
    let res2 = app.oneshot(req2).await.unwrap();
    assert_eq!(res2.status(), StatusCode::OK);
    let body = res2.into_body().collect().await.unwrap().to_bytes();
    let v2: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v2["terminal"], false);
}

#[tokio::test]
async fn missing_operator_token_blocks_terminal_proof() {
    let app = api::router(test_ctx(true));
    let v = create_swap(&app).await;
    let id = v["swap_id"].as_str().unwrap();

    let req = Request::builder()
        .method("POST")
        .uri(format!("/v1/admin/swaps/{id}/submit-terminal-proof"))
        .header("content-type", "application/json")
        .body(Body::from(
            r#"{"side":"btc","outcome":"claim","txid":"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"}"#,
        ))
        .unwrap();
    let res = app.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}
