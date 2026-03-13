use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse},
    routing::{get, post},
    Json, Router,
};
use chrono::Utc;
use rand::RngCore;
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use tracing::info;
use uuid::Uuid;

use crate::{
    model::{
        CreatePublicSwapRequest, CreatePublicSwapResponse, MarkReviewRequest, PauseIntakeRequest,
        PublicSwapView, StatusResponse, SubmitBtcTxidRequest, Swap, SwapState,
    },
    state_machine::{can_transition, default_next_action},
    AppCtx,
};

pub fn router(ctx: AppCtx) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/", get(index_html))
        .route("/v1/public-swaps", post(create_public_swap))
        .route("/v1/public-swaps/:id", get(get_public_swap))
        .route("/v1/public-swaps/:id/status", get(get_status))
        .route("/v1/public-swaps/:id/events", get(get_public_events))
        .route("/v1/public-swaps/:id/submit-btc-txid", post(submit_btc_txid))
        .route("/v1/public-swaps/:id/request-refund", post(request_refund))
        .route("/v1/admin/intake", post(set_intake))
        .route("/v1/admin/swaps", get(list_live_swaps))
        .route("/v1/admin/swaps/:id/manual-review", post(mark_manual_review))
        .with_state(ctx)
}

pub async fn poll_progression(ctx: AppCtx) -> anyhow::Result<()> {
    let swaps = ctx.storage.list_live_swaps()?;
    for mut s in swaps {
        if s.manual_review {
            continue;
        }

        if s.state == SwapState::SecretCommitted
            && s.btc_funding_txid.is_none()
            && ctx.cfg.auto_detect_btc
        {
            if let Some(addr) = s.btc_htlc_address.clone() {
                if let Some(txid) = ctx
                    .btc
                    .autodetect_funding_txid(&addr, s.expected_amount_sats)
                    .await?
                {
                    s.btc_funding_txid = Some(txid.clone());
                    transition(
                        &ctx,
                        &mut s,
                        SwapState::BtcHtlcCreated,
                        "btc_funding_detected",
                        json!({"btc_txid": txid}),
                    )?;
                    ctx.storage.update_swap(&s)?;
                }
            }
        }

        if s.state == SwapState::BtcHtlcCreated {
            if let Some(txid) = s.btc_funding_txid.clone() {
                let conf = ctx.btc.tx_confirmations(&txid).await.unwrap_or(0);
                s.btc_confirmations = conf;
                if conf >= ctx.cfg.btc_min_confirmations {
                    transition(
                        &ctx,
                        &mut s,
                        SwapState::BtcHtlcConfirmed,
                        "btc_confirmed",
                        json!({"btc_txid": txid, "confirmations": conf}),
                    )?;
                    if ctx.cfg.auto_create_irium_htlc {
                        if let Ok(Some(irium_txid)) = ctx.irium.create_htlc(&s.secret_hash_hex).await {
                            s.irium_htlc_txid = Some(irium_txid.clone());
                            transition(
                                &ctx,
                                &mut s,
                                SwapState::IriumHtlcCreated,
                                "irium_htlc_created",
                                json!({"irium_txid": irium_txid}),
                            )?;
                            transition(
                                &ctx,
                                &mut s,
                                SwapState::IriumHtlcConfirmed,
                                "irium_htlc_confirmed",
                                json!({"irium_txid": irium_txid}),
                            )?;
                            let spend_txid = format!("auto-claim:{}", s.irium_htlc_txid.clone().unwrap_or_default());
                            s.irium_spend_txid = Some(spend_txid.clone());
                            transition(
                                &ctx,
                                &mut s,
                                SwapState::ClaimInitiated,
                                "claim_initiated",
                                json!({"irium_spend_txid": spend_txid}),
                            )?;
                            transition(
                                &ctx,
                                &mut s,
                                SwapState::Claimed,
                                "claimed",
                                json!({"auto": true}),
                            )?;
                        }
                    }
                }
                ctx.storage.update_swap(&s)?;
            }
        }
    }
    Ok(())
}

async fn healthz(State(ctx): State<AppCtx>) -> impl IntoResponse {
    let paused = *ctx.intake_paused.read().await;
    Json(json!({"ok": true, "intake_paused": paused}))
}

async fn index_html() -> Html<&'static str> {
    Html(include_str!("../static/index.html"))
}

#[derive(Deserialize)]
struct TokenQuery {
    token: Option<String>,
}

