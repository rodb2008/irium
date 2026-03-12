use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use std::{collections::HashSet, sync::Arc};
use tokio::sync::RwLock;
use tower::ServiceExt;

use crate::{api, btc::BtcClient, irium::IriumClient, storage::Storage, AppConfig, AppCtx};

fn test_ctx() -> AppCtx {
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
        },
        btc: BtcClient::disabled(1),
        irium: IriumClient::disabled(),
        intake_paused: Arc::new(RwLock::new(false)),
    }
}

#[tokio::test]
async fn public_swap_requires_invite() {
    let app = api::router(test_ctx());
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
    let app = api::router(test_ctx());
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
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
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
    let app = api::router(test_ctx());
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
