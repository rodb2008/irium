use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SwapState {
    Created,
    Quoted,
    Accepted,
    SecretCommitted,
    IriumHtlcCreated,
    IriumHtlcConfirmed,
    BtcHtlcCreated,
    BtcHtlcConfirmed,
    ClaimInitiated,
    Claimed,
    RefundPending,
    Refunded,
    Failed,
    Expired,
    ManualReview,
}

impl SwapState {
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Claimed | Self::Refunded | Self::Failed | Self::Expired
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Swap {
    pub id: String,
    pub tester_handle: String,
    pub session_token: String,
    pub btc_receive_address: String,
    pub btc_htlc_address: Option<String>,
    pub btc_funding_txid: Option<String>,
    pub btc_spent_txid: Option<String>,
    pub irium_htlc_txid: Option<String>,
    pub irium_htlc_vout: Option<u32>,
    pub irium_spend_txid: Option<String>,
    pub secret_hash_hex: String,
    pub state: SwapState,
    pub next_action: String,
    pub expected_amount_sats: u64,
    pub btc_confirmations: u32,
    pub timeout_height_hint: Option<u64>,
    pub manual_review: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwapEvent {
    pub id: i64,
    pub swap_id: String,
    pub event_type: String,
    pub payload: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreatePublicSwapRequest {
    pub tester_handle: String,
    pub btc_testnet_receive_address: String,
    pub invite_code: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct CreatePublicSwapResponse {
    pub swap_id: String,
    pub session_token: String,
    pub state: SwapState,
    pub next_action: String,
}

#[derive(Debug, Serialize)]
pub struct PublicSwapView {
    pub swap_id: String,
    pub state: SwapState,
    pub next_action: String,
    pub btc_htlc_address: Option<String>,
    pub expected_amount_sats: u64,
    pub btc_confirmations: u32,
    pub btc_funding_txid: Option<String>,
    pub btc_spent_txid: Option<String>,
    pub irium_htlc_txid: Option<String>,
    pub irium_htlc_vout: Option<u32>,
    pub irium_spend_txid: Option<String>,
    pub timeout_height_hint: Option<u64>,
    pub success: bool,
    pub refunded: bool,
    pub failed: bool,
}

#[derive(Debug, Deserialize)]
pub struct SubmitBtcTxidRequest {
    pub btc_txid: String,
}

#[derive(Debug, Deserialize)]
pub struct SubmitTerminalProofRequest {
    pub side: String,
    pub outcome: String,
    pub txid: String,
}

#[derive(Debug, Serialize)]
pub struct StatusResponse {
    pub state: SwapState,
    pub next_action: String,
    pub terminal: bool,
}

#[derive(Debug, Deserialize)]
pub struct MarkReviewRequest {
    pub manual_review: bool,
}

#[derive(Debug, Deserialize)]
pub struct PauseIntakeRequest {
    pub paused: bool,
}