fn gen_secret_hash() -> (String, String) {
    let mut secret = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut secret);
    let mut hasher = Sha256::new();
    hasher.update(secret);
    let digest = hasher.finalize();
    (hex::encode(secret), hex::encode(digest))
}

fn build_public_view(s: &Swap) -> PublicSwapView {
    PublicSwapView {
        swap_id: s.id.clone(),
        state: s.state,
        next_action: s.next_action.clone(),
        btc_htlc_address: s.btc_htlc_address.clone(),
        expected_amount_sats: s.expected_amount_sats,
        btc_confirmations: s.btc_confirmations,
        btc_funding_txid: s.btc_funding_txid.clone(),
        btc_spent_txid: s.btc_spent_txid.clone(),
        irium_htlc_txid: s.irium_htlc_txid.clone(),
        irium_spend_txid: s.irium_spend_txid.clone(),
        timeout_height_hint: s.timeout_height_hint,
        success: s.state == SwapState::Claimed,
        refunded: s.state == SwapState::Refunded,
        failed: matches!(s.state, SwapState::Failed | SwapState::Expired),
    }
}

fn token_ok(headers: &HeaderMap, q: &TokenQuery, s: &Swap) -> bool {
    let header = headers.get("x-session-token").and_then(|v| v.to_str().ok());
    if let Some(h) = header {
        return h == s.session_token;
    }
    q.token.as_deref() == Some(s.session_token.as_str())
}

fn operator_ok(ctx: &AppCtx, headers: &HeaderMap) -> bool {
    headers
        .get("x-operator-token")
        .and_then(|v| v.to_str().ok())
        .map(|v| v == ctx.cfg.operator_token)
        .unwrap_or(false)
}

async fn create_public_swap(
    State(ctx): State<AppCtx>,
    Json(req): Json<CreatePublicSwapRequest>,
) -> Result<Json<CreatePublicSwapResponse>, (StatusCode, String)> {
    if *ctx.intake_paused.read().await {
        return Err((StatusCode::SERVICE_UNAVAILABLE, "intake_paused".to_string()));
    }
    if req.tester_handle.trim().is_empty() || req.btc_testnet_receive_address.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, "missing_required_fields".to_string()));
    }
    if !ctx.cfg.invite_codes.is_empty() {
        let code = req.invite_code.clone().unwrap_or_default();
        if !ctx.cfg.invite_codes.contains(code.trim()) {
            return Err((StatusCode::FORBIDDEN, "invalid_invite_code".to_string()));
        }
    }

    let swap_id = Uuid::new_v4().to_string();
    let session_token = Uuid::new_v4().to_string();
    let (_, secret_hash_hex) = gen_secret_hash();
    let btc_htlc_address = ctx
        .btc
        .get_new_address(&format!("swap-{}", &swap_id[..8]))
        .await
        .map_err(|e| {
            (
                StatusCode::BAD_GATEWAY,
                format!("btc_address_generation_failed:{e}"),
            )
        })?;

    let now = Utc::now();
    let mut swap = Swap {
        id: swap_id.clone(),
        tester_handle: req.tester_handle,
        session_token: session_token.clone(),
        btc_receive_address: req.btc_testnet_receive_address,
        btc_htlc_address: Some(btc_htlc_address.clone()),
        btc_funding_txid: None,
        btc_spent_txid: None,
        irium_htlc_txid: None,
        irium_spend_txid: None,
        secret_hash_hex,
        state: SwapState::Created,
        next_action: default_next_action(SwapState::Created).to_string(),
        expected_amount_sats: ctx.cfg.expected_amount_sats,
        btc_confirmations: 0,
        timeout_height_hint: None,
        manual_review: false,
        created_at: now,
        updated_at: now,
    };

    transition(
        &ctx,
        &mut swap,
        SwapState::Quoted,
        "swap_quoted",
        json!({"btc_htlc_address": btc_htlc_address}),
    )
    .map_err(internal_err)?;
    transition(
        &ctx,
        &mut swap,
        SwapState::Accepted,
        "swap_accepted",
        json!({}),
    )
    .map_err(internal_err)?;
    let secret_hash_for_event = swap.secret_hash_hex.clone();
    transition(
        &ctx,
        &mut swap,
        SwapState::SecretCommitted,
        "secret_committed",
        json!({"secret_hash": secret_hash_for_event}),
    )
    .map_err(internal_err)?;

    ctx.storage.insert_swap(&swap).map_err(internal_err)?;
    ctx.storage
        .append_event(
            &swap.id,
            "public_swap_created",
            json!({"tester": swap.tester_handle}),
        )
        .map_err(internal_err)?;

    Ok(Json(CreatePublicSwapResponse {
        swap_id,
        session_token,
        state: swap.state,
        next_action: swap.next_action,
    }))
}

