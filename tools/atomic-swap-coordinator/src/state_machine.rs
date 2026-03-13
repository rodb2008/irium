use crate::model::SwapState;

pub fn can_transition(from: SwapState, to: SwapState) -> bool {
    if from == to {
        return true;
    }
    if from.is_terminal() {
        return false;
    }
    matches!(
        (from, to),
        (SwapState::Created, SwapState::Quoted)
            | (SwapState::Quoted, SwapState::Accepted)
            | (SwapState::Accepted, SwapState::SecretCommitted)
            | (SwapState::SecretCommitted, SwapState::BtcHtlcCreated)
            | (SwapState::BtcHtlcCreated, SwapState::BtcHtlcConfirmed)
            | (SwapState::BtcHtlcConfirmed, SwapState::IriumHtlcCreated)
            | (SwapState::IriumHtlcCreated, SwapState::IriumHtlcConfirmed)
            | (SwapState::IriumHtlcConfirmed, SwapState::ClaimInitiated)
            | (SwapState::ClaimInitiated, SwapState::Claimed)
            | (SwapState::BtcHtlcConfirmed, SwapState::RefundPending)
            | (SwapState::IriumHtlcConfirmed, SwapState::RefundPending)
            | (SwapState::RefundPending, SwapState::Refunded)
            | (_, SwapState::ManualReview)
            | (SwapState::ManualReview, SwapState::Accepted)
            | (SwapState::ManualReview, SwapState::SecretCommitted)
            | (SwapState::ManualReview, SwapState::RefundPending)
            | (_, SwapState::Failed)
            | (_, SwapState::Expired)
    )
}

pub fn default_next_action(state: SwapState) -> &'static str {
    match state {
        SwapState::Created => "wait_quote",
        SwapState::Quoted => "wait_accept",
        SwapState::Accepted => "wait_secret_commit",
        SwapState::SecretCommitted => "send_btc_to_htlc_address",
        SwapState::BtcHtlcCreated => "wait_btc_confirmations",
        SwapState::BtcHtlcConfirmed => "wait_irium_htlc_creation",
        SwapState::IriumHtlcCreated => "wait_irium_confirmations",
        SwapState::IriumHtlcConfirmed => "claim_swap",
        SwapState::ClaimInitiated => "wait_claim_confirmation",
        SwapState::Claimed => "completed",
        SwapState::RefundPending => "wait_refund_execution",
        SwapState::Refunded => "refunded",
        SwapState::Failed => "failed",
        SwapState::Expired => "expired",
        SwapState::ManualReview => "operator_review_required",
    }
}