async fn get_public_swap(
    Path(id): Path<String>,
    Query(q): Query<TokenQuery>,
    headers: HeaderMap,
    State(ctx): State<AppCtx>,
) -> Result<Json<PublicSwapView>, StatusCode> {
    let swap = ctx
        .storage
        .get_swap_public(&id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    if !token_ok(&headers, &q, &swap) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(Json(build_public_view(&swap)))
}

async fn get_status(
    Path(id): Path<String>,
    Query(q): Query<TokenQuery>,
    headers: HeaderMap,
    State(ctx): State<AppCtx>,
) -> Result<Json<StatusResponse>, StatusCode> {
    let swap = ctx
        .storage
        .get_swap_public(&id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    if !token_ok(&headers, &q, &swap) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(Json(StatusResponse {
        state: swap.state,
        next_action: swap.next_action,
        terminal: swap.state.is_terminal(),
    }))
}

async fn get_public_events(
    Path(id): Path<String>,
    Query(q): Query<TokenQuery>,
    headers: HeaderMap,
    State(ctx): State<AppCtx>,
) -> Result<Json<Vec<serde_json::Value>>, StatusCode> {
    let swap = ctx
        .storage
        .get_swap_public(&id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    if !token_ok(&headers, &q, &swap) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let events = ctx
        .storage
        .list_events(&id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(
        events
            .into_iter()
            .map(|e| {
                json!({
                    "id": e.id,
                    "event_type": e.event_type,
                    "payload": e.payload,
                    "created_at": e.created_at,
                })
            })
            .collect(),
    ))
}

async fn submit_btc_txid(
    Path(id): Path<String>,
    Query(q): Query<TokenQuery>,
    headers: HeaderMap,
    State(ctx): State<AppCtx>,
    Json(req): Json<SubmitBtcTxidRequest>,
) -> Result<Json<StatusResponse>, (StatusCode, String)> {
    let mut swap = ctx
        .storage
        .get_swap_public(&id)
        .map_err(internal_err)?
        .ok_or((StatusCode::NOT_FOUND, "swap_not_found".to_string()))?;

    if !token_ok(&headers, &q, &swap) {
        return Err((StatusCode::UNAUTHORIZED, "invalid_session_token".to_string()));
    }
    if swap.manual_review {
        return Err((StatusCode::LOCKED, "swap_in_manual_review".to_string()));
    }
    let expected_addr = swap
        .btc_htlc_address
        .clone()
        .ok_or((StatusCode::BAD_REQUEST, "btc_htlc_address_missing".to_string()))?;
    let ok = ctx
        .btc
        .validate_funding_tx(&req.btc_txid, &expected_addr, swap.expected_amount_sats)
        .await
        .map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                format!("btc_txid_validation_failed:{e}"),
            )
        })?;
    if !ok {
        return Err((
            StatusCode::BAD_REQUEST,
            "btc_txid_not_matching_expected_htlc_output".to_string(),
        ));
    }

    swap.btc_funding_txid = Some(req.btc_txid.clone());
    transition(
        &ctx,
        &mut swap,
        SwapState::BtcHtlcCreated,
        "btc_txid_submitted",
        json!({"btc_txid": req.btc_txid}),
    )
    .map_err(internal_err)?;

    let conf = ctx
        .btc
        .tx_confirmations(swap.btc_funding_txid.as_deref().unwrap())
        .await
        .unwrap_or(0);
    swap.btc_confirmations = conf;
    if conf >= ctx.cfg.btc_min_confirmations {
        transition(
            &ctx,
            &mut swap,
            SwapState::BtcHtlcConfirmed,
            "btc_confirmed",
            json!({"confirmations": conf}),
        )
        .map_err(internal_err)?;
    }

    ctx.storage.update_swap(&swap).map_err(internal_err)?;
    Ok(Json(StatusResponse {
        state: swap.state,
        next_action: swap.next_action,
        terminal: swap.state.is_terminal(),
    }))
}

async fn request_refund(
    Path(id): Path<String>,
    Query(q): Query<TokenQuery>,
    headers: HeaderMap,
    State(ctx): State<AppCtx>,
) -> Result<Json<StatusResponse>, (StatusCode, String)> {
    let mut swap = ctx
        .storage
        .get_swap_public(&id)
        .map_err(internal_err)?
        .ok_or((StatusCode::NOT_FOUND, "swap_not_found".to_string()))?;

    if !token_ok(&headers, &q, &swap) {
        return Err((StatusCode::UNAUTHORIZED, "invalid_session_token".to_string()));
    }
    if swap.manual_review {
        return Err((StatusCode::LOCKED, "swap_in_manual_review".to_string()));
    }
    if swap.state.is_terminal() {
        return Ok(Json(StatusResponse {
            state: swap.state,
            next_action: swap.next_action,
            terminal: true,
        }));
    }

    transition(
        &ctx,
        &mut swap,
        SwapState::RefundPending,
        "refund_requested",
        json!({"by": "tester"}),
    )
    .map_err(internal_err)?;

    let refund_txid = format!("auto-refund:{}", swap.btc_funding_txid.clone().unwrap_or_else(|| swap.id.clone()));
    swap.btc_spent_txid = Some(refund_txid.clone());
    swap.irium_spend_txid = Some(refund_txid);
    transition(
        &ctx,
        &mut swap,
        SwapState::Refunded,
        "refunded",
        json!({"auto": true}),
    )
    .map_err(internal_err)?;

    ctx.storage.update_swap(&swap).map_err(internal_err)?;
    Ok(Json(StatusResponse {
        state: swap.state,
        next_action: swap.next_action,
        terminal: swap.state.is_terminal(),
    }))
}

async fn set_intake(
    State(ctx): State<AppCtx>,
    headers: HeaderMap,
    Json(req): Json<PauseIntakeRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    if !operator_ok(&ctx, &headers) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    *ctx.intake_paused.write().await = req.paused;
    ctx.storage
        .set_intake_paused(req.paused)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json!({"paused": req.paused})))
}

async fn list_live_swaps(
    State(ctx): State<AppCtx>,
    headers: HeaderMap,
) -> Result<Json<Vec<PublicSwapView>>, StatusCode> {
    if !operator_ok(&ctx, &headers) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let swaps = ctx
        .storage
        .list_live_swaps()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(swaps.iter().map(build_public_view).collect()))
}

async fn mark_manual_review(
    Path(id): Path<String>,
    State(ctx): State<AppCtx>,
    headers: HeaderMap,
    Json(req): Json<MarkReviewRequest>,
) -> Result<Json<StatusResponse>, (StatusCode, String)> {
    if !operator_ok(&ctx, &headers) {
        return Err((
            StatusCode::UNAUTHORIZED,
            "operator_token_invalid".to_string(),
        ));
    }
    let mut swap = ctx
        .storage
        .get_swap_public(&id)
        .map_err(internal_err)?
        .ok_or((StatusCode::NOT_FOUND, "swap_not_found".to_string()))?;

    swap.manual_review = req.manual_review;
    if req.manual_review {
        swap.state = SwapState::ManualReview;
        swap.next_action = default_next_action(SwapState::ManualReview).to_string();
        ctx.storage
            .append_event(&swap.id, "manual_review_enabled", json!({}))
            .map_err(internal_err)?;
    } else {
        swap.state = SwapState::Accepted;
        swap.next_action = default_next_action(SwapState::Accepted).to_string();
        ctx.storage
            .append_event(&swap.id, "manual_review_cleared", json!({}))
            .map_err(internal_err)?;
    }
    ctx.storage.update_swap(&swap).map_err(internal_err)?;
    Ok(Json(StatusResponse {
        state: swap.state,
        next_action: swap.next_action,
        terminal: swap.state.is_terminal(),
    }))
}

fn transition(
    ctx: &AppCtx,
    swap: &mut Swap,
    next: SwapState,
    event: &str,
    payload: serde_json::Value,
) -> anyhow::Result<()> {
    if !can_transition(swap.state, next) {
        return Err(anyhow::anyhow!("invalid_transition"));
    }
    swap.state = next;
    swap.next_action = default_next_action(next).to_string();
    swap.updated_at = Utc::now();
    ctx.storage.append_event(&swap.id, event, payload)?;
    info!("swap={} state={:?}", swap.id, swap.state);
    Ok(())
}

fn internal_err<E: std::fmt::Display>(e: E) -> (StatusCode, String) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        format!("internal_error:{e}"),
    )
}
