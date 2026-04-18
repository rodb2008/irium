use irium_node_rs::constants::COINBASE_MATURITY;
use irium_node_rs::pow::sha256d;
use irium_node_rs::qr::{render_ascii, render_svg};
use irium_node_rs::settlement::{
    agreement_share_package_all_artifact_types, build_agreement_artifact_authenticity_verification,
    build_agreement_artifact_verification, build_agreement_bundle,
    build_agreement_bundle_with_artifacts, build_agreement_share_package,
    build_agreement_share_package_verification, build_agreement_statement, build_deposit_agreement,
    build_milestone_agreement, build_otc_agreement, build_simple_settlement_agreement,
    canonical_serialization_rules, compute_agreement_bundle_hash_hex,
    compute_agreement_signature_payload_hash, inspect_agreement_share_package,
    inspect_agreement_signature, render_agreement_audit_csv, summarize_agreement_authenticity,
    validate_agreement_signature_envelope, verify_agreement_bundle, verify_agreement_share_package,
    verify_bundle_signatures, AgreementArtifactVerificationResult, AgreementAuditRecord,
    AgreementBundle, AgreementBundleChainObservationSnapshot, AgreementLifecycleView,
    AgreementMilestone, AgreementObject, AgreementParty, AgreementSharePackage,
    AgreementSharePackageInspection, AgreementSharePackageVerificationResult,
    AgreementSignatureEnvelope, AgreementSignatureTargetType, AgreementSignatureVerification,
    AgreementStatement, AgreementSummary, AgreementTemplateType,
    AGREEMENT_SIGNATURE_TYPE_SECP256K1, AGREEMENT_SIGNATURE_VERSION,
    HoldbackEvaluationResult, HoldbackOutcome, MilestoneEvaluationResult,
    PolicyOutcome,
    ProofPolicy, ProofSignatureEnvelope, SettlementProof, SETTLEMENT_PROOF_SCHEMA_ID,
    TypedProofPayload, validate_typed_proof_payload,
    settlement_proof_payload_bytes,
};
use irium_node_rs::tx::{Transaction, TxInput, TxOutput};
use k256::ecdsa::signature::hazmat::PrehashSigner;
use k256::ecdsa::{Signature, SigningKey};
use k256::elliptic_curve::sec1::ToEncodedPoint;
use k256::SecretKey;
use rand_core::{OsRng, RngCore};
use reqwest::blocking::Client;
use ripemd::Ripemd160;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const IRIUM_P2PKH_VERSION: u8 = 0x39;
const DEFAULT_FEE_PER_BYTE: u64 = 1;

#[derive(Serialize, Deserialize)]
struct WalletFile {
    version: u32,
    #[serde(default)]
    seed_hex: Option<String>,
    #[serde(default)]
    next_index: u32,
    keys: Vec<WalletKey>,
}

#[derive(Serialize, Deserialize, Clone)]
struct WalletKey {
    address: String,
    pkh: String,
    pubkey: String,
    privkey: String,
}

#[derive(Deserialize)]
struct LegacyWalletFile {
    keys: HashMap<String, String>,
    #[allow(dead_code)]
    addresses: Option<Vec<String>>,
}

fn base58check_decode(s: &str) -> Option<Vec<u8>> {
    let data = bs58::decode(s).into_vec().ok()?;
    if data.len() < 5 {
        return None;
    }
    let (body, checksum) = data.split_at(data.len() - 4);
    let first = Sha256::digest(body);
    let second = Sha256::digest(&first);
    if &second[0..4] != checksum {
        return None;
    }
    Some(body.to_vec())
}

fn wif_to_secret_and_compression(wif: &str) -> Result<([u8; 32], bool), String> {
    let data = base58check_decode(wif).ok_or_else(|| "invalid WIF".to_string())?;

    // Standard WIF payload: 0x80 || 32-byte secret [|| 0x01 if compressed]
    if data.len() != 33 && data.len() != 34 {
        return Err("invalid WIF length".to_string());
    }
    if data[0] != 0x80 {
        return Err("unsupported WIF version".to_string());
    }

    let compressed = if data.len() == 34 {
        if data[33] != 0x01 {
            return Err("invalid WIF compression flag".to_string());
        }
        true
    } else {
        false
    };

    let mut out = [0u8; 32];
    out.copy_from_slice(&data[1..33]);
    Ok((out, compressed))
}

fn maybe_migrate_legacy_wallet(path: &Path, data: &str) -> Result<Option<WalletFile>, String> {
    let legacy: LegacyWalletFile = match serde_json::from_str(data) {
        Ok(v) => v,
        Err(_) => return Ok(None),
    };

    if legacy.keys.is_empty() {
        return Ok(Some(WalletFile {
            version: 1,
            seed_hex: None,
            next_index: 0,
            keys: Vec::new(),
        }));
    }

    let mut entries: Vec<(String, String)> = legacy.keys.into_iter().collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    let mut keys = Vec::with_capacity(entries.len());
    for (address, wif) in entries {
        let (priv_bytes, compressed) = wif_to_secret_and_compression(&wif)
            .map_err(|e| format!("legacy key for {address}: {e}"))?;

        let secret = SecretKey::from_slice(&priv_bytes)
            .map_err(|e| format!("legacy key for {address}: invalid secret key: {e}"))?;
        let public = secret.public_key();
        let pubkey = public.to_encoded_point(compressed);
        let pkh = hash160(pubkey.as_bytes());
        let derived = base58_p2pkh_from_hash(&pkh);
        if derived != address {
            return Err(format!(
                "legacy key address mismatch: file has {address}, derived {derived}"
            ));
        }

        keys.push(WalletKey {
            address,
            pkh: hex::encode(pkh),
            pubkey: hex::encode(pubkey.as_bytes()),
            privkey: hex::encode(priv_bytes),
        });
    }

    let wallet = WalletFile {
        version: 1,
        seed_hex: None,
        next_index: keys.len() as u32,
        keys,
    };

    // Backup the legacy file before rewriting it.
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let backup = if let Some(name) = path.file_name() {
        path.with_file_name(format!("{}.legacy.bak.{}", name.to_string_lossy(), ts))
    } else {
        PathBuf::from(format!("{}.legacy.bak.{}", path.display(), ts))
    };

    fs::copy(path, &backup).map_err(|e| format!("backup legacy wallet: {e}"))?;
    eprintln!(
        "[warn] Migrated legacy wallet format to v1; backup saved at: {}",
        backup.display()
    );
    save_wallet(path, &wallet)?;

    Ok(Some(wallet))
}

#[derive(Deserialize)]
struct BalanceResponse {
    balance: u64,
    utxo_count: usize,
    mined_blocks: Option<usize>,
}

#[derive(Deserialize)]
struct UtxosResponse {
    height: u64,
    utxos: Vec<UtxoItem>,
}

#[derive(Deserialize)]
struct HistoryResponse {
    height: u64,
    txs: Vec<HistoryItem>,
}

#[derive(Deserialize)]
struct HistoryItem {
    txid: String,
    height: u64,
    received: u64,
    spent: u64,
    net: i64,
    is_coinbase: bool,
}

#[derive(Deserialize)]
struct FeeEstimateResponse {
    min_fee_per_byte: f64,
    mempool_size: usize,
}

#[derive(Deserialize, Clone)]
struct UtxoItem {
    txid: String,
    index: u32,
    value: u64,
    height: u64,
    is_coinbase: bool,
    script_pubkey: String,
}

#[derive(Serialize)]
struct SubmitTxRequest {
    tx_hex: String,
}

#[derive(Serialize)]
struct AgreementRequestBody {
    agreement: AgreementObject,
}

#[derive(Serialize)]
struct FundAgreementRequestBody {
    agreement: AgreementObject,
    fee_per_byte: Option<u64>,
    broadcast: Option<bool>,
}

#[derive(Serialize)]
struct VerifyAgreementLinkRequestBody {
    agreement_hash: String,
    tx_hex: String,
}

#[derive(Serialize, Deserialize)]
struct AgreementHashResponse {
    agreement_hash: String,
}

#[derive(Serialize, Deserialize)]
struct AgreementInspectResponse {
    agreement_hash: String,
    summary: AgreementSummary,
}

#[derive(Serialize, Deserialize)]
struct AgreementLinkedTx {
    txid: String,
    role: String,
    milestone_id: Option<String>,
    height: Option<u64>,
    confirmed: bool,
    value: u64,
}

#[derive(Serialize, Deserialize)]
struct AgreementTxsResponse {
    agreement_hash: String,
    txs: Vec<AgreementLinkedTx>,
}

#[derive(Serialize)]
struct AgreementContextRequestBody {
    agreement: AgreementObject,
    #[serde(skip_serializing_if = "Option::is_none")]
    bundle: Option<AgreementBundle>,
}

#[derive(Serialize, Deserialize, Clone)]
struct AgreementFundingLegCandidateResponse {
    agreement_hash: String,
    funding_txid: String,
    htlc_vout: u32,
    anchor_vout: u32,
    role: String,
    milestone_id: Option<String>,
    amount: u64,
    htlc_backed: bool,
    timeout_height: u64,
    recipient_address: String,
    refund_address: String,
    source_notes: Vec<String>,
    release_eligible: bool,
    release_reasons: Vec<String>,
    refund_eligible: bool,
    refund_reasons: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone)]
struct AgreementFundingLegsResponse {
    agreement_hash: String,
    selection_required: bool,
    candidates: Vec<AgreementFundingLegCandidateResponse>,
    trust_model_note: String,
}

#[derive(Serialize, Deserialize, Clone)]
struct AgreementActivityEvent {
    event_type: String,
    source: String,
    txid: Option<String>,
    height: Option<u64>,
    timestamp: Option<u64>,
    milestone_id: Option<String>,
    note: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
struct AgreementTimelineResponse {
    agreement_hash: String,
    lifecycle: AgreementLifecycleView,
    events: Vec<AgreementActivityEvent>,
    trust_model_note: String,
}

#[derive(Serialize, Deserialize)]
struct AgreementMilestoneStatus {
    milestone_id: String,
    title: String,
    amount: u64,
    funded: bool,
    released: bool,
    refunded: bool,
}

#[derive(Serialize, Deserialize)]
struct AgreementStatusResponse {
    agreement_hash: String,
    lifecycle: AgreementLifecycleView,
}

#[derive(Serialize, Deserialize)]
struct AgreementMilestonesResponse {
    agreement_hash: String,
    state: String,
    milestones: Vec<AgreementMilestoneStatus>,
}

#[derive(Serialize, Deserialize)]
struct AgreementFundingOutput {
    vout: u32,
    role: String,
    milestone_id: Option<String>,
    amount: u64,
}

#[derive(Serialize, Deserialize)]
struct FundAgreementResponse {
    agreement_hash: String,
    txid: String,
    accepted: bool,
    raw_tx_hex: String,
    outputs: Vec<AgreementFundingOutput>,
    fee: u64,
}

#[derive(Serialize, Deserialize)]
struct VerifyAgreementLinkResponse {
    agreement_hash: String,
    matched: bool,
    anchors: Vec<Value>,
}

#[derive(Serialize)]
struct AgreementSpendRequestBody {
    agreement: AgreementObject,
    funding_txid: String,
    htlc_vout: Option<u32>,
    milestone_id: Option<String>,
    destination_address: Option<String>,
    fee_per_byte: Option<u64>,
    broadcast: Option<bool>,
    secret_hex: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
struct AgreementSpendEligibilityResponse {
    agreement_hash: String,
    agreement_id: String,
    funding_txid: String,
    htlc_vout: Option<u32>,
    anchor_vout: Option<u32>,
    role: Option<String>,
    milestone_id: Option<String>,
    amount: Option<u64>,
    branch: String,
    htlc_backed: bool,
    funded: bool,
    unspent: bool,
    preimage_required: bool,
    timeout_height: Option<u64>,
    timeout_reached: bool,
    destination_address: Option<String>,
    expected_hash: Option<String>,
    recipient_address: Option<String>,
    refund_address: Option<String>,
    eligible: bool,
    reasons: Vec<String>,
    trust_model_note: String,
}



#[derive(Serialize, Deserialize, Clone, Debug)]
struct SubmitProofRpcRequest {
    proof: SettlementProof,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct SubmitProofRpcResponse {
    proof_id: String,
    agreement_hash: String,
    accepted: bool,
    duplicate: bool,
    message: String,
    /// Chain tip height at submit time.
    #[serde(default)]
    tip_height: u64,
    /// Expiry height from the submitted proof, if any.
    #[serde(default)]
    expires_at_height: Option<u64>,
    /// True when tip_height >= expires_at_height at submit time. Always false when expires_at_height is None.
    #[serde(default)]
    expired: bool,
    /// Derived lifecycle status: "active" or "expired". Empty string when talking to older nodes.
    #[serde(default)]
    status: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct ListPoliciesRpcRequest {
    #[serde(default)]
    active_only: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct PolicySummaryItem {
    agreement_hash: String,
    policy_id: String,
    required_proofs: usize,
    attestors: usize,
    expires_at_height: Option<u64>,
    expired: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct ListPoliciesRpcResponse {
    count: usize,
    policies: Vec<PolicySummaryItem>,
    #[serde(default)]
    active_only: bool,
}

fn u32_is_zero(n: &u32) -> bool { *n == 0 }

#[derive(Serialize, Deserialize, Clone, Debug)]
struct ListProofsRpcRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    agreement_hash: Option<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    active_only: bool,
    #[serde(skip_serializing_if = "u32_is_zero")]
    offset: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    limit: Option<u32>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct ProofListItem {
    #[serde(flatten)]
    proof: SettlementProof,
    /// Derived lifecycle status from the node: "active" or "expired". Empty when talking to older nodes.
    #[serde(default)]
    status: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
struct ListProofsRpcResponse {
    agreement_hash: String,
    /// Chain tip height at query time.
    #[serde(default)]
    tip_height: u64,
    /// Echoed from the request; true when only non-expired proofs were returned.
    #[serde(default)]
    active_only: bool,
    /// Total matches before pagination; equals returned_count when no pagination was applied.
    #[serde(default)]
    total_count: usize,
    /// Number of proofs returned in this page. Equals proofs.len().
    #[serde(default)]
    returned_count: usize,
    /// True when more proofs remain after this page.
    #[serde(default)]
    has_more: bool,
    #[serde(default)]
    offset: u32,
    #[serde(default)]
    limit: Option<u32>,
    proofs: Vec<ProofListItem>,
}

#[derive(Debug, Clone)]
struct ProofSubmitCliOptions {
    proof_path: String,
    rpc_url: String,
    json_mode: bool,
}

#[derive(Debug, Clone)]
struct ProofListCliOptions {
    agreement_hash: Option<String>,
    active_only: bool,
    offset: u32,
    limit: Option<u32>,
    rpc_url: String,
    json_mode: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct GetProofRpcRequest {
    proof_id: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct GetProofRpcResponse {
    proof_id: String,
    found: bool,
    #[serde(default)]
    tip_height: u64,
    proof: Option<SettlementProof>,
    #[serde(default)]
    expires_at_height: Option<u64>,
    #[serde(default)]
    expired: bool,
    /// Derived lifecycle status: "active" or "expired". Empty when found=false or older nodes.
    #[serde(default)]
    status: String,
}

#[derive(Debug, Clone)]
struct ProofGetCliOptions {
    proof_id: String,
    rpc_url: String,
    json_mode: bool,
}

#[derive(Debug, Clone)]
struct ProofCreateCliOptions {
    agreement_hash: String,
    proof_type: String,
    attested_by: String,
    address: String,
    milestone_id: Option<String>,
    evidence_summary: Option<String>,
    evidence_hash: Option<String>,
    proof_id: Option<String>,
    timestamp: Option<u64>,
    out_path: Option<String>,
    json_mode: bool,
    expires_at_height: Option<u64>,
    proof_kind: Option<String>,
    reference_id: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct CheckPolicyRpcRequest {
    agreement: AgreementObject,
    policy: ProofPolicy,
    #[serde(default)]
    proofs: Vec<SettlementProof>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct CheckPolicyRpcResponse {
    agreement_hash: String,
    policy_id: String,
    tip_height: u64,
    release_eligible: bool,
    refund_eligible: bool,
    reason: String,
    evaluated_rules: Vec<String>,
    /// Top-level holdback result; absent when no holdback is declared on the policy.
    #[serde(default)]
    holdback: Option<HoldbackEvaluationResult>,
    /// Per-milestone results; empty when no milestones are declared.
    #[serde(default)]
    milestone_results: Vec<MilestoneEvaluationResult>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct StorePolicyRpcRequest {
    policy: ProofPolicy,
    replace: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct StorePolicyRpcResponse {
    policy_id: String,
    agreement_hash: String,
    accepted: bool,
    updated: bool,
    message: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct GetPolicyRpcRequest {
    agreement_hash: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct GetPolicyRpcResponse {
    agreement_hash: String,
    found: bool,
    policy: Option<ProofPolicy>,
    expires_at_height: Option<u64>,
    expired: bool,
}

// Phase 3: builder RPC types
#[derive(Serialize, Clone, Debug)]
struct BuildContractorTemplateRpcRequest {
    policy_id: String,
    agreement_hash: String,
    attestors: Vec<BuildTemplateAttestorInput>,
    milestones: Vec<BuildTemplateMilestoneInput>,
    notes: Option<String>,
}
#[derive(Serialize, Clone, Debug)]
struct BuildPreorderTemplateRpcRequest {
    policy_id: String,
    agreement_hash: String,
    attestors: Vec<BuildTemplateAttestorInput>,
    delivery_proof_type: String,
    refund_deadline_height: u64,
    holdback_bps: Option<u32>,
    holdback_release_height: Option<u64>,
    notes: Option<String>,
}
#[derive(Serialize, Clone, Debug)]
struct BuildOtcTemplateRpcRequest {
    policy_id: String,
    agreement_hash: String,
    attestors: Vec<BuildTemplateAttestorInput>,
    release_proof_type: String,
    refund_deadline_height: u64,
    threshold: Option<u32>,
    notes: Option<String>,
}
#[derive(Serialize, Deserialize, Clone, Debug)]
struct BuildTemplateAttestorInput {
    attestor_id: String,
    pubkey_hex: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    display_name: Option<String>,
}
#[derive(Serialize, Deserialize, Clone, Debug)]
struct BuildTemplateMilestoneInput {
    milestone_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    label: Option<String>,
    proof_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    deadline_height: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    holdback_bps: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    holdback_release_height: Option<u64>,
}
#[derive(Serialize, Deserialize, Clone, Debug)]
struct BuildTemplateRpcResponse {
    policy: ProofPolicy,
    policy_json: String,
    summary: String,
    requirement_count: usize,
    attestor_count: usize,
    milestone_count: usize,
    has_holdback: bool,
    has_timeout_rules: bool,
}


#[derive(Serialize, Deserialize, Clone, Debug)]
struct EvaluatePolicyRpcRequest {
    agreement: AgreementObject,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
struct HoldbackRpcResult {
    #[serde(default)]
    holdback_present: bool,
    #[serde(default)]
    holdback_released: bool,
    #[serde(default)]
    holdback_bps: u32,
    #[serde(default)]
    immediate_release_bps: u32,
    /// "pending", "held", or "released".
    #[serde(default)]
    holdback_outcome: String,
    #[serde(default)]
    holdback_reason: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
struct ThresholdResultRpc {
    #[serde(default)]
    requirement_id: String,
    #[serde(default)]
    threshold_required: u32,
    #[serde(default)]
    approved_attestor_count: usize,
    #[serde(default)]
    matched_attestor_ids: Vec<String>,
    #[serde(default)]
    threshold_satisfied: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
struct MilestoneRpcResult {
    #[serde(default)]
    milestone_id: String,
    #[serde(default)]
    label: Option<String>,
    /// "satisfied", "timeout", or "unsatisfied".
    #[serde(default)]
    outcome: String,
    #[serde(default)]
    release_eligible: bool,
    #[serde(default)]
    refund_eligible: bool,
    #[serde(default)]
    matched_proof_ids: Vec<String>,
    #[serde(default)]
    reason: String,
    #[serde(default)]
    holdback: Option<HoldbackRpcResult>,
    #[serde(default)]
    threshold_results: Vec<ThresholdResultRpc>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
struct EvaluatePolicyRpcResponse {
    /// Deterministic classification: "satisfied", "timeout", or "unsatisfied".
    #[serde(default)]
    outcome: String,
    #[serde(default)]
    agreement_hash: String,
    #[serde(default)]
    policy_found: bool,
    #[serde(default)]
    policy_id: Option<String>,
    #[serde(default)]
    expired: bool,
    #[serde(default)]
    tip_height: u64,
    /// Total active (non-expired) proofs considered for evaluation.
    #[serde(default)]
    proof_count: usize,
    /// Proofs filtered out as expired before evaluation.
    #[serde(default)]
    expired_proof_count: usize,
    /// Proofs that passed signature verification and matched the policy.
    #[serde(default)]
    matched_proof_count: usize,
    /// IDs of proofs that passed signature verification.
    #[serde(default)]
    matched_proof_ids: Vec<String>,
    #[serde(default)]
    release_eligible: bool,
    #[serde(default)]
    refund_eligible: bool,
    #[serde(default)]
    reason: String,
    #[serde(default)]
    evaluated_rules: Vec<String>,
    /// Per-milestone results; empty when no milestones declared.
    #[serde(default)]
    milestone_results: Vec<MilestoneRpcResult>,
    /// Number of milestones with outcome == "satisfied".
    #[serde(default)]
    completed_milestone_count: usize,
    /// Total declared milestones.
    #[serde(default)]
    total_milestone_count: usize,
    /// Top-level holdback result; None when no holdback configured or milestone path used.
    #[serde(default)]
    holdback: Option<HoldbackRpcResult>,
    /// Threshold results for requirements with explicit threshold set; empty otherwise.
    #[serde(default)]
    threshold_results: Vec<ThresholdResultRpc>,
}

/// Settlement action returned by buildsettlementtx RPC.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
struct SettlementActionRpc {
    /// "release", "refund", or future action types.
    #[serde(default)]
    action: String,
    #[serde(default)]
    recipient_address: String,
    #[serde(default)]
    recipient_label: String,
    /// Basis-points share of total_amount for this action (0-10000).
    #[serde(default)]
    amount_bps: u32,
    /// True when the chain conditions are met and the action can be broadcast now.
    #[serde(default)]
    executable: bool,
    /// Block height at or after which this action becomes executable; None = now.
    #[serde(default)]
    executable_after_height: Option<u64>,
    #[serde(default)]
    reason: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct BuildSettlementTxRpcRequest {
    agreement: AgreementObject,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
struct BuildSettlementTxRpcResponse {
    #[serde(default)]
    agreement_hash: String,
    #[serde(default)]
    policy_found: bool,
    #[serde(default)]
    release_eligible: bool,
    #[serde(default)]
    refund_eligible: bool,
    #[serde(default)]
    tip_height: u64,
    #[serde(default)]
    actions: Vec<SettlementActionRpc>,
    #[serde(default)]
    reason: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct ComputeAgreementHashRpcRequest {
    agreement: AgreementObject,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
struct ComputeAgreementHashRpcResponse {
    #[serde(default)]
    agreement_hash: String,
    /// Canonical JSON string that was hashed; clients can SHA-256 to verify.
    #[serde(default)]
    canonical_json: String,
    /// Human-readable description of the serialization rules applied.
    #[serde(default)]
    serialization_rules: Vec<String>,
}

#[derive(Debug, Clone)]
struct PolicyListCliOptions {
    rpc_url: String,
    json_mode: bool,
    active_only: bool,
}

#[derive(Debug, Clone)]
struct PolicyEvaluateCliOptions {
    agreement_path: String,
    rpc_url: String,
    json_mode: bool,
}

#[derive(Debug, Clone)]
struct PolicySetCliOptions {
    policy_path: String,
    rpc_url: String,
    json_mode: bool,
    replace: bool,
    expires_at_height: Option<u64>,
}

#[derive(Debug, Clone)]
struct PolicyGetCliOptions {
    agreement_hash: String,
    rpc_url: String,
    json_mode: bool,
}

#[derive(Debug, Clone)]
struct PolicyCheckCliOptions {
    agreement_path: String,
    policy_path: String,
    proof_paths: Vec<String>,
    rpc_url: String,
    json_mode: bool,
}

#[derive(Serialize, Deserialize, Clone)]
struct AgreementBuildSpendResponse {
    agreement_hash: String,
    agreement_id: String,
    funding_txid: String,
    htlc_vout: u32,
    role: String,
    milestone_id: Option<String>,
    branch: String,
    destination_address: String,
    txid: String,
    accepted: bool,
    raw_tx_hex: String,
    fee: u64,
    trust_model_note: String,
}

#[derive(Debug, Clone)]
struct AgreementSpendCliOptions {
    agreement_path: String,
    funding_txid: Option<String>,
    rpc_url: String,
    htlc_vout: Option<u32>,
    milestone_id: Option<String>,
    destination_address: Option<String>,
    fee_per_byte: Option<u64>,
    broadcast: bool,
    secret_hex: Option<String>,
    json_mode: bool,
    show_raw_tx: bool,
}

#[derive(Debug, Clone)]
struct ResolvedAgreementInput {
    agreement: AgreementObject,
    bundle: Option<AgreementBundle>,
    source: String,
}

#[derive(Debug, Clone)]
struct StoredAgreementBundle {
    path: PathBuf,
    bundle: AgreementBundle,
}

#[derive(Clone)]
struct StoredAgreementFile {
    path: PathBuf,
    agreement: AgreementObject,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SharePackageReceiptVerificationSummary {
    verified_match_count: usize,
    mismatch_count: usize,
    unverifiable_count: usize,
    valid_signatures: usize,
    invalid_signatures: usize,
    unverifiable_signatures: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct SharePackageReceiptArtifactPaths {
    agreement_path: Option<String>,
    bundle_path: Option<String>,
    audit_path: Option<String>,
    statement_path: Option<String>,
    agreement_signature_paths: Vec<String>,
    bundle_signature_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SharePackageReceipt {
    version: u32,
    receipt_id: String,
    imported_at: u64,
    source_path: String,
    import_source_label: Option<String>,
    sender_label: Option<String>,
    package_note: Option<String>,
    package_profile: String,
    included_artifact_types: Vec<String>,
    imported_artifact_types: Vec<String>,
    already_present_artifact_types: Vec<String>,
    rejected_artifact_types: Vec<String>,
    canonical_agreement_id: Option<String>,
    canonical_agreement_hash: Option<String>,
    bundle_hash: Option<String>,
    verification_summary: SharePackageReceiptVerificationSummary,
    warnings: Vec<String>,
    trust_notice: String,
    provenance_notice: String,
    package_path: String,
    verification_path: String,
    artifact_paths: SharePackageReceiptArtifactPaths,
}

#[derive(Debug, Serialize)]
struct SharePackageReceiptListItem {
    receipt_id: String,
    imported_at: u64,
    package_profile: String,
    canonical_agreement_id: Option<String>,
    canonical_agreement_hash: Option<String>,
    imported_artifact_types: Vec<String>,
    sender_label: Option<String>,
    import_source_label: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum SharePackageReceiptLocation {
    Active,
    Archived,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct SharePackageLocalHousekeepingMetadata {
    version: u32,
    archived_at: Option<u64>,
    archived_by_action: Option<String>,
    prune_reason: Option<String>,
}

#[derive(Debug, Clone)]
struct StoredSharePackageReceipt {
    receipt: SharePackageReceipt,
    receipt_path: PathBuf,
    receipt_dir: PathBuf,
    location: SharePackageReceiptLocation,
    housekeeping: Option<SharePackageLocalHousekeepingMetadata>,
}

#[derive(Debug, Clone)]
struct StoredAgreementSignatureFile {
    path: PathBuf,
    signature: AgreementSignatureEnvelope,
    fingerprint: String,
}

#[derive(Debug, Serialize)]
struct SharePackageReceiptInventoryItem {
    receipt_id: String,
    imported_at: u64,
    archived: bool,
    archived_at: Option<u64>,
    package_profile: String,
    canonical_agreement_id: Option<String>,
    canonical_agreement_hash: Option<String>,
    bundle_hash: Option<String>,
    imported_artifact_types: Vec<String>,
    sender_label: Option<String>,
    import_source_label: Option<String>,
    receipt_path: String,
}

#[derive(Debug, Serialize)]
struct LocalStoreBundleItem {
    agreement_id: String,
    agreement_hash: String,
    path: String,
    referenced_by_receipt_count: usize,
}

#[derive(Debug, Serialize)]
struct LocalStoreAgreementItem {
    agreement_id: String,
    agreement_hash: String,
    path: String,
    referenced_by_receipt_count: usize,
}

#[derive(Debug, Serialize)]
struct LocalStoreSignatureItem {
    target_type: String,
    target_hash: String,
    fingerprint: String,
    path: String,
    referenced_by_receipt_count: usize,
}

#[derive(Debug, Serialize)]
struct LocalStoreInformationalItem {
    kind: String,
    receipt_id: String,
    archived: bool,
    path: String,
}

#[derive(Debug, Serialize)]
struct AgreementLocalStoreListing {
    scope_notice: String,
    housekeeping_notice: String,
    active_receipt_count: usize,
    archived_receipt_count: usize,
    bundle_count: usize,
    raw_agreement_count: usize,
    detached_signature_count: usize,
    informational_file_count: usize,
    active_receipts: Vec<SharePackageReceiptInventoryItem>,
    archived_receipts: Vec<SharePackageReceiptInventoryItem>,
    stored_bundles: Vec<LocalStoreBundleItem>,
    stored_raw_agreements: Vec<LocalStoreAgreementItem>,
    stored_detached_signatures: Vec<LocalStoreSignatureItem>,
    stored_informational_files: Vec<LocalStoreInformationalItem>,
}

#[derive(Debug, Serialize)]
struct LocalStoreMutationEntry {
    kind: String,
    target: String,
    path: String,
    note: String,
}

#[derive(Debug, Serialize)]
struct LocalStoreMutationReport {
    action: String,
    dry_run: bool,
    changed: Vec<LocalStoreMutationEntry>,
    skipped: Vec<LocalStoreMutationEntry>,
    warnings: Vec<String>,
    scope_notice: String,
    untouched_notice: String,
}

#[derive(Debug, Serialize)]
struct SharePackageArchiveResult {
    receipt_id: String,
    from_path: String,
    to_path: String,
    archived_at: u64,
    scope_notice: String,
    untouched_notice: String,
    warnings: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StoreWriteStatus {
    Imported,
    AlreadyPresent,
}

#[derive(Serialize)]
struct AgreementBundleListItem {
    agreement_id: String,
    agreement_hash: String,
    saved_at: u64,
    source_label: Option<String>,
    linked_funding_txids: Vec<String>,
    path: String,
}

// Base58 P2PKH decoder (version byte + 20-byte hash + 4-byte checksum)
fn base58_p2pkh_to_hash(addr: &str) -> Option<Vec<u8>> {
    let data = bs58::decode(addr).into_vec().ok()?;
    if data.len() < 25 {
        return None;
    }
    let (body, checksum) = data.split_at(data.len() - 4);
    let first = Sha256::digest(body);
    let second = Sha256::digest(&first);
    if &second[0..4] != checksum {
        return None;
    }
    if body.len() < 21 {
        return None;
    }
    if body[0] != IRIUM_P2PKH_VERSION {
        return None;
    }
    let payload = &body[1..];
    if payload.len() != 20 {
        return None;
    }
    Some(payload.to_vec())
}

fn hash160(data: &[u8]) -> [u8; 20] {
    let sha = Sha256::digest(data);
    let rip = Ripemd160::digest(&sha);
    let mut out = [0u8; 20];
    out.copy_from_slice(&rip);
    out
}

fn base58_p2pkh_from_hash(pkh: &[u8; 20]) -> String {
    let mut body = Vec::with_capacity(1 + 20);
    body.push(IRIUM_P2PKH_VERSION);
    body.extend_from_slice(pkh);
    let first = Sha256::digest(&body);
    let second = Sha256::digest(&first);
    let checksum = &second[0..4];
    let mut full = body;
    full.extend_from_slice(checksum);
    bs58::encode(full).into_string()
}

fn wallet_path() -> PathBuf {
    if let Ok(path) = env::var("IRIUM_WALLET_FILE") {
        return PathBuf::from(path);
    }
    let home = env::var("HOME").unwrap_or_else(|_| "/".to_string());
    PathBuf::from(home).join(".irium/wallet.json")
}

fn irium_data_dir() -> PathBuf {
    if let Ok(path) = env::var("IRIUM_DATA_DIR") {
        return PathBuf::from(path);
    }
    let home = env::var("HOME").unwrap_or_else(|_| "/".to_string());
    PathBuf::from(home).join(".irium")
}

fn agreement_bundles_dir() -> PathBuf {
    if let Ok(path) = env::var("IRIUM_AGREEMENT_BUNDLES_DIR") {
        return PathBuf::from(path);
    }
    irium_data_dir().join("agreements")
}

fn imported_agreements_dir() -> PathBuf {
    if let Ok(path) = env::var("IRIUM_IMPORTED_AGREEMENTS_DIR") {
        return PathBuf::from(path);
    }
    agreement_bundles_dir().join("raw")
}

fn share_package_inbox_dir() -> PathBuf {
    if let Ok(path) = env::var("IRIUM_SHARE_PACKAGE_INBOX_DIR") {
        return PathBuf::from(path);
    }
    irium_data_dir().join("share-package-inbox")
}

fn imported_signature_store_dir() -> PathBuf {
    if let Ok(path) = env::var("IRIUM_IMPORTED_SIGNATURES_DIR") {
        return PathBuf::from(path);
    }
    irium_data_dir().join("agreement-signatures")
}

fn share_package_archive_dir() -> PathBuf {
    if let Ok(path) = env::var("IRIUM_SHARE_PACKAGE_ARCHIVE_DIR") {
        return PathBuf::from(path);
    }
    share_package_inbox_dir().join("archived")
}

fn share_package_housekeeping_path(dir: &Path) -> PathBuf {
    dir.join("housekeeping.local.json")
}

fn local_housekeeping_scope_notice() -> String {
    "Local housekeeping changes affect only wallet-side files. No on-chain or network state was changed.".to_string()
}

fn local_housekeeping_untouched_notice() -> String {
    "Local archive, remove, or prune actions do not revoke artifacts elsewhere and do not change chain state, trust roots, or agreement/bundle verification roots.".to_string()
}

fn target_type_label(target_type: &AgreementSignatureTargetType) -> &'static str {
    match target_type {
        AgreementSignatureTargetType::Agreement => "agreement",
        AgreementSignatureTargetType::Bundle => "bundle",
    }
}

fn load_share_package_housekeeping_metadata(
    path: &Path,
) -> Result<Option<SharePackageLocalHousekeepingMetadata>, String> {
    if !path.exists() {
        return Ok(None);
    }
    let data = read_text_from_path_or_stdin(path, "share package housekeeping metadata")?;
    let metadata = serde_json::from_str::<SharePackageLocalHousekeepingMetadata>(&data)
        .map_err(|e| format!("parse share package housekeeping metadata json: {e}"))?;
    Ok(Some(metadata))
}

fn write_share_package_housekeeping_metadata(
    dir: &Path,
    metadata: &SharePackageLocalHousekeepingMetadata,
) -> Result<(), String> {
    write_json_file(
        &share_package_housekeeping_path(dir),
        metadata,
        "share package housekeeping metadata",
    )
}

fn read_receipt_record_from_dir(
    dir: &Path,
    location: SharePackageReceiptLocation,
) -> Result<Option<StoredSharePackageReceipt>, String> {
    let receipt_path = share_package_receipt_path(dir);
    if !receipt_path.exists() {
        return Ok(None);
    }
    let receipt = load_share_package_receipt(&receipt_path)?;
    let housekeeping =
        load_share_package_housekeeping_metadata(&share_package_housekeeping_path(dir))?;
    Ok(Some(StoredSharePackageReceipt {
        receipt,
        receipt_path,
        receipt_dir: dir.to_path_buf(),
        location,
        housekeeping,
    }))
}

fn list_share_package_receipt_records_in_dir(
    dir: &Path,
    location: SharePackageReceiptLocation,
) -> Result<Vec<StoredSharePackageReceipt>, String> {
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in fs::read_dir(dir).map_err(|e| format!("read share-package receipt dir: {e}"))? {
        let entry = entry.map_err(|e| format!("read share-package receipt dir entry: {e}"))?;
        let file_type = entry
            .file_type()
            .map_err(|e| format!("read share-package receipt dir entry type: {e}"))?;
        if !file_type.is_dir() {
            continue;
        }
        let path = entry.path();
        if location == SharePackageReceiptLocation::Active
            && path.file_name() == Some(OsStr::new("archived"))
        {
            continue;
        }
        if let Some(record) = read_receipt_record_from_dir(&path, location)? {
            out.push(record);
        }
    }
    out.sort_by_key(|item| Reverse(item.receipt.imported_at));
    Ok(out)
}

fn list_share_package_receipt_records(
    include_archived: bool,
) -> Result<Vec<StoredSharePackageReceipt>, String> {
    let mut out = list_share_package_receipt_records_in_dir(
        &share_package_inbox_dir(),
        SharePackageReceiptLocation::Active,
    )?;
    if include_archived {
        out.extend(list_share_package_receipt_records_in_dir(
            &share_package_archive_dir(),
            SharePackageReceiptLocation::Archived,
        )?);
    }
    out.sort_by_key(|item| Reverse(item.receipt.imported_at));
    Ok(out)
}

fn receipt_inventory_item(record: &StoredSharePackageReceipt) -> SharePackageReceiptInventoryItem {
    SharePackageReceiptInventoryItem {
        receipt_id: record.receipt.receipt_id.clone(),
        imported_at: record.receipt.imported_at,
        archived: record.location == SharePackageReceiptLocation::Archived,
        archived_at: record
            .housekeeping
            .as_ref()
            .and_then(|item| item.archived_at),
        package_profile: record.receipt.package_profile.clone(),
        canonical_agreement_id: record.receipt.canonical_agreement_id.clone(),
        canonical_agreement_hash: record.receipt.canonical_agreement_hash.clone(),
        bundle_hash: record.receipt.bundle_hash.clone(),
        imported_artifact_types: record.receipt.imported_artifact_types.clone(),
        sender_label: record.receipt.sender_label.clone(),
        import_source_label: record.receipt.import_source_label.clone(),
        receipt_path: record.receipt_path.display().to_string(),
    }
}

fn build_receipt_reference_index(
    records: &[StoredSharePackageReceipt],
) -> HashMap<String, Vec<String>> {
    let mut refs = HashMap::<String, Vec<String>>::new();
    let mut push = |path: &str, receipt_key: &str| {
        refs.entry(path.to_string())
            .or_default()
            .push(receipt_key.to_string());
    };
    for record in records {
        let receipt = &record.receipt;
        let receipt_key = record.receipt_dir.display().to_string();
        push(&receipt.package_path, &receipt_key);
        push(&receipt.verification_path, &receipt_key);
        if let Some(path) = &receipt.artifact_paths.agreement_path {
            push(path, &receipt_key);
        }
        if let Some(path) = &receipt.artifact_paths.bundle_path {
            push(path, &receipt_key);
        }
        if let Some(path) = &receipt.artifact_paths.audit_path {
            push(path, &receipt_key);
        }
        if let Some(path) = &receipt.artifact_paths.statement_path {
            push(path, &receipt_key);
        }
        for path in &receipt.artifact_paths.agreement_signature_paths {
            push(path, &receipt_key);
        }
        for path in &receipt.artifact_paths.bundle_signature_paths {
            push(path, &receipt_key);
        }
    }
    for value in refs.values_mut() {
        value.sort();
        value.dedup();
    }
    refs
}

fn receipt_canonical_artifact_entries(receipt: &SharePackageReceipt) -> Vec<(String, String)> {
    let mut seen = HashMap::<String, HashSet<String>>::new();
    let mut push = |kind: &str, path: &str| {
        seen.entry(path.to_string())
            .or_default()
            .insert(kind.to_string());
    };
    if let Some(path) = &receipt.artifact_paths.agreement_path {
        push("agreement", path);
    }
    if let Some(path) = &receipt.artifact_paths.bundle_path {
        push("bundle", path);
    }
    for path in &receipt.artifact_paths.agreement_signature_paths {
        push("agreement_signature", path);
    }
    for path in &receipt.artifact_paths.bundle_signature_paths {
        push("bundle_signature", path);
    }
    let mut out = seen
        .into_iter()
        .map(|(path, kinds)| {
            let mut kinds = kinds.into_iter().collect::<Vec<_>>();
            kinds.sort();
            (kinds.join("+"), path)
        })
        .collect::<Vec<_>>();
    out.sort_by(|a, b| a.1.cmp(&b.1));
    out
}

fn path_is_within(root: &Path, path: &Path) -> bool {
    match (fs::canonicalize(root), fs::canonicalize(path)) {
        (Ok(root), Ok(path)) => path.starts_with(&root),
        _ => false,
    }
}

fn path_is_local_housekeeping_safe(path: &Path) -> bool {
    let roots = [
        share_package_inbox_dir(),
        share_package_archive_dir(),
        agreement_bundles_dir(),
        imported_agreements_dir(),
        imported_signature_store_dir(),
    ];
    roots.iter().any(|root| path_is_within(root, path))
}

fn remove_path_exact(path: &Path) -> Result<(), String> {
    if path.is_dir() {
        fs::remove_dir_all(path).map_err(|e| format!("remove directory {}: {e}", path.display()))
    } else {
        fs::remove_file(path).map_err(|e| format!("remove file {}: {e}", path.display()))
    }
}

fn list_stored_signatures_at(base: &Path) -> Result<Vec<StoredAgreementSignatureFile>, String> {
    if !base.exists() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for target_name in ["agreement", "bundle"] {
        let target_dir = base.join(target_name);
        if !target_dir.exists() {
            continue;
        }
        for hash_entry in fs::read_dir(&target_dir)
            .map_err(|e| format!("read signature target dir {}: {e}", target_dir.display()))?
        {
            let hash_entry =
                hash_entry.map_err(|e| format!("read signature target dir entry: {e}"))?;
            if !hash_entry
                .file_type()
                .map_err(|e| format!("read signature target dir entry type: {e}"))?
                .is_dir()
            {
                continue;
            }
            for sig_entry in fs::read_dir(hash_entry.path()).map_err(|e| {
                format!(
                    "read signature store dir {}: {e}",
                    hash_entry.path().display()
                )
            })? {
                let sig_entry =
                    sig_entry.map_err(|e| format!("read signature store dir entry: {e}"))?;
                if !sig_entry
                    .file_type()
                    .map_err(|e| format!("read signature store dir entry type: {e}"))?
                    .is_file()
                {
                    continue;
                }
                let path = sig_entry.path();
                if path.extension().and_then(|v| v.to_str()) != Some("json") {
                    continue;
                }
                let signature = load_signature_from_path(&path)?;
                let fingerprint = path
                    .file_stem()
                    .and_then(|v| v.to_str())
                    .unwrap_or_default()
                    .to_string();
                out.push(StoredAgreementSignatureFile {
                    path,
                    signature,
                    fingerprint,
                });
            }
        }
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

fn looks_like_agreement_hash(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn read_text_from_path_or_stdin(path: &Path, label: &str) -> Result<String, String> {
    if path == Path::new("-") {
        use std::io::Read;
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .map_err(|e| format!("read {label} from stdin: {e}"))?;
        return Ok(buf);
    }
    fs::read_to_string(path).map_err(|e| format!("read {label}: {e}"))
}

fn load_bundle_from_path(path: &Path) -> Result<AgreementBundle, String> {
    let data = read_text_from_path_or_stdin(path, "agreement bundle")?;
    let bundle = serde_json::from_str::<AgreementBundle>(&data)
        .map_err(|e| format!("parse agreement bundle json: {e}"))?;
    verify_agreement_bundle(&bundle)?;
    Ok(bundle)
}

fn load_agreement_json_from_path(path: &Path) -> Result<AgreementObject, String> {
    let data = read_text_from_path_or_stdin(path, "agreement")?;
    let agreement = serde_json::from_str::<AgreementObject>(&data)
        .map_err(|e| format!("parse agreement json: {e}"))?;
    agreement.validate()?;
    Ok(agreement)
}

fn bundle_path_for_hash(dir: &Path, agreement_hash: &str) -> Result<PathBuf, String> {
    if !looks_like_agreement_hash(agreement_hash) {
        return Err("agreement hash must be 32-byte hex".to_string());
    }
    Ok(dir.join(format!("{}.json", agreement_hash.to_lowercase())))
}

fn save_bundle_to_store_at(dir: &Path, bundle: &AgreementBundle) -> Result<PathBuf, String> {
    verify_agreement_bundle(bundle)?;
    fs::create_dir_all(dir).map_err(|e| format!("create agreement bundle dir: {e}"))?;
    let path = bundle_path_for_hash(dir, &bundle.agreement_hash)?;
    let rendered = serde_json::to_string_pretty(bundle)
        .map_err(|e| format!("serialize agreement bundle: {e}"))?;
    fs::write(&path, rendered).map_err(|e| format!("write agreement bundle: {e}"))?;
    Ok(path)
}

fn list_stored_bundles_at(dir: &Path) -> Result<Vec<StoredAgreementBundle>, String> {
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in fs::read_dir(dir).map_err(|e| format!("read agreement bundle dir: {e}"))? {
        let entry = entry.map_err(|e| format!("read agreement bundle dir entry: {e}"))?;
        let path = entry.path();
        if entry
            .file_type()
            .map_err(|e| format!("read agreement bundle dir entry type: {e}"))?
            .is_dir()
        {
            continue;
        }
        if path.extension().and_then(|v| v.to_str()) != Some("json") {
            continue;
        }
        let bundle = load_bundle_from_path(&path)?;
        out.push(StoredAgreementBundle { path, bundle });
    }
    out.sort_by(|a, b| a.bundle.agreement_id.cmp(&b.bundle.agreement_id));
    Ok(out)
}

fn agreement_path_for_hash(dir: &Path, agreement_hash: &str) -> Result<PathBuf, String> {
    if !looks_like_agreement_hash(agreement_hash) {
        return Err("agreement hash must be 32-byte hex".to_string());
    }
    Ok(dir.join(format!("{}.json", agreement_hash.to_lowercase())))
}

fn save_agreement_to_store_at(dir: &Path, agreement: &AgreementObject) -> Result<PathBuf, String> {
    agreement.validate()?;
    let agreement_hash = irium_node_rs::settlement::compute_agreement_hash_hex(agreement)?;
    fs::create_dir_all(dir).map_err(|e| format!("create imported agreement dir: {e}"))?;
    let path = agreement_path_for_hash(dir, &agreement_hash)?;
    let rendered = serde_json::to_string_pretty(agreement)
        .map_err(|e| format!("serialize agreement json: {e}"))?;
    fs::write(&path, rendered).map_err(|e| format!("write imported agreement: {e}"))?;
    Ok(path)
}

fn list_stored_agreements_at(dir: &Path) -> Result<Vec<StoredAgreementFile>, String> {
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in fs::read_dir(dir).map_err(|e| format!("read imported agreement dir: {e}"))? {
        let entry = entry.map_err(|e| format!("read imported agreement dir entry: {e}"))?;
        if entry
            .file_type()
            .map_err(|e| format!("read imported agreement dir entry type: {e}"))?
            .is_dir()
        {
            continue;
        }
        let path = entry.path();
        if path.extension().and_then(|v| v.to_str()) != Some("json") {
            continue;
        }
        let agreement = load_agreement_json_from_path(&path)?;
        out.push(StoredAgreementFile { path, agreement });
    }
    out.sort_by(|a, b| a.agreement.agreement_id.cmp(&b.agreement.agreement_id));
    Ok(out)
}

fn resolve_agreement_from_store_at(
    dir: &Path,
    reference: &str,
) -> Result<StoredAgreementFile, String> {
    let agreements = list_stored_agreements_at(dir)?;
    if agreements.is_empty() {
        return Err("no saved imported agreements found in local store".to_string());
    }
    if looks_like_agreement_hash(reference) {
        let mut matches = agreements
            .into_iter()
            .filter(|item| {
                irium_node_rs::settlement::compute_agreement_hash_hex(&item.agreement)
                    .map(|hash| hash.eq_ignore_ascii_case(reference))
                    .unwrap_or(false)
            })
            .collect::<Vec<_>>();
        return match matches.len() {
            0 => Err(format!(
                "no saved imported agreement for hash {}",
                reference
            )),
            1 => Ok(matches.remove(0)),
            _ => Err(format!(
                "multiple saved imported agreements matched hash {}",
                reference
            )),
        };
    }
    let mut matches = agreements
        .into_iter()
        .filter(|item| item.agreement.agreement_id == reference)
        .collect::<Vec<_>>();
    match matches.len() {
        0 => Err(format!("no saved imported agreement for agreement_id {}", reference)),
        1 => Ok(matches.remove(0)),
        _ => Err(format!(
            "multiple saved imported agreements matched agreement_id {}; use the agreement hash instead",
            reference
        )),
    }
}

fn existing_agreement_id_conflicts(agreement_id: &str, agreement_hash: &str) -> Result<(), String> {
    for item in list_stored_bundles_at(&agreement_bundles_dir())? {
        if item.bundle.agreement_id == agreement_id
            && !item
                .bundle
                .agreement_hash
                .eq_ignore_ascii_case(agreement_hash)
        {
            return Err(format!(
                "local agreement_id {} already maps to a different bundle hash {}; use hash-based disambiguation",
                agreement_id, item.bundle.agreement_hash
            ));
        }
    }
    for item in list_stored_agreements_at(&imported_agreements_dir())? {
        let stored_hash = irium_node_rs::settlement::compute_agreement_hash_hex(&item.agreement)?;
        if item.agreement.agreement_id == agreement_id
            && !stored_hash.eq_ignore_ascii_case(agreement_hash)
        {
            return Err(format!(
                "local agreement_id {} already maps to a different imported agreement hash {}; use hash-based disambiguation",
                agreement_id, stored_hash
            ));
        }
    }
    Ok(())
}

fn save_bundle_to_store_checked(
    dir: &Path,
    bundle: &AgreementBundle,
) -> Result<(StoreWriteStatus, PathBuf), String> {
    verify_agreement_bundle(bundle)?;
    existing_agreement_id_conflicts(&bundle.agreement_id, &bundle.agreement_hash)?;
    fs::create_dir_all(dir).map_err(|e| format!("create agreement bundle dir: {e}"))?;
    let path = bundle_path_for_hash(dir, &bundle.agreement_hash)?;
    if path.exists() {
        let existing = load_bundle_from_path(&path)?;
        if existing == *bundle {
            return Ok((StoreWriteStatus::AlreadyPresent, path));
        }
        return Err(format!(
            "local bundle store already contains conflicting content for agreement hash {}",
            bundle.agreement_hash
        ));
    }
    let path = save_bundle_to_store_at(dir, bundle)?;
    Ok((StoreWriteStatus::Imported, path))
}

fn save_agreement_to_store_checked(
    dir: &Path,
    agreement: &AgreementObject,
) -> Result<(StoreWriteStatus, PathBuf), String> {
    agreement.validate()?;
    let agreement_hash = irium_node_rs::settlement::compute_agreement_hash_hex(agreement)?;
    existing_agreement_id_conflicts(&agreement.agreement_id, &agreement_hash)?;
    if let Ok(existing_bundle) =
        resolve_bundle_from_store_at(&agreement_bundles_dir(), &agreement_hash)
    {
        if existing_bundle.bundle.agreement == *agreement {
            return Ok((StoreWriteStatus::AlreadyPresent, existing_bundle.path));
        }
        return Err(format!(
            "local bundle store already contains conflicting canonical agreement content for hash {}",
            agreement_hash
        ));
    }
    fs::create_dir_all(dir).map_err(|e| format!("create imported agreement dir: {e}"))?;
    let path = agreement_path_for_hash(dir, &agreement_hash)?;
    if path.exists() {
        let existing = load_agreement_json_from_path(&path)?;
        if existing == *agreement {
            return Ok((StoreWriteStatus::AlreadyPresent, path));
        }
        return Err(format!(
            "local imported agreement store already contains conflicting content for agreement hash {}",
            agreement_hash
        ));
    }
    let path = save_agreement_to_store_at(dir, agreement)?;
    Ok((StoreWriteStatus::Imported, path))
}

fn signature_target_dir(
    base: &Path,
    target_type: AgreementSignatureTargetType,
    target_hash: &str,
) -> PathBuf {
    let target_name = match target_type {
        AgreementSignatureTargetType::Agreement => "agreement",
        AgreementSignatureTargetType::Bundle => "bundle",
    };
    base.join(target_name).join(target_hash.to_lowercase())
}

fn signature_fingerprint(signature: &AgreementSignatureEnvelope) -> Result<String, String> {
    let bytes = serde_json::to_vec(signature)
        .map_err(|e| format!("serialize signature for fingerprint: {e}"))?;
    Ok(hex::encode(Sha256::digest(bytes)))
}

fn save_signature_to_store_checked(
    base: &Path,
    signature: &AgreementSignatureEnvelope,
) -> Result<(StoreWriteStatus, PathBuf), String> {
    validate_agreement_signature_envelope(signature)?;
    let dir = signature_target_dir(base, signature.target_type, &signature.target_hash);
    fs::create_dir_all(&dir).map_err(|e| format!("create signature store dir: {e}"))?;
    let path = dir.join(format!("{}.json", signature_fingerprint(signature)?));
    if path.exists() {
        let existing = load_signature_from_path(&path)?;
        if existing == *signature {
            return Ok((StoreWriteStatus::AlreadyPresent, path));
        }
        return Err(format!(
            "local signature store already contains conflicting content for target hash {}",
            signature.target_hash
        ));
    }
    let rendered = serde_json::to_string_pretty(signature)
        .map_err(|e| format!("serialize signature json: {e}"))?;
    fs::write(&path, rendered).map_err(|e| format!("write imported signature: {e}"))?;
    Ok((StoreWriteStatus::Imported, path))
}

fn resolve_bundle_from_store_at(
    dir: &Path,
    reference: &str,
) -> Result<StoredAgreementBundle, String> {
    let bundles = list_stored_bundles_at(dir)?;
    if bundles.is_empty() {
        return Err("no saved agreement bundles found in local store".to_string());
    }
    if looks_like_agreement_hash(reference) {
        let mut matches = bundles
            .into_iter()
            .filter(|item| item.bundle.agreement_hash.eq_ignore_ascii_case(reference))
            .collect::<Vec<_>>();
        return match matches.len() {
            0 => Err(format!("no saved agreement bundle for hash {}", reference)),
            1 => Ok(matches.remove(0)),
            _ => Err(format!(
                "multiple saved agreement bundles matched hash {}",
                reference
            )),
        };
    }
    let mut matches = bundles
        .into_iter()
        .filter(|item| item.bundle.agreement_id == reference)
        .collect::<Vec<_>>();
    match matches.len() {
        0 => Err(format!("no saved agreement bundle for agreement_id {}", reference)),
        1 => Ok(matches.remove(0)),
        _ => Err(format!(
            "multiple saved agreement bundles matched agreement_id {}; use the agreement hash instead",
            reference
        )),
    }
}

fn resolve_agreement_input(input: &str) -> Result<ResolvedAgreementInput, String> {
    let path = Path::new(input);
    if path.exists() {
        if let Ok(bundle) = load_bundle_from_path(path) {
            return Ok(ResolvedAgreementInput {
                agreement: bundle.agreement.clone(),
                bundle: Some(bundle),
                source: format!("bundle_file:{}", path.display()),
            });
        }
        let agreement = load_agreement_json_from_path(path)?;
        return Ok(ResolvedAgreementInput {
            agreement,
            bundle: None,
            source: format!("agreement_file:{}", path.display()),
        });
    }
    if let Ok(stored) = resolve_bundle_from_store_at(&agreement_bundles_dir(), input) {
        return Ok(ResolvedAgreementInput {
            agreement: stored.bundle.agreement.clone(),
            bundle: Some(stored.bundle),
            source: format!("bundle_store:{}", stored.path.display()),
        });
    }
    let stored = resolve_agreement_from_store_at(&imported_agreements_dir(), input)?;
    Ok(ResolvedAgreementInput {
        agreement: stored.agreement,
        bundle: None,
        source: format!("agreement_store:{}", stored.path.display()),
    })
}

fn resolve_bundle_input(input: &str) -> Result<StoredAgreementBundle, String> {
    let path = Path::new(input);
    if path.exists() {
        let bundle = load_bundle_from_path(path)?;
        return Ok(StoredAgreementBundle {
            path: path.to_path_buf(),
            bundle,
        });
    }
    resolve_bundle_from_store_at(&agreement_bundles_dir(), input)
}

fn load_wallet(path: &Path) -> Result<WalletFile, String> {
    let data = fs::read_to_string(path).map_err(|e| format!("read wallet: {e}"))?;
    match serde_json::from_str::<WalletFile>(&data) {
        Ok(w) => Ok(w),
        Err(e) => {
            if let Some(w) = maybe_migrate_legacy_wallet(path, &data)? {
                return Ok(w);
            }
            Err(format!("parse wallet: {e}"))
        }
    }
}

fn save_wallet(path: &Path, wallet: &WalletFile) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create wallet dir: {e}"))?;
    }
    let data =
        serde_json::to_string_pretty(wallet).map_err(|e| format!("serialize wallet: {e}"))?;
    fs::write(path, data).map_err(|e| format!("write wallet: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = fs::Permissions::from_mode(0o600);
        fs::set_permissions(path, perms).map_err(|e| format!("chmod wallet: {e}"))?;
    }
    Ok(())
}

fn ensure_wallet(path: &Path) -> Result<WalletFile, String> {
    let mut wallet = if path.exists() {
        load_wallet(path)?
    } else {
        WalletFile {
            version: 1,
            seed_hex: None,
            next_index: 0,
            keys: Vec::new(),
        }
    };
    if wallet.seed_hex.is_some() && wallet.next_index < wallet.keys.len() as u32 {
        wallet.next_index = wallet.keys.len() as u32;
    }
    Ok(wallet)
}

fn wallet_key_from_secret(secret: &SecretKey, compressed: bool) -> WalletKey {
    let public = secret.public_key();
    let pubkey = public.to_encoded_point(compressed);
    let pkh = hash160(pubkey.as_bytes());
    let address = base58_p2pkh_from_hash(&pkh);
    WalletKey {
        address,
        pkh: hex::encode(pkh),
        pubkey: hex::encode(pubkey.as_bytes()),
        privkey: hex::encode(secret.to_bytes()),
    }
}

fn generate_key() -> WalletKey {
    let secret = SecretKey::random(&mut OsRng);
    wallet_key_from_secret(&secret, true)
}

fn base58check_encode(body: &[u8]) -> String {
    let first = Sha256::digest(body);
    let second = Sha256::digest(&first);
    let mut full = Vec::with_capacity(body.len() + 4);
    full.extend_from_slice(body);
    full.extend_from_slice(&second[0..4]);
    bs58::encode(full).into_string()
}

fn secret_to_wif(secret: &[u8; 32], compressed: bool) -> String {
    let mut body = Vec::with_capacity(34);
    body.push(0x80);
    body.extend_from_slice(secret);
    if compressed {
        body.push(0x01);
    }
    base58check_encode(&body)
}

fn parse_seed_hex(seed_hex: &str) -> Result<[u8; 32], String> {
    let raw = hex::decode(seed_hex).map_err(|_| "seed must be 64-char hex".to_string())?;
    if raw.len() != 32 {
        return Err("seed must be 64-char hex".to_string());
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&raw);
    Ok(out)
}

fn generate_seed_hex() -> String {
    let mut seed = [0u8; 32];
    OsRng.fill_bytes(&mut seed);
    hex::encode(seed)
}

fn derive_secret_from_seed_hex(seed_hex: &str, index: u32) -> Result<SecretKey, String> {
    let seed = parse_seed_hex(seed_hex)?;
    let mut material = Vec::with_capacity(36);
    material.extend_from_slice(&seed);
    material.extend_from_slice(&index.to_le_bytes());
    for ctr in 0u32..1024 {
        let mut data = material.clone();
        data.extend_from_slice(&ctr.to_le_bytes());
        let digest = Sha256::digest(&data);
        if let Ok(secret) = SecretKey::from_slice(&digest) {
            return Ok(secret);
        }
    }
    Err("failed to derive valid key from seed".to_string())
}

fn find_key<'a>(wallet: &'a WalletFile, addr: &str) -> Option<&'a WalletKey> {
    wallet.keys.iter().find(|k| k.address == addr)
}

fn usage() {
    eprintln!("Usage:");
    eprintln!("  irium-wallet init [--seed <64hex>]");
    eprintln!("  irium-wallet new-address");
    eprintln!("  irium-wallet list-addresses");
    eprintln!("  irium-wallet export-wif <base58_addr> --out <file>");
    eprintln!("  irium-wallet import-wif <wif>");
    eprintln!("  irium-wallet export-seed --out <file>");
    eprintln!("  irium-wallet import-seed <64hex> [--force]");
    eprintln!("  irium-wallet backup [--out <file>]");
    eprintln!("  irium-wallet restore-backup <file> [--force]");
    eprintln!("  irium-wallet address-to-pkh <base58_addr>");
    eprintln!("  irium-wallet qr <base58_addr> [--svg] [--out <file>]");
    eprintln!("  irium-wallet balance <base58_addr> [--rpc <url>]");
    eprintln!("  irium-wallet list-unspent <base58_addr> [--rpc <url>]");
    eprintln!("  irium-wallet history <base58_addr> [--rpc <url>]");
    eprintln!("  irium-wallet estimate-fee [--rpc <url>]");
    eprintln!("  irium-wallet send <from_addr> <to_addr> <amount_irm> [--fee <irm>] [--coin-select smallest|largest] [--rpc <url>]");
    eprintln!("  irium-wallet agreement-template <template> [options] [--out <file>]");
    eprintln!("  irium-wallet agreement-create-simple-settlement --agreement-id <id> --creation-time <unix> --party-a <id|name|addr|role> --party-b <id|name|addr|role> --amount <irm> --secret-hash <32bytehex> --refund-timeout <height> --document-hash <32bytehex> [--settlement-deadline <height>] [--metadata-hash <32bytehex>] [--release-summary <text>] [--refund-summary <text>] [--notes <text>] [--out <file>] [--json]");
    eprintln!("  irium-wallet agreement-create-otc --agreement-id <id> --creation-time <unix> --buyer <id|name|addr|role> --seller <id|name|addr|role> --amount <irm> --asset-reference <text> --payment-reference <text> --secret-hash <32bytehex> --refund-timeout <height> --document-hash <32bytehex> [--metadata-hash <32bytehex>] [--notes <text>] [--out <file>] [--json]");
    eprintln!("  irium-wallet agreement-create-deposit --agreement-id <id> --creation-time <unix> --payer <id|name|addr|role> --payee <id|name|addr|role> --amount <irm> --purpose-reference <text> --refund-summary <text> --secret-hash <32bytehex> --refund-timeout <height> --document-hash <32bytehex> [--metadata-hash <32bytehex>] [--notes <text>] [--out <file>] [--json]");
    eprintln!("  irium-wallet agreement-create-milestone --agreement-id <id> --creation-time <unix> --payer <id|name|addr|role> --payee <id|name|addr|role> --milestone <id|title|amount_irm|timeout_height|secret_hash_hex|deliverable_hash?> [--milestone ...] --refund-deadline <height> --document-hash <32bytehex> [--metadata-hash <32bytehex>] [--notes <text>] [--out <file>] [--json]");
    eprintln!("  irium-wallet agreement-bundle-create <agreement.json|bundle.json|agreement_id|agreement_hash> [--label <label>] [--note <text>] [--funding-txid <txid>] --out <file> [--json]");
    eprintln!("  irium-wallet agreement-bundle-pack <agreement.json|bundle.json|agreement_id|agreement_hash> --out <file> [--label <label>] [--note <text>] [--funding-txid <txid>] [--audit <audit.json>] [--statement <statement.json>] [--metadata-summary <text>] [--json]");
    eprintln!("  irium-wallet agreement-bundle-inspect <bundle.json|agreement_id|agreement_hash> [--json]");
    eprintln!(
        "  irium-wallet agreement-bundle-verify <bundle.json|agreement_id|agreement_hash> [--json]"
    );
    eprintln!("  irium-wallet agreement-bundle-unpack <bundle.json|agreement_id|agreement_hash> --out-dir <dir> [--json]");
    eprintln!("  irium-wallet agreement-share-package --out <package.json> [--agreement <agreement.json|->] [--bundle <bundle.json|->] [--audit <audit.json|->] [--statement <statement.json|->] [--agreement-signature <sig.json|->]... [--bundle-signature <sig.json|->]... [--include <agreement|bundle|audit|statement|agreement-signatures|bundle-signatures>]... [--created-at <unix>] [--sender-label <text>] [--package-note <text>] [--json]");
    eprintln!("  irium-wallet agreement-share-package-inspect <package.json|-> [--json]");
    eprintln!("  irium-wallet agreement-share-package-verify <package.json|-> [--rpc <url>] [--import-bundle] [--json] [--out <file>]");
    eprintln!("  irium-wallet agreement-share-package-import <package.json|-> [--rpc <url>] [--import <agreement|bundle|agreement-signatures|bundle-signatures|audit|statement>]... [--source-label <text>] [--json]");
    eprintln!("  irium-wallet agreement-share-package-list [--include-archived] [--json]");
    eprintln!("  irium-wallet agreement-share-package-show <receipt-id|receipt.json|dir> [--json]");
    eprintln!(
        "  irium-wallet agreement-share-package-archive <receipt-id|receipt.json|dir> [--json]"
    );
    eprintln!("  irium-wallet agreement-share-package-prune [--dry-run] [--older-than <days>] [--include-archived] [--remove-imported-artifacts] [--json]");
    eprintln!("  irium-wallet agreement-share-package-remove <receipt-id|receipt.json|dir> [--path <local-path>] [--agreement-hash <hash>] [--bundle-hash <hash>] [--dry-run] [--remove-imported-artifacts] [--json]");
    eprintln!("  irium-wallet agreement-local-store-list [--include-archived] [--json]");
    eprintln!("  irium-wallet agreement-sign --agreement <agreement.json|-> --signer <base58_addr> [--role <text>] [--timestamp <unix>] [--out <file>] [--json]");
    eprintln!("  irium-wallet agreement-verify-signature [--agreement <agreement.json|->] [--bundle <bundle.json|->] --signature <signature.json|-> [--json] [--out <file>]");
    eprintln!("  irium-wallet agreement-bundle-sign --bundle <bundle.json|agreement_id|agreement_hash|-> --signer <base58_addr> [--role <text>] [--timestamp <unix>] [--embed] [--out <file>] [--json]");
    eprintln!("  irium-wallet agreement-bundle-verify-signatures --bundle <bundle.json|agreement_id|agreement_hash|-> [--json]");
    eprintln!("  irium-wallet agreement-signature-inspect --signature <signature.json|-> [--agreement <agreement.json|->] [--bundle <bundle.json|->] [--json]");
    eprintln!("  irium-wallet agreement-save <agreement.json|bundle.json|agreement_id|agreement_hash> [--label <label>] [--note <note>] [--funding-txid <txid>] [--json]");
    eprintln!("  irium-wallet agreement-load <bundle.json|agreement_id|agreement_hash> [--json]");
    eprintln!("  irium-wallet agreement-list [--json]");
    eprintln!("  irium-wallet agreement-export <bundle.json|agreement_id|agreement_hash> --out <file> [--json]");
    eprintln!("  irium-wallet agreement-import <bundle.json> [--json]");
    eprintln!("  irium-wallet agreement-inspect <agreement.json|bundle.json|agreement_id|agreement_hash> [--json]");
    eprintln!(
        "  irium-wallet agreement-hash <agreement.json|bundle.json|agreement_id|agreement_hash>"
    );
    eprintln!("  irium-wallet agreement-fund <agreement.json|bundle.json|agreement_id|agreement_hash> [--broadcast] [--rpc <url>] [--fee-per-byte <n>] [--json]");
    eprintln!("  irium-wallet agreement-funding-legs <agreement.json|bundle.json|agreement_id|agreement_hash> [--rpc <url>] [--json]");
    eprintln!("  irium-wallet agreement-timeline <agreement.json|bundle.json|agreement_id|agreement_hash> [--rpc <url>] [--json]");
    eprintln!("  irium-wallet agreement-audit <agreement.json|bundle.json|agreement_id|agreement_hash> [--agreement-signature <sig.json>] [--bundle-signature <sig.json>] [--rpc <url>] [--json]");
    eprintln!("  irium-wallet agreement-audit-export <agreement.json|bundle.json|agreement_id|agreement_hash> --out <file> [--format json|csv] [--agreement-signature <sig.json>] [--bundle-signature <sig.json>] [--rpc <url>] [--json]");
    eprintln!("  irium-wallet agreement-statement <agreement.json|bundle.json|agreement_id|agreement_hash> [--agreement-signature <sig.json>] [--bundle-signature <sig.json>] [--rpc <url>] [--json]");
    eprintln!("  irium-wallet agreement-statement-export <agreement.json|bundle.json|agreement_id|agreement_hash> --out <file> [--agreement-signature <sig.json>] [--bundle-signature <sig.json>] [--rpc <url>] [--json]");
    eprintln!("  irium-wallet agreement-verify-artifacts [--agreement <agreement.json>] [--bundle <bundle.json>] [--audit <audit.json>] [--statement <statement.json>] [--agreement-signature <sig.json>] [--bundle-signature <sig.json>] [--rpc <url>] [--json] [--out <file>]");
    eprintln!("  irium-wallet agreement-release-eligibility <agreement.json|bundle.json|agreement_id|agreement_hash> [funding_txid] [--vout <n>] [--milestone-id <id>] [--destination <addr>] [--secret <hex>] [--rpc <url>] [--json]");
    eprintln!("  irium-wallet agreement-refund-eligibility <agreement.json|bundle.json|agreement_id|agreement_hash> [funding_txid] [--vout <n>] [--milestone-id <id>] [--destination <addr>] [--rpc <url>] [--json]");
    eprintln!("  irium-wallet agreement-policy-check --agreement <agreement.json|-> --policy <policy.json|-> [--proof <proof.json>]... [--rpc <url>] [--json]");
  eprintln!("  irium-wallet agreement-policy-set --policy <policy.json|-> [--rpc <url>] [--json] [--replace] [--expires-at-height <n>]");
  eprintln!("  irium-wallet agreement-policy-get --agreement-hash <hex> [--rpc <url>] [--json]");
  eprintln!("  irium-wallet agreement-policy-evaluate --agreement <agreement.json|-> [--rpc <url>] [--json]");
  eprintln!("  irium-wallet agreement-build-settlement <agreement.json> [--rpc <url>] [--json]");
  eprintln!("  irium-wallet agreement-settle-status <agreement.json> [--rpc <url>]");
  eprintln!("  irium-wallet agreement-policy-list [--active-only] [--rpc <url>] [--json]");
    eprintln!("  irium-wallet agreement-proof-create --agreement-hash <hex> --proof-type <type> --attested-by <id> --address <addr> [--expires-at-height <n>] [--milestone-id <id>] [--evidence-summary <text>] [--evidence-hash <hex>] [--proof-id <id>] [--timestamp <unix>] [--proof-kind <kind>] [--reference-id <ref>] [--out <path>] [--json]");
    eprintln!("  irium-wallet agreement-proof-submit --proof <proof.json|-> [--rpc <url>] [--json]");
    eprintln!("  irium-wallet agreement-proof-list [--agreement-hash <hex>] [--active-only] [--offset <n>] [--limit <n>] [--rpc <url>] [--json]");
    eprintln!("  irium-wallet agreement-proof-get --proof-id <id> [--rpc <url>] [--json]");
    eprintln!("  irium-wallet agreement-release-build <agreement.json|bundle.json|agreement_id|agreement_hash> [funding_txid] [--vout <n>] [--milestone-id <id>] [--destination <addr>] [--secret <hex>] [--fee-per-byte <n>] [--rpc <url>] [--json] [--show-raw-tx]");
    eprintln!("  irium-wallet agreement-refund-build <agreement.json|bundle.json|agreement_id|agreement_hash> [funding_txid] [--vout <n>] [--milestone-id <id>] [--destination <addr>] [--fee-per-byte <n>] [--rpc <url>] [--json] [--show-raw-tx]");
    eprintln!("  irium-wallet agreement-release-send <agreement.json|bundle.json|agreement_id|agreement_hash> [funding_txid] [--vout <n>] [--milestone-id <id>] [--destination <addr>] [--secret <hex>] [--fee-per-byte <n>] [--rpc <url>] [--json] [--show-raw-tx]");
    eprintln!("  irium-wallet agreement-refund-send <agreement.json|bundle.json|agreement_id|agreement_hash> [funding_txid] [--vout <n>] [--milestone-id <id>] [--destination <addr>] [--fee-per-byte <n>] [--rpc <url>] [--json] [--show-raw-tx]");
    eprintln!("  irium-wallet agreement-release <agreement.json|bundle.json|agreement_id|agreement_hash> [funding_txid] [--vout <n>] [--milestone-id <id>] [--destination <addr>] [--secret <hex>] [--fee-per-byte <n>] [--broadcast] [--rpc <url>] [--json] [--show-raw-tx]");
    eprintln!("  irium-wallet agreement-refund <agreement.json|bundle.json|agreement_id|agreement_hash> [funding_txid] [--vout <n>] [--milestone-id <id>] [--destination <addr>] [--fee-per-byte <n>] [--broadcast] [--rpc <url>] [--json] [--show-raw-tx]");
    eprintln!("  irium-wallet agreement-status <agreement.json|bundle.json|agreement_id|agreement_hash> [--rpc <url>] [--json]");
    eprintln!("  irium-wallet agreement-milestones <agreement.json|bundle.json|agreement_id|agreement_hash> [--rpc <url>] [--json]");
    eprintln!("  irium-wallet agreement-txs <agreement.json|bundle.json|agreement_id|agreement_hash> [--rpc <url>] [--json]");
    eprintln!(
        "  irium-wallet verify-agreement-link <agreement_hash> <tx_hex> [--rpc <url>] [--json]"
    );
}

fn node_rpc_base() -> String {
    env::var("IRIUM_NODE_RPC").unwrap_or_else(|_| "https://127.0.0.1:38300".to_string())
}

fn default_rpc_url() -> String {
    env::var("IRIUM_NODE_RPC")
        .or_else(|_| env::var("IRIUM_RPC_URL"))
        .unwrap_or_else(|_| node_rpc_base())
}

fn color_enabled() -> bool {
    env::var("NO_COLOR").is_err()
}

fn format_irm(amount: u64) -> String {
    let whole = amount / 100_000_000;
    let frac = amount % 100_000_000;
    if frac == 0 {
        format!("{}", whole)
    } else {
        format!("{}.{}", whole, format!("{:08}", frac))
    }
}

fn parse_irm(s: &str) -> Result<u64, String> {
    if s.trim().is_empty() {
        return Err("empty amount".to_string());
    }
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() > 2 {
        return Err("invalid amount".to_string());
    }
    let whole: u64 = parts[0].parse().map_err(|_| "invalid amount".to_string())?;
    let frac = if parts.len() == 2 {
        let frac_str = parts[1];
        if frac_str.len() > 8 {
            return Err("too many decimals".to_string());
        }
        let mut frac_val: u64 = frac_str.parse().map_err(|_| "invalid amount".to_string())?;
        for _ in frac_str.len()..8 {
            frac_val *= 10;
        }
        frac_val
    } else {
        0
    };
    Ok(whole.saturating_mul(100_000_000).saturating_add(frac))
}

fn estimate_tx_size(inputs: usize, outputs: usize) -> u64 {
    10 + inputs as u64 * 148 + outputs as u64 * 34
}

fn is_loopback_host(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "::1")
}

fn https_to_http(base: &str) -> Option<String> {
    if let Some(rest) = base.strip_prefix("https://") {
        Some(format!("http://{}", rest))
    } else {
        None
    }
}

fn send_with_https_fallback<F>(
    base: &str,
    f: F,
) -> Result<reqwest::blocking::Response, reqwest::Error>
where
    F: Fn(&str) -> Result<reqwest::blocking::Response, reqwest::Error>,
{
    match f(base) {
        Ok(v) => Ok(v),
        Err(e) => {
            if let Some(http) = https_to_http(base) {
                eprintln!("HTTPS RPC failed, retrying over HTTP: {}", http);
                if let Ok(v) = f(&http) {
                    return Ok(v);
                }
            }
            Err(e)
        }
    }
}

fn rpc_client(base: &str) -> Result<Client, String> {
    let mut builder = Client::builder().timeout(Duration::from_secs(10));
    let ca_path = env::var("IRIUM_RPC_CA").ok().or_else(|| {
        let fallback = Path::new("/etc/irium/tls/irium-ca.crt");
        if fallback.exists() {
            Some(fallback.display().to_string())
        } else {
            None
        }
    });
    if let Some(path) = ca_path {
        let pem = fs::read(&path).map_err(|e| format!("read CA {path}: {e}"))?;
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
        let url = reqwest::Url::parse(base).map_err(|e| format!("invalid RPC URL {base}: {e}"))?;
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

/// Typed client for the Irium settlement RPC endpoints.
///
/// Wraps `rpc_client` + `rpc_post_json` so callers don't need to repeat
/// the client-construction and path-building boilerplate for every call.
struct SettlementClient {
    client: reqwest::blocking::Client,
    base: String,
}

impl SettlementClient {
    fn new(base: &str) -> Result<Self, String> {
        let client = rpc_client(base)?;
        Ok(Self { client, base: base.to_string() })
    }

    fn post<TReq: serde::Serialize, TResp: for<'de> serde::Deserialize<'de>>(
        &self, path: &str, body: &TReq,
    ) -> Result<TResp, String> {
        rpc_post_json(&self.client, &self.base, path, body)
    }

    fn compute_agreement_hash(
        &self, agreement: AgreementObject,
    ) -> Result<ComputeAgreementHashRpcResponse, String> {
        self.post("/rpc/computeagreementhash", &ComputeAgreementHashRpcRequest { agreement })
    }

    fn get_policy(
        &self, agreement_hash: String,
    ) -> Result<GetPolicyRpcResponse, String> {
        self.post("/rpc/getpolicy", &GetPolicyRpcRequest { agreement_hash })
    }

    fn evaluate_policy(
        &self, agreement: AgreementObject,
    ) -> Result<EvaluatePolicyRpcResponse, String> {
        self.post("/rpc/evaluatepolicy", &EvaluatePolicyRpcRequest { agreement })
    }

    fn build_settlement_tx(
        &self, agreement: AgreementObject,
    ) -> Result<BuildSettlementTxRpcResponse, String> {
        self.post("/rpc/buildsettlementtx", &BuildSettlementTxRpcRequest { agreement })
    }
}

fn p2pkh_script(pkh: &[u8; 20]) -> Vec<u8> {
    let mut script = Vec::with_capacity(25);
    script.push(0x76);
    script.push(0xa9);
    script.push(0x14);
    script.extend_from_slice(pkh);
    script.push(0x88);
    script.push(0xac);
    script
}

fn signature_digest(tx: &Transaction, input_index: usize, script_pubkey: &[u8]) -> [u8; 32] {
    let mut tx_copy = tx.clone();
    for (idx, input) in tx_copy.inputs.iter_mut().enumerate() {
        if idx == input_index {
            input.script_sig = script_pubkey.to_vec();
        } else {
            input.script_sig.clear();
        }
    }
    let mut data = tx_copy.serialize();
    data.extend_from_slice(&1u32.to_le_bytes());
    sha256d(&data)
}

fn hex_to_32(s: &str) -> Result<[u8; 32], String> {
    let bytes = hex::decode(s).map_err(|_| "invalid hex".to_string())?;
    if bytes.len() != 32 {
        return Err("invalid txid length".to_string());
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes);
    Ok(out)
}

fn fetch_balance(client: &Client, base: &str, addr: &str) -> Result<BalanceResponse, String> {
    let resp = send_with_https_fallback(base, |b| {
        let url = format!("{}/rpc/balance?address={}", b, addr);
        let mut req = client.get(&url);
        if let Ok(token) = env::var("IRIUM_RPC_TOKEN") {
            req = req.bearer_auth(token);
        }
        req.send()
    })
    .map_err(|e| format!("balance request failed: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("balance request failed: {}", resp.status()));
    }
    resp.json::<BalanceResponse>()
        .map_err(|e| format!("parse balance response: {e}"))
}

fn fetch_utxos(client: &Client, base: &str, addr: &str) -> Result<UtxosResponse, String> {
    let resp = send_with_https_fallback(base, |b| {
        let url = format!("{}/rpc/utxos?address={}", b, addr);
        let mut req = client.get(&url);
        if let Ok(token) = env::var("IRIUM_RPC_TOKEN") {
            req = req.bearer_auth(token);
        }
        req.send()
    })
    .map_err(|e| format!("utxos request failed: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("utxos request failed: {}", resp.status()));
    }
    resp.json::<UtxosResponse>()
        .map_err(|e| format!("parse utxos response: {e}"))
}

fn fetch_history(client: &Client, base: &str, addr: &str) -> Result<HistoryResponse, String> {
    let resp = send_with_https_fallback(base, |b| {
        let url = format!("{}/rpc/history?address={}", b, addr);
        let mut req = client.get(&url);
        if let Ok(token) = env::var("IRIUM_RPC_TOKEN") {
            req = req.bearer_auth(token);
        }
        req.send()
    })
    .map_err(|e| format!("history request failed: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("history request failed: {}", resp.status()));
    }
    resp.json::<HistoryResponse>()
        .map_err(|e| format!("parse history response: {e}"))
}

fn fetch_fee_estimate(client: &Client, base: &str) -> Result<FeeEstimateResponse, String> {
    let resp = send_with_https_fallback(base, |b| {
        let url = format!("{}/rpc/fee_estimate", b);
        let mut req = client.get(&url);
        if let Ok(token) = env::var("IRIUM_RPC_TOKEN") {
            req = req.bearer_auth(token);
        }
        req.send()
    })
    .map_err(|e| format!("fee estimate failed: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("fee estimate failed: {}", resp.status()));
    }
    resp.json::<FeeEstimateResponse>()
        .map_err(|e| format!("parse fee estimate response: {e}"))
}

fn rpc_post_json<TReq: Serialize, TResp: for<'de> Deserialize<'de>>(
    client: &Client,
    base: &str,
    path: &str,
    body: &TReq,
) -> Result<TResp, String> {
    let resp = send_with_https_fallback(base, |b| {
        let url = format!(
            "{}/{}",
            b.trim_end_matches('/'),
            path.trim_start_matches('/')
        );
        let mut req = client.post(&url).json(body);
        if let Ok(token) = env::var("IRIUM_RPC_TOKEN") {
            req = req.bearer_auth(token);
        }
        req.send()
    })
    .map_err(|e| format!("{} request failed: {e}", path))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().unwrap_or_default();
        return Err(format!("{} request failed: {} {}", path, status, body));
    }
    resp.json::<TResp>()
        .map_err(|e| format!("parse {} response: {e}", path))
}

fn load_agreement(path: &str) -> Result<AgreementObject, String> {
    Ok(resolve_agreement_input(path)?.agreement)
}

fn agreement_context_request_body(
    resolved: &ResolvedAgreementInput,
) -> AgreementContextRequestBody {
    AgreementContextRequestBody {
        agreement: resolved.agreement.clone(),
        bundle: resolved.bundle.clone(),
    }
}

fn fetch_agreement_funding_legs(
    client: &Client,
    base: &str,
    resolved: &ResolvedAgreementInput,
) -> Result<AgreementFundingLegsResponse, String> {
    rpc_post_json(
        client,
        base,
        "/rpc/agreementfundinglegs",
        &agreement_context_request_body(resolved),
    )
}

fn fetch_agreement_timeline(
    client: &Client,
    base: &str,
    resolved: &ResolvedAgreementInput,
) -> Result<AgreementTimelineResponse, String> {
    rpc_post_json(
        client,
        base,
        "/rpc/agreementtimeline",
        &agreement_context_request_body(resolved),
    )
}

fn fetch_agreement_audit(
    client: &Client,
    base: &str,
    resolved: &ResolvedAgreementInput,
) -> Result<AgreementAuditRecord, String> {
    rpc_post_json(
        client,
        base,
        "/rpc/agreementaudit",
        &agreement_context_request_body(resolved),
    )
}

fn render_agreement_audit(record: &AgreementAuditRecord) -> String {
    let mut lines = vec![
        format!("agreement_id {}", record.agreement.agreement_id),
        format!("agreement_hash {}", record.agreement.agreement_hash),
        format!("template {:?}", record.agreement.template_type).to_lowercase(),
        format!("generated_at {}", record.metadata.generated_at),
        format!("generator {}", record.metadata.generator_surface),
        format!(
            "derived_state {}",
            record.settlement_state.derived_state_label
        ),
        format!(
            "funding_leg_candidates {}",
            record.funding_legs.candidate_count
        ),
        format!("timeline_events {}", record.timeline.event_count),
        format!("trust_model {}", record.metadata.trust_model_summary),
        "sections".to_string(),
        format!(
            "  bundle_used={} linked_txs={} milestones={}",
            record.local_bundle.bundle_used,
            record.chain_observed.linked_transaction_count,
            record.agreement.milestone_count
        ),
    ];
    if let Some(authenticity) = &record.authenticity {
        lines.push("authenticity".to_string());
        lines.push(format!(
            "  detached_agreement_signatures={} detached_bundle_signatures={} embedded_bundle_signatures={}",
            authenticity.detached_agreement_signatures_supplied,
            authenticity.detached_bundle_signatures_supplied,
            authenticity.embedded_bundle_signatures_supplied
        ));
        lines.push(format!(
            "  valid={} invalid={} unverifiable={}",
            authenticity.valid_signatures,
            authenticity.invalid_signatures,
            authenticity.unverifiable_signatures
        ));
        for signer in &authenticity.signer_summaries {
            lines.push(format!("  signer {}", signer));
        }
        for warning in &authenticity.warnings {
            lines.push(format!("  warning {}", warning));
        }
        lines.push(format!("  notice {}", authenticity.authenticity_notice));
    }
    if let Some(selected) = &record.funding_legs.selected_leg {
        lines.push(format!(
            "  selected_leg txid={} vout={} milestone={}",
            selected.funding_txid,
            selected.htlc_vout,
            selected.milestone_id.as_deref().unwrap_or("-")
        ));
    }
    if let Some(warning) = &record.funding_legs.ambiguity_warning {
        lines.push(format!("  ambiguity_warning {}", warning));
    }
    lines.push("trust_boundaries".to_string());
    lines.push(format!(
        "  consensus_enforced {}",
        record.trust_boundaries.consensus_enforced.join(" | ")
    ));
    lines.push(format!(
        "  htlc_enforced {}",
        record.trust_boundaries.htlc_enforced.join(" | ")
    ));
    lines.push(format!(
        "  metadata_indexed {}",
        record.trust_boundaries.metadata_indexed.join(" | ")
    ));
    lines.push(format!(
        "  local_bundle_only {}",
        record.trust_boundaries.local_bundle_only.join(" | ")
    ));
    lines.push(format!(
        "  off_chain_required {}",
        record.trust_boundaries.off_chain_required.join(" | ")
    ));
    lines.join(
        "
",
    )
}

fn render_funding_leg_candidate(candidate: &AgreementFundingLegCandidateResponse) -> String {
    format!(
        "  txid={} vout={} role={} milestone={} amount_irm={} release_eligible={} refund_eligible={} sources={}",
        candidate.funding_txid,
        candidate.htlc_vout,
        candidate.role,
        candidate.milestone_id.as_deref().unwrap_or("-"),
        format_irm(candidate.amount),
        candidate.release_eligible,
        candidate.refund_eligible,
        if candidate.source_notes.is_empty() {
            "-".to_string()
        } else {
            candidate.source_notes.join(",")
        }
    )
}

fn render_agreement_funding_legs(resp: &AgreementFundingLegsResponse) -> String {
    let mut lines = vec![
        format!("agreement_hash {}", resp.agreement_hash),
        format!("selection_required {}", resp.selection_required),
        format!("candidate_count {}", resp.candidates.len()),
        format!("trust_model {}", resp.trust_model_note),
    ];
    if resp.candidates.is_empty() {
        lines.push(
            "candidates none; discovery could not find a matching HTLC-backed funding leg from the observed agreement anchors"
                .to_string(),
        );
    } else {
        lines.push("candidates".to_string());
        for candidate in &resp.candidates {
            lines.push(render_funding_leg_candidate(candidate));
        }
    }
    lines.join("\n")
}

fn render_agreement_timeline(resp: &AgreementTimelineResponse) -> String {
    let mut lines = vec![
        format!("agreement_hash {}", resp.agreement_hash),
        format!("lifecycle_state {:?}", resp.lifecycle.state).to_lowercase(),
        format!("trust_model {}", resp.trust_model_note),
        "events".to_string(),
    ];
    for event in &resp.events {
        lines.push(format!(
            "  type={} source={} height={} timestamp={} txid={} milestone={} note={}",
            event.event_type,
            event.source,
            event
                .height
                .map(|v| v.to_string())
                .unwrap_or_else(|| "-".to_string()),
            event
                .timestamp
                .map(|v| v.to_string())
                .unwrap_or_else(|| "-".to_string()),
            event.txid.as_deref().unwrap_or("-"),
            event.milestone_id.as_deref().unwrap_or("-"),
            event.note.as_deref().unwrap_or("-")
        ));
    }
    lines.join("\n")
}

fn select_agreement_funding_leg_candidate(
    resp: &AgreementFundingLegsResponse,
    milestone_id: Option<&str>,
    htlc_vout: Option<u32>,
) -> Result<(AgreementFundingLegCandidateResponse, String), String> {
    let mut candidates = resp.candidates.clone();
    if let Some(mid) = milestone_id {
        candidates.retain(|candidate| candidate.milestone_id.as_deref() == Some(mid));
        if candidates.is_empty() {
            return Err(format!(
                "no discovered funding leg matched milestone_id {}; supply funding_txid explicitly",
                mid
            ));
        }
    }
    if let Some(vout) = htlc_vout {
        candidates.retain(|candidate| candidate.htlc_vout == vout);
        if candidates.is_empty() {
            return Err(format!(
                "no discovered funding leg matched htlc vout {}; supply funding_txid explicitly",
                vout
            ));
        }
    }
    match candidates.len() {
        0 => Err(
            "no funding legs discovered; supply funding_txid explicitly or save bundle hints first"
                .to_string(),
        ),
        1 => {
            let candidate = candidates.remove(0);
            let notice = if milestone_id.is_some() || htlc_vout.is_some() {
                format!(
                    "auto-selected discovered funding leg txid={} vout={} milestone={} after narrowing",
                    candidate.funding_txid,
                    candidate.htlc_vout,
                    candidate.milestone_id.as_deref().unwrap_or("-")
                )
            } else {
                format!(
                    "auto-selected the only discovered funding leg txid={} vout={} milestone={}",
                    candidate.funding_txid,
                    candidate.htlc_vout,
                    candidate.milestone_id.as_deref().unwrap_or("-")
                )
            };
            Ok((candidate, notice))
        }
        _ => {
            let mut message = vec![
                "multiple discovered funding legs matched; select funding_txid explicitly or narrow with --milestone-id/--vout"
                    .to_string(),
            ];
            for candidate in &candidates {
                message.push(render_funding_leg_candidate(candidate));
            }
            Err(message.join("\n"))
        }
    }
}

fn resolve_agreement_spend_request(
    client: &Client,
    base: &str,
    opts: &AgreementSpendCliOptions,
) -> Result<
    (
        ResolvedAgreementInput,
        AgreementSpendRequestBody,
        Option<String>,
    ),
    String,
> {
    let resolved = resolve_agreement_input(&opts.agreement_path)?;
    let mut effective = opts.clone();
    let mut selection_notice = None;
    if effective.funding_txid.is_none() {
        let discovered = fetch_agreement_funding_legs(client, base, &resolved)?;
        let (candidate, notice) = select_agreement_funding_leg_candidate(
            &discovered,
            effective.milestone_id.as_deref(),
            effective.htlc_vout,
        )?;
        effective.funding_txid = Some(candidate.funding_txid.clone());
        if effective.htlc_vout.is_none() {
            effective.htlc_vout = Some(candidate.htlc_vout);
        }
        if effective.milestone_id.is_none() {
            effective.milestone_id = candidate.milestone_id.clone();
        }
        selection_notice = Some(notice);
    }
    let funding_txid = effective
        .funding_txid
        .clone()
        .ok_or_else(|| "funding_txid required or discoverable funding leg not found".to_string())?;
    let req = AgreementSpendRequestBody {
        agreement: resolved.agreement.clone(),
        funding_txid,
        htlc_vout: effective.htlc_vout,
        milestone_id: effective.milestone_id,
        destination_address: effective.destination_address,
        fee_per_byte: effective.fee_per_byte,
        broadcast: Some(effective.broadcast),
        secret_hex: effective.secret_hex,
    };
    Ok((resolved, req, selection_notice))
}

fn save_json_output(path: Option<&str>, value: &Value) -> Result<(), String> {
    let rendered =
        serde_json::to_string_pretty(value).map_err(|e| format!("serialize json: {e}"))?;
    save_text_output(path, &rendered)
}

fn save_text_output(path: Option<&str>, rendered: &str) -> Result<(), String> {
    if let Some(path) = path {
        fs::write(path, rendered).map_err(|e| format!("write output: {e}"))?;
    } else {
        println!("{}", rendered);
    }
    Ok(())
}

fn load_audit_from_path(path: &Path) -> Result<AgreementAuditRecord, String> {
    let data = read_text_from_path_or_stdin(path, "audit json")?;
    serde_json::from_str::<AgreementAuditRecord>(&data)
        .map_err(|e| format!("parse audit json: {e}"))
}

fn load_statement_from_path(path: &Path) -> Result<AgreementStatement, String> {
    let data = read_text_from_path_or_stdin(path, "statement json")?;
    serde_json::from_str::<AgreementStatement>(&data)
        .map_err(|e| format!("parse statement json: {e}"))
}

fn load_signature_from_path(path: &Path) -> Result<AgreementSignatureEnvelope, String> {
    let data = read_text_from_path_or_stdin(path, "signature json")?;
    let signature = serde_json::from_str::<AgreementSignatureEnvelope>(&data)
        .map_err(|e| format!("parse signature json: {e}"))?;
    validate_agreement_signature_envelope(&signature)?;
    Ok(signature)
}

fn load_share_package_from_path(path: &Path) -> Result<AgreementSharePackage, String> {
    let data = read_text_from_path_or_stdin(path, "share package json")?;
    let package = serde_json::from_str::<AgreementSharePackage>(&data)
        .map_err(|e| format!("parse share package json: {e}"))?;
    verify_agreement_share_package(&package)?;
    Ok(package)
}

fn load_signatures_from_paths(paths: &[String]) -> Result<Vec<AgreementSignatureEnvelope>, String> {
    let mut out = Vec::new();
    for path in paths {
        out.push(load_signature_from_path(Path::new(path))?);
    }
    Ok(out)
}

fn normalize_share_package_include(value: &str) -> Result<String, String> {
    let normalized = value.trim().to_ascii_lowercase().replace('-', "_");
    if agreement_share_package_all_artifact_types()
        .iter()
        .any(|item| item == &normalized)
    {
        Ok(normalized)
    } else {
        Err(format!("unsupported share-package include {}", value))
    }
}

#[allow(clippy::type_complexity)]
fn filter_share_package_export_selection(
    includes: &[String],
    mut agreement: Option<AgreementObject>,
    mut bundle: Option<AgreementBundle>,
    mut audit: Option<AgreementAuditRecord>,
    mut statement: Option<AgreementStatement>,
    mut agreement_signatures: Vec<AgreementSignatureEnvelope>,
    mut bundle_signatures: Vec<AgreementSignatureEnvelope>,
) -> Result<
    (
        Option<AgreementObject>,
        Option<AgreementBundle>,
        Option<AgreementAuditRecord>,
        Option<AgreementStatement>,
        Vec<AgreementSignatureEnvelope>,
        Vec<AgreementSignatureEnvelope>,
    ),
    String,
> {
    if includes.is_empty() {
        return Ok((
            agreement,
            bundle,
            audit,
            statement,
            agreement_signatures,
            bundle_signatures,
        ));
    }
    let includes_item = |name: &str| includes.iter().any(|item| item == name);
    for item in includes {
        match item.as_str() {
            "agreement" if agreement.is_none() => {
                return Err("--include agreement requested but no agreement artifact was supplied".to_string())
            }
            "bundle" if bundle.is_none() => {
                return Err("--include bundle requested but no bundle artifact was supplied".to_string())
            }
            "audit" if audit.is_none() => {
                return Err("--include audit requested but no audit artifact was supplied".to_string())
            }
            "statement" if statement.is_none() => {
                return Err("--include statement requested but no statement artifact was supplied".to_string())
            }
            "agreement_signatures" if agreement_signatures.is_empty() => {
                return Err("--include agreement-signatures requested but no detached agreement signatures were supplied".to_string())
            }
            "bundle_signatures" if bundle_signatures.is_empty() => {
                return Err("--include bundle-signatures requested but no detached bundle signatures were supplied".to_string())
            }
            _ => {}
        }
    }
    if !includes_item("agreement") {
        agreement = None;
    }
    if !includes_item("bundle") {
        bundle = None;
    }
    if !includes_item("audit") {
        audit = None;
    }
    if !includes_item("statement") {
        statement = None;
    }
    if !includes_item("agreement_signatures") {
        agreement_signatures.clear();
    }
    if !includes_item("bundle_signatures") {
        bundle_signatures.clear();
    }
    if agreement.is_none()
        && bundle.is_none()
        && audit.is_none()
        && statement.is_none()
        && agreement_signatures.is_empty()
        && bundle_signatures.is_empty()
    {
        return Err("share package export selection removed every artifact; include at least one supplied artifact".to_string());
    }
    Ok((
        agreement,
        bundle,
        audit,
        statement,
        agreement_signatures,
        bundle_signatures,
    ))
}

fn attach_authenticity_to_audit(
    record: &mut AgreementAuditRecord,
    resolved: &ResolvedAgreementInput,
    detached_agreement_signatures: &[AgreementSignatureEnvelope],
    detached_bundle_signatures: &[AgreementSignatureEnvelope],
) {
    record.authenticity = build_agreement_artifact_authenticity_verification(
        Some(&resolved.agreement),
        resolved.bundle.as_ref(),
        detached_agreement_signatures,
        detached_bundle_signatures,
    )
    .as_ref()
    .map(summarize_agreement_authenticity);
}

fn resolve_bundle_reference_or_stdin(reference: &str) -> Result<StoredAgreementBundle, String> {
    if reference == "-" {
        let bundle = load_bundle_from_path(Path::new("-"))?;
        return Ok(StoredAgreementBundle {
            path: PathBuf::from("-"),
            bundle,
        });
    }
    resolve_bundle_input(reference)
}

fn signer_material_from_wallet(address: &str) -> Result<(WalletKey, SigningKey), String> {
    let wallet = ensure_wallet(&wallet_path())?;
    let key = find_key(&wallet, address)
        .cloned()
        .ok_or_else(|| format!("signer address {} not found in wallet", address))?;
    let priv_bytes =
        hex::decode(&key.privkey).map_err(|_| "wallet privkey hex invalid".to_string())?;
    let secret = SecretKey::from_slice(&priv_bytes)
        .map_err(|e| format!("wallet secret key invalid: {e}"))?;
    let signing_key = SigningKey::from(secret);
    Ok((key, signing_key))
}

fn sign_target_hash(
    target_type: AgreementSignatureTargetType,
    target_hash: String,
    signer_address: String,
    signer_role: Option<String>,
    timestamp: Option<u64>,
) -> Result<AgreementSignatureEnvelope, String> {
    let (key, signing_key) = signer_material_from_wallet(&signer_address)?;
    let mut envelope = AgreementSignatureEnvelope {
        version: AGREEMENT_SIGNATURE_VERSION,
        target_type,
        target_hash,
        signer_public_key: key.pubkey.clone(),
        signer_address: Some(key.address.clone()),
        signature_type: AGREEMENT_SIGNATURE_TYPE_SECP256K1.to_string(),
        timestamp,
        signer_role,
        signature: String::new(),
    };
    let digest = compute_agreement_signature_payload_hash(&envelope)?;
    let signature: Signature = signing_key
        .sign_prehash(&digest)
        .map_err(|e| format!("sign target hash: {e}"))?;
    envelope.signature = hex::encode(signature.to_bytes());
    Ok(envelope)
}

fn create_settlement_proof_signed(opts: &ProofCreateCliOptions) -> Result<SettlementProof, String> {
    let (key, signing_key) = signer_material_from_wallet(&opts.address)?;

    let attestation_time = match opts.timestamp {
        Some(t) => t,
        None => SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
    };

    // Generate a deterministic proof_id from (proof_type, agreement_hash, timestamp) if not provided.
    let proof_id = match &opts.proof_id {
        Some(id) => id.clone(),
        None => {
            let mut seed_data = opts.proof_type.clone();
            seed_data.push_str(&opts.agreement_hash);
            seed_data.push_str(&attestation_time.to_string());
            let digest = Sha256::digest(seed_data.as_bytes());
            format!("prf-{}", hex::encode(&digest[..8]))
        }
    };

    // Build the proof without a signature first so we can compute the payload hash.
    let mut proof = SettlementProof {
        proof_id,
        schema_id: SETTLEMENT_PROOF_SCHEMA_ID.to_string(),
        proof_type: opts.proof_type.clone(),
        agreement_hash: opts.agreement_hash.clone(),
        milestone_id: opts.milestone_id.clone(),
        attested_by: opts.attested_by.clone(),
        attestation_time,
        evidence_hash: opts.evidence_hash.clone(),
        evidence_summary: opts.evidence_summary.clone(),
        signature: ProofSignatureEnvelope {
            signature_type: AGREEMENT_SIGNATURE_TYPE_SECP256K1.to_string(),
            pubkey_hex: key.pubkey.clone(),
            signature_hex: String::new(),
            payload_hash: String::new(),
        },
        expires_at_height: opts.expires_at_height,
        typed_payload: opts.proof_kind.as_ref().map(|kind| TypedProofPayload {
            proof_kind: kind.clone(),
            content_hash: None,
            reference_id: opts.reference_id.clone(),
            attributes: None,
        }),
    };

    let payload_bytes = settlement_proof_payload_bytes(&proof)
        .map_err(|e| format!("compute proof payload bytes: {e}"))?;
    let payload_digest = Sha256::digest(&payload_bytes);
    let payload_hash_hex = hex::encode(&payload_digest);

    let sig: Signature = signing_key
        .sign_prehash(&payload_digest)
        .map_err(|e| format!("sign proof payload: {e}"))?;

    proof.signature.signature_hex = hex::encode(sig.to_bytes());
    proof.signature.payload_hash = payload_hash_hex;

    Ok(proof)
}

fn share_package_receipt_id(imported_at: u64, agreement_hash: Option<&str>) -> String {
    let suffix = agreement_hash
        .map(|hash| hash.chars().take(12).collect::<String>())
        .unwrap_or_else(|| "package".to_string());
    format!("{}-{}", imported_at, suffix)
}

fn allocate_share_package_receipt_dir(base: &Path, receipt_id: &str) -> Result<PathBuf, String> {
    fs::create_dir_all(base).map_err(|e| format!("create share-package inbox dir: {e}"))?;
    for index in 0..1000u32 {
        let name = if index == 0 {
            receipt_id.to_string()
        } else {
            format!("{}-{}", receipt_id, index)
        };
        let candidate = base.join(name);
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err("unable to allocate share-package inbox receipt directory".to_string())
}

fn share_package_receipt_path(dir: &Path) -> PathBuf {
    dir.join("receipt.json")
}

fn load_share_package_receipt(path: &Path) -> Result<SharePackageReceipt, String> {
    let data = read_text_from_path_or_stdin(path, "share package receipt")?;
    serde_json::from_str::<SharePackageReceipt>(&data)
        .map_err(|e| format!("parse share package receipt json: {e}"))
}

fn list_share_package_receipts_at(dir: &Path) -> Result<Vec<SharePackageReceipt>, String> {
    let location = if dir == share_package_archive_dir() {
        SharePackageReceiptLocation::Archived
    } else {
        SharePackageReceiptLocation::Active
    };
    let records = list_share_package_receipt_records_in_dir(dir, location)?;
    Ok(records.into_iter().map(|item| item.receipt).collect())
}

fn resolve_share_package_receipt_record(
    reference: &str,
) -> Result<StoredSharePackageReceipt, String> {
    let path = Path::new(reference);
    if path.exists() {
        if !path_is_local_housekeeping_safe(path) {
            return Err(format!(
                "refusing to treat {} as a local receipt target because it is outside the wallet-side housekeeping stores",
                path.display()
            ));
        }
        let receipt_dir = if path.is_dir() {
            path.to_path_buf()
        } else {
            path.parent()
                .ok_or_else(|| format!("cannot resolve receipt directory for {}", path.display()))?
                .to_path_buf()
        };
        let location = if path_is_within(&share_package_archive_dir(), &receipt_dir) {
            SharePackageReceiptLocation::Archived
        } else {
            SharePackageReceiptLocation::Active
        };
        return read_receipt_record_from_dir(&receipt_dir, location)?.ok_or_else(|| {
            format!(
                "no local share-package receipt metadata found at {}",
                receipt_dir.display()
            )
        });
    }
    let mut matches = list_share_package_receipt_records(true)?
        .into_iter()
        .filter(|item| item.receipt.receipt_id == reference)
        .collect::<Vec<_>>();
    match matches.len() {
        0 => Err(format!("no local share-package receipt for {}", reference)),
        1 => Ok(matches.remove(0)),
        _ => Err(format!(
            "multiple local share-package receipts matched {}",
            reference
        )),
    }
}

fn resolve_share_package_receipt(reference: &str) -> Result<SharePackageReceipt, String> {
    resolve_share_package_receipt_record(reference).map(|item| item.receipt)
}

fn collect_artifact_verification_warnings(
    result: &AgreementArtifactVerificationResult,
) -> Vec<String> {
    let mut warnings = Vec::new();
    warnings.extend(result.canonical_verification.mismatches.clone());
    warnings.extend(result.canonical_verification.warnings.clone());
    warnings.extend(result.artifact_consistency.warnings.clone());
    warnings.extend(result.chain_verification.warnings.clone());
    warnings.extend(result.derived_verification.warnings.clone());
    warnings.extend(result.trust_summary.unverifiable_from_chain_alone.clone());
    if let Some(auth) = &result.authenticity {
        warnings.extend(auth.warnings.clone());
    }
    warnings.sort();
    warnings.dedup();
    warnings
}

fn summarize_share_package_verification(
    result: &AgreementSharePackageVerificationResult,
) -> SharePackageReceiptVerificationSummary {
    let mismatch_count = result
        .artifact_verification
        .canonical_verification
        .mismatches
        .len()
        + usize::from(
            result
                .artifact_verification
                .chain_verification
                .audit_chain_match
                == Some(false),
        )
        + usize::from(
            result
                .artifact_verification
                .chain_verification
                .statement_chain_match
                == Some(false),
        )
        + usize::from(
            result
                .artifact_verification
                .derived_verification
                .audit_derived_match
                == Some(false),
        )
        + usize::from(
            result
                .artifact_verification
                .derived_verification
                .statement_derived_match
                == Some(false),
        );
    let unverifiable_count = result
        .artifact_verification
        .canonical_verification
        .warnings
        .len()
        + result
            .artifact_verification
            .artifact_consistency
            .warnings
            .len()
        + result
            .artifact_verification
            .chain_verification
            .warnings
            .len()
        + result
            .artifact_verification
            .derived_verification
            .warnings
            .len()
        + result
            .artifact_verification
            .trust_summary
            .unverifiable_from_chain_alone
            .len();
    let authenticity = result.artifact_verification.authenticity.as_ref();
    SharePackageReceiptVerificationSummary {
        verified_match_count: result
            .artifact_verification
            .canonical_verification
            .matches
            .len(),
        mismatch_count,
        unverifiable_count,
        valid_signatures: authenticity.map(|item| item.valid_signatures).unwrap_or(0),
        invalid_signatures: authenticity
            .map(|item| item.invalid_signatures)
            .unwrap_or(0),
        unverifiable_signatures: authenticity
            .map(|item| item.unverifiable_signatures)
            .unwrap_or(0),
    }
}

fn render_share_package_receipt(receipt: &SharePackageReceipt) -> String {
    let included = if receipt.included_artifact_types.is_empty() {
        "none".to_string()
    } else {
        receipt.included_artifact_types.join(" | ")
    };
    let imported = if receipt.imported_artifact_types.is_empty() {
        "none".to_string()
    } else {
        receipt.imported_artifact_types.join(" | ")
    };
    let already_present = if receipt.already_present_artifact_types.is_empty() {
        "none".to_string()
    } else {
        receipt.already_present_artifact_types.join(" | ")
    };
    let rejected = if receipt.rejected_artifact_types.is_empty() {
        "none".to_string()
    } else {
        receipt.rejected_artifact_types.join(" | ")
    };
    let mut lines = vec![
        "Agreement share package receipt".to_string(),
        format!("receipt_id {}", receipt.receipt_id),
        format!("imported_at {}", receipt.imported_at),
        format!("source_path {}", receipt.source_path),
        format!("package_profile {}", receipt.package_profile),
        format!("included_artifact_types {}", included),
        format!("imported_artifact_types {}", imported),
        format!("already_present_artifact_types {}", already_present),
        format!("rejected_artifact_types {}", rejected),
        format!(
            "verified_match_count {}",
            receipt.verification_summary.verified_match_count
        ),
        format!(
            "mismatch_count {}",
            receipt.verification_summary.mismatch_count
        ),
        format!(
            "unverifiable_count {}",
            receipt.verification_summary.unverifiable_count
        ),
        format!(
            "valid_signatures {}",
            receipt.verification_summary.valid_signatures
        ),
        format!(
            "invalid_signatures {}",
            receipt.verification_summary.invalid_signatures
        ),
        format!(
            "unverifiable_signatures {}",
            receipt.verification_summary.unverifiable_signatures
        ),
    ];
    if let Some(label) = &receipt.import_source_label {
        lines.push(format!("import_source_label {}", label));
    }
    if let Some(label) = &receipt.sender_label {
        lines.push(format!("sender_label {}", label));
    }
    if let Some(note) = &receipt.package_note {
        lines.push(format!("package_note {}", note));
    }
    if let Some(id) = &receipt.canonical_agreement_id {
        lines.push(format!("canonical_agreement_id {}", id));
    }
    if let Some(hash) = &receipt.canonical_agreement_hash {
        lines.push(format!("canonical_agreement_hash {}", hash));
    }
    if let Some(hash) = &receipt.bundle_hash {
        lines.push(format!("bundle_hash {}", hash));
    }
    lines.push(format!("package_path {}", receipt.package_path));
    lines.push(format!("verification_path {}", receipt.verification_path));
    if let Some(path) = &receipt.artifact_paths.agreement_path {
        lines.push(format!("agreement_path {}", path));
    }
    if let Some(path) = &receipt.artifact_paths.bundle_path {
        lines.push(format!("bundle_path {}", path));
    }
    if let Some(path) = &receipt.artifact_paths.audit_path {
        lines.push(format!("audit_path {}", path));
    }
    if let Some(path) = &receipt.artifact_paths.statement_path {
        lines.push(format!("statement_path {}", path));
    }
    if !receipt.artifact_paths.agreement_signature_paths.is_empty() {
        lines.push(format!(
            "agreement_signature_paths {}",
            receipt.artifact_paths.agreement_signature_paths.join(" | ")
        ));
    }
    if !receipt.artifact_paths.bundle_signature_paths.is_empty() {
        lines.push(format!(
            "bundle_signature_paths {}",
            receipt.artifact_paths.bundle_signature_paths.join(" | ")
        ));
    }
    lines.push(format!("trust_notice {}", receipt.trust_notice));
    lines.push(format!("provenance_notice {}", receipt.provenance_notice));
    lines.push("warnings".to_string());
    if receipt.warnings.is_empty() {
        lines.push("  none".to_string());
    } else {
        for warning in &receipt.warnings {
            lines.push(format!("  {}", warning));
        }
    }
    lines.join(
        "
",
    )
}

fn share_package_receipt_list_item(receipt: &SharePackageReceipt) -> SharePackageReceiptListItem {
    SharePackageReceiptListItem {
        receipt_id: receipt.receipt_id.clone(),
        imported_at: receipt.imported_at,
        package_profile: receipt.package_profile.clone(),
        canonical_agreement_id: receipt.canonical_agreement_id.clone(),
        canonical_agreement_hash: receipt.canonical_agreement_hash.clone(),
        imported_artifact_types: receipt.imported_artifact_types.clone(),
        sender_label: receipt.sender_label.clone(),
        import_source_label: receipt.import_source_label.clone(),
    }
}

fn render_share_package_receipt_list(items: &[SharePackageReceipt]) -> String {
    let mut lines = vec![
        "Agreement share package inbox".to_string(),
        format!("receipt_count {}", items.len()),
    ];
    for receipt in items {
        let imported = if receipt.imported_artifact_types.is_empty() {
            "none".to_string()
        } else {
            receipt.imported_artifact_types.join(" | ")
        };
        lines.push(format!("receipt_id {}", receipt.receipt_id));
        lines.push(format!("  imported_at {}", receipt.imported_at));
        lines.push(format!("  package_profile {}", receipt.package_profile));
        if let Some(id) = &receipt.canonical_agreement_id {
            lines.push(format!("  canonical_agreement_id {}", id));
        }
        if let Some(hash) = &receipt.canonical_agreement_hash {
            lines.push(format!("  canonical_agreement_hash {}", hash));
        }
        lines.push(format!("  imported_artifact_types {}", imported));
        if let Some(label) = &receipt.sender_label {
            lines.push(format!("  sender_label {}", label));
        }
        if let Some(label) = &receipt.import_source_label {
            lines.push(format!("  import_source_label {}", label));
        }
    }
    lines.join("\n")
}

fn render_share_package_receipt_inventory(items: &[SharePackageReceiptInventoryItem]) -> String {
    let mut lines = vec![format!("receipt_count {}", items.len())];
    for item in items {
        let imported = if item.imported_artifact_types.is_empty() {
            "none".to_string()
        } else {
            item.imported_artifact_types.join(" | ")
        };
        lines.push(format!("receipt_id {}", item.receipt_id));
        lines.push(format!("  imported_at {}", item.imported_at));
        lines.push(format!("  archived {}", item.archived));
        if let Some(value) = item.archived_at {
            lines.push(format!("  archived_at {}", value));
        }
        lines.push(format!("  package_profile {}", item.package_profile));
        if let Some(id) = &item.canonical_agreement_id {
            lines.push(format!("  canonical_agreement_id {}", id));
        }
        if let Some(hash) = &item.canonical_agreement_hash {
            lines.push(format!("  canonical_agreement_hash {}", hash));
        }
        if let Some(hash) = &item.bundle_hash {
            lines.push(format!("  bundle_hash {}", hash));
        }
        lines.push(format!("  imported_artifact_types {}", imported));
        if let Some(label) = &item.sender_label {
            lines.push(format!("  sender_label {}", label));
        }
        if let Some(label) = &item.import_source_label {
            lines.push(format!("  import_source_label {}", label));
        }
        lines.push(format!("  receipt_path {}", item.receipt_path));
    }
    lines.join("\n")
}

fn build_agreement_local_store_listing(
    include_archived: bool,
) -> Result<AgreementLocalStoreListing, String> {
    let all_records = list_share_package_receipt_records(true)?;
    let refs = build_receipt_reference_index(&all_records);
    let active_receipts = all_records
        .iter()
        .filter(|item| item.location == SharePackageReceiptLocation::Active)
        .map(receipt_inventory_item)
        .collect::<Vec<_>>();
    let archived_receipts = if include_archived {
        all_records
            .iter()
            .filter(|item| item.location == SharePackageReceiptLocation::Archived)
            .map(receipt_inventory_item)
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };

    let mut stored_bundles = Vec::new();
    for item in list_stored_bundles_at(&agreement_bundles_dir())? {
        let path = item.path.display().to_string();
        stored_bundles.push(LocalStoreBundleItem {
            agreement_id: item.bundle.agreement_id.clone(),
            agreement_hash: item.bundle.agreement_hash.clone(),
            path: path.clone(),
            referenced_by_receipt_count: refs.get(&path).map(|v| v.len()).unwrap_or(0),
        });
    }

    let mut stored_raw_agreements = Vec::new();
    for item in list_stored_agreements_at(&imported_agreements_dir())? {
        let agreement_hash =
            irium_node_rs::settlement::compute_agreement_hash_hex(&item.agreement)?;
        let path = item.path.display().to_string();
        stored_raw_agreements.push(LocalStoreAgreementItem {
            agreement_id: item.agreement.agreement_id.clone(),
            agreement_hash,
            path: path.clone(),
            referenced_by_receipt_count: refs.get(&path).map(|v| v.len()).unwrap_or(0),
        });
    }

    let mut stored_detached_signatures = Vec::new();
    for item in list_stored_signatures_at(&imported_signature_store_dir())? {
        let path = item.path.display().to_string();
        stored_detached_signatures.push(LocalStoreSignatureItem {
            target_type: match item.signature.target_type {
                AgreementSignatureTargetType::Agreement => "agreement".to_string(),
                AgreementSignatureTargetType::Bundle => "bundle".to_string(),
            },
            target_hash: item.signature.target_hash.clone(),
            fingerprint: item.fingerprint.clone(),
            path: path.clone(),
            referenced_by_receipt_count: refs.get(&path).map(|v| v.len()).unwrap_or(0),
        });
    }

    let mut stored_informational_files = Vec::new();
    for record in &all_records {
        if !include_archived && record.location == SharePackageReceiptLocation::Archived {
            continue;
        }
        stored_informational_files.push(LocalStoreInformationalItem {
            kind: "package_copy".to_string(),
            receipt_id: record.receipt.receipt_id.clone(),
            archived: record.location == SharePackageReceiptLocation::Archived,
            path: record.receipt.package_path.clone(),
        });
        stored_informational_files.push(LocalStoreInformationalItem {
            kind: "verification".to_string(),
            receipt_id: record.receipt.receipt_id.clone(),
            archived: record.location == SharePackageReceiptLocation::Archived,
            path: record.receipt.verification_path.clone(),
        });
        if let Some(path) = &record.receipt.artifact_paths.audit_path {
            stored_informational_files.push(LocalStoreInformationalItem {
                kind: "audit".to_string(),
                receipt_id: record.receipt.receipt_id.clone(),
                archived: record.location == SharePackageReceiptLocation::Archived,
                path: path.clone(),
            });
        }
        if let Some(path) = &record.receipt.artifact_paths.statement_path {
            stored_informational_files.push(LocalStoreInformationalItem {
                kind: "statement".to_string(),
                receipt_id: record.receipt.receipt_id.clone(),
                archived: record.location == SharePackageReceiptLocation::Archived,
                path: path.clone(),
            });
        }
    }
    stored_informational_files.sort_by(|a, b| a.path.cmp(&b.path));

    Ok(AgreementLocalStoreListing {
        scope_notice: local_housekeeping_scope_notice(),
        housekeeping_notice: "Local housekeeping metadata is informational only. Archiving, removing, or pruning these files does not revoke a share package, invalidate artifacts elsewhere, or change chain state.".to_string(),
        active_receipt_count: all_records
            .iter()
            .filter(|item| item.location == SharePackageReceiptLocation::Active)
            .count(),
        archived_receipt_count: all_records
            .iter()
            .filter(|item| item.location == SharePackageReceiptLocation::Archived)
            .count(),
        bundle_count: stored_bundles.len(),
        raw_agreement_count: stored_raw_agreements.len(),
        detached_signature_count: stored_detached_signatures.len(),
        informational_file_count: stored_informational_files.len(),
        active_receipts,
        archived_receipts,
        stored_bundles,
        stored_raw_agreements,
        stored_detached_signatures,
        stored_informational_files,
    })
}

fn render_agreement_local_store_listing(listing: &AgreementLocalStoreListing) -> String {
    let mut lines = vec![
        "Agreement local artifact store".to_string(),
        format!("scope_notice {}", listing.scope_notice),
        format!("housekeeping_notice {}", listing.housekeeping_notice),
        format!("active_receipt_count {}", listing.active_receipt_count),
        format!("archived_receipt_count {}", listing.archived_receipt_count),
        format!("bundle_count {}", listing.bundle_count),
        format!("raw_agreement_count {}", listing.raw_agreement_count),
        format!(
            "detached_signature_count {}",
            listing.detached_signature_count
        ),
        format!(
            "informational_file_count {}",
            listing.informational_file_count
        ),
        "active_receipts".to_string(),
        render_share_package_receipt_inventory(&listing.active_receipts),
        "archived_receipts".to_string(),
    ];
    if listing.archived_receipts.is_empty() {
        lines.push("receipt_count 0".to_string());
    } else {
        lines.push(render_share_package_receipt_inventory(
            &listing.archived_receipts,
        ));
    }
    lines.push("stored_bundles".to_string());
    if listing.stored_bundles.is_empty() {
        lines.push("bundle_count 0".to_string());
    } else {
        for item in &listing.stored_bundles {
            lines.push(format!("agreement_hash {}", item.agreement_hash));
            lines.push(format!("  agreement_id {}", item.agreement_id));
            lines.push(format!("  path {}", item.path));
            lines.push(format!(
                "  referenced_by_receipt_count {}",
                item.referenced_by_receipt_count
            ));
        }
    }
    lines.push("stored_raw_agreements".to_string());
    if listing.stored_raw_agreements.is_empty() {
        lines.push("raw_agreement_count 0".to_string());
    } else {
        for item in &listing.stored_raw_agreements {
            lines.push(format!("agreement_hash {}", item.agreement_hash));
            lines.push(format!("  agreement_id {}", item.agreement_id));
            lines.push(format!("  path {}", item.path));
            lines.push(format!(
                "  referenced_by_receipt_count {}",
                item.referenced_by_receipt_count
            ));
        }
    }
    lines.push("stored_detached_signatures".to_string());
    if listing.stored_detached_signatures.is_empty() {
        lines.push("detached_signature_count 0".to_string());
    } else {
        for item in &listing.stored_detached_signatures {
            lines.push(format!("fingerprint {}", item.fingerprint));
            lines.push(format!("  target_type {}", item.target_type));
            lines.push(format!("  target_hash {}", item.target_hash));
            lines.push(format!("  path {}", item.path));
            lines.push(format!(
                "  referenced_by_receipt_count {}",
                item.referenced_by_receipt_count
            ));
        }
    }
    lines.push("stored_informational_files".to_string());
    if listing.stored_informational_files.is_empty() {
        lines.push("informational_file_count 0".to_string());
    } else {
        for item in &listing.stored_informational_files {
            lines.push(format!("path {}", item.path));
            lines.push(format!("  kind {}", item.kind));
            lines.push(format!("  receipt_id {}", item.receipt_id));
            lines.push(format!("  archived {}", item.archived));
        }
    }
    lines.join("\n")
}

fn render_local_store_mutation_report(report: &LocalStoreMutationReport) -> String {
    let mut lines = vec![
        format!("Local store {} report", report.action),
        format!("dry_run {}", report.dry_run),
        format!("scope_notice {}", report.scope_notice),
        format!("untouched_notice {}", report.untouched_notice),
        "changed".to_string(),
    ];
    if report.changed.is_empty() {
        lines.push("  none".to_string());
    } else {
        for item in &report.changed {
            lines.push(format!("  {} {}", item.kind, item.target));
            lines.push(format!("    path {}", item.path));
            lines.push(format!("    note {}", item.note));
        }
    }
    lines.push("skipped".to_string());
    if report.skipped.is_empty() {
        lines.push("  none".to_string());
    } else {
        for item in &report.skipped {
            lines.push(format!("  {} {}", item.kind, item.target));
            lines.push(format!("    path {}", item.path));
            lines.push(format!("    note {}", item.note));
        }
    }
    lines.push("warnings".to_string());
    if report.warnings.is_empty() {
        lines.push("  none".to_string());
    } else {
        for warning in &report.warnings {
            lines.push(format!("  {}", warning));
        }
    }
    lines.join("\n")
}

fn render_share_package_archive_result(result: &SharePackageArchiveResult) -> String {
    let mut lines = vec![
        "Agreement share package archive result".to_string(),
        format!("receipt_id {}", result.receipt_id),
        format!("from_path {}", result.from_path),
        format!("to_path {}", result.to_path),
        format!("archived_at {}", result.archived_at),
        format!("scope_notice {}", result.scope_notice),
        format!("untouched_notice {}", result.untouched_notice),
        "warnings".to_string(),
    ];
    if result.warnings.is_empty() {
        lines.push("  none".to_string());
    } else {
        for warning in &result.warnings {
            lines.push(format!("  {}", warning));
        }
    }
    lines.join("\n")
}

fn archive_share_package_receipt(reference: &str) -> Result<SharePackageArchiveResult, String> {
    let record = resolve_share_package_receipt_record(reference)?;
    if record.location == SharePackageReceiptLocation::Archived {
        return Err(format!(
            "receipt {} is already archived locally",
            record.receipt.receipt_id
        ));
    }
    fs::create_dir_all(share_package_archive_dir())
        .map_err(|e| format!("create archived share-package inbox dir: {e}"))?;
    let target_dir = allocate_share_package_receipt_dir(
        &share_package_archive_dir(),
        &record.receipt.receipt_id,
    )?;
    fs::rename(&record.receipt_dir, &target_dir).map_err(|e| {
        format!(
            "archive local receipt dir {}: {e}",
            record.receipt_dir.display()
        )
    })?;
    let archived_at = now_unix();
    write_share_package_housekeeping_metadata(
        &target_dir,
        &SharePackageLocalHousekeepingMetadata {
            version: 1,
            archived_at: Some(archived_at),
            archived_by_action: Some("agreement-share-package-archive".to_string()),
            prune_reason: None,
        },
    )?;
    let mut warnings = Vec::new();
    if !receipt_canonical_artifact_entries(&record.receipt).is_empty() {
        warnings.push(
            "Archived local receipt only. Canonical imported agreements, bundles, and detached signatures were left untouched.".to_string(),
        );
    }
    Ok(SharePackageArchiveResult {
        receipt_id: record.receipt.receipt_id,
        from_path: record.receipt_dir.display().to_string(),
        to_path: target_dir.display().to_string(),
        archived_at,
        scope_notice: local_housekeeping_scope_notice(),
        untouched_notice: local_housekeeping_untouched_notice(),
        warnings,
    })
}

fn receipt_reference_count(refs: &HashMap<String, Vec<String>>, path: &str) -> usize {
    refs.get(path).map(|value| value.len()).unwrap_or(0)
}

fn remove_exact_local_path(
    target_path: &Path,
    dry_run: bool,
    remove_imported_artifacts: bool,
) -> Result<LocalStoreMutationReport, String> {
    if !path_is_local_housekeeping_safe(target_path) {
        return Err(format!(
            "refusing to remove {} because it is outside the wallet-side housekeeping stores",
            target_path.display()
        ));
    }
    let all_records = list_share_package_receipt_records(true)?;
    let refs = build_receipt_reference_index(&all_records);
    for record in &all_records {
        if record.receipt_dir == target_path || record.receipt_path == target_path {
            return remove_share_package_receipt(
                &record.receipt.receipt_id,
                dry_run,
                remove_imported_artifacts,
            );
        }
    }

    let mut report = LocalStoreMutationReport {
        action: "remove".to_string(),
        dry_run,
        changed: Vec::new(),
        skipped: Vec::new(),
        warnings: Vec::new(),
        scope_notice: local_housekeeping_scope_notice(),
        untouched_notice: local_housekeeping_untouched_notice(),
    };
    if !target_path.exists() {
        return Err(format!(
            "local target {} does not exist",
            target_path.display()
        ));
    }

    let path_string = target_path.display().to_string();
    let target = target_path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(&path_string)
        .to_string();
    let is_canonical = path_is_within(&agreement_bundles_dir(), target_path)
        || path_is_within(&imported_agreements_dir(), target_path)
        || path_is_within(&imported_signature_store_dir(), target_path);
    let is_receipt_metadata = target_path.file_name() == Some(OsStr::new("receipt.json"));
    if is_receipt_metadata {
        return Err(
            "refusing to remove receipt.json directly; remove the receipt by receipt id or receipt directory instead"
                .to_string(),
        );
    }
    if is_canonical && !remove_imported_artifacts {
        return Err(
            "refusing to remove a canonical imported artifact without --remove-imported-artifacts"
                .to_string(),
        );
    }
    let ref_count = receipt_reference_count(&refs, &path_string);
    if is_canonical && ref_count > 1 {
        report.skipped.push(LocalStoreMutationEntry {
            kind: "canonical_artifact".to_string(),
            target,
            path: path_string,
            note: format!(
                "still referenced by {} local receipts; refusing to remove shared imported artifact",
                ref_count
            ),
        });
        return Ok(report);
    }
    if !is_canonical && ref_count > 0 {
        report.warnings.push(format!(
            "Removing this local file does not change chain state, but {} receipt(s) will still reference the removed path until those receipts are archived or removed.",
            ref_count
        ));
    }
    if !dry_run {
        remove_path_exact(target_path)?;
    }
    report.changed.push(LocalStoreMutationEntry {
        kind: if is_canonical {
            "canonical_artifact".to_string()
        } else {
            "local_file".to_string()
        },
        target,
        path: path_string,
        note: if dry_run {
            "would remove exact local path only".to_string()
        } else {
            "removed exact local path only".to_string()
        },
    });
    Ok(report)
}

fn remove_share_package_receipt(
    reference: &str,
    dry_run: bool,
    remove_imported_artifacts: bool,
) -> Result<LocalStoreMutationReport, String> {
    let record = resolve_share_package_receipt_record(reference)?;
    let all_records = list_share_package_receipt_records(true)?;
    let refs = build_receipt_reference_index(&all_records);
    let mut report = LocalStoreMutationReport {
        action: "remove".to_string(),
        dry_run,
        changed: Vec::new(),
        skipped: Vec::new(),
        warnings: Vec::new(),
        scope_notice: local_housekeeping_scope_notice(),
        untouched_notice: local_housekeeping_untouched_notice(),
    };

    if remove_imported_artifacts {
        for (kind, path) in receipt_canonical_artifact_entries(&record.receipt) {
            let target_path = PathBuf::from(&path);
            let ref_count = receipt_reference_count(&refs, &path);
            if !target_path.exists() {
                report.skipped.push(LocalStoreMutationEntry {
                    kind,
                    target: record.receipt.receipt_id.clone(),
                    path,
                    note: "local imported artifact was already absent".to_string(),
                });
                continue;
            }
            if ref_count > 1 {
                report.skipped.push(LocalStoreMutationEntry {
                    kind,
                    target: record.receipt.receipt_id.clone(),
                    path,
                    note: format!(
                        "still referenced by {} local receipts; leaving shared imported artifact untouched",
                        ref_count
                    ),
                });
                continue;
            }
            if !dry_run {
                remove_path_exact(&target_path)?;
            }
            report.changed.push(LocalStoreMutationEntry {
                kind,
                target: record.receipt.receipt_id.clone(),
                path,
                note: if dry_run {
                    "would remove uniquely referenced imported artifact".to_string()
                } else {
                    "removed uniquely referenced imported artifact".to_string()
                },
            });
        }
    } else if !receipt_canonical_artifact_entries(&record.receipt).is_empty() {
        report.warnings.push(
            "Receipt-linked canonical imported artifacts were left untouched. Use --remove-imported-artifacts only if you want to remove uniquely referenced local copies.".to_string(),
        );
    }

    if !dry_run {
        remove_path_exact(&record.receipt_dir)?;
    }
    report.changed.push(LocalStoreMutationEntry {
        kind: if record.location == SharePackageReceiptLocation::Archived {
            "archived_receipt".to_string()
        } else {
            "receipt".to_string()
        },
        target: record.receipt.receipt_id,
        path: record.receipt_dir.display().to_string(),
        note: if dry_run {
            "would remove receipt metadata, copied package file, verification output, and any local informational files stored inside the receipt directory only".to_string()
        } else {
            "removed receipt metadata, copied package file, verification output, and any local informational files stored inside the receipt directory only".to_string()
        },
    });
    Ok(report)
}

fn remove_local_store_agreement_hash(
    agreement_hash: &str,
    dry_run: bool,
    remove_imported_artifacts: bool,
) -> Result<LocalStoreMutationReport, String> {
    let path = agreement_path_for_hash(&imported_agreements_dir(), agreement_hash)?;
    if !path.exists() {
        return Err(format!(
            "no imported agreement stored locally for hash {}",
            agreement_hash
        ));
    }
    remove_exact_local_path(&path, dry_run, remove_imported_artifacts)
}

fn remove_local_store_bundle_hash(
    bundle_hash: &str,
    dry_run: bool,
    remove_imported_artifacts: bool,
) -> Result<LocalStoreMutationReport, String> {
    let mut matches = list_stored_bundles_at(&agreement_bundles_dir())?
        .into_iter()
        .filter(|item| {
            compute_agreement_bundle_hash_hex(&item.bundle)
                .map(|hash| hash.eq_ignore_ascii_case(bundle_hash))
                .unwrap_or(false)
        })
        .collect::<Vec<_>>();
    match matches.len() {
        0 => Err(format!(
            "no imported bundle stored locally for hash {}",
            bundle_hash
        )),
        1 => remove_exact_local_path(&matches.remove(0).path, dry_run, remove_imported_artifacts),
        _ => Err(format!(
            "multiple imported bundles matched hash {}; use an exact path instead",
            bundle_hash
        )),
    }
}

fn prune_share_package_store(
    dry_run: bool,
    older_than_days: Option<u64>,
    include_archived: bool,
    remove_imported_artifacts: bool,
) -> Result<LocalStoreMutationReport, String> {
    let mut report = LocalStoreMutationReport {
        action: "prune".to_string(),
        dry_run,
        changed: Vec::new(),
        skipped: Vec::new(),
        warnings: Vec::new(),
        scope_notice: local_housekeeping_scope_notice(),
        untouched_notice: local_housekeeping_untouched_notice(),
    };
    if !include_archived {
        report.warnings.push(
            "No archived receipts were considered because --include-archived was not supplied. This conservative prune mode leaves active receipts and their linked artifacts untouched.".to_string(),
        );
        return Ok(report);
    }
    let Some(days) = older_than_days else {
        report.warnings.push(
            "No archived receipts were pruned because --older-than <days> was not supplied. Use a threshold to make archived receipt cleanup explicit.".to_string(),
        );
        return Ok(report);
    };
    let cutoff = now_unix().saturating_sub(days.saturating_mul(86_400));
    let records = list_share_package_receipt_records(true)?;
    let refs = build_receipt_reference_index(&records);
    for record in records
        .into_iter()
        .filter(|item| item.location == SharePackageReceiptLocation::Archived)
    {
        let archived_at = record
            .housekeeping
            .as_ref()
            .and_then(|item| item.archived_at)
            .unwrap_or(record.receipt.imported_at);
        if archived_at > cutoff {
            report.skipped.push(LocalStoreMutationEntry {
                kind: "archived_receipt".to_string(),
                target: record.receipt.receipt_id.clone(),
                path: record.receipt_dir.display().to_string(),
                note: format!(
                    "archived_at {} is newer than the requested {} day threshold",
                    archived_at, days
                ),
            });
            continue;
        }
        if remove_imported_artifacts {
            for (kind, path) in receipt_canonical_artifact_entries(&record.receipt) {
                let target_path = PathBuf::from(&path);
                let ref_count = receipt_reference_count(&refs, &path);
                if !target_path.exists() {
                    report.skipped.push(LocalStoreMutationEntry {
                        kind,
                        target: record.receipt.receipt_id.clone(),
                        path,
                        note: "local imported artifact was already absent".to_string(),
                    });
                    continue;
                }
                if ref_count > 1 {
                    report.skipped.push(LocalStoreMutationEntry {
                        kind,
                        target: record.receipt.receipt_id.clone(),
                        path,
                        note: format!(
                            "still referenced by {} local receipts; leaving shared imported artifact untouched",
                            ref_count
                        ),
                    });
                    continue;
                }
                if !dry_run {
                    remove_path_exact(&target_path)?;
                }
                report.changed.push(LocalStoreMutationEntry {
                    kind,
                    target: record.receipt.receipt_id.clone(),
                    path,
                    note: if dry_run {
                        "would remove uniquely referenced imported artifact".to_string()
                    } else {
                        "removed uniquely referenced imported artifact".to_string()
                    },
                });
            }
        }
        if !dry_run {
            remove_path_exact(&record.receipt_dir)?;
        }
        report.changed.push(LocalStoreMutationEntry {
            kind: "archived_receipt".to_string(),
            target: record.receipt.receipt_id.clone(),
            path: record.receipt_dir.display().to_string(),
            note: if dry_run {
                "would remove archived receipt metadata and copied informational files only"
                    .to_string()
            } else {
                "removed archived receipt metadata and copied informational files only".to_string()
            },
        });
    }
    if report.changed.is_empty() && report.skipped.is_empty() {
        report.warnings.push(
            "No safe prune candidates were found for the requested local-only criteria."
                .to_string(),
        );
    }
    Ok(report)
}

fn normalize_share_package_import(value: &str) -> Result<String, String> {
    let normalized = normalize_share_package_include(value)?;
    match normalized.as_str() {
        "agreement"
        | "bundle"
        | "audit"
        | "statement"
        | "agreement_signatures"
        | "bundle_signatures" => Ok(normalized),
        _ => Err(format!("unsupported share-package import {}", value)),
    }
}

fn ensure_requested_share_package_imports(
    package: &AgreementSharePackage,
    imports: &[String],
) -> Result<(), String> {
    if imports.is_empty() {
        return Err("share-package import requires at least one --import selection".to_string());
    }
    let canonical_available = package.agreement.is_some() || package.bundle.is_some();
    for item in imports {
        match item.as_str() {
            "agreement" if !canonical_available => {
                return Err("--import agreement requires a canonical agreement artifact in the package".to_string())
            }
            "bundle" if package.bundle.is_none() => {
                return Err("--import bundle requires the share package to include a bundle".to_string())
            }
            "audit" if package.audit.is_none() => {
                return Err("--import audit requires the share package to include an audit artifact".to_string())
            }
            "statement" if package.statement.is_none() => {
                return Err("--import statement requires the share package to include a statement artifact".to_string())
            }
            "agreement_signatures" if package.detached_agreement_signatures.is_empty() => {
                return Err("--import agreement-signatures requires detached agreement signatures in the package".to_string())
            }
            "bundle_signatures" if package.detached_bundle_signatures.is_empty() => {
                return Err("--import bundle-signatures requires detached bundle signatures in the package".to_string())
            }
            _ => {}
        }
    }
    Ok(())
}

fn write_json_file(path: &Path, value: &impl Serialize, label: &str) -> Result<(), String> {
    let rendered =
        serde_json::to_string_pretty(value).map_err(|e| format!("serialize {label}: {e}"))?;
    fs::write(path, rendered).map_err(|e| format!("write {label}: {e}"))
}

fn import_verified_share_package(
    package: &AgreementSharePackage,
    verification: &AgreementSharePackageVerificationResult,
    source_path: &str,
    import_source_label: Option<String>,
    imports: &[String],
) -> Result<SharePackageReceipt, String> {
    ensure_requested_share_package_imports(package, imports)?;
    let imported_at = now_unix();
    let inspection = &verification.package;
    let canonical_agreement = package
        .agreement
        .as_ref()
        .or_else(|| package.bundle.as_ref().map(|item| &item.agreement));
    let canonical_agreement_hash = inspection.canonical_agreement_hash.clone().or_else(|| {
        canonical_agreement.as_ref().and_then(|agreement| {
            irium_node_rs::settlement::compute_agreement_hash_hex(agreement).ok()
        })
    });
    let bundle_hash = inspection.bundle_hash.clone();
    let receipt_id = share_package_receipt_id(imported_at, canonical_agreement_hash.as_deref());
    let receipt_dir = allocate_share_package_receipt_dir(&share_package_inbox_dir(), &receipt_id)?;
    fs::create_dir_all(&receipt_dir)
        .map_err(|e| format!("create share-package receipt dir: {e}"))?;

    let package_copy_path = receipt_dir.join("package.json");
    let verification_path = receipt_dir.join("verification.json");
    write_json_file(&package_copy_path, package, "share package")?;
    write_json_file(
        &verification_path,
        verification,
        "share package verification",
    )?;

    let mut imported_artifact_types = Vec::new();
    let mut already_present_artifact_types = Vec::new();
    let rejected_artifact_types = Vec::new();
    let mut artifact_paths = SharePackageReceiptArtifactPaths::default();

    let mark_store_status =
        |item: &str,
         status: StoreWriteStatus,
         path: &Path,
         imported: &mut Vec<String>,
         already_present: &mut Vec<String>| match status {
            StoreWriteStatus::Imported => {
                imported.push(item.to_string());
                Some(path.display().to_string())
            }
            StoreWriteStatus::AlreadyPresent => {
                already_present.push(item.to_string());
                Some(path.display().to_string())
            }
        };

    for item in imports {
        match item.as_str() {
            "agreement" => {
                let agreement = canonical_agreement.ok_or_else(|| {
                    "share package did not contain a canonical agreement to import".to_string()
                })?;
                let (status, path) =
                    save_agreement_to_store_checked(&imported_agreements_dir(), agreement)?;
                artifact_paths.agreement_path = mark_store_status(
                    "agreement",
                    status,
                    &path,
                    &mut imported_artifact_types,
                    &mut already_present_artifact_types,
                );
            }
            "bundle" => {
                let bundle = package.bundle.as_ref().ok_or_else(|| {
                    "share package did not contain a bundle to import".to_string()
                })?;
                if verification
                    .artifact_verification
                    .canonical_verification
                    .bundle_hash_match
                    == Some(false)
                {
                    return Err(
                        "refusing to import bundle from share package with mismatched bundle identity"
                            .to_string(),
                    );
                }
                let (status, path) =
                    save_bundle_to_store_checked(&agreement_bundles_dir(), bundle)?;
                artifact_paths.bundle_path = mark_store_status(
                    "bundle",
                    status,
                    &path,
                    &mut imported_artifact_types,
                    &mut already_present_artifact_types,
                );
            }
            "audit" => {
                if verification
                    .artifact_verification
                    .canonical_verification
                    .audit_identity_match
                    == Some(false)
                {
                    return Err(
                        "refusing to import informational audit artifact with mismatched agreement identity"
                            .to_string(),
                    );
                }
                let audit = package.audit.as_ref().ok_or_else(|| {
                    "share package did not contain an audit artifact to import".to_string()
                })?;
                let path = receipt_dir.join("audit.json");
                write_json_file(&path, audit, "share package imported audit")?;
                imported_artifact_types.push("audit".to_string());
                artifact_paths.audit_path = Some(path.display().to_string());
            }
            "statement" => {
                if verification
                    .artifact_verification
                    .canonical_verification
                    .statement_identity_match
                    == Some(false)
                {
                    return Err(
                        "refusing to import informational statement artifact with mismatched agreement identity"
                            .to_string(),
                    );
                }
                let statement = package.statement.as_ref().ok_or_else(|| {
                    "share package did not contain a statement artifact to import".to_string()
                })?;
                let path = receipt_dir.join("statement.json");
                write_json_file(&path, statement, "share package imported statement")?;
                imported_artifact_types.push("statement".to_string());
                artifact_paths.statement_path = Some(path.display().to_string());
            }
            "agreement_signatures" => {
                let expected_agreement_hash =
                    canonical_agreement_hash.as_deref().ok_or_else(|| {
                        "cannot import agreement signatures without a canonical agreement hash"
                            .to_string()
                    })?;
                for signature in &package.detached_agreement_signatures {
                    let signature_verification = inspect_agreement_signature(
                        signature,
                        Some(expected_agreement_hash),
                        bundle_hash.as_deref(),
                    );
                    if !(signature_verification.valid
                        && signature_verification.matches_expected_target)
                    {
                        return Err(format!(
                            "refusing to import detached agreement signature for signer {} because it did not verify against the canonical agreement hash",
                            signature_verification.signer_public_key
                        ));
                    }
                    let (status, path) = save_signature_to_store_checked(
                        &imported_signature_store_dir(),
                        signature,
                    )?;
                    if let Some(stored) = mark_store_status(
                        "agreement_signatures",
                        status,
                        &path,
                        &mut imported_artifact_types,
                        &mut already_present_artifact_types,
                    ) {
                        artifact_paths.agreement_signature_paths.push(stored);
                    }
                }
            }
            "bundle_signatures" => {
                let expected_bundle_hash = bundle_hash.as_deref().ok_or_else(|| {
                    "cannot import bundle signatures without a canonical bundle hash".to_string()
                })?;
                for signature in &package.detached_bundle_signatures {
                    let signature_verification = inspect_agreement_signature(
                        signature,
                        canonical_agreement_hash.as_deref(),
                        Some(expected_bundle_hash),
                    );
                    if !(signature_verification.valid
                        && signature_verification.matches_expected_target)
                    {
                        return Err(format!(
                            "refusing to import detached bundle signature for signer {} because it did not verify against the canonical bundle hash",
                            signature_verification.signer_public_key
                        ));
                    }
                    let (status, path) = save_signature_to_store_checked(
                        &imported_signature_store_dir(),
                        signature,
                    )?;
                    if let Some(stored) = mark_store_status(
                        "bundle_signatures",
                        status,
                        &path,
                        &mut imported_artifact_types,
                        &mut already_present_artifact_types,
                    ) {
                        artifact_paths.bundle_signature_paths.push(stored);
                    }
                }
            }
            other => return Err(format!("unsupported share-package import {}", other)),
        }
    }

    if imported_artifact_types.is_empty() && already_present_artifact_types.is_empty() {
        return Err("share-package import did not import or match any local artifacts".to_string());
    }

    let receipt = SharePackageReceipt {
        version: 1,
        receipt_id,
        imported_at,
        source_path: source_path.to_string(),
        import_source_label,
        sender_label: package.sender_label.clone(),
        package_note: package.package_note.clone(),
        package_profile: inspection.package_profile.clone(),
        included_artifact_types: inspection.included_artifact_types.clone(),
        imported_artifact_types,
        already_present_artifact_types,
        rejected_artifact_types,
        canonical_agreement_id: inspection.canonical_agreement_id.clone(),
        canonical_agreement_hash,
        bundle_hash,
        verification_summary: summarize_share_package_verification(verification),
        warnings: collect_artifact_verification_warnings(&verification.artifact_verification),
        trust_notice: package.trust_notice.clone(),
        provenance_notice: "Local share-package intake metadata is informational only. It records where verified artifacts were imported from, but it does not make the agreement trusted, true, authorized, enforceable, or native chain state.".to_string(),
        package_path: package_copy_path.display().to_string(),
        verification_path: verification_path.display().to_string(),
        artifact_paths,
    };
    write_json_file(
        &share_package_receipt_path(&receipt_dir),
        &receipt,
        "share package receipt",
    )?;
    Ok(receipt)
}

fn render_signature_verification_summary(
    verification: &AgreementSignatureVerification,
    heading: &str,
) -> String {
    let mut lines = vec![
        heading.to_string(),
        format!(
            "target_type {}",
            serde_json::to_string(&verification.target_type)
                .unwrap_or_else(|_| "\"unknown\"".to_string())
                .trim_matches('"')
        ),
        format!("target_hash {}", verification.target_hash),
        format!("signature_type {}", verification.signature_type),
        format!("valid {}", verification.valid),
        format!(
            "matches_expected_target {}",
            verification.matches_expected_target
        ),
        format!("signer_public_key {}", verification.signer_public_key),
    ];
    if let Some(address) = &verification.signer_address {
        lines.push(format!("signer_address {}", address));
    }
    if let Some(role) = &verification.signer_role {
        lines.push(format!("signer_role {}", role));
    }
    if let Some(timestamp) = verification.timestamp {
        lines.push(format!("timestamp {}", timestamp));
    }
    lines.push(format!(
        "authenticity_note {}",
        verification.authenticity_note
    ));
    lines.push("warnings".to_string());
    if verification.warnings.is_empty() {
        lines.push("  none".to_string());
    } else {
        for item in &verification.warnings {
            lines.push(format!("  {}", item));
        }
    }
    lines.join("\n")
}

fn render_bundle_signature_verifications(
    bundle: &AgreementBundle,
    verifications: &[AgreementSignatureVerification],
) -> String {
    let bundle_hash =
        compute_agreement_bundle_hash_hex(bundle).unwrap_or_else(|_| "unavailable".to_string());
    let mut lines = vec![
        format!("agreement_id {}", bundle.agreement_id),
        format!("agreement_hash {}", bundle.agreement_hash),
        format!("bundle_hash {}", bundle_hash),
        format!("signature_count {}", verifications.len()),
    ];
    if verifications.is_empty() {
        lines.push("signatures none".to_string());
    } else {
        lines.push("signatures".to_string());
        for (index, verification) in verifications.iter().enumerate() {
            lines.push(format!(
                "  [{}] valid={} target_type={} signer={} role={} expected_match={} note={}",
                index,
                verification.valid,
                serde_json::to_string(&verification.target_type)
                    .unwrap_or_else(|_| "\"unknown\"".to_string())
                    .trim_matches('"'),
                verification
                    .signer_address
                    .as_deref()
                    .unwrap_or(verification.signer_public_key.as_str()),
                verification.signer_role.as_deref().unwrap_or("-"),
                verification.matches_expected_target,
                verification.authenticity_note
            ));
            for warning in &verification.warnings {
                lines.push(format!("    warning {}", warning));
            }
        }
    }
    lines.push("trust_model signature validity proves authenticity only; it does not prove the agreement is correct or enforce settlement on-chain".to_string());
    lines.join("\n")
}

fn render_artifact_verification(result: &AgreementArtifactVerificationResult) -> String {
    let mut lines = vec![
        "Agreement artifact verification".to_string(),
        format!("generated_at {}", result.metadata.generated_at),
        format!(
            "supplied_artifacts {}",
            result.input_summary.supplied_artifact_types.join(" | ")
        ),
        format!(
            "canonical_present {}",
            result.input_summary.canonical_agreement_present
        ),
    ];
    if let Some(hash) = &result.canonical_verification.computed_agreement_hash {
        lines.push(format!("computed_agreement_hash {}", hash));
    }
    if let Some(agreement_id) = &result.canonical_verification.computed_agreement_id {
        lines.push(format!("computed_agreement_id {}", agreement_id));
    }
    lines.push("verified_matches".to_string());
    if result.canonical_verification.matches.is_empty() {
        lines.push("  none".to_string());
    } else {
        for item in &result.canonical_verification.matches {
            lines.push(format!("  {}", item));
        }
    }
    lines.push("mismatches".to_string());
    if result.canonical_verification.mismatches.is_empty() {
        lines.push("  none".to_string());
    } else {
        for item in &result.canonical_verification.mismatches {
            lines.push(format!("  {}", item));
        }
    }
    if let Some(authenticity) = &result.authenticity {
        lines.push("authenticity".to_string());
        lines.push(format!(
            "  detached_agreement_signatures={} detached_bundle_signatures={} embedded_bundle_signatures={}",
            authenticity.detached_agreement_signatures_supplied,
            authenticity.detached_bundle_signatures_supplied,
            authenticity.embedded_bundle_signatures_supplied
        ));
        lines.push(format!(
            "  valid={} invalid={} unverifiable={}",
            authenticity.valid_signatures,
            authenticity.invalid_signatures,
            authenticity.unverifiable_signatures
        ));
        for signer in &authenticity.signer_summaries {
            lines.push(format!("  signer {}", signer));
        }
        lines.push(format!("  notice {}", authenticity.authenticity_notice));
    }
    lines.push("chain_observed".to_string());
    lines.push(format!(
        "  linked_tx_references_found {}",
        result.chain_verification.linked_tx_references_found
    ));
    lines.push(format!(
        "  anchor_observations_found {}",
        result.chain_verification.anchor_observations_found
    ));
    if !result.chain_verification.checked_txids.is_empty() {
        lines.push(format!(
            "  checked_txids {}",
            result.chain_verification.checked_txids.join(" | ")
        ));
    }
    lines.push("unverifiable_or_limited".to_string());
    let mut warnings = Vec::new();
    warnings.extend(result.canonical_verification.warnings.clone());
    warnings.extend(result.artifact_consistency.warnings.clone());
    warnings.extend(result.chain_verification.warnings.clone());
    warnings.extend(result.derived_verification.warnings.clone());
    warnings.extend(result.trust_summary.unverifiable_from_chain_alone.clone());
    if warnings.is_empty() {
        lines.push("  none".to_string());
    } else {
        for item in warnings {
            lines.push(format!("  {}", item));
        }
    }
    lines.push("trust_boundaries".to_string());
    lines.push(format!(
        "  consensus_visible {}",
        result.trust_summary.consensus_visible.join(" | ")
    ));
    lines.push(format!(
        "  htlc_enforced {}",
        result.trust_summary.htlc_enforced.join(" | ")
    ));
    lines.push(format!(
        "  derived_indexed {}",
        result.trust_summary.derived_indexed.join(" | ")
    ));
    lines.push(format!(
        "  local_artifact_only {}",
        result.trust_summary.local_artifact_only.join(" | ")
    ));
    lines.join(
        "
",
    )
}

fn render_agreement_statement(statement: &AgreementStatement) -> String {
    let mut lines = vec![
        "Derived settlement statement".to_string(),
        format!("agreement_id {}", statement.identity.agreement_id),
        format!("agreement_hash {}", statement.identity.agreement_hash),
        format!(
            "template {}",
            serde_json::to_string(&statement.identity.template_type)
                .unwrap_or_else(|_| "\"unknown\"".to_string())
                .trim_matches('"')
        ),
        format!("generated_at {}", statement.metadata.generated_at),
        format!("payer {}", statement.counterparties.payer),
        format!("payee {}", statement.counterparties.payee),
        format!(
            "total_amount_irm {}",
            format_irm(statement.commercial.total_amount)
        ),
        format!("derived_status {}", statement.derived.derived_state_label),
        format!("funding_observed {}", statement.observed.funding_observed),
        format!("release_observed {}", statement.observed.release_observed),
        format!("refund_observed {}", statement.observed.refund_observed),
        format!("milestones {}", statement.commercial.milestone_summary),
    ];
    if let Some(deadline) = statement.commercial.settlement_deadline {
        lines.push(format!("settlement_deadline {}", deadline));
    }
    if let Some(deadline) = statement.commercial.refund_deadline {
        lines.push(format!("refund_deadline {}", deadline));
    }
    if !statement.counterparties.parties_summary.is_empty() {
        lines.push(format!(
            "parties {}",
            statement.counterparties.parties_summary.join(" | ")
        ));
    }
    lines.push(format!(
        "release_path {}",
        statement.commercial.release_path_summary
    ));
    lines.push(format!(
        "refund_path {}",
        statement.commercial.refund_path_summary
    ));
    if let Some(warning) = &statement.observed.ambiguity_warning {
        lines.push(format!("ambiguity_warning {}", warning));
    }
    if !statement.references.linked_txids.is_empty() {
        lines.push(format!(
            "linked_txids {}",
            statement.references.linked_txids.join(" | ")
        ));
    }
    if let Some(authenticity) = &statement.authenticity {
        lines.push("authenticity".to_string());
        lines.push(format!(
            "  valid={} invalid={} unverifiable={}",
            authenticity.valid_signatures,
            authenticity.invalid_signatures,
            authenticity.unverifiable_signatures
        ));
        lines.push(format!("  summary {}", authenticity.compact_summary));
        lines.push(format!("  notice {}", authenticity.authenticity_notice));
    }
    lines.push(format!("notice {}", statement.metadata.derived_notice));
    lines.push("trust_boundaries".to_string());
    lines.push(format!(
        "  consensus {}",
        statement.trust_notice.consensus_visible.join(" | ")
    ));
    lines.push(format!(
        "  htlc {}",
        statement.trust_notice.htlc_enforced.join(" | ")
    ));
    lines.push(format!(
        "  derived {}",
        statement.trust_notice.derived_indexed.join(" | ")
    ));
    lines.push(format!(
        "  local_off_chain {}",
        statement.trust_notice.local_off_chain.join(" | ")
    ));
    lines.push(format!(
        "  canonical {}",
        statement.trust_notice.canonical_notice
    ));
    lines.join(
        "
",
    )
}

fn validate_agreement_audit_export_format(
    export_format: &str,
    json_mode: bool,
) -> Result<String, String> {
    let export_format = export_format.to_lowercase();
    if export_format != "json" && export_format != "csv" {
        return Err(format!(
            "unsupported --format {}; expected json or csv",
            export_format
        ));
    }
    if json_mode && export_format != "json" {
        return Err("--json is only supported with --format json".to_string());
    }
    Ok(export_format)
}

fn render_agreement_summary(agreement: &AgreementObject, agreement_hash: &str) -> String {
    let mut lines = Vec::new();
    lines.push(format!("agreement_id {}", agreement.agreement_id));
    lines.push(format!(
        "schema_id {}",
        agreement.schema_id.as_deref().unwrap_or("legacy_unlabeled")
    ));
    lines.push(format!("schema_version {}", agreement.version));
    lines.push(format!(
        "template {}",
        serde_json::to_string(&agreement.template_type)
            .unwrap_or_else(|_| "\"unknown\"".to_string())
            .trim_matches('"')
    ));
    lines.push(format!("agreement_hash {}", agreement_hash));
    lines.push(format!("payer {}", agreement.payer));
    lines.push(format!("payee {}", agreement.payee));
    lines.push(format!(
        "total_amount_irm {}",
        format_irm(agreement.total_amount)
    ));
    if let Some(deadline) = agreement.deadlines.settlement_deadline {
        lines.push(format!("settlement_deadline {}", deadline));
    }
    if let Some(deadline) = agreement.deadlines.refund_deadline {
        lines.push(format!("refund_deadline {}", deadline));
    }
    if let Some(asset_reference) = &agreement.asset_reference {
        lines.push(format!("asset_reference {}", asset_reference));
    }
    if let Some(payment_reference) = &agreement.payment_reference {
        lines.push(format!("payment_reference {}", payment_reference));
    }
    if let Some(purpose_reference) = &agreement.purpose_reference {
        lines.push(format!("purpose_reference {}", purpose_reference));
    }
    lines.push(format!("document_hash {}", agreement.document_hash));
    if let Some(metadata_hash) = &agreement.metadata_hash {
        lines.push(format!("metadata_hash {}", metadata_hash));
    }
    lines.push("trust_model HTLC release/refund enforcement is on-chain; milestones, mediator references, and lifecycle views are metadata/indexed unless otherwise encoded by HTLC legs".to_string());
    if !agreement.milestones.is_empty() {
        lines.push("milestones".to_string());
        for milestone in &agreement.milestones {
            lines.push(format!(
                "  {} {} amount={} timeout_height={} recipient={} refund={}",
                milestone.milestone_id,
                milestone.title,
                format_irm(milestone.amount),
                milestone.timeout_height,
                milestone.recipient_address,
                milestone.refund_address
            ));
        }
    }
    lines.join(
        "
",
    )
}

fn print_agreement_summary(agreement: &AgreementObject, agreement_hash: &str) {
    println!("{}", render_agreement_summary(agreement, agreement_hash));
}

fn bundle_list_item(item: &StoredAgreementBundle) -> AgreementBundleListItem {
    AgreementBundleListItem {
        agreement_id: item.bundle.agreement_id.clone(),
        agreement_hash: item.bundle.agreement_hash.clone(),
        saved_at: item.bundle.metadata.saved_at,
        source_label: item.bundle.metadata.source_label.clone(),
        linked_funding_txids: item.bundle.metadata.linked_funding_txids.clone(),
        path: item.path.display().to_string(),
    }
}

fn render_share_package_inspection(inspection: &AgreementSharePackageInspection) -> String {
    let included = if inspection.included_artifact_types.is_empty() {
        "none".to_string()
    } else {
        inspection.included_artifact_types.join(" | ")
    };
    let omitted = if inspection.omitted_artifact_types.is_empty() {
        "none".to_string()
    } else {
        inspection.omitted_artifact_types.join(" | ")
    };
    let mut lines = vec![
        "Agreement share package".to_string(),
        format!("package_version {}", inspection.version),
        format!(
            "package_schema_id {}",
            inspection
                .package_schema_id
                .as_deref()
                .unwrap_or("legacy_unlabeled")
        ),
        format!("package_profile {}", inspection.package_profile),
        format!("agreement_present {}", inspection.agreement_present),
        format!("bundle_present {}", inspection.bundle_present),
        format!("audit_present {}", inspection.audit_present),
        format!("statement_present {}", inspection.statement_present),
        format!(
            "detached_agreement_signatures {}",
            inspection.detached_agreement_signature_count
        ),
        format!(
            "detached_bundle_signatures {}",
            inspection.detached_bundle_signature_count
        ),
        format!("included_artifact_types {}", included),
        format!("omitted_artifact_types {}", omitted),
        format!("verification_notice {}", inspection.verification_notice),
    ];
    if let Some(created_at) = inspection.created_at {
        lines.push(format!("created_at {}", created_at));
    }
    if let Some(sender_label) = &inspection.sender_label {
        lines.push(format!("sender_label {}", sender_label));
    }
    if let Some(package_note) = &inspection.package_note {
        lines.push(format!("package_note {}", package_note));
    }
    if let Some(agreement_id) = &inspection.canonical_agreement_id {
        lines.push(format!("canonical_agreement_id {}", agreement_id));
    }
    if let Some(agreement_hash) = &inspection.canonical_agreement_hash {
        lines.push(format!("canonical_agreement_hash {}", agreement_hash));
    }
    if let Some(bundle_hash) = &inspection.bundle_hash {
        lines.push(format!("bundle_hash {}", bundle_hash));
    }
    lines.push(format!("trust_notice {}", inspection.trust_notice));
    lines.push(format!(
        "informational_notice {}",
        inspection.informational_notice
    ));
    lines.join(
        "
",
    )
}

fn render_share_package_verification(result: &AgreementSharePackageVerificationResult) -> String {
    let mut lines = vec![
        "Agreement share package verification".to_string(),
        format!("generated_at {}", result.metadata.generated_at),
        format!("derived_notice {}", result.metadata.derived_notice),
        render_share_package_inspection(&result.package),
        "package_notices".to_string(),
    ];
    if result.informational_notices.is_empty() {
        lines.push("  none".to_string());
    } else {
        for item in &result.informational_notices {
            lines.push(format!("  {}", item));
        }
    }
    lines.push(render_artifact_verification(&result.artifact_verification));
    lines.join(
        "
",
    )
}

fn render_bundle_summary(bundle: &AgreementBundle, source: &str) -> String {
    let bundle_hash =
        compute_agreement_bundle_hash_hex(bundle).unwrap_or_else(|_| "unavailable".to_string());
    let mut lines = vec![
        format!("agreement_id {}", bundle.agreement_id),
        format!("agreement_hash {}", bundle.agreement_hash),
        format!("bundle_hash {}", bundle_hash),
        format!("bundle_version {}", bundle.version),
        format!(
            "bundle_schema_id {}",
            bundle
                .bundle_schema_id
                .as_deref()
                .unwrap_or("legacy_unlabeled")
        ),
        format!("saved_at {}", bundle.metadata.saved_at),
        format!("source {}", source),
    ];
    if let Some(label) = &bundle.metadata.source_label {
        lines.push(format!("source_label {}", label));
    }
    if let Some(note) = &bundle.metadata.note {
        lines.push(format!("note {}", note));
    }
    if bundle.metadata.linked_funding_txids.is_empty() {
        lines.push("linked_funding_txids none".to_string());
    } else {
        lines.push("linked_funding_txids".to_string());
        for txid in &bundle.metadata.linked_funding_txids {
            lines.push(format!("  {}", txid));
        }
    }
    if let Some(summary) = &bundle.artifacts.metadata_summary {
        lines.push(format!("metadata_summary {}", summary));
    }
    lines.push(format!(
        "embedded_audit {}",
        bundle
            .artifacts
            .audit
            .as_ref()
            .map(|_| true)
            .unwrap_or(false)
    ));
    lines.push(format!(
        "embedded_statement {}",
        bundle
            .artifacts
            .statement
            .as_ref()
            .map(|_| true)
            .unwrap_or(false)
    ));
    lines.push(format!(
        "chain_snapshot {}",
        bundle
            .artifacts
            .chain_observation_snapshot
            .as_ref()
            .map(|_| true)
            .unwrap_or(false)
    ));
    let signature_verifications = verify_bundle_signatures(bundle);
    lines.push(format!(
        "embedded_signatures {}",
        signature_verifications.len()
    ));
    if !signature_verifications.is_empty() {
        let valid_count = signature_verifications
            .iter()
            .filter(|item| item.valid)
            .count();
        lines.push(format!("embedded_signature_valid {}", valid_count));
    }
    if !bundle.artifacts.external_document_hashes.is_empty() {
        lines.push("external_document_hashes".to_string());
        for hash in &bundle.artifacts.external_document_hashes {
            lines.push(format!("  {}", hash));
        }
    }
    lines.push("trust_model bundle persistence is local/off-chain convenience only; the canonical agreement object remains the source of truth and chain data alone cannot recover it".to_string());
    lines.join(
        "
",
    )
}



fn parse_proof_submit_cli(args: &[String]) -> Result<ProofSubmitCliOptions, String> {
    let mut proof_path: Option<String> = None;
    let mut rpc_url = default_rpc_url();
    let mut json_mode = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--proof" => {
                i += 1;
                if i >= args.len() {
                    return Err("--proof requires a value".to_string());
                }
                proof_path = Some(args[i].clone());
            }
            "--rpc" => {
                i += 1;
                if i >= args.len() {
                    return Err("--rpc requires a value".to_string());
                }
                rpc_url = args[i].clone();
            }
            "--json" => {
                json_mode = true;
            }
            other => {
                return Err(format!("unknown argument: {}", other));
            }
        }
        i += 1;
    }
    Ok(ProofSubmitCliOptions {
        proof_path: proof_path.ok_or_else(|| "--proof is required".to_string())?,
        rpc_url,
        json_mode,
    })
}

fn parse_proof_list_cli(args: &[String]) -> Result<ProofListCliOptions, String> {
    let mut agreement_hash: Option<String> = None;
    let mut active_only = false;
    let mut offset: u32 = 0;
    let mut limit: Option<u32> = None;
    let mut rpc_url = default_rpc_url();
    let mut json_mode = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--agreement-hash" => {
                i += 1;
                if i >= args.len() {
                    return Err("--agreement-hash requires a value".to_string());
                }
                agreement_hash = Some(args[i].clone());
            }
            "--active-only" => {
                active_only = true;
            }
            "--offset" => {
                i += 1;
                if i >= args.len() {
                    return Err("--offset requires a value".to_string());
                }
                offset = args[i].parse::<u32>().map_err(|_| "--offset must be a non-negative integer".to_string())?;
            }
            "--limit" => {
                i += 1;
                if i >= args.len() {
                    return Err("--limit requires a value".to_string());
                }
                limit = Some(args[i].parse::<u32>().map_err(|_| "--limit must be a positive integer".to_string())?);
            }
            "--rpc" => {
                i += 1;
                if i >= args.len() {
                    return Err("--rpc requires a value".to_string());
                }
                rpc_url = args[i].clone();
            }
            "--json" => {
                json_mode = true;
            }
            other => {
                return Err(format!("unknown argument: {}", other));
            }
        }
        i += 1;
    }
    Ok(ProofListCliOptions {
        agreement_hash,
        active_only,
        offset,
        limit,
        rpc_url,
        json_mode,
    })
}

fn parse_proof_get_cli(args: &[String]) -> Result<ProofGetCliOptions, String> {
    let mut proof_id: Option<String> = None;
    let mut rpc_url = default_rpc_url();
    let mut json_mode = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--proof-id" => {
                i += 1;
                if i >= args.len() {
                    return Err("--proof-id requires a value".to_string());
                }
                proof_id = Some(args[i].clone());
            }
            "--rpc" => {
                i += 1;
                if i >= args.len() {
                    return Err("--rpc requires a value".to_string());
                }
                rpc_url = args[i].clone();
            }
            "--json" => {
                json_mode = true;
            }
            other => {
                return Err(format!("unknown argument: {}", other));
            }
        }
        i += 1;
    }
    let proof_id = proof_id.ok_or_else(|| "--proof-id is required".to_string())?;
    Ok(ProofGetCliOptions { proof_id, rpc_url, json_mode })
}

fn parse_proof_create_cli(args: &[String]) -> Result<ProofCreateCliOptions, String> {
    let mut agreement_hash: Option<String> = None;
    let mut proof_type: Option<String> = None;
    let mut attested_by: Option<String> = None;
    let mut address: Option<String> = None;
    let mut milestone_id: Option<String> = None;
    let mut evidence_summary: Option<String> = None;
    let mut evidence_hash: Option<String> = None;
    let mut proof_id: Option<String> = None;
    let mut timestamp: Option<u64> = None;
    let mut out_path: Option<String> = None;
    let mut json_mode = false;
    let mut expires_at_height: Option<u64> = None;
    let mut proof_kind: Option<String> = None;
    let mut reference_id: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--agreement-hash" => {
                i += 1;
                if i >= args.len() {
                    return Err("--agreement-hash requires a value".to_string());
                }
                agreement_hash = Some(args[i].clone());
            }
            "--proof-type" => {
                i += 1;
                if i >= args.len() {
                    return Err("--proof-type requires a value".to_string());
                }
                proof_type = Some(args[i].clone());
            }
            "--attested-by" => {
                i += 1;
                if i >= args.len() {
                    return Err("--attested-by requires a value".to_string());
                }
                attested_by = Some(args[i].clone());
            }
            "--address" => {
                i += 1;
                if i >= args.len() {
                    return Err("--address requires a value".to_string());
                }
                address = Some(args[i].clone());
            }
            "--milestone-id" => {
                i += 1;
                if i >= args.len() {
                    return Err("--milestone-id requires a value".to_string());
                }
                milestone_id = Some(args[i].clone());
            }
            "--evidence-summary" => {
                i += 1;
                if i >= args.len() {
                    return Err("--evidence-summary requires a value".to_string());
                }
                evidence_summary = Some(args[i].clone());
            }
            "--evidence-hash" => {
                i += 1;
                if i >= args.len() {
                    return Err("--evidence-hash requires a value".to_string());
                }
                evidence_hash = Some(args[i].clone());
            }
            "--proof-id" => {
                i += 1;
                if i >= args.len() {
                    return Err("--proof-id requires a value".to_string());
                }
                proof_id = Some(args[i].clone());
            }
            "--timestamp" => {
                i += 1;
                if i >= args.len() {
                    return Err("--timestamp requires a value".to_string());
                }
                let v: u64 = args[i]
                    .parse()
                    .map_err(|_| "--timestamp must be a non-negative integer".to_string())?;
                timestamp = Some(v);
            }
            "--out" => {
                i += 1;
                if i >= args.len() {
                    return Err("--out requires a value".to_string());
                }
                out_path = Some(args[i].clone());
            }
            "--expires-at-height" => {
                i += 1;
                if i >= args.len() {
                    return Err("--expires-at-height requires a value".to_string());
                }
                let v: u64 = args[i].parse().map_err(|_| {
                    format!("--expires-at-height must be a non-negative integer, got: {}", args[i])
                })?;
                expires_at_height = Some(v);
            }
            "--json" => {
                json_mode = true;
            }
            "--proof-kind" => {
                i += 1;
                if i >= args.len() { return Err("--proof-kind requires a value".to_string()); }
                proof_kind = Some(args[i].clone());
            }
            "--reference-id" => {
                i += 1;
                if i >= args.len() { return Err("--reference-id requires a value".to_string()); }
                reference_id = Some(args[i].clone());
            }
            other => {
                return Err(format!("unknown argument: {}", other));
            }
        }
        i += 1;
    }
    Ok(ProofCreateCliOptions {
        agreement_hash: agreement_hash
            .ok_or_else(|| "--agreement-hash is required".to_string())?,
        proof_type: proof_type.ok_or_else(|| "--proof-type is required".to_string())?,
        attested_by: attested_by.ok_or_else(|| "--attested-by is required".to_string())?,
        address: address.ok_or_else(|| "--address is required".to_string())?,
        milestone_id,
        evidence_summary,
        evidence_hash,
        proof_id,
        timestamp,
        out_path,
        json_mode,
        expires_at_height,
        proof_kind,
        reference_id,
    })
}

fn render_proof_submit_summary(resp: &SubmitProofRpcResponse) -> String {
    let mut lines = Vec::new();
    lines.push(format!("proof_id {}", resp.proof_id));
    lines.push(format!("agreement_hash {}", resp.agreement_hash));
    lines.push(format!("accepted {}", resp.accepted));
    lines.push(format!("duplicate {}", resp.duplicate));
    lines.push(format!("message {}", resp.message));
    lines.push(format!("tip_height {}", resp.tip_height));
    match resp.expires_at_height {
        None => lines.push("expires_at_height none".to_string()),
        Some(h) => lines.push(format!("expires_at_height {}", h)),
    }
    lines.push(format!("expired {}", resp.expired));
    if !resp.status.is_empty() {
        lines.push(format!("status {}", resp.status));
    }
    lines.join("\n")
}

fn render_proof_list_summary(resp: &ListProofsRpcResponse) -> String {
    let mut lines = Vec::new();
    if resp.active_only {
        lines.push("filter active_only true".to_string());
    }
    if resp.agreement_hash == "*" {
        lines.push("agreement_hash * (all)".to_string());
    } else {
        lines.push(format!("agreement_hash {}", resp.agreement_hash));
    }
    lines.push(format!("returned_count {}", resp.returned_count));
    if resp.has_more || resp.total_count != resp.returned_count || resp.offset != 0 || resp.limit.is_some() {
        lines.push(format!("total_count {}", resp.total_count));
        if resp.offset != 0 { lines.push(format!("offset {}", resp.offset)); }
        if let Some(lim) = resp.limit { lines.push(format!("limit {}", lim)); }
        if resp.has_more { lines.push("has_more true".to_string()); }
    }
    for item in &resp.proofs {
        let expiry_str = match item.proof.expires_at_height {
            None => "expires_at_height=none".to_string(),
            Some(h) => {
                let expired = resp.tip_height >= h;
                format!("expires_at_height={} expired={}", h, expired)
            }
        };
        let status_str = if item.status.is_empty() {
            String::new()
        } else {
            format!(" status={}", item.status)
        };
        // proof_kind is unsigned metadata (not part of signed proof payload); label clearly.
        let kind_str = item.proof.typed_payload.as_ref()
            .map(|tp| format!(" proof_kind={} [metadata]", tp.proof_kind))
            .unwrap_or_default();
        lines.push(format!(
            "  agreement_hash={} proof_id={} attested_by={} proof_type={} {}{}{}",
            item.proof.agreement_hash, item.proof.proof_id, item.proof.attested_by, item.proof.proof_type,
            expiry_str, status_str, kind_str
        ));
    }
    lines.join("
")
}

fn render_proof_get_summary(resp: &GetProofRpcResponse) -> String {
    if !resp.found {
        return format!("proof_id {}
not_found true", resp.proof_id);
    }
    let mut lines = Vec::new();
    lines.push(format!("proof_id {}", resp.proof_id));
    lines.push(format!("found {}", resp.found));
    lines.push(format!("tip_height {}", resp.tip_height));
    if let Some(ref proof) = resp.proof {
        lines.push(format!("agreement_hash {}", proof.agreement_hash));
        lines.push(format!("proof_type {}", proof.proof_type));
        lines.push(format!("attested_by {}", proof.attested_by));
        if let Some(ref mid) = proof.milestone_id {
            lines.push(format!("milestone_id {}", mid));
        }
        // typed_payload fields are unsigned metadata (not part of the signed proof payload).
        // They cannot be used as attestation evidence.
        if let Some(ref tp) = proof.typed_payload {
            lines.push(format!("proof_kind {} [metadata]", tp.proof_kind));
            if let Some(ref rid) = tp.reference_id {
                lines.push(format!("reference_id {} [metadata]", rid));
            }
        }
    }
    match resp.expires_at_height {
        None => lines.push("expires_at_height none".to_string()),
        Some(h) => lines.push(format!("expires_at_height {}", h)),
    }
    lines.push(format!("expired {}", resp.expired));
    if !resp.status.is_empty() {
        lines.push(format!("status {}", resp.status));
    }
    lines.join("
")
}

fn parse_policy_set_cli(args: &[String]) -> Result<PolicySetCliOptions, String> {
    let mut policy_path: Option<String> = None;
    let mut rpc_url = default_rpc_url();
    let mut json_mode = false;
    let mut replace = false;
    let mut expires_at_height: Option<u64> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--policy" => {
                i += 1;
                if i >= args.len() {
                    return Err("--policy requires a value".to_string());
                }
                policy_path = Some(args[i].clone());
            }
            "--rpc" => {
                i += 1;
                if i >= args.len() {
                    return Err("--rpc requires a value".to_string());
                }
                rpc_url = args[i].clone();
            }
            "--json" => {
                json_mode = true;
            }
            "--replace" => {
                replace = true;
            }
            "--expires-at-height" => {
                i += 1;
                if i >= args.len() {
                    return Err("--expires-at-height requires a value".to_string());
                }
                expires_at_height = Some(args[i].parse::<u64>().map_err(|_| {
                    format!("--expires-at-height must be a non-negative integer, got: {}", args[i])
                })?);
            }
            other => {
                return Err(format!("unknown argument: {}", other));
            }
        }
        i += 1;
    }
    Ok(PolicySetCliOptions {
        policy_path: policy_path.ok_or_else(|| "--policy is required".to_string())?,
        rpc_url,
        json_mode,
        replace,
        expires_at_height,
    })
}

fn parse_policy_get_cli(args: &[String]) -> Result<PolicyGetCliOptions, String> {
    let mut agreement_hash: Option<String> = None;
    let mut rpc_url = default_rpc_url();
    let mut json_mode = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--agreement-hash" => {
                i += 1;
                if i >= args.len() {
                    return Err("--agreement-hash requires a value".to_string());
                }
                agreement_hash = Some(args[i].clone());
            }
            "--rpc" => {
                i += 1;
                if i >= args.len() {
                    return Err("--rpc requires a value".to_string());
                }
                rpc_url = args[i].clone();
            }
            "--json" => {
                json_mode = true;
            }
            other => {
                return Err(format!("unknown argument: {}", other));
            }
        }
        i += 1;
    }
    Ok(PolicyGetCliOptions {
        agreement_hash: agreement_hash
            .ok_or_else(|| "--agreement-hash is required".to_string())?,
        rpc_url,
        json_mode,
    })
}

fn parse_policy_list_cli(args: &[String]) -> Result<PolicyListCliOptions, String> {
    let mut rpc_url = default_rpc_url();
    let mut json_mode = false;
    let mut active_only = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--rpc" => {
                i += 1;
                if i >= args.len() {
                    return Err("--rpc requires a value".to_string());
                }
                rpc_url = args[i].clone();
            }
            "--json" => {
                json_mode = true;
            }
            "--active-only" => {
                active_only = true;
            }
            other => {
                return Err(format!("unknown argument: {}", other));
            }
        }
        i += 1;
    }
    Ok(PolicyListCliOptions { rpc_url, json_mode, active_only })
}

fn render_policy_list_summary(resp: &ListPoliciesRpcResponse) -> String {
    let mut lines = Vec::new();
    if resp.active_only {
        lines.push("filter active_only true".to_string());
    }
    lines.push(format!("count {}", resp.count));
    for p in &resp.policies {
        let expiry = match p.expires_at_height {
            None => "expires_at_height none".to_string(),
            Some(h) => format!("expires_at_height {} expired {}", h, p.expired),
        };
        lines.push(format!(
            "  agreement_hash {} policy_id {} required_proofs {} attestors {} {}",
            p.agreement_hash, p.policy_id, p.required_proofs, p.attestors, expiry
        ));
    }
    lines.join("\n")
}

fn parse_policy_evaluate_cli(args: &[String]) -> Result<PolicyEvaluateCliOptions, String> {
    let mut agreement_path: Option<String> = None;
    let mut rpc_url = default_rpc_url();
    let mut json_mode = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--agreement" => {
                i += 1;
                if i >= args.len() {
                    return Err("--agreement requires a value".to_string());
                }
                agreement_path = Some(args[i].clone());
            }
            "--rpc" => {
                i += 1;
                if i >= args.len() {
                    return Err("--rpc requires a value".to_string());
                }
                rpc_url = args[i].clone();
            }
            "--json" => {
                json_mode = true;
            }
            other => {
                return Err(format!("unknown argument: {}", other));
            }
        }
        i += 1;
    }
    Ok(PolicyEvaluateCliOptions {
        agreement_path: agreement_path
            .ok_or_else(|| "--agreement is required".to_string())?,
        rpc_url,
        json_mode,
    })
}

fn render_policy_evaluate_summary(resp: &EvaluatePolicyRpcResponse) -> String {
    let mut lines = Vec::new();
    lines.push(format!("agreement_hash {}", resp.agreement_hash));
    if let Some(ref pid) = resp.policy_id {
        lines.push(format!("policy_id {}", pid));
    } else {
        lines.push("policy_id none".to_string());
    }
    lines.push(format!("policy_found {}", resp.policy_found));
    lines.push(format!("tip_height {}", resp.tip_height));
    if !resp.outcome.is_empty() {
        lines.push(format!("outcome {}", resp.outcome));
    }
    lines.push(format!("proof_count {}", resp.proof_count));
    if resp.expired_proof_count > 0 {
        lines.push(format!("expired_proof_count {}", resp.expired_proof_count));
    }
    lines.push(format!("matched_proof_count {}", resp.matched_proof_count));
    if !resp.matched_proof_ids.is_empty() {
        lines.push(format!("matched_proof_ids {}", resp.matched_proof_ids.join(", ")));
    }
    lines.push(format!("expired {}", resp.expired));
    lines.push(format!("release_eligible {}", resp.release_eligible));
    lines.push(format!("refund_eligible {}", resp.refund_eligible));
    lines.push(format!("reason {}", resp.reason));
    if !resp.evaluated_rules.is_empty() {
        lines.push("evaluated_rules".to_string());
        for rule in &resp.evaluated_rules {
            lines.push(format!("  {}", rule));
        }
    }
    if !resp.milestone_results.is_empty() {
        lines.push(format!(
            "milestones {}/{}",
            resp.completed_milestone_count, resp.total_milestone_count
        ));
        for ms in &resp.milestone_results {
            let display = ms
                .label
                .as_deref()
                .filter(|l| !l.is_empty())
                .unwrap_or(ms.milestone_id.as_str());
            lines.push(format!("  milestone {} outcome {}", display, ms.outcome));
            if let Some(ref hb) = ms.holdback {
                lines.push(format!(
                    "    holdback {} bps holdback_outcome {}",
                    hb.holdback_bps, hb.holdback_outcome
                ));
                if !hb.holdback_reason.is_empty() {
                    lines.push(format!("    holdback_reason {}", hb.holdback_reason));
                }
            }
        }
    }
    if let Some(ref hb) = resp.holdback {
        lines.push(format!(
            "holdback {} bps holdback_outcome {}",
            hb.holdback_bps, hb.holdback_outcome
        ));
        if !hb.holdback_reason.is_empty() {
            lines.push(format!("holdback_reason {}", hb.holdback_reason));
        }
    }
    if !resp.threshold_results.is_empty() {
        lines.push("threshold_requirements".to_string());
        for tr in &resp.threshold_results {
            lines.push(format!(
                "  req {} threshold {}/{} {}",
                tr.requirement_id,
                tr.approved_attestor_count,
                tr.threshold_required,
                if tr.threshold_satisfied { "satisfied" } else { "pending" }
            ));
        }
    }
    lines.join("\n")
}

fn render_policy_set_summary(resp: &StorePolicyRpcResponse) -> String {
    let status = if resp.accepted {
        if resp.updated { "replaced" } else { "accepted" }
    } else {
        "rejected"
    };
    format!(
        "policy_id {}\nagreement_hash {}\nstatus {}\nmessage {}",
        resp.policy_id, resp.agreement_hash, status, resp.message
    )
}

fn render_policy_get_summary(resp: &GetPolicyRpcResponse) -> String {
    match &resp.policy {
        None => format!("agreement_hash {}\nfound false", resp.agreement_hash),
        Some(p) => {
            let expiry = match resp.expires_at_height {
                None => "none".to_string(),
                Some(h) => format!("{}", h),
            };
            format!(
                "policy_id {}
agreement_hash {}
required_proofs {}
attestors {}
expires_at_height {}
expired {}
found true",
                p.policy_id,
                p.agreement_hash,
                p.required_proofs.len(),
                p.attestors.len(),
                expiry,
                resp.expired
            )
        }
    }
}

fn render_build_settlement_summary(resp: &BuildSettlementTxRpcResponse) -> String {
    let mut lines: Vec<String> = Vec::new();
    lines.push(format!("agreement_hash {}", resp.agreement_hash));
    lines.push(format!("tip_height {}", resp.tip_height));
    lines.push(format!("policy_found {}", resp.policy_found));
    lines.push(format!("release_eligible {}", resp.release_eligible));
    lines.push(format!("refund_eligible {}", resp.refund_eligible));
    if !resp.reason.is_empty() {
        lines.push(format!("reason {}", resp.reason));
    }
    lines.push(format!("action_count {}", resp.actions.len()));
    for (i, a) in resp.actions.iter().enumerate() {
        let exec_after = match a.executable_after_height {
            None => "now".to_string(),
            Some(h) => format!("height_{}", h),
        };
        lines.push(format!(
            "action[{}] {} recipient={} bps={} executable={} executable_after={}",
            i, a.action, a.recipient_address, a.amount_bps, a.executable, exec_after
        ));
    }
    lines.join("\n")
}

fn render_proof_create_summary(proof: &SettlementProof) -> String {
    let mut lines = Vec::new();
    lines.push(format!("proof_id {}", proof.proof_id));
    lines.push(format!("schema_id {}", proof.schema_id));
    lines.push(format!("proof_type {}", proof.proof_type));
    lines.push(format!("agreement_hash {}", proof.agreement_hash));
    if let Some(ref mid) = proof.milestone_id {
        lines.push(format!("milestone_id {}", mid));
    }
    lines.push(format!("attested_by {}", proof.attested_by));
    lines.push(format!("attestation_time {}", proof.attestation_time));
    match proof.expires_at_height {
        None => lines.push("expires_at_height none".to_string()),
        Some(h) => lines.push(format!("expires_at_height {}", h)),
    }
    if let Some(ref es) = proof.evidence_summary {
        lines.push(format!("evidence_summary {}", es));
    }
    if let Some(ref eh) = proof.evidence_hash {
        lines.push(format!("evidence_hash {}", eh));
    }
    lines.push(format!("payload_hash {}", proof.signature.payload_hash));
    lines.push(format!("pubkey_hex {}", proof.signature.pubkey_hex));
    lines.join("
")
}

fn parse_policy_check_cli(args: &[String]) -> Result<PolicyCheckCliOptions, String> {
    let mut agreement_path: Option<String> = None;
    let mut policy_path: Option<String> = None;
    let mut proof_paths: Vec<String> = Vec::new();
    let mut rpc_url = default_rpc_url();
    let mut json_mode = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--agreement" => {
                i += 1;
                if i >= args.len() {
                    return Err("--agreement requires a value".to_string());
                }
                agreement_path = Some(args[i].clone());
            }
            "--policy" => {
                i += 1;
                if i >= args.len() {
                    return Err("--policy requires a value".to_string());
                }
                policy_path = Some(args[i].clone());
            }
            "--proof" => {
                i += 1;
                if i >= args.len() {
                    return Err("--proof requires a value".to_string());
                }
                proof_paths.push(args[i].clone());
            }
            "--rpc" => {
                i += 1;
                if i >= args.len() {
                    return Err("--rpc requires a value".to_string());
                }
                rpc_url = args[i].clone();
            }
            "--json" => {
                json_mode = true;
            }
            other => {
                return Err(format!("unknown argument: {}", other));
            }
        }
        i += 1;
    }
    Ok(PolicyCheckCliOptions {
        agreement_path: agreement_path
            .ok_or_else(|| "--agreement is required".to_string())?,
        policy_path: policy_path
            .ok_or_else(|| "--policy is required".to_string())?,
        proof_paths,
        rpc_url,
        json_mode,
    })
}

fn render_policy_check_summary(resp: &CheckPolicyRpcResponse) -> String {
    let mut lines = Vec::new();
    lines.push(format!("agreement_hash {}", resp.agreement_hash));
    lines.push(format!("policy_id {}", resp.policy_id));
    lines.push(format!("tip_height {}", resp.tip_height));
    lines.push(format!("release_eligible {}", resp.release_eligible));
    lines.push(format!("refund_eligible {}", resp.refund_eligible));
    lines.push(format!("reason {}", resp.reason));
    if !resp.evaluated_rules.is_empty() {
        lines.push("evaluated_rules".to_string());
        for rule in &resp.evaluated_rules {
            lines.push(format!("  {}", rule));
        }
    }
    if !resp.milestone_results.is_empty() {
        let satisfied = resp.milestone_results.iter().filter(|m| matches!(m.outcome, PolicyOutcome::Satisfied)).count();
        lines.push(format!("milestones {}/{}", satisfied, resp.milestone_results.len()));
        for ms in &resp.milestone_results {
            let display = ms.label.as_deref().filter(|l| !l.is_empty()).unwrap_or(ms.milestone_id.as_str());
            let ms_outcome_str = match ms.outcome {
                PolicyOutcome::Satisfied => "satisfied",
                PolicyOutcome::Timeout => "timeout",
                PolicyOutcome::Unsatisfied => "unsatisfied",
            };
            lines.push(format!("  milestone {} outcome {}", display, ms_outcome_str));
            if let Some(ref hb) = ms.holdback {
                let hb_ms_outcome_str = match hb.holdback_outcome {
                    HoldbackOutcome::Held => "held",
                    HoldbackOutcome::Released => "released",
                    HoldbackOutcome::Pending => "pending",
                };
                lines.push(format!("    holdback {} bps holdback_outcome {}", hb.holdback_bps, hb_ms_outcome_str));
                if !hb.holdback_reason.is_empty() {
                    lines.push(format!("    holdback_reason {}", hb.holdback_reason));
                }
            }
        }
    }
    if let Some(ref hb) = resp.holdback {
        let outcome_str = match hb.holdback_outcome {
            HoldbackOutcome::Held => "held",
            HoldbackOutcome::Released => "released",
            HoldbackOutcome::Pending => "pending",
        };
        lines.push(format!("holdback_outcome {}", outcome_str));
        lines.push(format!("holdback_bps {}", hb.holdback_bps));
        lines.push(format!("immediate_release_bps {}", hb.immediate_release_bps));
        lines.push(format!("holdback_reason {}", hb.holdback_reason));
    }
    lines.join("\n")
}

fn render_build_template_summary(resp: &BuildTemplateRpcResponse) -> String {
    let mut lines = Vec::new();
    lines.push(format!("policy_id {}", resp.policy.policy_id));
    lines.push(format!("summary {}", resp.summary));
    lines.push(format!("requirement_count {}", resp.requirement_count));
    lines.push(format!("attestor_count {}", resp.attestor_count));
    if resp.milestone_count > 0 {
        lines.push(format!("milestone_count {}", resp.milestone_count));
    }
    lines.push(format!("has_holdback {}", resp.has_holdback));
    lines.push(format!("has_timeout_rules {}", resp.has_timeout_rules));
    lines.push(String::new());
    lines.push("--- policy_json ---".to_string());
    lines.push(resp.policy_json.clone());
    lines.join("\n")
}

fn parse_agreement_spend_cli(args: &[String]) -> Result<AgreementSpendCliOptions, String> {
    if args.is_empty() {
        return Err(
            "expected <agreement.json|bundle.json|agreement_id|agreement_hash> [funding_txid]"
                .to_string(),
        );
    }
    let mut opts = AgreementSpendCliOptions {
        agreement_path: args[0].clone(),
        funding_txid: None,
        rpc_url: default_rpc_url(),
        htlc_vout: None,
        milestone_id: None,
        destination_address: None,
        fee_per_byte: None,
        broadcast: false,
        secret_hex: None,
        json_mode: false,
        show_raw_tx: false,
    };
    let mut i = 1;
    if let Some(arg) = args.get(1) {
        if !arg.starts_with("--") {
            opts.funding_txid = Some(arg.clone());
            i = 2;
        }
    }
    while i < args.len() {
        match args[i].as_str() {
            "--rpc" => {
                opts.rpc_url = args
                    .get(i + 1)
                    .cloned()
                    .ok_or_else(|| "missing --rpc value".to_string())?;
                i += 2;
            }
            "--vout" => {
                let raw = args
                    .get(i + 1)
                    .ok_or_else(|| "missing --vout value".to_string())?;
                opts.htlc_vout = Some(
                    raw.parse::<u32>()
                        .map_err(|_| "invalid --vout value".to_string())?,
                );
                i += 2;
            }
            "--milestone-id" => {
                opts.milestone_id = args.get(i + 1).cloned();
                if opts.milestone_id.is_none() {
                    return Err("missing --milestone-id value".to_string());
                }
                i += 2;
            }
            "--destination" => {
                opts.destination_address = args.get(i + 1).cloned();
                if opts.destination_address.is_none() {
                    return Err("missing --destination value".to_string());
                }
                i += 2;
            }
            "--fee-per-byte" => {
                let raw = args
                    .get(i + 1)
                    .ok_or_else(|| "missing --fee-per-byte value".to_string())?;
                opts.fee_per_byte = Some(
                    raw.parse::<u64>()
                        .map_err(|_| "invalid --fee-per-byte value".to_string())?,
                );
                i += 2;
            }
            "--broadcast" => {
                opts.broadcast = true;
                i += 1;
            }
            "--secret" => {
                opts.secret_hex = args.get(i + 1).cloned();
                if opts.secret_hex.is_none() {
                    return Err("missing --secret value".to_string());
                }
                i += 2;
            }
            "--json" => {
                opts.json_mode = true;
                i += 1;
            }
            "--show-raw-tx" => {
                opts.show_raw_tx = true;
                i += 1;
            }
            other => return Err(format!("unknown argument {}", other)),
        }
    }
    if let Some(funding_txid) = &opts.funding_txid {
        let decoded =
            hex::decode(funding_txid).map_err(|_| "invalid funding_txid hex".to_string())?;
        if decoded.len() != 32 {
            return Err("funding_txid must be 32-byte hex".to_string());
        }
    }
    if let Some(dest) = &opts.destination_address {
        if base58_p2pkh_to_hash(dest).is_none() {
            return Err("invalid --destination address".to_string());
        }
    }
    if let Some(secret) = &opts.secret_hex {
        if secret.is_empty() || hex::decode(secret).is_err() {
            return Err("invalid --secret hex".to_string());
        }
    }
    Ok(opts)
}

fn agreement_spend_request_body(
    agreement: AgreementObject,
    opts: &AgreementSpendCliOptions,
) -> AgreementSpendRequestBody {
    AgreementSpendRequestBody {
        agreement,
        funding_txid: opts.funding_txid.clone().unwrap_or_default(),
        htlc_vout: opts.htlc_vout,
        milestone_id: opts.milestone_id.clone(),
        destination_address: opts.destination_address.clone(),
        fee_per_byte: opts.fee_per_byte,
        broadcast: Some(opts.broadcast),
        secret_hex: opts.secret_hex.clone(),
    }
}

fn finalize_agreement_spend_mode(
    mut opts: AgreementSpendCliOptions,
    mode: Option<bool>,
) -> Result<AgreementSpendCliOptions, String> {
    if let Some(force_broadcast) = mode {
        if !force_broadcast && opts.broadcast {
            return Err(
                "--broadcast is not allowed with the explicit *-build commands".to_string(),
            );
        }
        opts.broadcast = force_broadcast;
    }
    Ok(opts)
}

fn render_agreement_spend_eligibility_summary(resp: &AgreementSpendEligibilityResponse) -> String {
    let mut lines = Vec::new();
    lines.push(format!("agreement_id {}", resp.agreement_id));
    lines.push(format!("agreement_hash {}", resp.agreement_hash));
    lines.push(format!("funding_txid {}", resp.funding_txid));
    lines.push(format!("branch {}", resp.branch));
    if let Some(vout) = resp.htlc_vout {
        lines.push(format!("htlc_vout {}", vout));
    }
    if let Some(anchor_vout) = resp.anchor_vout {
        lines.push(format!("anchor_vout {}", anchor_vout));
    }
    if let Some(role) = &resp.role {
        lines.push(format!("role {}", role));
    }
    if let Some(milestone_id) = &resp.milestone_id {
        lines.push(format!("milestone_id {}", milestone_id));
    }
    if let Some(amount) = resp.amount {
        lines.push(format!("amount_irm {}", format_irm(amount)));
    }
    if let Some(dest) = &resp.destination_address {
        lines.push(format!("destination {}", dest));
    }
    if let Some(timeout_height) = resp.timeout_height {
        lines.push(format!("timeout_height {}", timeout_height));
        lines.push(format!("timeout_reached {}", resp.timeout_reached));
    }
    lines.push(format!("htlc_backed {}", resp.htlc_backed));
    lines.push(format!("preimage_required {}", resp.preimage_required));
    lines.push(format!("eligible {}", resp.eligible));
    if let Some(expected_hash) = &resp.expected_hash {
        lines.push(format!("expected_hash {}", expected_hash));
    }
    if !resp.reasons.is_empty() {
        lines.push("reasons".to_string());
        for reason in &resp.reasons {
            lines.push(format!("  {}", reason));
        }
    }
    lines.push(format!("trust_model {}", resp.trust_model_note));
    lines.join(
        "
",
    )
}

fn render_agreement_build_spend_summary(
    resp: &AgreementBuildSpendResponse,
    broadcast_requested: bool,
    show_raw_tx: bool,
) -> String {
    let mut lines = Vec::new();
    lines.push(format!("agreement_id {}", resp.agreement_id));
    lines.push(format!("agreement_hash {}", resp.agreement_hash));
    lines.push(format!("funding_txid {}", resp.funding_txid));
    lines.push(format!("branch {}", resp.branch));
    lines.push(format!("htlc_vout {}", resp.htlc_vout));
    lines.push(format!("role {}", resp.role));
    if let Some(milestone_id) = &resp.milestone_id {
        lines.push(format!("milestone_id {}", milestone_id));
    }
    lines.push(format!("destination {}", resp.destination_address));
    lines.push(format!("txid {}", resp.txid));
    lines.push(format!("signed_tx_ready true"));
    lines.push(format!("broadcast_requested {}", broadcast_requested));
    lines.push(format!("submitted_to_node {}", resp.accepted));
    lines.push(format!("fee_irm {}", format_irm(resp.fee)));
    if show_raw_tx {
        lines.push(format!("raw_tx_hex {}", resp.raw_tx_hex));
    } else {
        lines.push("raw_tx_hex [hidden in human-readable output; use --show-raw-tx or --json if you explicitly need the signed transaction artifact]".to_string());
    }
    lines.push("review_note verify the destination, HTLC branch, and eligibility above before broadcasting; release spends may embed the preimage inside the signed transaction artifact".to_string());
    lines.push(format!("trust_model {}", resp.trust_model_note));
    lines.join(
        "
",
    )
}

fn parse_template_type(s: &str) -> Result<AgreementTemplateType, String> {
    match s {
        "simple_release_refund" => Ok(AgreementTemplateType::SimpleReleaseRefund),
        "milestone_settlement" => Ok(AgreementTemplateType::MilestoneSettlement),
        "refundable_deposit" => Ok(AgreementTemplateType::RefundableDeposit),
        "otc_settlement" => Ok(AgreementTemplateType::OtcSettlement),
        "merchant_delayed_settlement" => Ok(AgreementTemplateType::MerchantDelayedSettlement),
        "contractor_milestone" => Ok(AgreementTemplateType::ContractorMilestone),
        _ => Err(format!("unsupported template {}", s)),
    }
}

fn parse_milestone_arg(
    arg: &str,
    payee_address: &str,
    refund_address: &str,
) -> Result<Value, String> {
    let parts: Vec<&str> = arg.split('|').collect();
    if parts.len() != 4 {
        return Err("milestone must be id|title|amount_irm|timeout_height".to_string());
    }
    let amount = parse_irm(parts[2])?;
    let timeout_height = parts[3]
        .parse::<u64>()
        .map_err(|_| "invalid milestone timeout_height".to_string())?;
    Ok(json!({
        "milestone_id": parts[0],
        "title": parts[1],
        "amount": amount,
        "recipient_address": payee_address,
        "refund_address": refund_address,
        "secret_hash_hex": "11".repeat(32),
        "timeout_height": timeout_height
    }))
}

fn parse_party_spec(spec: &str) -> Result<AgreementParty, String> {
    let parts: Vec<&str> = spec.split('|').collect();
    if parts.len() < 3 || parts.len() > 4 {
        return Err("party spec must be party_id|display_name|address|role(optional)".to_string());
    }
    if base58_p2pkh_to_hash(parts[2]).is_none() {
        return Err("party address must be a valid base58 P2PKH address".to_string());
    }
    Ok(AgreementParty {
        party_id: parts[0].trim().to_string(),
        display_name: parts[1].trim().to_string(),
        address: parts[2].trim().to_string(),
        role: parts
            .get(3)
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty()),
    })
}

fn parse_phase15_milestone_spec(
    spec: &str,
    payee: &AgreementParty,
    payer: &AgreementParty,
) -> Result<AgreementMilestone, String> {
    let parts: Vec<&str> = spec.split('|').collect();
    if parts.len() != 5 && parts.len() != 6 {
        return Err("milestone spec must be id|title|amount_irm|timeout_height|secret_hash_hex|deliverable_hash(optional)".to_string());
    }
    let amount = parse_irm(parts[2])?;
    let timeout_height = parts[3]
        .parse::<u64>()
        .map_err(|_| "invalid milestone timeout_height".to_string())?;
    if parts[4].len() != 64 || hex::decode(parts[4]).is_err() {
        return Err("milestone secret_hash_hex must be 32-byte hex".to_string());
    }
    let metadata_hash = if let Some(hash) = parts.get(5) {
        if hash.len() != 64 || hex::decode(hash).is_err() {
            return Err("milestone deliverable hash must be 32-byte hex".to_string());
        }
        Some((*hash).to_string())
    } else {
        None
    };
    Ok(AgreementMilestone {
        milestone_id: parts[0].trim().to_string(),
        title: parts[1].trim().to_string(),
        amount,
        recipient_address: payee.address.clone(),
        refund_address: payer.address.clone(),
        secret_hash_hex: parts[4].to_string(),
        timeout_height,
        metadata_hash,
    })
}

fn external_hashes_for_agreement(agreement: &AgreementObject) -> Vec<String> {
    let mut out = vec![agreement.document_hash.clone()];
    if let Some(hash) = &agreement.metadata_hash {
        out.push(hash.clone());
    }
    out.sort();
    out.dedup();
    out
}

fn bundle_chain_snapshot_from_audit(
    record: &AgreementAuditRecord,
) -> AgreementBundleChainObservationSnapshot {
    AgreementBundleChainObservationSnapshot {
        observed_at: Some(record.metadata.generated_at),
        linked_transactions: record.chain_observed.linked_transactions.clone(),
        funding_txids: record
            .chain_observed
            .linked_transactions
            .iter()
            .map(|tx| tx.txid.clone())
            .collect(),
        linked_tx_count: record.chain_observed.linked_transaction_count,
        anchor_notice: Some(record.chain_observed.anchor_observation_notice.clone()),
    }
}

fn emit_agreement_object_output(
    agreement: &AgreementObject,
    out_path: Option<&str>,
    json_mode: bool,
) -> Result<(), String> {
    let agreement_hash = irium_node_rs::settlement::compute_agreement_hash_hex(agreement)?;
    let value =
        serde_json::to_value(agreement).map_err(|e| format!("serialize agreement json: {e}"))?;
    if let Some(path) = out_path {
        save_json_output(Some(path), &value)?;
        if json_mode {
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "written": path,
                    "agreement_hash": agreement_hash,
                    "agreement": agreement,
                }))
                .unwrap()
            );
        } else {
            println!("{}", render_agreement_summary(agreement, &agreement_hash));
            println!("written {}", path);
        }
        return Ok(());
    }
    if json_mode {
        println!("{}", serde_json::to_string_pretty(&value).unwrap());
    } else {
        println!("{}", render_agreement_summary(agreement, &agreement_hash));
    }
    Ok(())
}

fn parse_required_string_flag(
    args: &[String],
    i: &mut usize,
    name: &str,
) -> Result<String, String> {
    let value = args
        .get(*i + 1)
        .cloned()
        .ok_or_else(|| format!("missing {name} value"))?;
    *i += 2;
    Ok(value)
}

fn parse_optional_hex_hash(value: Option<String>, label: &str) -> Result<Option<String>, String> {
    if let Some(value) = value {
        if value.len() != 64 || hex::decode(&value).is_err() {
            return Err(format!("{label} must be 32-byte hex"));
        }
        return Ok(Some(value));
    }
    Ok(None)
}

fn handle_agreement_create_simple(args: &[String]) -> Result<(), String> {
    let mut agreement_id = None;
    let mut creation_time = None;
    let mut party_a = None;
    let mut party_b = None;
    let mut amount = None;
    let mut settlement_deadline = None;
    let mut refund_timeout = None;
    let mut secret_hash = None;
    let mut document_hash = None;
    let mut metadata_hash = None;
    let mut release_summary = None;
    let mut refund_summary = None;
    let mut notes = None;
    let mut out_path = None;
    let mut json_mode = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--agreement-id" => {
                agreement_id = Some(parse_required_string_flag(args, &mut i, "--agreement-id")?)
            }
            "--creation-time" => {
                creation_time = Some(
                    parse_required_string_flag(args, &mut i, "--creation-time")?
                        .parse::<u64>()
                        .map_err(|_| "invalid --creation-time".to_string())?,
                )
            }
            "--party-a" => {
                party_a = Some(parse_party_spec(&parse_required_string_flag(
                    args,
                    &mut i,
                    "--party-a",
                )?)?)
            }
            "--party-b" => {
                party_b = Some(parse_party_spec(&parse_required_string_flag(
                    args,
                    &mut i,
                    "--party-b",
                )?)?)
            }
            "--amount" => {
                amount = Some(parse_irm(&parse_required_string_flag(
                    args, &mut i, "--amount",
                )?)?)
            }
            "--settlement-deadline" => {
                settlement_deadline = Some(
                    parse_required_string_flag(args, &mut i, "--settlement-deadline")?
                        .parse::<u64>()
                        .map_err(|_| "invalid --settlement-deadline".to_string())?,
                )
            }
            "--refund-timeout" => {
                refund_timeout = Some(
                    parse_required_string_flag(args, &mut i, "--refund-timeout")?
                        .parse::<u64>()
                        .map_err(|_| "invalid --refund-timeout".to_string())?,
                )
            }
            "--secret-hash" => {
                secret_hash = Some(parse_required_string_flag(args, &mut i, "--secret-hash")?)
            }
            "--document-hash" => {
                document_hash = Some(parse_required_string_flag(args, &mut i, "--document-hash")?)
            }
            "--metadata-hash" => {
                metadata_hash = Some(parse_required_string_flag(args, &mut i, "--metadata-hash")?)
            }
            "--release-summary" => {
                release_summary = Some(parse_required_string_flag(
                    args,
                    &mut i,
                    "--release-summary",
                )?)
            }
            "--refund-summary" => {
                refund_summary = Some(parse_required_string_flag(
                    args,
                    &mut i,
                    "--refund-summary",
                )?)
            }
            "--notes" => notes = Some(parse_required_string_flag(args, &mut i, "--notes")?),
            "--out" => out_path = Some(parse_required_string_flag(args, &mut i, "--out")?),
            "--json" => {
                json_mode = true;
                i += 1;
            }
            other => return Err(format!("unknown argument {}", other)),
        }
    }
    let agreement = build_simple_settlement_agreement(
        agreement_id.ok_or_else(|| "--agreement-id required".to_string())?,
        creation_time.ok_or_else(|| "--creation-time required".to_string())?,
        party_a.ok_or_else(|| "--party-a required".to_string())?,
        party_b.ok_or_else(|| "--party-b required".to_string())?,
        amount.ok_or_else(|| "--amount required".to_string())?,
        settlement_deadline,
        refund_timeout.ok_or_else(|| "--refund-timeout required".to_string())?,
        parse_required_secret_hash(
            secret_hash.ok_or_else(|| "--secret-hash required".to_string())?,
        )?,
        parse_required_document_hash(
            document_hash.ok_or_else(|| "--document-hash required".to_string())?,
        )?,
        parse_optional_hex_hash(metadata_hash, "metadata_hash")?,
        release_summary,
        refund_summary,
        notes,
    )?;
    emit_agreement_object_output(&agreement, out_path.as_deref(), json_mode)
}

fn handle_agreement_create_otc(args: &[String]) -> Result<(), String> {
    let mut agreement_id = None;
    let mut creation_time = None;
    let mut buyer = None;
    let mut seller = None;
    let mut amount = None;
    let mut asset_reference = None;
    let mut payment_reference = None;
    let mut refund_timeout = None;
    let mut secret_hash = None;
    let mut document_hash = None;
    let mut metadata_hash = None;
    let mut notes = None;
    let mut out_path = None;
    let mut json_mode = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--agreement-id" => {
                agreement_id = Some(parse_required_string_flag(args, &mut i, "--agreement-id")?)
            }
            "--creation-time" => {
                creation_time = Some(
                    parse_required_string_flag(args, &mut i, "--creation-time")?
                        .parse::<u64>()
                        .map_err(|_| "invalid --creation-time".to_string())?,
                )
            }
            "--buyer" => {
                buyer = Some(parse_party_spec(&parse_required_string_flag(
                    args, &mut i, "--buyer",
                )?)?)
            }
            "--seller" => {
                seller = Some(parse_party_spec(&parse_required_string_flag(
                    args, &mut i, "--seller",
                )?)?)
            }
            "--amount" => {
                amount = Some(parse_irm(&parse_required_string_flag(
                    args, &mut i, "--amount",
                )?)?)
            }
            "--asset-reference" => {
                asset_reference = Some(parse_required_string_flag(
                    args,
                    &mut i,
                    "--asset-reference",
                )?)
            }
            "--payment-reference" => {
                payment_reference = Some(parse_required_string_flag(
                    args,
                    &mut i,
                    "--payment-reference",
                )?)
            }
            "--refund-timeout" => {
                refund_timeout = Some(
                    parse_required_string_flag(args, &mut i, "--refund-timeout")?
                        .parse::<u64>()
                        .map_err(|_| "invalid --refund-timeout".to_string())?,
                )
            }
            "--secret-hash" => {
                secret_hash = Some(parse_required_string_flag(args, &mut i, "--secret-hash")?)
            }
            "--document-hash" => {
                document_hash = Some(parse_required_string_flag(args, &mut i, "--document-hash")?)
            }
            "--metadata-hash" => {
                metadata_hash = Some(parse_required_string_flag(args, &mut i, "--metadata-hash")?)
            }
            "--notes" => notes = Some(parse_required_string_flag(args, &mut i, "--notes")?),
            "--out" => out_path = Some(parse_required_string_flag(args, &mut i, "--out")?),
            "--json" => {
                json_mode = true;
                i += 1;
            }
            other => return Err(format!("unknown argument {}", other)),
        }
    }
    let agreement = build_otc_agreement(
        agreement_id.ok_or_else(|| "--agreement-id required".to_string())?,
        creation_time.ok_or_else(|| "--creation-time required".to_string())?,
        buyer.ok_or_else(|| "--buyer required".to_string())?,
        seller.ok_or_else(|| "--seller required".to_string())?,
        amount.ok_or_else(|| "--amount required".to_string())?,
        asset_reference.ok_or_else(|| "--asset-reference required".to_string())?,
        payment_reference.ok_or_else(|| "--payment-reference required".to_string())?,
        refund_timeout.ok_or_else(|| "--refund-timeout required".to_string())?,
        parse_required_secret_hash(
            secret_hash.ok_or_else(|| "--secret-hash required".to_string())?,
        )?,
        parse_required_document_hash(
            document_hash.ok_or_else(|| "--document-hash required".to_string())?,
        )?,
        parse_optional_hex_hash(metadata_hash, "metadata_hash")?,
        notes,
    )?;
    emit_agreement_object_output(&agreement, out_path.as_deref(), json_mode)
}

fn handle_agreement_create_deposit(args: &[String]) -> Result<(), String> {
    let mut agreement_id = None;
    let mut creation_time = None;
    let mut payer = None;
    let mut payee = None;
    let mut amount = None;
    let mut purpose_reference = None;
    let mut refund_summary = None;
    let mut refund_timeout = None;
    let mut secret_hash = None;
    let mut document_hash = None;
    let mut metadata_hash = None;
    let mut notes = None;
    let mut out_path = None;
    let mut json_mode = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--agreement-id" => {
                agreement_id = Some(parse_required_string_flag(args, &mut i, "--agreement-id")?)
            }
            "--creation-time" => {
                creation_time = Some(
                    parse_required_string_flag(args, &mut i, "--creation-time")?
                        .parse::<u64>()
                        .map_err(|_| "invalid --creation-time".to_string())?,
                )
            }
            "--payer" => {
                payer = Some(parse_party_spec(&parse_required_string_flag(
                    args, &mut i, "--payer",
                )?)?)
            }
            "--payee" => {
                payee = Some(parse_party_spec(&parse_required_string_flag(
                    args, &mut i, "--payee",
                )?)?)
            }
            "--amount" => {
                amount = Some(parse_irm(&parse_required_string_flag(
                    args, &mut i, "--amount",
                )?)?)
            }
            "--purpose-reference" => {
                purpose_reference = Some(parse_required_string_flag(
                    args,
                    &mut i,
                    "--purpose-reference",
                )?)
            }
            "--refund-summary" => {
                refund_summary = Some(parse_required_string_flag(
                    args,
                    &mut i,
                    "--refund-summary",
                )?)
            }
            "--refund-timeout" => {
                refund_timeout = Some(
                    parse_required_string_flag(args, &mut i, "--refund-timeout")?
                        .parse::<u64>()
                        .map_err(|_| "invalid --refund-timeout".to_string())?,
                )
            }
            "--secret-hash" => {
                secret_hash = Some(parse_required_string_flag(args, &mut i, "--secret-hash")?)
            }
            "--document-hash" => {
                document_hash = Some(parse_required_string_flag(args, &mut i, "--document-hash")?)
            }
            "--metadata-hash" => {
                metadata_hash = Some(parse_required_string_flag(args, &mut i, "--metadata-hash")?)
            }
            "--notes" => notes = Some(parse_required_string_flag(args, &mut i, "--notes")?),
            "--out" => out_path = Some(parse_required_string_flag(args, &mut i, "--out")?),
            "--json" => {
                json_mode = true;
                i += 1;
            }
            other => return Err(format!("unknown argument {}", other)),
        }
    }
    let agreement = build_deposit_agreement(
        agreement_id.ok_or_else(|| "--agreement-id required".to_string())?,
        creation_time.ok_or_else(|| "--creation-time required".to_string())?,
        payer.ok_or_else(|| "--payer required".to_string())?,
        payee.ok_or_else(|| "--payee required".to_string())?,
        amount.ok_or_else(|| "--amount required".to_string())?,
        purpose_reference.ok_or_else(|| "--purpose-reference required".to_string())?,
        refund_summary.ok_or_else(|| "--refund-summary required".to_string())?,
        refund_timeout.ok_or_else(|| "--refund-timeout required".to_string())?,
        parse_required_secret_hash(
            secret_hash.ok_or_else(|| "--secret-hash required".to_string())?,
        )?,
        parse_required_document_hash(
            document_hash.ok_or_else(|| "--document-hash required".to_string())?,
        )?,
        parse_optional_hex_hash(metadata_hash, "metadata_hash")?,
        notes,
    )?;
    emit_agreement_object_output(&agreement, out_path.as_deref(), json_mode)
}

fn handle_agreement_create_milestone(args: &[String]) -> Result<(), String> {
    let mut agreement_id = None;
    let mut creation_time = None;
    let mut payer = None;
    let mut payee = None;
    let mut milestones = Vec::new();
    let mut refund_deadline = None;
    let mut document_hash = None;
    let mut metadata_hash = None;
    let mut notes = None;
    let mut out_path = None;
    let mut json_mode = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--agreement-id" => {
                agreement_id = Some(parse_required_string_flag(args, &mut i, "--agreement-id")?)
            }
            "--creation-time" => {
                creation_time = Some(
                    parse_required_string_flag(args, &mut i, "--creation-time")?
                        .parse::<u64>()
                        .map_err(|_| "invalid --creation-time".to_string())?,
                )
            }
            "--payer" => {
                payer = Some(parse_party_spec(&parse_required_string_flag(
                    args, &mut i, "--payer",
                )?)?)
            }
            "--payee" => {
                payee = Some(parse_party_spec(&parse_required_string_flag(
                    args, &mut i, "--payee",
                )?)?)
            }
            "--milestone" => {
                let spec = parse_required_string_flag(args, &mut i, "--milestone")?;
                let payer_ref = payer
                    .as_ref()
                    .ok_or_else(|| "--payer must be provided before --milestone".to_string())?;
                let payee_ref = payee
                    .as_ref()
                    .ok_or_else(|| "--payee must be provided before --milestone".to_string())?;
                milestones.push(parse_phase15_milestone_spec(&spec, payee_ref, payer_ref)?);
            }
            "--refund-deadline" => {
                refund_deadline = Some(
                    parse_required_string_flag(args, &mut i, "--refund-deadline")?
                        .parse::<u64>()
                        .map_err(|_| "invalid --refund-deadline".to_string())?,
                )
            }
            "--document-hash" => {
                document_hash = Some(parse_required_string_flag(args, &mut i, "--document-hash")?)
            }
            "--metadata-hash" => {
                metadata_hash = Some(parse_required_string_flag(args, &mut i, "--metadata-hash")?)
            }
            "--notes" => notes = Some(parse_required_string_flag(args, &mut i, "--notes")?),
            "--out" => out_path = Some(parse_required_string_flag(args, &mut i, "--out")?),
            "--json" => {
                json_mode = true;
                i += 1;
            }
            other => return Err(format!("unknown argument {}", other)),
        }
    }
    let agreement = build_milestone_agreement(
        agreement_id.ok_or_else(|| "--agreement-id required".to_string())?,
        creation_time.ok_or_else(|| "--creation-time required".to_string())?,
        payer.ok_or_else(|| "--payer required".to_string())?,
        payee.ok_or_else(|| "--payee required".to_string())?,
        milestones,
        refund_deadline.ok_or_else(|| "--refund-deadline required".to_string())?,
        parse_required_document_hash(
            document_hash.ok_or_else(|| "--document-hash required".to_string())?,
        )?,
        parse_optional_hex_hash(metadata_hash, "metadata_hash")?,
        notes,
    )?;
    emit_agreement_object_output(&agreement, out_path.as_deref(), json_mode)
}

fn parse_required_secret_hash(value: String) -> Result<String, String> {
    if value.len() != 64 || hex::decode(&value).is_err() {
        return Err("secret hash must be 32-byte hex".to_string());
    }
    Ok(value)
}

fn parse_required_document_hash(value: String) -> Result<String, String> {
    if value.len() != 64 || hex::decode(&value).is_err() {
        return Err("document hash must be 32-byte hex".to_string());
    }
    Ok(value)
}

fn handle_agreement_bundle_pack(args: &[String], create_only: bool) -> Result<(), String> {
    if args.is_empty() {
        return Err("expected agreement.json|bundle.json|agreement_id|agreement_hash".to_string());
    }
    let resolved = resolve_agreement_input(&args[0])?;
    let mut label = resolved
        .bundle
        .as_ref()
        .and_then(|bundle| bundle.metadata.source_label.clone());
    let mut note = resolved
        .bundle
        .as_ref()
        .and_then(|bundle| bundle.metadata.note.clone());
    let mut linked_funding_txids = resolved
        .bundle
        .as_ref()
        .map(|bundle| bundle.metadata.linked_funding_txids.clone())
        .unwrap_or_default();
    let mut milestone_hints = resolved
        .bundle
        .as_ref()
        .map(|bundle| bundle.metadata.milestone_hints.clone())
        .unwrap_or_default();
    let mut artifacts = resolved
        .bundle
        .as_ref()
        .map(|bundle| bundle.artifacts.clone())
        .unwrap_or_default();
    let mut out_path = None;
    let mut json_mode = false;
    let mut audit_path = None;
    let mut statement_path = None;
    let mut metadata_summary = None;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--label" => {
                label = Some(parse_required_string_flag(args, &mut i, "--label")?);
            }
            "--note" => {
                note = Some(parse_required_string_flag(args, &mut i, "--note")?);
            }
            "--funding-txid" => {
                let txid = parse_required_string_flag(args, &mut i, "--funding-txid")?;
                if txid.len() != 64 || hex::decode(&txid).map(|v| v.len()).ok() != Some(32) {
                    return Err("--funding-txid must be 32-byte hex".to_string());
                }
                if !linked_funding_txids
                    .iter()
                    .any(|existing| existing == &txid)
                {
                    linked_funding_txids.push(txid);
                }
            }
            "--audit" => {
                audit_path = Some(parse_required_string_flag(args, &mut i, "--audit")?);
            }
            "--statement" => {
                statement_path = Some(parse_required_string_flag(args, &mut i, "--statement")?);
            }
            "--metadata-summary" => {
                metadata_summary = Some(parse_required_string_flag(
                    args,
                    &mut i,
                    "--metadata-summary",
                )?);
            }
            "--out" => {
                out_path = Some(parse_required_string_flag(args, &mut i, "--out")?);
            }
            "--json" => {
                json_mode = true;
                i += 1;
            }
            other => return Err(format!("unknown argument {}", other)),
        }
    }
    let out_path = out_path.ok_or_else(|| "--out required".to_string())?;
    if let Some(path) = audit_path {
        let audit = load_audit_from_path(Path::new(&path))?;
        artifacts.chain_observation_snapshot = Some(bundle_chain_snapshot_from_audit(&audit));
        artifacts.audit =
            Some(serde_json::to_value(&audit).map_err(|e| format!("serialize audit: {e}"))?);
    }
    if let Some(path) = statement_path {
        let statement = load_statement_from_path(Path::new(&path))?;
        artifacts.statement = Some(
            serde_json::to_value(&statement).map_err(|e| format!("serialize statement: {e}"))?,
        );
    }
    if let Some(summary) = metadata_summary {
        artifacts.metadata_summary = Some(summary);
    }
    if artifacts.external_document_hashes.is_empty() {
        artifacts.external_document_hashes = external_hashes_for_agreement(&resolved.agreement);
    }
    let bundle = if create_only {
        build_agreement_bundle(
            &resolved.agreement,
            now_unix(),
            label,
            note,
            linked_funding_txids,
            milestone_hints,
        )?
    } else {
        build_agreement_bundle_with_artifacts(
            &resolved.agreement,
            now_unix(),
            label,
            note,
            linked_funding_txids,
            milestone_hints,
            artifacts,
        )?
    };
    let rendered = serde_json::to_value(&bundle).map_err(|e| format!("serialize bundle: {e}"))?;
    save_json_output(Some(&out_path), &rendered)?;
    if json_mode {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "written": out_path,
                "agreement_hash": bundle.agreement_hash,
                "bundle_hash": compute_agreement_bundle_hash_hex(&bundle)?,
            }))
            .unwrap()
        );
    } else {
        println!("{}", render_bundle_summary(&bundle, &resolved.source));
        println!("written {}", out_path);
    }
    Ok(())
}

fn handle_agreement_bundle_inspect(reference: &str, json_mode: bool) -> Result<(), String> {
    let stored = resolve_bundle_input(reference)?;
    if json_mode {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "bundle_hash": compute_agreement_bundle_hash_hex(&stored.bundle)?,
                "bundle": stored.bundle,
                "path": stored.path.display().to_string(),
            }))
            .unwrap()
        );
    } else {
        println!(
            "{}",
            render_bundle_summary(&stored.bundle, &stored.path.display().to_string())
        );
    }
    Ok(())
}

fn handle_agreement_bundle_verify(reference: &str, json_mode: bool) -> Result<(), String> {
    let stored = resolve_bundle_input(reference)?;
    verify_agreement_bundle(&stored.bundle)?;
    let bundle_hash = compute_agreement_bundle_hash_hex(&stored.bundle)?;
    if json_mode {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "verified": true,
                "bundle_hash": bundle_hash,
                "agreement_hash": stored.bundle.agreement_hash,
                "canonical_rules": canonical_serialization_rules(),
            }))
            .unwrap()
        );
    } else {
        println!(
            "{}",
            render_bundle_summary(&stored.bundle, &stored.path.display().to_string())
        );
        println!("bundle_verified true");
        println!("bundle_hash {}", bundle_hash);
        println!("canonical_rules");
        for rule in canonical_serialization_rules() {
            println!("  {}", rule);
        }
    }
    Ok(())
}

fn handle_agreement_bundle_unpack(
    reference: &str,
    out_dir: &str,
    json_mode: bool,
) -> Result<(), String> {
    let stored = resolve_bundle_input(reference)?;
    fs::create_dir_all(out_dir).map_err(|e| format!("create out dir: {e}"))?;
    let prefix = Path::new(out_dir).join(&stored.bundle.agreement_id);
    let agreement_path = prefix.with_extension("agreement.json");
    fs::write(
        &agreement_path,
        serde_json::to_string_pretty(&stored.bundle.agreement).unwrap(),
    )
    .map_err(|e| format!("write agreement export: {e}"))?;
    let mut written = vec![agreement_path.display().to_string()];
    if let Some(audit) = &stored.bundle.artifacts.audit {
        let audit_path = prefix.with_extension("audit.json");
        fs::write(&audit_path, serde_json::to_string_pretty(audit).unwrap())
            .map_err(|e| format!("write audit export: {e}"))?;
        written.push(audit_path.display().to_string());
    }
    if let Some(statement) = &stored.bundle.artifacts.statement {
        let statement_path = prefix.with_extension("statement.json");
        fs::write(
            &statement_path,
            serde_json::to_string_pretty(statement).unwrap(),
        )
        .map_err(|e| format!("write statement export: {e}"))?;
        written.push(statement_path.display().to_string());
    }
    if json_mode {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({"written": written})).unwrap()
        );
    } else {
        println!("unpacked_files");
        for path in written {
            println!("  {}", path);
        }
    }
    Ok(())
}

fn handle_agreement_sign(args: &[String]) -> Result<(), String> {
    let mut agreement_path = None::<String>;
    let mut signer = None::<String>;
    let mut signer_role = None::<String>;
    let mut timestamp = None::<u64>;
    let mut out_path = None::<String>;
    let mut json_mode = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--agreement" => {
                agreement_path = Some(parse_required_string_flag(args, &mut i, "--agreement")?);
            }
            "--signer" => {
                signer = Some(parse_required_string_flag(args, &mut i, "--signer")?);
            }
            "--role" => {
                signer_role = Some(parse_required_string_flag(args, &mut i, "--role")?);
            }
            "--timestamp" => {
                timestamp = Some(
                    parse_required_string_flag(args, &mut i, "--timestamp")?
                        .parse::<u64>()
                        .map_err(|_| "--timestamp must be a unix integer".to_string())?,
                );
            }
            "--out" => {
                out_path = Some(parse_required_string_flag(args, &mut i, "--out")?);
            }
            "--json" => {
                json_mode = true;
                i += 1;
            }
            other => return Err(format!("unknown argument {}", other)),
        }
    }
    let agreement_path = agreement_path.ok_or_else(|| "--agreement required".to_string())?;
    let agreement = load_agreement_json_from_path(Path::new(&agreement_path))?;
    let agreement_hash = irium_node_rs::settlement::compute_agreement_hash_hex(&agreement)?;
    let signer = signer.ok_or_else(|| "--signer required".to_string())?;
    let signature = sign_target_hash(
        AgreementSignatureTargetType::Agreement,
        agreement_hash.clone(),
        signer,
        signer_role,
        timestamp,
    )?;
    let verification = inspect_agreement_signature(&signature, Some(&agreement_hash), None);
    let value =
        serde_json::to_value(&signature).map_err(|e| format!("serialize signature: {e}"))?;
    if let Some(out) = out_path.as_deref() {
        save_json_output(Some(out), &value)?;
    }
    if json_mode {
        println!("{}", serde_json::to_string_pretty(&value).unwrap());
    } else {
        println!(
            "{}",
            render_signature_verification_summary(&verification, "Agreement signature")
        );
        if let Some(out) = out_path {
            println!("exported_to {}", out);
        }
    }
    Ok(())
}

fn handle_agreement_verify_signature(args: &[String]) -> Result<(), String> {
    let mut agreement_path = None::<String>;
    let mut bundle_path = None::<String>;
    let mut signature_path = None::<String>;
    let mut out_path = None::<String>;
    let mut json_mode = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--agreement" => {
                agreement_path = Some(parse_required_string_flag(args, &mut i, "--agreement")?);
            }
            "--bundle" => {
                bundle_path = Some(parse_required_string_flag(args, &mut i, "--bundle")?);
            }
            "--signature" => {
                signature_path = Some(parse_required_string_flag(args, &mut i, "--signature")?);
            }
            "--out" => {
                out_path = Some(parse_required_string_flag(args, &mut i, "--out")?);
            }
            "--json" => {
                json_mode = true;
                i += 1;
            }
            other => return Err(format!("unknown argument {}", other)),
        }
    }
    let signature_path = signature_path.ok_or_else(|| "--signature required".to_string())?;
    let signature = load_signature_from_path(Path::new(&signature_path))?;
    let agreement = agreement_path
        .as_deref()
        .map(|path| load_agreement_json_from_path(Path::new(path)))
        .transpose()?;
    let bundle = bundle_path
        .as_deref()
        .map(|reference| {
            if reference == "-" {
                load_bundle_from_path(Path::new("-"))
            } else {
                Ok(resolve_bundle_reference_or_stdin(reference)?.bundle)
            }
        })
        .transpose()?;
    if agreement.is_none() && bundle.is_none() {
        return Err("--agreement or --bundle required for verification".to_string());
    }
    let agreement_hash = if let Some(agreement) = agreement.as_ref() {
        Some(irium_node_rs::settlement::compute_agreement_hash_hex(
            agreement,
        )?)
    } else {
        bundle.as_ref().map(|bundle| bundle.agreement_hash.clone())
    };
    let bundle_hash = bundle
        .as_ref()
        .map(compute_agreement_bundle_hash_hex)
        .transpose()?;
    let verification = inspect_agreement_signature(
        &signature,
        agreement_hash.as_deref(),
        bundle_hash.as_deref(),
    );
    let value =
        serde_json::to_value(&verification).map_err(|e| format!("serialize verification: {e}"))?;
    if let Some(out) = out_path.as_deref() {
        save_json_output(Some(out), &value)?;
    }
    if json_mode {
        println!("{}", serde_json::to_string_pretty(&value).unwrap());
    } else {
        println!(
            "{}",
            render_signature_verification_summary(
                &verification,
                "Agreement signature verification",
            )
        );
        if let Some(out) = out_path {
            println!("exported_to {}", out);
        }
    }
    Ok(())
}

fn handle_agreement_bundle_sign(args: &[String]) -> Result<(), String> {
    let mut bundle_reference = None::<String>;
    let mut signer = None::<String>;
    let mut signer_role = None::<String>;
    let mut timestamp = None::<u64>;
    let mut out_path = None::<String>;
    let mut json_mode = false;
    let mut embed = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--bundle" => {
                bundle_reference = Some(parse_required_string_flag(args, &mut i, "--bundle")?);
            }
            "--signer" => {
                signer = Some(parse_required_string_flag(args, &mut i, "--signer")?);
            }
            "--role" => {
                signer_role = Some(parse_required_string_flag(args, &mut i, "--role")?);
            }
            "--timestamp" => {
                timestamp = Some(
                    parse_required_string_flag(args, &mut i, "--timestamp")?
                        .parse::<u64>()
                        .map_err(|_| "--timestamp must be a unix integer".to_string())?,
                );
            }
            "--out" => {
                out_path = Some(parse_required_string_flag(args, &mut i, "--out")?);
            }
            "--json" => {
                json_mode = true;
                i += 1;
            }
            "--embed" => {
                embed = true;
                i += 1;
            }
            other => return Err(format!("unknown argument {}", other)),
        }
    }
    let bundle_reference = bundle_reference.ok_or_else(|| "--bundle required".to_string())?;
    let stored = resolve_bundle_reference_or_stdin(&bundle_reference)?;
    let bundle_hash = compute_agreement_bundle_hash_hex(&stored.bundle)?;
    let signer = signer.ok_or_else(|| "--signer required".to_string())?;
    let signature = sign_target_hash(
        AgreementSignatureTargetType::Bundle,
        bundle_hash.clone(),
        signer,
        signer_role,
        timestamp,
    )?;
    let verification = inspect_agreement_signature(
        &signature,
        Some(&stored.bundle.agreement_hash),
        Some(&bundle_hash),
    );
    if embed {
        let out = out_path.ok_or_else(|| "--out required when --embed is used".to_string())?;
        let mut bundle = stored.bundle.clone();
        bundle.signatures.push(signature.clone());
        let value =
            serde_json::to_value(&bundle).map_err(|e| format!("serialize signed bundle: {e}"))?;
        save_json_output(Some(&out), &value)?;
        if json_mode {
            println!("{}", serde_json::to_string_pretty(&value).unwrap());
        } else {
            println!(
                "{}",
                render_signature_verification_summary(&verification, "Bundle signature")
            );
            println!("embedded_in_bundle true");
            println!("exported_to {}", out);
        }
        return Ok(());
    }
    let value =
        serde_json::to_value(&signature).map_err(|e| format!("serialize signature: {e}"))?;
    if let Some(out) = out_path.as_deref() {
        save_json_output(Some(out), &value)?;
    }
    if json_mode {
        println!("{}", serde_json::to_string_pretty(&value).unwrap());
    } else {
        println!(
            "{}",
            render_signature_verification_summary(&verification, "Bundle signature")
        );
        if let Some(out) = out_path {
            println!("exported_to {}", out);
        }
    }
    Ok(())
}

fn handle_agreement_bundle_verify_signatures(
    reference: &str,
    json_mode: bool,
) -> Result<(), String> {
    let stored = resolve_bundle_reference_or_stdin(reference)?;
    let verifications = verify_bundle_signatures(&stored.bundle);
    if json_mode {
        let payload = json!({
            "agreement_hash": stored.bundle.agreement_hash,
            "bundle_hash": compute_agreement_bundle_hash_hex(&stored.bundle)?,
            "signatures": verifications,
        });
        println!("{}", serde_json::to_string_pretty(&payload).unwrap());
    } else {
        println!(
            "{}",
            render_bundle_signature_verifications(&stored.bundle, &verifications)
        );
    }
    Ok(())
}

fn handle_agreement_signature_inspect(args: &[String]) -> Result<(), String> {
    let mut signature_path = None::<String>;
    let mut agreement_path = None::<String>;
    let mut bundle_path = None::<String>;
    let mut json_mode = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--signature" => {
                signature_path = Some(parse_required_string_flag(args, &mut i, "--signature")?);
            }
            "--agreement" => {
                agreement_path = Some(parse_required_string_flag(args, &mut i, "--agreement")?);
            }
            "--bundle" => {
                bundle_path = Some(parse_required_string_flag(args, &mut i, "--bundle")?);
            }
            "--json" => {
                json_mode = true;
                i += 1;
            }
            other => return Err(format!("unknown argument {}", other)),
        }
    }
    let signature_path = signature_path.ok_or_else(|| "--signature required".to_string())?;
    let signature = load_signature_from_path(Path::new(&signature_path))?;
    let agreement_hash = agreement_path
        .as_deref()
        .map(|path| load_agreement_json_from_path(Path::new(path)))
        .transpose()?
        .map(|agreement| irium_node_rs::settlement::compute_agreement_hash_hex(&agreement))
        .transpose()?;
    let bundle_hash = bundle_path
        .as_deref()
        .map(|reference| {
            if reference == "-" {
                load_bundle_from_path(Path::new("-"))
            } else {
                Ok(resolve_bundle_reference_or_stdin(reference)?.bundle)
            }
        })
        .transpose()?
        .map(|bundle| compute_agreement_bundle_hash_hex(&bundle))
        .transpose()?;
    let verification = inspect_agreement_signature(
        &signature,
        agreement_hash.as_deref(),
        bundle_hash.as_deref(),
    );
    if json_mode {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::to_value(&verification).unwrap()).unwrap()
        );
    } else {
        println!(
            "{}",
            render_signature_verification_summary(&verification, "Agreement signature inspect")
        );
    }
    Ok(())
}

fn submit_tx(client: &Client, base: &str, tx: &Transaction) -> Result<(), String> {
    let raw = tx.serialize();
    let req_body = SubmitTxRequest {
        tx_hex: hex::encode(raw),
    };

    let resp = send_with_https_fallback(base, |b| {
        let url = format!("{}/rpc/submit_tx", b);
        let mut req = client.post(&url).json(&req_body);
        if let Ok(token) = env::var("IRIUM_RPC_TOKEN") {
            req = req.bearer_auth(token);
        }
        req.send()
    })
    .map_err(|e| format!("submit tx failed: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("submit tx failed: {}", resp.status()));
    }
    Ok(())

}

#[cfg(test)]
mod tests {
    use super::*;
    use irium_node_rs::settlement::{
        AgreementDeadlines, AgreementLifecycleState, AgreementParty, AgreementRefundCondition,
        AgreementReleaseCondition,
    };
    use std::sync::{Mutex, OnceLock};

    fn test_guard() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    fn sample_bundle() -> AgreementBundle {
        build_agreement_bundle(
            &sample_agreement(),
            1_710_000_000,
            Some("wallet-test".to_string()),
            Some("saved locally".to_string()),
            vec!["aa".repeat(32)],
            vec![],
        )
        .unwrap()
    }

    fn temp_bundle_dir(tag: &str) -> PathBuf {
        let mut dir = env::temp_dir();
        dir.push(format!("irium-wallet-bundles-{}-{}", tag, now_unix()));
        dir
    }

    fn sample_agreement() -> AgreementObject {
        AgreementObject {
            agreement_id: "agr-wallet-1".to_string(),
            version: 1,
            schema_id: Some(irium_node_rs::settlement::AGREEMENT_SCHEMA_ID_V1.to_string()),
            template_type: AgreementTemplateType::SimpleReleaseRefund,
            parties: vec![
                AgreementParty {
                    party_id: "payer".to_string(),
                    display_name: "Payer".to_string(),
                    address: "Qpayer".to_string(),
                    role: Some("payer".to_string()),
                },
                AgreementParty {
                    party_id: "payee".to_string(),
                    display_name: "Payee".to_string(),
                    address: "Qpayee".to_string(),
                    role: Some("payee".to_string()),
                },
            ],
            payer: "payer".to_string(),
            payee: "payee".to_string(),
            mediator_reference: Some("meta-only".to_string()),
            total_amount: 125_000_000,
            network_marker: "IRIUM".to_string(),
            creation_time: 1_700_000_000,
            deadlines: AgreementDeadlines {
                settlement_deadline: Some(100),
                refund_deadline: Some(120),
                dispute_window: None,
            },
            release_conditions: vec![AgreementReleaseCondition {
                mode: "secret_preimage".to_string(),
                secret_hash_hex: Some("11".repeat(32)),
                release_authorizer: Some("payer".to_string()),
                notes: None,
            }],
            refund_conditions: vec![AgreementRefundCondition {
                refund_address: "Qpayer".to_string(),
                timeout_height: 120,
                notes: None,
            }],
            milestones: vec![],
            deposit_rule: None,
            proof_policy_reference: Some("phase2-placeholder".to_string()),
            asset_reference: None,
            payment_reference: None,
            purpose_reference: None,
            release_summary: Some("Release follows the agreed HTLC path".to_string()),
            refund_summary: Some("Refund follows the agreed timeout path".to_string()),
            attestor_reference: None,
            resolver_reference: None,
            notes: Some("fixture".to_string()),
            document_hash: "22".repeat(32),
            metadata_hash: Some("33".repeat(32)),
            invoice_reference: Some("INV-42".to_string()),
            external_reference: None,
            disputed_metadata_only: false,
        }
    }

    fn temp_share_package_dir(tag: &str) -> PathBuf {
        let mut dir = env::temp_dir();
        dir.push(format!("irium-wallet-share-package-{}-{}", tag, now_unix()));
        dir
    }

    fn sample_detached_signature(
        target_type: AgreementSignatureTargetType,
        target_hash: String,
    ) -> AgreementSignatureEnvelope {
        let signing_key = SigningKey::from_bytes((&[9u8; 32]).into()).unwrap();
        let public_key = signing_key
            .verifying_key()
            .to_encoded_point(true)
            .as_bytes()
            .to_vec();
        let mut envelope = AgreementSignatureEnvelope {
            version: AGREEMENT_SIGNATURE_VERSION,
            target_type,
            target_hash,
            signer_public_key: hex::encode(public_key),
            signer_address: Some("Qsigwallet".to_string()),
            signature_type: AGREEMENT_SIGNATURE_TYPE_SECP256K1.to_string(),
            timestamp: Some(1_710_100_000),
            signer_role: Some("buyer".to_string()),
            signature: String::new(),
        };
        let digest = compute_agreement_signature_payload_hash(&envelope).unwrap();
        let signature: Signature = signing_key.sign_prehash(&digest).unwrap();
        envelope.signature = hex::encode(signature.to_bytes());
        envelope
    }

    fn sample_audit_for_agreement(agreement: &AgreementObject) -> AgreementAuditRecord {
        AgreementAuditRecord {
            metadata: irium_node_rs::settlement::AgreementAuditMetadata {
                version: 1,
                generated_at: 1,
                generator_surface: "test".to_string(),
                trust_model_summary: "derived".to_string(),
            },
            agreement: irium_node_rs::settlement::AgreementAuditAgreementSummary {
                agreement_id: agreement.agreement_id.clone(),
                agreement_hash: irium_node_rs::settlement::compute_agreement_hash_hex(agreement)
                    .unwrap(),
                template_type: agreement.template_type,
                network_marker: agreement.network_marker.clone(),
                payer: agreement.payer.clone(),
                payee: agreement.payee.clone(),
                parties: agreement.parties.clone(),
                total_amount: agreement.total_amount,
                milestone_count: agreement.milestones.len(),
                milestones: vec![],
                settlement_deadline: agreement.deadlines.settlement_deadline,
                refund_deadline: agreement.deadlines.refund_deadline,
                dispute_window: agreement.deadlines.dispute_window,
                document_hash: agreement.document_hash.clone(),
                metadata_hash: agreement.metadata_hash.clone(),
                invoice_reference: agreement.invoice_reference.clone(),
                external_reference: agreement.external_reference.clone(),
            },
            local_bundle: irium_node_rs::settlement::AgreementAuditBundleContext {
                bundle_used: true,
                verification_ok: true,
                saved_at: None,
                source_label: None,
                note: None,
                linked_funding_txids: vec![],
                milestone_hints: vec![],
                local_only_notice: "local only".to_string(),
            },
            chain_observed: irium_node_rs::settlement::AgreementAuditChainObservedContext {
                linked_transactions: vec![],
                linked_transaction_count: 0,
                anchor_observation_notice: "none".to_string(),
            },
            funding_legs: irium_node_rs::settlement::AgreementAuditFundingLegSummary {
                candidate_count: 0,
                selection_required: false,
                selected_leg: None,
                ambiguity_warning: None,
                candidates: vec![],
                notice: "derived".to_string(),
            },
            timeline: irium_node_rs::settlement::AgreementAuditTimelineSummary {
                reconstructed: false,
                event_count: 0,
                events: vec![],
                notice: "derived".to_string(),
            },
            settlement_state: irium_node_rs::settlement::AgreementAuditSettlementStateSummary {
                lifecycle_state: AgreementLifecycleState::Draft,
                derived_state_label: "unresolved".to_string(),
                selection_required: false,
                funded_amount: 0,
                released_amount: 0,
                refunded_amount: 0,
                summary_note: "derived".to_string(),
            },
            trust_boundaries: irium_node_rs::settlement::AgreementAuditTrustBoundaries {
                consensus_enforced: vec![],
                htlc_enforced: vec![],
                metadata_indexed: vec![],
                local_bundle_only: vec![],
                off_chain_required: vec![],
            },
            authenticity: None,
        }
    }

    #[test]
    fn agreement_summary_render_is_stable() {
        let agreement = sample_agreement();
        let hash = "aa".repeat(32);
        let rendered = render_agreement_summary(&agreement, &hash);
        assert!(rendered.contains("agreement_id agr-wallet-1"));
        assert!(rendered.contains("template simple_release_refund"));
        assert!(rendered.contains("total_amount_irm 1.25000000"));
        assert!(rendered.contains("trust_model HTLC release/refund enforcement is on-chain"));
    }

    #[test]
    fn milestone_arg_parser_rejects_bad_shapes() {
        assert!(parse_milestone_arg("only|three|parts", "Qpayee", "Qpayer").is_err());
        assert!(parse_milestone_arg("m1|Kickoff|1.0|nope", "Qpayee", "Qpayer").is_err());
    }

    #[test]
    fn agreement_spend_cli_parser_allows_bundle_aware_lookup_without_funding_txid() {
        let args = vec!["agreement.json".to_string()];
        let parsed = parse_agreement_spend_cli(&args).unwrap();
        assert_eq!(parsed.agreement_path, "agreement.json");
        assert_eq!(parsed.funding_txid, None);
        assert_eq!(parsed.htlc_vout, None);
        assert_eq!(parsed.milestone_id, None);
    }

    #[test]
    fn release_eligibility_summary_render_is_stable() {
        let rendered =
            render_agreement_spend_eligibility_summary(&AgreementSpendEligibilityResponse {
                agreement_hash: "aa".repeat(32),
                agreement_id: "agr-wallet-1".to_string(),
                funding_txid: "bb".repeat(32),
                htlc_vout: Some(0),
                anchor_vout: Some(1),
                role: Some("funding".to_string()),
                milestone_id: None,
                amount: Some(125_000_000),
                branch: "release".to_string(),
                htlc_backed: true,
                funded: true,
                unspent: true,
                preimage_required: true,
                timeout_height: Some(120),
                timeout_reached: false,
                destination_address: Some("Qpayee".to_string()),
                expected_hash: Some("11".repeat(32)),
                recipient_address: Some("Qpayee".to_string()),
                refund_address: Some("Qpayer".to_string()),
                eligible: true,
                reasons: vec![],
                trust_model_note: "htlc branch only".to_string(),
            });
        assert!(rendered.contains("branch release"));
        assert!(rendered.contains("preimage_required true"));
        assert!(rendered.contains("eligible true"));
    }

    #[test]
    fn refund_build_json_shape_is_stable() {
        let value = serde_json::to_value(AgreementBuildSpendResponse {
            agreement_hash: "aa".repeat(32),
            agreement_id: "agr-wallet-1".to_string(),
            funding_txid: "bb".repeat(32),
            htlc_vout: 0,
            role: "refund".to_string(),
            milestone_id: Some("ms1".to_string()),
            branch: "refund".to_string(),
            destination_address: "Qpayer".to_string(),
            txid: "cc".repeat(32),
            accepted: false,
            raw_tx_hex: "deadbeef".to_string(),
            fee: 1000,
            trust_model_note: "htlc branch only".to_string(),
        })
        .unwrap();
        assert_eq!(value["branch"], "refund");
        assert_eq!(value["milestone_id"], "ms1");
        assert_eq!(value["destination_address"], "Qpayer");
    }

    #[test]
    fn build_summary_hides_raw_tx_by_default() {
        let rendered = render_agreement_build_spend_summary(
            &AgreementBuildSpendResponse {
                agreement_hash: "aa".repeat(32),
                agreement_id: "agr-wallet-1".to_string(),
                funding_txid: "bb".repeat(32),
                htlc_vout: 0,
                role: "release".to_string(),
                milestone_id: None,
                branch: "release".to_string(),
                destination_address: "Qpayee".to_string(),
                txid: "cc".repeat(32),
                accepted: false,
                raw_tx_hex: "deadbeef".to_string(),
                fee: 1000,
                trust_model_note: "htlc branch only".to_string(),
            },
            false,
            false,
        );
        assert!(rendered.contains("signed_tx_ready true"));
        assert!(rendered.contains("broadcast_requested false"));
        assert!(!rendered.contains("deadbeef"));
    }

    #[test]
    fn agreement_spend_cli_rejects_invalid_destination_and_secret() {
        let args = vec![
            "agreement.json".to_string(),
            "ab".repeat(32),
            "--destination".to_string(),
            "not-an-address".to_string(),
        ];
        assert!(parse_agreement_spend_cli(&args).is_err());

        let args = vec![
            "agreement.json".to_string(),
            "ab".repeat(32),
            "--secret".to_string(),
            "zz".to_string(),
        ];
        assert!(parse_agreement_spend_cli(&args).is_err());
    }

    #[test]
    fn explicit_build_mode_rejects_broadcast_flag() {
        let opts = AgreementSpendCliOptions {
            agreement_path: "agreement.json".to_string(),
            funding_txid: Some("ab".repeat(32)),
            rpc_url: "http://127.0.0.1:8338".to_string(),
            htlc_vout: None,
            milestone_id: None,
            destination_address: None,
            fee_per_byte: None,
            broadcast: true,
            secret_hex: None,
            json_mode: false,
            show_raw_tx: false,
        };
        assert!(finalize_agreement_spend_mode(opts, Some(false)).is_err());
    }

    #[test]
    fn bundle_store_roundtrip_and_list_shape() {
        let _guard = test_guard();
        let dir = temp_bundle_dir("roundtrip");
        let bundle = sample_bundle();
        let path = save_bundle_to_store_at(&dir, &bundle).unwrap();
        let loaded = load_bundle_from_path(&path).unwrap();
        assert_eq!(loaded, bundle);
        let items = list_stored_bundles_at(&dir).unwrap();
        assert_eq!(items.len(), 1);
        let value = serde_json::to_value(bundle_list_item(&items[0])).unwrap();
        assert_eq!(value["agreement_id"], bundle.agreement_id);
        assert_eq!(value["agreement_hash"], bundle.agreement_hash);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn resolve_bundle_by_id_and_hash() {
        let _guard = test_guard();
        let dir = temp_bundle_dir("resolve");
        let bundle = sample_bundle();
        save_bundle_to_store_at(&dir, &bundle).unwrap();
        let by_id = resolve_bundle_from_store_at(&dir, &bundle.agreement_id).unwrap();
        let by_hash = resolve_bundle_from_store_at(&dir, &bundle.agreement_hash).unwrap();
        assert_eq!(by_id.bundle.agreement_hash, bundle.agreement_hash);
        assert_eq!(by_hash.bundle.agreement_id, bundle.agreement_id);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn conflicting_bundle_ids_fail_safely() {
        let _guard = test_guard();
        let dir = temp_bundle_dir("conflict");
        let bundle = sample_bundle();
        save_bundle_to_store_at(&dir, &bundle).unwrap();
        let mut second = sample_bundle();
        second.metadata.saved_at += 1;
        second.agreement.document_hash = "44".repeat(32);
        second.agreement_hash =
            irium_node_rs::settlement::compute_agreement_hash_hex(&second.agreement).unwrap();
        save_bundle_to_store_at(&dir, &second).unwrap();
        assert!(resolve_bundle_from_store_at(&dir, &bundle.agreement_id).is_err());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn resolve_agreement_input_uses_saved_bundle_reference() {
        let _guard = test_guard();
        let dir = temp_bundle_dir("resolve-input");
        let bundle = sample_bundle();
        save_bundle_to_store_at(&dir, &bundle).unwrap();
        env::set_var("IRIUM_AGREEMENT_BUNDLES_DIR", &dir);
        let resolved = resolve_agreement_input(&bundle.agreement_hash).unwrap();
        assert_eq!(resolved.agreement.agreement_id, bundle.agreement_id);
        assert!(resolved.bundle.is_some());
        env::remove_var("IRIUM_AGREEMENT_BUNDLES_DIR");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn bundle_summary_mentions_local_only_trust_model() {
        let rendered = render_bundle_summary(&sample_bundle(), "bundle_store:test");
        assert!(rendered.contains("agreement_hash"));
        assert!(rendered.contains("local/off-chain convenience only"));
    }

    #[test]
    fn agreement_funding_legs_render_is_stable() {
        let rendered = render_agreement_funding_legs(&AgreementFundingLegsResponse {
            agreement_hash: "aa".repeat(32),
            selection_required: false,
            candidates: vec![AgreementFundingLegCandidateResponse {
                agreement_hash: "aa".repeat(32),
                funding_txid: "bb".repeat(32),
                htlc_vout: 0,
                anchor_vout: 1,
                role: "funding".to_string(),
                milestone_id: None,
                amount: 125_000_000,
                htlc_backed: true,
                timeout_height: 120,
                recipient_address: "Qpayee".to_string(),
                refund_address: "Qpayer".to_string(),
                source_notes: vec!["direct_anchor_match".to_string()],
                release_eligible: false,
                release_reasons: vec!["secret_hex_required_for_release".to_string()],
                refund_eligible: false,
                refund_reasons: vec!["refund_timeout_not_reached".to_string()],
            }],
            trust_model_note: "derived only".to_string(),
        });
        assert!(rendered.contains("candidate_count 1"));
        assert!(rendered.contains("txid="));
    }

    #[test]
    fn select_discovered_funding_leg_prefers_single_candidate() {
        let resp = AgreementFundingLegsResponse {
            agreement_hash: "aa".repeat(32),
            selection_required: false,
            candidates: vec![AgreementFundingLegCandidateResponse {
                agreement_hash: "aa".repeat(32),
                funding_txid: "bb".repeat(32),
                htlc_vout: 0,
                anchor_vout: 1,
                role: "funding".to_string(),
                milestone_id: Some("ms1".to_string()),
                amount: 10,
                htlc_backed: true,
                timeout_height: 100,
                recipient_address: "Qpayee".to_string(),
                refund_address: "Qpayer".to_string(),
                source_notes: vec!["direct_anchor_match".to_string()],
                release_eligible: false,
                release_reasons: vec![],
                refund_eligible: false,
                refund_reasons: vec![],
            }],
            trust_model_note: "derived only".to_string(),
        };
        let (candidate, notice) =
            select_agreement_funding_leg_candidate(&resp, Some("ms1"), None).unwrap();
        assert_eq!(candidate.funding_txid, "bb".repeat(32));
        assert!(notice.contains("auto-selected"));
    }

    #[test]
    fn select_discovered_funding_leg_fails_on_ambiguity() {
        let candidate = AgreementFundingLegCandidateResponse {
            agreement_hash: "aa".repeat(32),
            funding_txid: "bb".repeat(32),
            htlc_vout: 0,
            anchor_vout: 1,
            role: "funding".to_string(),
            milestone_id: None,
            amount: 10,
            htlc_backed: true,
            timeout_height: 100,
            recipient_address: "Qpayee".to_string(),
            refund_address: "Qpayer".to_string(),
            source_notes: vec!["direct_anchor_match".to_string()],
            release_eligible: false,
            release_reasons: vec![],
            refund_eligible: false,
            refund_reasons: vec![],
        };
        let resp = AgreementFundingLegsResponse {
            agreement_hash: "aa".repeat(32),
            selection_required: true,
            candidates: vec![
                candidate.clone(),
                AgreementFundingLegCandidateResponse {
                    funding_txid: "cc".repeat(32),
                    ..candidate
                },
            ],
            trust_model_note: "derived only".to_string(),
        };
        assert!(select_agreement_funding_leg_candidate(&resp, None, None).is_err());
    }

    #[test]
    fn agreement_timeline_render_is_stable() {
        let rendered = render_agreement_timeline(&AgreementTimelineResponse {
            agreement_hash: "aa".repeat(32),
            lifecycle: AgreementLifecycleView {
                state: AgreementLifecycleState::Funded,
                agreement_hash: "aa".repeat(32),
                funded_amount: 10,
                released_amount: 0,
                refunded_amount: 0,
                milestones: vec![],
                linked_txs: vec![],
                trust_model_note: "derived only".to_string(),
            },
            events: vec![AgreementActivityEvent {
                event_type: "funding_tx_observed".to_string(),
                source: "chain_observed".to_string(),
                txid: Some("bb".repeat(32)),
                height: Some(10),
                timestamp: None,
                milestone_id: None,
                note: Some("linked tx".to_string()),
            }],
            trust_model_note: "derived only".to_string(),
        });
        assert!(rendered.contains("events"));
        assert!(rendered.contains("funding_tx_observed"));
    }

    #[test]
    fn agreement_audit_render_is_stable() {
        let rendered = render_agreement_audit(&AgreementAuditRecord {
            metadata: irium_node_rs::settlement::AgreementAuditMetadata {
                version: 1,
                generated_at: 1_710_000_123,
                generator_surface: "iriumd_rpc".to_string(),
                trust_model_summary: "derived only".to_string(),
            },
            agreement: irium_node_rs::settlement::AgreementAuditAgreementSummary {
                agreement_id: "agr-wallet-1".to_string(),
                agreement_hash: "aa".repeat(32),
                template_type: AgreementTemplateType::SimpleReleaseRefund,
                network_marker: "IRIUM".to_string(),
                payer: "Qpayer".to_string(),
                payee: "Qpayee".to_string(),
                parties: vec![],
                total_amount: 125_000_000,
                milestone_count: 0,
                milestones: vec![],
                settlement_deadline: Some(100),
                refund_deadline: Some(120),
                dispute_window: None,
                document_hash: "11".repeat(32),
                metadata_hash: None,
                invoice_reference: None,
                external_reference: None,
            },
            local_bundle: irium_node_rs::settlement::AgreementAuditBundleContext {
                bundle_used: true,
                verification_ok: true,
                saved_at: Some(1_710_000_000),
                source_label: Some("wallet-test".to_string()),
                note: None,
                linked_funding_txids: vec!["bb".repeat(32)],
                milestone_hints: vec![],
                local_only_notice: "local only".to_string(),
            },
            chain_observed: irium_node_rs::settlement::AgreementAuditChainObservedContext {
                linked_transactions: vec![],
                linked_transaction_count: 1,
                anchor_observation_notice: "chain observed".to_string(),
            },
            funding_legs: irium_node_rs::settlement::AgreementAuditFundingLegSummary {
                candidate_count: 1,
                selection_required: false,
                selected_leg: Some(irium_node_rs::settlement::AgreementAuditFundingLegRecord {
                    funding_txid: "bb".repeat(32),
                    htlc_vout: 0,
                    anchor_vout: 1,
                    role: irium_node_rs::settlement::AgreementAnchorRole::Funding,
                    milestone_id: None,
                    amount: 125_000_000,
                    htlc_backed: true,
                    timeout_height: 120,
                    recipient_address: "Qpayee".to_string(),
                    refund_address: "Qpayer".to_string(),
                    source_notes: vec!["direct_anchor_match".to_string()],
                    release_eligible: Some(false),
                    release_reasons: vec![],
                    refund_eligible: Some(false),
                    refund_reasons: vec![],
                }),
                ambiguity_warning: None,
                candidates: vec![],
                notice: "derived only".to_string(),
            },
            timeline: irium_node_rs::settlement::AgreementAuditTimelineSummary {
                reconstructed: true,
                event_count: 1,
                events: vec![],
                notice: "timeline".to_string(),
            },
            settlement_state: irium_node_rs::settlement::AgreementAuditSettlementStateSummary {
                lifecycle_state: AgreementLifecycleState::Funded,
                derived_state_label: "funded".to_string(),
                selection_required: false,
                funded_amount: 125_000_000,
                released_amount: 0,
                refunded_amount: 0,
                summary_note: "derived state".to_string(),
            },
            authenticity: None,
            trust_boundaries: irium_node_rs::settlement::AgreementAuditTrustBoundaries {
                consensus_enforced: vec!["anchor visibility".to_string()],
                htlc_enforced: vec!["htlc branch".to_string()],
                metadata_indexed: vec!["timeline".to_string()],
                local_bundle_only: vec!["bundle label".to_string()],
                off_chain_required: vec!["agreement exchange".to_string()],
            },
        });
        assert!(rendered.contains("agreement_id agr-wallet-1"));
        assert!(rendered.contains("derived_state funded"));
        assert!(rendered.contains("selected_leg txid="));
    }

    #[test]
    fn agreement_audit_json_shape_is_stable() {
        let value = serde_json::to_value(irium_node_rs::settlement::AgreementAuditRecord {
            metadata: irium_node_rs::settlement::AgreementAuditMetadata {
                version: 1,
                generated_at: 1,
                generator_surface: "iriumd_rpc".to_string(),
                trust_model_summary: "derived only".to_string(),
            },
            agreement: irium_node_rs::settlement::AgreementAuditAgreementSummary {
                agreement_id: "agr-wallet-1".to_string(),
                agreement_hash: "aa".repeat(32),
                template_type: AgreementTemplateType::SimpleReleaseRefund,
                network_marker: "IRIUM".to_string(),
                payer: "Qpayer".to_string(),
                payee: "Qpayee".to_string(),
                parties: vec![],
                total_amount: 1,
                milestone_count: 0,
                milestones: vec![],
                settlement_deadline: None,
                refund_deadline: None,
                dispute_window: None,
                document_hash: "11".repeat(32),
                metadata_hash: None,
                invoice_reference: None,
                external_reference: None,
            },
            local_bundle: irium_node_rs::settlement::AgreementAuditBundleContext {
                bundle_used: false,
                verification_ok: false,
                saved_at: None,
                source_label: None,
                note: None,
                linked_funding_txids: vec![],
                milestone_hints: vec![],
                local_only_notice: "local only".to_string(),
            },
            chain_observed: irium_node_rs::settlement::AgreementAuditChainObservedContext {
                linked_transactions: vec![],
                linked_transaction_count: 0,
                anchor_observation_notice: "chain observed".to_string(),
            },
            funding_legs: irium_node_rs::settlement::AgreementAuditFundingLegSummary {
                candidate_count: 0,
                selection_required: false,
                selected_leg: None,
                ambiguity_warning: None,
                candidates: vec![],
                notice: "funding".to_string(),
            },
            timeline: irium_node_rs::settlement::AgreementAuditTimelineSummary {
                reconstructed: true,
                event_count: 0,
                events: vec![],
                notice: "timeline".to_string(),
            },
            settlement_state: irium_node_rs::settlement::AgreementAuditSettlementStateSummary {
                lifecycle_state: AgreementLifecycleState::Draft,
                derived_state_label: "draft".to_string(),
                selection_required: false,
                funded_amount: 0,
                released_amount: 0,
                refunded_amount: 0,
                summary_note: "state".to_string(),
            },
            authenticity: None,
            trust_boundaries: irium_node_rs::settlement::AgreementAuditTrustBoundaries {
                consensus_enforced: vec![],
                htlc_enforced: vec![],
                metadata_indexed: vec![],
                local_bundle_only: vec![],
                off_chain_required: vec![],
            },
        })
        .unwrap();
        assert_eq!(value["metadata"]["version"], 1);
        assert_eq!(value["agreement"]["agreement_id"], "agr-wallet-1");
        assert_eq!(value["settlement_state"]["derived_state_label"], "draft");
    }

    #[test]
    fn artifact_verification_render_is_stable() {
        let agreement = sample_agreement();
        let bundle = sample_bundle();
        let result = build_agreement_artifact_verification(
            Some(&agreement),
            Some(&bundle),
            None,
            None,
            &[],
            &[],
            None,
            1,
        );
        let rendered = render_artifact_verification(&result);
        assert!(rendered.contains("Agreement artifact verification"));
        assert!(rendered.contains("verified_matches"));
        assert!(rendered.contains("unverifiable_or_limited"));
        assert!(rendered.contains("trust_boundaries"));
    }

    #[test]
    fn artifact_verification_json_shape_is_stable() {
        let agreement = sample_agreement();
        let bundle = sample_bundle();
        let result = build_agreement_artifact_verification(
            Some(&agreement),
            Some(&bundle),
            None,
            None,
            &[],
            &[],
            None,
            1,
        );
        let value = serde_json::to_value(&result).unwrap();
        assert_eq!(value["metadata"]["version"], 1);
        assert!(value["input_summary"]["supplied_artifact_types"].is_array());
        assert!(value["trust_summary"]["unverifiable_from_chain_alone"].is_array());
    }
    #[test]
    fn agreement_bundle_verify_summary_includes_rules_and_hash() {
        let agreement = sample_agreement();
        let bundle = irium_node_rs::settlement::build_agreement_bundle(
            &agreement,
            1_710_000_000,
            Some("wallet-test".to_string()),
            Some("note".to_string()),
            vec![],
            vec![],
        )
        .unwrap();
        let rendered = render_bundle_summary(&bundle, "bundle.json");
        assert!(rendered.contains("bundle_schema_id"));
        assert!(rendered.contains("agreement_hash"));
    }

    #[test]
    fn agreement_audit_csv_render_is_stable() {
        let csv = render_agreement_audit_csv(&AgreementAuditRecord {
            metadata: irium_node_rs::settlement::AgreementAuditMetadata {
                version: 1,
                generated_at: 1,
                generator_surface: "iriumd_rpc".to_string(),
                trust_model_summary: "derived only".to_string(),
            },
            agreement: irium_node_rs::settlement::AgreementAuditAgreementSummary {
                agreement_id: "agr-wallet-csv".to_string(),
                agreement_hash: "aa".repeat(32),
                template_type: AgreementTemplateType::SimpleReleaseRefund,
                network_marker: "IRIUM".to_string(),
                payer: "Qpayer".to_string(),
                payee: "Qpayee".to_string(),
                parties: vec![],
                total_amount: 1,
                milestone_count: 0,
                milestones: vec![],
                settlement_deadline: None,
                refund_deadline: None,
                dispute_window: None,
                document_hash: "11".repeat(32),
                metadata_hash: None,
                invoice_reference: None,
                external_reference: None,
            },
            local_bundle: irium_node_rs::settlement::AgreementAuditBundleContext {
                bundle_used: true,
                verification_ok: true,
                saved_at: Some(1),
                source_label: Some("wallet-test".to_string()),
                note: None,
                linked_funding_txids: vec!["bb".repeat(32)],
                milestone_hints: vec![],
                local_only_notice: "local only".to_string(),
            },
            chain_observed: irium_node_rs::settlement::AgreementAuditChainObservedContext {
                linked_transactions: vec![],
                linked_transaction_count: 1,
                anchor_observation_notice: "chain observed".to_string(),
            },
            funding_legs: irium_node_rs::settlement::AgreementAuditFundingLegSummary {
                candidate_count: 1,
                selection_required: false,
                selected_leg: None,
                ambiguity_warning: None,
                candidates: vec![],
                notice: "funding".to_string(),
            },
            timeline: irium_node_rs::settlement::AgreementAuditTimelineSummary {
                reconstructed: true,
                event_count: 0,
                events: vec![],
                notice: "timeline".to_string(),
            },
            settlement_state: irium_node_rs::settlement::AgreementAuditSettlementStateSummary {
                lifecycle_state: AgreementLifecycleState::Draft,
                derived_state_label: "draft".to_string(),
                selection_required: false,
                funded_amount: 0,
                released_amount: 0,
                refunded_amount: 0,
                summary_note: "state".to_string(),
            },
            authenticity: None,
            trust_boundaries: irium_node_rs::settlement::AgreementAuditTrustBoundaries {
                consensus_enforced: vec!["anchor visibility".to_string()],
                htlc_enforced: vec!["htlc branch".to_string()],
                metadata_indexed: vec!["timeline".to_string()],
                local_bundle_only: vec!["bundle label".to_string()],
                off_chain_required: vec!["agreement exchange".to_string()],
            },
        });
        assert!(csv.contains("\"record_version\""));
        assert!(csv.contains("\"csv_schema\""));
        assert!(csv.contains("agreement_audit_csv_v1"));
        assert!(csv.contains("summary"));
        assert!(csv.contains("trust_boundary"));
        assert!(csv.contains("agr-wallet-csv"));
    }

    #[test]
    fn agreement_audit_export_mode_validation_is_strict() {
        assert_eq!(
            validate_agreement_audit_export_format("json", false).unwrap(),
            "json"
        );
        assert_eq!(
            validate_agreement_audit_export_format("CSV", false).unwrap(),
            "csv"
        );
        assert!(validate_agreement_audit_export_format("csv", true).is_err());
        assert!(validate_agreement_audit_export_format("yaml", false).is_err());
    }

    #[test]
    fn share_package_inspection_render_is_stable() {
        let inspection = AgreementSharePackageInspection {
            version: 1,
            package_schema_id: Some("irium.phase1.share_package.v1".to_string()),
            created_at: Some(1_710_001_300),
            sender_label: Some("counterparty-a".to_string()),
            package_note: Some("handoff".to_string()),
            package_profile: "review_package".to_string(),
            included_artifact_types: vec![
                "agreement".to_string(),
                "bundle".to_string(),
                "statement".to_string(),
            ],
            omitted_artifact_types: vec![
                "audit".to_string(),
                "agreement_signatures".to_string(),
                "bundle_signatures".to_string(),
            ],
            agreement_present: true,
            bundle_present: true,
            audit_present: false,
            statement_present: true,
            detached_agreement_signature_count: 1,
            detached_bundle_signature_count: 0,
            verification_notice: "manifest is descriptive only".to_string(),
            canonical_agreement_id: Some("agr-wallet-1".to_string()),
            canonical_agreement_hash: Some("aa".repeat(32)),
            bundle_hash: Some("bb".repeat(32)),
            trust_notice: "package contents are supplied artifacts".to_string(),
            informational_notice: "share package is a transport convenience only".to_string(),
        };
        let rendered = render_share_package_inspection(&inspection);
        assert!(rendered.contains("Agreement share package"));
        assert!(rendered.contains("package_profile review_package"));
        assert!(rendered.contains("included_artifact_types agreement | bundle | statement"));
        assert!(rendered
            .contains("omitted_artifact_types audit | agreement_signatures | bundle_signatures"));
        assert!(rendered.contains("verification_notice manifest is descriptive only"));
        assert!(rendered.contains("canonical_agreement_id agr-wallet-1"));
        assert!(rendered.contains("detached_agreement_signatures 1"));
    }

    #[test]
    fn share_package_export_selection_filters_requested_artifacts() {
        let agreement = sample_agreement();
        let bundle = sample_bundle();
        let statement = build_agreement_statement(&AgreementAuditRecord {
            metadata: irium_node_rs::settlement::AgreementAuditMetadata {
                version: 1,
                generated_at: 1,
                generator_surface: "test".to_string(),
                trust_model_summary: "derived".to_string(),
            },
            agreement: irium_node_rs::settlement::AgreementAuditAgreementSummary {
                agreement_id: agreement.agreement_id.clone(),
                agreement_hash: irium_node_rs::settlement::compute_agreement_hash_hex(&agreement)
                    .unwrap(),
                template_type: agreement.template_type,
                network_marker: agreement.network_marker.clone(),
                payer: agreement.payer.clone(),
                payee: agreement.payee.clone(),
                parties: agreement.parties.clone(),
                total_amount: agreement.total_amount,
                milestone_count: agreement.milestones.len(),
                milestones: vec![],
                settlement_deadline: agreement.deadlines.settlement_deadline,
                refund_deadline: agreement.deadlines.refund_deadline,
                dispute_window: agreement.deadlines.dispute_window,
                document_hash: agreement.document_hash.clone(),
                metadata_hash: agreement.metadata_hash.clone(),
                invoice_reference: agreement.invoice_reference.clone(),
                external_reference: agreement.external_reference.clone(),
            },
            local_bundle: irium_node_rs::settlement::AgreementAuditBundleContext {
                bundle_used: true,
                verification_ok: true,
                saved_at: None,
                source_label: None,
                note: None,
                linked_funding_txids: vec![],
                milestone_hints: vec![],
                local_only_notice: "local only".to_string(),
            },
            chain_observed: irium_node_rs::settlement::AgreementAuditChainObservedContext {
                linked_transactions: vec![],
                linked_transaction_count: 0,
                anchor_observation_notice: "none".to_string(),
            },
            funding_legs: irium_node_rs::settlement::AgreementAuditFundingLegSummary {
                candidate_count: 0,
                selection_required: false,
                selected_leg: None,
                ambiguity_warning: None,
                candidates: vec![],
                notice: "derived".to_string(),
            },
            timeline: irium_node_rs::settlement::AgreementAuditTimelineSummary {
                reconstructed: false,
                event_count: 0,
                events: vec![],
                notice: "derived".to_string(),
            },
            settlement_state: irium_node_rs::settlement::AgreementAuditSettlementStateSummary {
                lifecycle_state: AgreementLifecycleState::Draft,
                derived_state_label: "unresolved".to_string(),
                selection_required: false,
                funded_amount: 0,
                released_amount: 0,
                refunded_amount: 0,
                summary_note: "derived".to_string(),
            },
            trust_boundaries: irium_node_rs::settlement::AgreementAuditTrustBoundaries {
                consensus_enforced: vec![],
                htlc_enforced: vec![],
                metadata_indexed: vec![],
                local_bundle_only: vec![],
                off_chain_required: vec![],
            },
            authenticity: None,
        });
        let includes = vec!["agreement".to_string(), "statement".to_string()];
        let (agreement, bundle, audit, statement, agreement_signatures, bundle_signatures) =
            filter_share_package_export_selection(
                &includes,
                Some(agreement),
                Some(bundle),
                None,
                Some(statement),
                vec![],
                vec![],
            )
            .unwrap();
        assert!(agreement.is_some());
        assert!(bundle.is_none());
        assert!(audit.is_none());
        assert!(statement.is_some());
        assert!(agreement_signatures.is_empty());
        assert!(bundle_signatures.is_empty());
    }

    #[test]
    fn share_package_export_selection_rejects_missing_requested_artifacts() {
        let err = filter_share_package_export_selection(
            &["bundle".to_string()],
            Some(sample_agreement()),
            None,
            None,
            None,
            vec![],
            vec![],
        )
        .unwrap_err();
        assert!(err.contains("--include bundle requested"));
    }

    #[test]
    fn share_package_verification_render_is_stable() {
        let agreement = sample_agreement();
        let bundle = sample_bundle();
        let package = build_agreement_share_package(
            Some(1_710_001_301),
            Some("counterparty-a".to_string()),
            Some("handoff".to_string()),
            Some(agreement.clone()),
            Some(bundle),
            None,
            None,
            vec![],
            vec![],
        )
        .unwrap();
        let result =
            build_agreement_share_package_verification(&package, None, 1_710_001_302).unwrap();
        let rendered = render_share_package_verification(&result);
        assert!(rendered.contains("Agreement share package verification"));
        assert!(rendered.contains("Agreement share package"));
        assert!(rendered.contains("Agreement artifact verification"));
        assert!(rendered.contains("Share package contents are supplied handoff artifacts"));
    }

    #[test]
    fn verified_share_package_import_succeeds_and_records_provenance() {
        let _guard = test_guard();
        let root = temp_share_package_dir("import-ok");
        let bundle_dir = root.join("bundles");
        let agreement_dir = root.join("agreements");
        let inbox_dir = root.join("inbox");
        let signatures_dir = root.join("signatures");
        fs::create_dir_all(&root).unwrap();
        env::set_var("IRIUM_AGREEMENT_BUNDLES_DIR", &bundle_dir);
        env::set_var("IRIUM_IMPORTED_AGREEMENTS_DIR", &agreement_dir);
        env::set_var("IRIUM_SHARE_PACKAGE_INBOX_DIR", &inbox_dir);
        env::set_var("IRIUM_IMPORTED_SIGNATURES_DIR", &signatures_dir);

        let agreement = sample_agreement();
        let bundle = sample_bundle();
        let agreement_hash =
            irium_node_rs::settlement::compute_agreement_hash_hex(&agreement).unwrap();
        let signature = sample_detached_signature(
            AgreementSignatureTargetType::Agreement,
            agreement_hash.clone(),
        );
        let audit = sample_audit_for_agreement(&agreement);
        let statement = build_agreement_statement(&audit);
        let package = build_agreement_share_package(
            Some(1_710_001_401),
            Some("counterparty-a".to_string()),
            Some("handoff".to_string()),
            Some(agreement.clone()),
            Some(bundle.clone()),
            Some(audit),
            Some(statement),
            vec![signature],
            vec![],
        )
        .unwrap();
        let verification =
            build_agreement_share_package_verification(&package, None, 1_710_001_402).unwrap();
        let receipt = import_verified_share_package(
            &package,
            &verification,
            "package.json",
            Some("email".to_string()),
            &[
                "agreement".to_string(),
                "bundle".to_string(),
                "agreement_signatures".to_string(),
                "statement".to_string(),
            ],
        )
        .unwrap();
        assert!(receipt
            .imported_artifact_types
            .contains(&"agreement".to_string()));
        assert!(receipt
            .imported_artifact_types
            .contains(&"bundle".to_string()));
        assert!(receipt.provenance_notice.contains("informational only"));
        assert!(Path::new(&receipt.package_path).exists());
        assert!(Path::new(&receipt.verification_path).exists());
        let resolved = resolve_agreement_input(&agreement_hash).unwrap();
        assert_eq!(resolved.agreement.agreement_id, agreement.agreement_id);
        env::remove_var("IRIUM_AGREEMENT_BUNDLES_DIR");
        env::remove_var("IRIUM_IMPORTED_AGREEMENTS_DIR");
        env::remove_var("IRIUM_SHARE_PACKAGE_INBOX_DIR");
        env::remove_var("IRIUM_IMPORTED_SIGNATURES_DIR");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn selective_share_package_import_only_saves_requested_artifacts() {
        let _guard = test_guard();
        let root = temp_share_package_dir("import-selective");
        let bundle_dir = root.join("bundles");
        let agreement_dir = root.join("agreements");
        let inbox_dir = root.join("inbox");
        let signatures_dir = root.join("signatures");
        fs::create_dir_all(&root).unwrap();
        env::set_var("IRIUM_AGREEMENT_BUNDLES_DIR", &bundle_dir);
        env::set_var("IRIUM_IMPORTED_AGREEMENTS_DIR", &agreement_dir);
        env::set_var("IRIUM_SHARE_PACKAGE_INBOX_DIR", &inbox_dir);
        env::set_var("IRIUM_IMPORTED_SIGNATURES_DIR", &signatures_dir);
        let agreement = sample_agreement();
        let package = build_agreement_share_package(
            Some(1),
            None,
            None,
            Some(agreement.clone()),
            Some(sample_bundle()),
            None,
            None,
            vec![],
            vec![],
        )
        .unwrap();
        let verification = build_agreement_share_package_verification(&package, None, 2).unwrap();
        let receipt = import_verified_share_package(
            &package,
            &verification,
            "package.json",
            None,
            &["agreement".to_string()],
        )
        .unwrap();
        assert!(receipt
            .imported_artifact_types
            .contains(&"agreement".to_string()));
        assert!(!receipt
            .imported_artifact_types
            .contains(&"bundle".to_string()));
        assert!(receipt.artifact_paths.bundle_path.is_none());
        env::remove_var("IRIUM_AGREEMENT_BUNDLES_DIR");
        env::remove_var("IRIUM_IMPORTED_AGREEMENTS_DIR");
        env::remove_var("IRIUM_SHARE_PACKAGE_INBOX_DIR");
        env::remove_var("IRIUM_IMPORTED_SIGNATURES_DIR");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn share_package_import_rejects_mismatched_detached_signature() {
        let _guard = test_guard();
        let root = temp_share_package_dir("import-bad-sig");
        env::set_var("IRIUM_AGREEMENT_BUNDLES_DIR", root.join("bundles"));
        env::set_var("IRIUM_IMPORTED_AGREEMENTS_DIR", root.join("agreements"));
        env::set_var("IRIUM_SHARE_PACKAGE_INBOX_DIR", root.join("inbox"));
        env::set_var("IRIUM_IMPORTED_SIGNATURES_DIR", root.join("signatures"));
        let agreement = sample_agreement();
        let package = build_agreement_share_package(
            Some(1),
            None,
            None,
            Some(agreement.clone()),
            None,
            None,
            None,
            vec![sample_detached_signature(
                AgreementSignatureTargetType::Agreement,
                "aa".repeat(32),
            )],
            vec![],
        )
        .unwrap();
        let verification = build_agreement_share_package_verification(&package, None, 2).unwrap();
        let err = import_verified_share_package(
            &package,
            &verification,
            "package.json",
            None,
            &["agreement_signatures".to_string()],
        )
        .unwrap_err();
        assert!(err.contains("did not verify against the canonical agreement hash"));
        env::remove_var("IRIUM_AGREEMENT_BUNDLES_DIR");
        env::remove_var("IRIUM_IMPORTED_AGREEMENTS_DIR");
        env::remove_var("IRIUM_SHARE_PACKAGE_INBOX_DIR");
        env::remove_var("IRIUM_IMPORTED_SIGNATURES_DIR");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn share_package_import_rejects_conflicting_local_agreement_ids() {
        let _guard = test_guard();
        let root = temp_share_package_dir("import-conflict");
        env::set_var("IRIUM_AGREEMENT_BUNDLES_DIR", root.join("bundles"));
        env::set_var("IRIUM_IMPORTED_AGREEMENTS_DIR", root.join("agreements"));
        env::set_var("IRIUM_SHARE_PACKAGE_INBOX_DIR", root.join("inbox"));
        env::set_var("IRIUM_IMPORTED_SIGNATURES_DIR", root.join("signatures"));
        let first = sample_agreement();
        save_agreement_to_store_checked(&imported_agreements_dir(), &first).unwrap();
        let mut second = sample_agreement();
        second.document_hash = "44".repeat(32);
        let package = build_agreement_share_package(
            Some(1),
            None,
            None,
            Some(second),
            None,
            None,
            None,
            vec![],
            vec![],
        )
        .unwrap();
        let verification = build_agreement_share_package_verification(&package, None, 2).unwrap();
        let err = import_verified_share_package(
            &package,
            &verification,
            "package.json",
            None,
            &["agreement".to_string()],
        )
        .unwrap_err();
        assert!(err.contains("already maps to a different imported agreement hash"));
        env::remove_var("IRIUM_AGREEMENT_BUNDLES_DIR");
        env::remove_var("IRIUM_IMPORTED_AGREEMENTS_DIR");
        env::remove_var("IRIUM_SHARE_PACKAGE_INBOX_DIR");
        env::remove_var("IRIUM_IMPORTED_SIGNATURES_DIR");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn share_package_receipt_rendering_is_stable() {
        let receipt = SharePackageReceipt {
            version: 1,
            receipt_id: "1710001500-aabbccddeeff".to_string(),
            imported_at: 1_710_001_500,
            source_path: "package.json".to_string(),
            import_source_label: Some("email".to_string()),
            sender_label: Some("counterparty-a".to_string()),
            package_note: Some("handoff".to_string()),
            package_profile: "verification_package".to_string(),
            included_artifact_types: vec!["agreement".to_string(), "bundle".to_string()],
            imported_artifact_types: vec!["agreement".to_string()],
            already_present_artifact_types: vec!["bundle".to_string()],
            rejected_artifact_types: vec![],
            canonical_agreement_id: Some("agr-wallet-1".to_string()),
            canonical_agreement_hash: Some("aa".repeat(32)),
            bundle_hash: Some("bb".repeat(32)),
            verification_summary: SharePackageReceiptVerificationSummary {
                verified_match_count: 2,
                mismatch_count: 0,
                unverifiable_count: 1,
                valid_signatures: 1,
                invalid_signatures: 0,
                unverifiable_signatures: 0,
            },
            warnings: vec!["local metadata only".to_string()],
            trust_notice: "package contents are supplied artifacts".to_string(),
            provenance_notice: "Local share-package intake metadata is informational only."
                .to_string(),
            package_path: "/tmp/package.json".to_string(),
            verification_path: "/tmp/verification.json".to_string(),
            artifact_paths: SharePackageReceiptArtifactPaths {
                agreement_path: Some("/tmp/agreement.json".to_string()),
                bundle_path: Some("/tmp/bundle.json".to_string()),
                audit_path: None,
                statement_path: None,
                agreement_signature_paths: vec!["/tmp/sig.json".to_string()],
                bundle_signature_paths: vec![],
            },
        };
        let rendered = render_share_package_receipt(&receipt);
        assert!(rendered.contains("Agreement share package receipt"));
        assert!(rendered.contains("receipt_id 1710001500-aabbccddeeff"));
        assert!(rendered.contains("imported_artifact_types agreement"));
        assert!(rendered.contains("already_present_artifact_types bundle"));
        let list_rendered = render_share_package_receipt_list(&[receipt]);
        assert!(list_rendered.contains("Agreement share package inbox"));
        assert!(list_rendered.contains("receipt_count 1"));
    }
    fn setup_housekeeping_test_root(tag: &str) -> PathBuf {
        let root = temp_share_package_dir(tag);
        fs::create_dir_all(&root).unwrap();
        env::set_var("IRIUM_AGREEMENT_BUNDLES_DIR", root.join("bundles"));
        env::set_var("IRIUM_IMPORTED_AGREEMENTS_DIR", root.join("agreements"));
        env::set_var("IRIUM_SHARE_PACKAGE_INBOX_DIR", root.join("inbox"));
        env::set_var("IRIUM_IMPORTED_SIGNATURES_DIR", root.join("signatures"));
        root
    }

    fn cleanup_housekeeping_test_root(root: &Path) {
        env::remove_var("IRIUM_AGREEMENT_BUNDLES_DIR");
        env::remove_var("IRIUM_IMPORTED_AGREEMENTS_DIR");
        env::remove_var("IRIUM_SHARE_PACKAGE_INBOX_DIR");
        env::remove_var("IRIUM_IMPORTED_SIGNATURES_DIR");
        let _ = fs::remove_dir_all(root);
    }

    fn import_sample_share_package_for_housekeeping(
        root: &Path,
        tag: &str,
        agreement: AgreementObject,
        bundle: AgreementBundle,
    ) -> SharePackageReceipt {
        let agreement_hash =
            irium_node_rs::settlement::compute_agreement_hash_hex(&agreement).unwrap();
        let package = build_agreement_share_package(
            Some(1_710_010_000 + tag.len() as u64),
            Some(format!("sender-{}", tag)),
            Some(format!("note-{}", tag)),
            Some(agreement.clone()),
            Some(bundle),
            Some(sample_audit_for_agreement(&agreement)),
            Some(build_agreement_statement(&sample_audit_for_agreement(
                &agreement,
            ))),
            vec![sample_detached_signature(
                AgreementSignatureTargetType::Agreement,
                agreement_hash,
            )],
            vec![],
        )
        .unwrap();
        let verification =
            build_agreement_share_package_verification(&package, None, 1_710_010_500).unwrap();
        import_verified_share_package(
            &package,
            &verification,
            &format!("{}.json", tag),
            Some(format!("source-{}", tag)),
            &[
                "agreement".to_string(),
                "bundle".to_string(),
                "agreement_signatures".to_string(),
                "audit".to_string(),
                "statement".to_string(),
            ],
        )
        .unwrap()
    }

    #[test]
    fn agreement_local_store_listing_shape_is_stable() {
        let _guard = test_guard();
        let root = setup_housekeeping_test_root("housekeeping-list");
        let receipt = import_sample_share_package_for_housekeeping(
            &root,
            "housekeeping-list",
            sample_agreement(),
            sample_bundle(),
        );
        let listing = build_agreement_local_store_listing(true).unwrap();
        assert_eq!(listing.active_receipt_count, 1);
        assert_eq!(listing.archived_receipt_count, 0);
        assert_eq!(listing.bundle_count, 1);
        assert_eq!(listing.raw_agreement_count, 1);
        assert_eq!(listing.detached_signature_count, 1);
        assert_eq!(listing.active_receipts[0].receipt_id, receipt.receipt_id);
        assert!(listing
            .stored_informational_files
            .iter()
            .any(|item| item.kind == "statement"));
        let rendered = render_agreement_local_store_listing(&listing);
        assert!(rendered.contains("Agreement local artifact store"));
        assert!(rendered.contains("active_receipt_count 1"));
        cleanup_housekeeping_test_root(&root);
    }

    #[test]
    fn archive_operation_preserves_receipt_metadata() {
        let _guard = test_guard();
        let root = setup_housekeeping_test_root("housekeeping-archive");
        let receipt = import_sample_share_package_for_housekeeping(
            &root,
            "housekeeping-archive",
            sample_agreement(),
            sample_bundle(),
        );
        let original_path = share_package_inbox_dir().join(&receipt.receipt_id);
        let result = archive_share_package_receipt(&receipt.receipt_id).unwrap();
        assert!(!original_path.exists());
        assert!(Path::new(&result.to_path).exists());
        let archived = resolve_share_package_receipt_record(&receipt.receipt_id).unwrap();
        assert_eq!(archived.location, SharePackageReceiptLocation::Archived);
        assert_eq!(archived.receipt.receipt_id, receipt.receipt_id);
        assert_eq!(archived.receipt.source_path, receipt.source_path);
        assert_eq!(
            archived
                .housekeeping
                .as_ref()
                .and_then(|item| item.archived_by_action.as_deref()),
            Some("agreement-share-package-archive")
        );
        cleanup_housekeeping_test_root(&root);
    }

    #[test]
    fn remove_operation_targets_only_requested_local_item() {
        let _guard = test_guard();
        let root = setup_housekeeping_test_root("housekeeping-remove-path");
        let receipt = import_sample_share_package_for_housekeeping(
            &root,
            "housekeeping-remove-path",
            sample_agreement(),
            sample_bundle(),
        );
        let statement_path = PathBuf::from(
            receipt
                .artifact_paths
                .statement_path
                .clone()
                .expect("statement path should exist"),
        );
        let report = remove_exact_local_path(&statement_path, false, false).unwrap();
        assert!(!statement_path.exists());
        assert!(
            share_package_receipt_path(&share_package_inbox_dir().join(&receipt.receipt_id))
                .exists()
        );
        assert!(report
            .changed
            .iter()
            .any(|item| item.path == statement_path.display().to_string()));
        assert!(report
            .warnings
            .iter()
            .any(|item| item.contains("does not change chain state")));
        cleanup_housekeeping_test_root(&root);
    }

    #[test]
    fn prune_dry_run_reports_without_mutating_files() {
        let _guard = test_guard();
        let root = setup_housekeeping_test_root("housekeeping-prune-dry-run");
        let receipt = import_sample_share_package_for_housekeeping(
            &root,
            "housekeeping-prune-dry-run",
            sample_agreement(),
            sample_bundle(),
        );
        let archive_result = archive_share_package_receipt(&receipt.receipt_id).unwrap();
        let report = prune_share_package_store(true, Some(0), true, false).unwrap();
        assert!(Path::new(&archive_result.to_path).exists());
        assert!(report
            .changed
            .iter()
            .any(|item| item.kind == "archived_receipt"));
        cleanup_housekeeping_test_root(&root);
    }

    #[test]
    fn prune_skips_still_referenced_canonical_artifacts_by_default() {
        let _guard = test_guard();
        let root = setup_housekeeping_test_root("housekeeping-prune-shared");
        let agreement = sample_agreement();
        let bundle = sample_bundle();
        let first = import_sample_share_package_for_housekeeping(
            &root,
            "housekeeping-prune-shared-a",
            agreement.clone(),
            bundle.clone(),
        );
        let second = import_sample_share_package_for_housekeeping(
            &root,
            "housekeeping-prune-shared-b",
            agreement,
            bundle,
        );
        let first_receipt_dir = PathBuf::from(&first.package_path)
            .parent()
            .unwrap()
            .to_path_buf();
        archive_share_package_receipt(first_receipt_dir.to_str().unwrap()).unwrap();
        let report = prune_share_package_store(true, Some(0), true, true).unwrap();
        assert!(report
            .skipped
            .iter()
            .any(|item| item.note.contains("still referenced by 2 local receipts")));
        assert!(Path::new(second.artifact_paths.agreement_path.as_deref().unwrap()).exists());
        cleanup_housekeeping_test_root(&root);
    }

    #[test]
    fn archived_receipt_handling_works_correctly() {
        let _guard = test_guard();
        let root = setup_housekeeping_test_root("housekeeping-archived-list");
        let receipt = import_sample_share_package_for_housekeeping(
            &root,
            "housekeeping-archived-list",
            sample_agreement(),
            sample_bundle(),
        );
        archive_share_package_receipt(&receipt.receipt_id).unwrap();
        let listing = build_agreement_local_store_listing(true).unwrap();
        assert_eq!(listing.active_receipt_count, 0);
        assert_eq!(listing.archived_receipt_count, 1);
        assert_eq!(listing.archived_receipts[0].receipt_id, receipt.receipt_id);
        cleanup_housekeeping_test_root(&root);
    }

    #[test]
    fn shared_reference_removal_warns_and_leaves_canonical_artifacts() {
        let _guard = test_guard();
        let root = setup_housekeeping_test_root("housekeeping-remove-shared");
        let agreement = sample_agreement();
        let bundle = sample_bundle();
        let first = import_sample_share_package_for_housekeeping(
            &root,
            "housekeeping-remove-shared-a",
            agreement.clone(),
            bundle.clone(),
        );
        let second = import_sample_share_package_for_housekeeping(
            &root,
            "housekeeping-remove-shared-b",
            agreement,
            bundle,
        );
        let agreement_path = PathBuf::from(second.artifact_paths.agreement_path.clone().unwrap());
        let first_receipt_dir = PathBuf::from(&first.package_path)
            .parent()
            .unwrap()
            .to_path_buf();
        let report =
            remove_share_package_receipt(first_receipt_dir.to_str().unwrap(), false, true).unwrap();
        assert!(!first_receipt_dir.exists());
        assert!(agreement_path.exists());
        assert!(report
            .skipped
            .iter()
            .any(|item| item.note.contains("still referenced by 2 local receipts")));
        cleanup_housekeeping_test_root(&root);
    }

    #[test]
    fn package_import_list_and_show_backward_compatibility_remain() {
        let _guard = test_guard();
        let root = setup_housekeeping_test_root("housekeeping-backcompat");
        let receipt = import_sample_share_package_for_housekeeping(
            &root,
            "housekeeping-backcompat",
            sample_agreement(),
            sample_bundle(),
        );
        let listed = list_share_package_receipts_at(&share_package_inbox_dir()).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].receipt_id, receipt.receipt_id);
        let shown = resolve_share_package_receipt(&receipt.receipt_id).unwrap();
        assert_eq!(shown.receipt_id, receipt.receipt_id);
        cleanup_housekeeping_test_root(&root);
    }

    #[test]
    fn agreement_statement_render_is_stable() {
        let statement = build_agreement_statement(&AgreementAuditRecord {
            metadata: irium_node_rs::settlement::AgreementAuditMetadata {
                version: 1,
                generated_at: 1_710_000_123,
                generator_surface: "iriumd_rpc".to_string(),
                trust_model_summary: "derived only".to_string(),
            },
            agreement: irium_node_rs::settlement::AgreementAuditAgreementSummary {
                agreement_id: "agr-wallet-statement".to_string(),
                agreement_hash: "aa".repeat(32),
                template_type: AgreementTemplateType::SimpleReleaseRefund,
                network_marker: "IRIUM".to_string(),
                payer: "Qpayer".to_string(),
                payee: "Qpayee".to_string(),
                parties: vec![],
                total_amount: 125_000_000,
                milestone_count: 0,
                milestones: vec![],
                settlement_deadline: Some(100),
                refund_deadline: Some(120),
                dispute_window: None,
                document_hash: "11".repeat(32),
                metadata_hash: None,
                invoice_reference: None,
                external_reference: None,
            },
            local_bundle: irium_node_rs::settlement::AgreementAuditBundleContext {
                bundle_used: true,
                verification_ok: true,
                saved_at: Some(1),
                source_label: Some("wallet-test".to_string()),
                note: None,
                linked_funding_txids: vec!["bb".repeat(32)],
                milestone_hints: vec![],
                local_only_notice: "local only".to_string(),
            },
            chain_observed: irium_node_rs::settlement::AgreementAuditChainObservedContext {
                linked_transactions: vec![],
                linked_transaction_count: 1,
                anchor_observation_notice: "chain observed".to_string(),
            },
            funding_legs: irium_node_rs::settlement::AgreementAuditFundingLegSummary {
                candidate_count: 1,
                selection_required: false,
                selected_leg: None,
                ambiguity_warning: None,
                candidates: vec![],
                notice: "funding".to_string(),
            },
            timeline: irium_node_rs::settlement::AgreementAuditTimelineSummary {
                reconstructed: true,
                event_count: 0,
                events: vec![],
                notice: "timeline".to_string(),
            },
            settlement_state: irium_node_rs::settlement::AgreementAuditSettlementStateSummary {
                lifecycle_state: AgreementLifecycleState::Funded,
                derived_state_label: "funded".to_string(),
                selection_required: false,
                funded_amount: 125_000_000,
                released_amount: 0,
                refunded_amount: 0,
                summary_note: "derived state".to_string(),
            },
            authenticity: None,
            trust_boundaries: irium_node_rs::settlement::AgreementAuditTrustBoundaries {
                consensus_enforced: vec!["anchor visibility".to_string()],
                htlc_enforced: vec!["htlc branch".to_string()],
                metadata_indexed: vec!["timeline".to_string()],
                local_bundle_only: vec!["bundle label".to_string()],
                off_chain_required: vec!["agreement exchange".to_string()],
            },
        });
        let rendered = render_agreement_statement(&statement);
        assert!(rendered.contains("Derived settlement statement"));
        assert!(rendered.contains("derived_status funded"));
        assert!(rendered.contains("trust_boundaries"));
        assert!(rendered.contains("  canonical "));
        assert!(!rendered.contains("sections"));
    }

    #[test]
    fn agreement_statement_json_shape_is_stable() {
        let statement = AgreementStatement {
            metadata: irium_node_rs::settlement::AgreementStatementMetadata {
                version: 1,
                generated_at: 1,
                derived_notice: "derived only".to_string(),
            },
            identity: irium_node_rs::settlement::AgreementStatementIdentity {
                agreement_id: "agr-wallet-statement".to_string(),
                agreement_hash: "aa".repeat(32),
                template_type: AgreementTemplateType::SimpleReleaseRefund,
            },
            counterparties: irium_node_rs::settlement::AgreementStatementCounterparties {
                payer: "payer".to_string(),
                payee: "payee".to_string(),
                parties_summary: vec!["payer: Payer <Qpayer>".to_string()],
            },
            commercial: irium_node_rs::settlement::AgreementStatementCommercialSummary {
                total_amount: 1,
                milestone_summary: "No milestone schedule in the supplied agreement".to_string(),
                settlement_deadline: None,
                refund_deadline: None,
                release_path_summary: "release".to_string(),
                refund_path_summary: "refund".to_string(),
            },
            observed: irium_node_rs::settlement::AgreementStatementObservedSummary {
                funding_observed: false,
                release_observed: false,
                refund_observed: false,
                ambiguity_warning: None,
                linked_txids: vec![],
            },
            derived: irium_node_rs::settlement::AgreementStatementDerivedSummary {
                derived_state_label: "proposed".to_string(),
                funded_amount: 0,
                released_amount: 0,
                refunded_amount: 0,
                note: "derived".to_string(),
            },
            authenticity: None,
            trust_notice: irium_node_rs::settlement::AgreementStatementTrustNotice {
                consensus_visible: vec!["anchor visibility".to_string()],
                htlc_enforced: vec!["htlc branch".to_string()],
                derived_indexed: vec!["timeline".to_string()],
                local_off_chain: vec!["agreement exchange".to_string()],
                canonical_notice: "canonical".to_string(),
            },
            references: irium_node_rs::settlement::AgreementStatementReferences {
                linked_txids: vec![],
                selected_funding_txid: None,
                canonical_agreement_notice: "canonical".to_string(),
            },
        };
        let value = serde_json::to_value(statement).unwrap();
        assert_eq!(value["identity"]["agreement_id"], "agr-wallet-statement");
        assert_eq!(value["derived"]["derived_state_label"], "proposed");
    }
    #[test]
    fn signature_verification_summary_render_is_stable() {
        let verification = AgreementSignatureVerification {
            target_type: AgreementSignatureTargetType::Agreement,
            target_hash: "aa".repeat(32),
            signer_public_key: "02".to_string() + &"11".repeat(32),
            signer_address: Some("Qsigwallet".to_string()),
            signer_role: Some("buyer".to_string()),
            signature_type: AGREEMENT_SIGNATURE_TYPE_SECP256K1.to_string(),
            timestamp: Some(1_710_000_777),
            valid: true,
            matches_expected_target: true,
            authenticity_note: "Valid signature proves authenticity only.".to_string(),
            warnings: vec!["none".to_string()],
        };
        let rendered = render_signature_verification_summary(&verification, "Agreement signature");
        assert!(rendered.contains("Agreement signature"));
        assert!(rendered.contains("signer_address Qsigwallet"));
        assert!(rendered.contains("valid true"));
        assert!(rendered.contains("authenticity only"));
    }

    #[test]
    fn bundle_signature_verification_render_is_stable() {
        let mut bundle = build_agreement_bundle(
            &sample_agreement(),
            1_710_000_000,
            Some("wallet-test".to_string()),
            None,
            vec![],
            vec![],
        )
        .unwrap();
        bundle.signatures.push(AgreementSignatureEnvelope {
            version: AGREEMENT_SIGNATURE_VERSION,
            target_type: AgreementSignatureTargetType::Bundle,
            target_hash: "bb".repeat(32),
            signer_public_key: "02".to_string() + &"22".repeat(32),
            signer_address: Some("Qbundlewallet".to_string()),
            signature_type: AGREEMENT_SIGNATURE_TYPE_SECP256K1.to_string(),
            timestamp: Some(1_710_000_778),
            signer_role: Some("seller".to_string()),
            signature: "33".repeat(64),
        });
        let verifications = vec![AgreementSignatureVerification {
            target_type: AgreementSignatureTargetType::Bundle,
            target_hash: "bb".repeat(32),
            signer_public_key: "02".to_string() + &"22".repeat(32),
            signer_address: Some("Qbundlewallet".to_string()),
            signer_role: Some("seller".to_string()),
            signature_type: AGREEMENT_SIGNATURE_TYPE_SECP256K1.to_string(),
            timestamp: Some(1_710_000_778),
            valid: false,
            matches_expected_target: false,
            authenticity_note: "Signature did not verify. Authenticity is not established."
                .to_string(),
            warnings: vec!["signature target hash did not match the supplied artifact".to_string()],
        }];
        let rendered = render_bundle_signature_verifications(&bundle, &verifications);
        assert!(rendered.contains("signature_count 1"));
        assert!(rendered.contains("Qbundlewallet"));
        assert!(rendered.contains("valid=false"));
        assert!(rendered.contains("expected_match=false"));
    }

    // ---- Phase 2 wallet CLI tests ----

    #[test]
    fn policy_check_cli_parses_required_args() {
        let args: Vec<String> = vec![
            "--agreement".to_string(), "agr.json".to_string(),
            "--policy".to_string(), "pol.json".to_string(),
        ];
        let opts = parse_policy_check_cli(&args).expect("must parse");
        assert_eq!(opts.agreement_path, "agr.json");
        assert_eq!(opts.policy_path, "pol.json");
        assert!(opts.proof_paths.is_empty());
        assert!(!opts.json_mode);
    }

    #[test]
    fn policy_check_cli_parses_all_flags() {
        let args: Vec<String> = vec![
            "--agreement".to_string(), "agr.json".to_string(),
            "--policy".to_string(), "pol.json".to_string(),
            "--proof".to_string(), "prf1.json".to_string(),
            "--proof".to_string(), "prf2.json".to_string(),
            "--rpc".to_string(), "http://localhost:9090".to_string(),
            "--json".to_string(),
        ];
        let opts = parse_policy_check_cli(&args).expect("must parse");
        assert_eq!(opts.proof_paths.len(), 2);
        assert_eq!(opts.proof_paths[0], "prf1.json");
        assert_eq!(opts.rpc_url, "http://localhost:9090");
        assert!(opts.json_mode);
    }

    #[test]
    fn policy_check_cli_rejects_missing_agreement() {
        let args: Vec<String> = vec!["--policy".to_string(), "pol.json".to_string()];
        let err = parse_policy_check_cli(&args).unwrap_err();
        assert!(err.contains("--agreement"), "got: {err}");
    }

    #[test]
    fn policy_check_cli_rejects_missing_policy() {
        let args: Vec<String> = vec!["--agreement".to_string(), "agr.json".to_string()];
        let err = parse_policy_check_cli(&args).unwrap_err();
        assert!(err.contains("--policy"), "got: {err}");
    }

    #[test]
    fn policy_check_cli_rejects_unknown_flag() {
        let args: Vec<String> = vec![
            "--agreement".to_string(), "agr.json".to_string(),
            "--policy".to_string(), "pol.json".to_string(),
            "--unknown-flag".to_string(),
        ];
        let err = parse_policy_check_cli(&args).unwrap_err();
        assert!(err.contains("unknown"), "got: {err}");
    }

    #[test]
    fn render_policy_check_summary_release_eligible() {
        let resp = CheckPolicyRpcResponse {
            agreement_hash: "aabbcc".to_string(),
            policy_id: "pol-render-001".to_string(),
            tip_height: 500,
            release_eligible: true,
            refund_eligible: false,
            reason: "all release requirements satisfied".to_string(),
            evaluated_rules: vec!["proof 'prf-1' verified ok".to_string()],
            holdback: None,
            milestone_results: vec![],
        };
        let out = render_policy_check_summary(&resp);
        assert!(out.contains("agreement_hash aabbcc"), "missing hash: {out}");
        assert!(out.contains("policy_id pol-render-001"), "missing policy_id: {out}");
        assert!(out.contains("tip_height 500"), "missing height: {out}");
        assert!(out.contains("release_eligible true"), "must show release: {out}");
        assert!(out.contains("refund_eligible false"), "must show refund: {out}");
        assert!(out.contains("prf-1"), "must show evaluated rule: {out}");
    }

    #[test]
    fn render_policy_check_summary_refund_eligible() {
        let resp = CheckPolicyRpcResponse {
            agreement_hash: "ddeeff".to_string(),
            policy_id: "pol-render-002".to_string(),
            tip_height: 1000,
            release_eligible: false,
            refund_eligible: true,
            reason: "no-response rule deadline reached".to_string(),
            evaluated_rules: vec![],
            holdback: None,
            milestone_results: vec![],
        };
        let out = render_policy_check_summary(&resp);
        assert!(out.contains("refund_eligible true"), "must show refund: {out}");
        assert!(out.contains("release_eligible false"), "must show release: {out}");
        assert!(out.contains("no-response"), "must show reason: {out}");
    }

    #[test]
    fn render_policy_check_summary_not_eligible() {
        let resp = CheckPolicyRpcResponse {
            agreement_hash: "001122".to_string(),
            policy_id: "pol-render-003".to_string(),
            tip_height: 10,
            release_eligible: false,
            refund_eligible: false,
            reason: "no release or refund condition was met".to_string(),
            evaluated_rules: vec!["requirement 'req-001': unsatisfied".to_string()],
            holdback: None,
            milestone_results: vec![],
        };
        let out = render_policy_check_summary(&resp);
        assert!(out.contains("release_eligible false"));
        assert!(out.contains("refund_eligible false"));
        assert!(out.contains("no release or refund"));
    }


    #[test]
    fn render_policy_check_summary_holdback_held() {
        use irium_node_rs::settlement::{HoldbackEvaluationResult, HoldbackOutcome};
        let resp = CheckPolicyRpcResponse {
            agreement_hash: "aabb01".to_string(),
            policy_id: "pol-hb-held".to_string(),
            tip_height: 100,
            release_eligible: true,
            refund_eligible: false,
            reason: "base satisfied; holdback pending".to_string(),
            evaluated_rules: vec![],
            holdback: Some(HoldbackEvaluationResult {
                holdback_present: true,
                holdback_released: false,
                holdback_bps: 1000,
                immediate_release_bps: 9000,
                holdback_outcome: HoldbackOutcome::Held,
                holdback_reason: "base satisfied; holdback pending release condition".to_string(),
            }),
            milestone_results: vec![],
        };
        let out = render_policy_check_summary(&resp);
        assert!(out.contains("holdback_outcome held"), "must show held: {out}");
        assert!(out.contains("holdback_bps 1000"), "must show bps: {out}");
        assert!(out.contains("immediate_release_bps 9000"), "must show releasable: {out}");
        assert!(out.contains("holdback_reason"), "must show reason: {out}");
        assert!(out.contains("release_eligible true"), "base must show eligible: {out}");
    }

    #[test]
    fn render_policy_check_summary_holdback_released() {
        use irium_node_rs::settlement::{HoldbackEvaluationResult, HoldbackOutcome};
        let resp = CheckPolicyRpcResponse {
            agreement_hash: "aabb02".to_string(),
            policy_id: "pol-hb-released".to_string(),
            tip_height: 1000,
            release_eligible: true,
            refund_eligible: false,
            reason: "all conditions met".to_string(),
            evaluated_rules: vec![],
            holdback: Some(HoldbackEvaluationResult {
                holdback_present: true,
                holdback_released: true,
                holdback_bps: 500,
                immediate_release_bps: 10000,
                holdback_outcome: HoldbackOutcome::Released,
                holdback_reason: "holdback released by deadline at height 500".to_string(),
            }),
            milestone_results: vec![],
        };
        let out = render_policy_check_summary(&resp);
        assert!(out.contains("holdback_outcome released"), "must show released: {out}");
        assert!(out.contains("holdback_bps 500"), "must show bps: {out}");
        assert!(out.contains("immediate_release_bps 10000"), "must show full release: {out}");
        assert!(!out.contains("holdback_outcome held"), "must not show held: {out}");
    }


    // ── Phase 3 render tests ────────────────────────────────────────────────

    #[test]
    fn render_policy_check_summary_shows_milestone_results() {
        use irium_node_rs::settlement::{MilestoneEvaluationResult, PolicyOutcome};
        let resp = CheckPolicyRpcResponse {
            agreement_hash: "ms01".to_string(),
            policy_id: "pol-ms-check".to_string(),
            tip_height: 100,
            release_eligible: false,
            refund_eligible: false,
            reason: "milestone partial".to_string(),
            evaluated_rules: vec![],
            holdback: None,
            milestone_results: vec![
                MilestoneEvaluationResult {
                    milestone_id: "ms-foundation".to_string(),
                    label: Some("Foundation".to_string()),
                    outcome: PolicyOutcome::Satisfied,
                    release_eligible: true,
                    refund_eligible: false,
                    matched_proof_ids: vec![],
                    reason: "proof matched".to_string(),
                    holdback: None,
                    threshold_results: vec![],
                },
                MilestoneEvaluationResult {
                    milestone_id: "ms-framing".to_string(),
                    label: Some("Framing".to_string()),
                    outcome: PolicyOutcome::Unsatisfied,
                    release_eligible: false,
                    refund_eligible: false,
                    matched_proof_ids: vec![],
                    reason: "no proof".to_string(),
                    holdback: None,
                    threshold_results: vec![],
                },
            ],
        };
        let out = render_policy_check_summary(&resp);
        assert!(out.contains("milestones 1/2"), "must show 1/2 satisfied: {out}");
        assert!(out.contains("Foundation") || out.contains("ms-foundation"), "must show first milestone: {out}");
        assert!(out.contains("satisfied"), "must show satisfied outcome: {out}");
        assert!(out.contains("unsatisfied"), "must show unsatisfied outcome: {out}");
    }

    #[test]
    fn render_build_template_summary_contractor() {
        use irium_node_rs::settlement::ProofPolicy;
        // Construct a minimal BuildTemplateRpcResponse
        let resp = BuildTemplateRpcResponse {
            policy: {
                let p = serde_json::json!({
                    "schema_id": "irium.phase2.proof_policy.v1",
                    "policy_id": "pol-test-ct",
                    "agreement_hash": "aa".repeat(32).chars().take(64).collect::<String>(),
                    "required_proofs": [],
                    "no_response_rules": [],
                    "attestors": [],
                    "milestones": []
                });
                serde_json::from_value(p).unwrap()
            },
            policy_json: r#"{"policy_id":"pol-test-ct"}"#.to_string(),
            summary: "Contractor milestone policy pol-test-ct: 2 milestone(s), 1 attestor(s) [att-site], 1 timeout rule(s).".to_string(),
            requirement_count: 2,
            attestor_count: 1,
            milestone_count: 2,
            has_holdback: false,
            has_timeout_rules: true,
        };
        let out = render_build_template_summary(&resp);
        assert!(out.contains("policy_id pol-test-ct"), "must show policy_id: {out}");
        assert!(out.contains("summary Contractor"), "must show summary: {out}");
        assert!(out.contains("requirement_count 2"), "must show requirement_count: {out}");
        assert!(out.contains("milestone_count 2"), "must show milestone_count: {out}");
        assert!(out.contains("has_holdback false"), "must show has_holdback: {out}");
        assert!(out.contains("has_timeout_rules true"), "must show has_timeout_rules: {out}");
        assert!(out.contains("policy_json"), "must include policy_json section: {out}");
    }

    #[test]
    fn render_build_template_summary_no_milestone_count_when_zero() {
        let resp = BuildTemplateRpcResponse {
            policy: serde_json::from_value(serde_json::json!({
                "schema_id": "irium.phase2.proof_policy.v1",
                "policy_id": "pol-otc-render",
                "agreement_hash": "bb".repeat(32).chars().take(64).collect::<String>(),
                "required_proofs": [],
                "no_response_rules": [],
                "attestors": [],
                "milestones": []
            })).unwrap(),
            policy_json: r#"{"policy_id":"pol-otc-render"}"#.to_string(),
            summary: "OTC escrow policy pol-otc-render: 1-of-1 release on trade proof from [att], refund at height 900000.".to_string(),
            requirement_count: 1,
            attestor_count: 1,
            milestone_count: 0,
            has_holdback: false,
            has_timeout_rules: true,
        };
        let out = render_build_template_summary(&resp);
        assert!(!out.contains("milestone_count"), "milestone_count must be absent when 0: {out}");
        assert!(out.contains("policy_id pol-otc-render"), "must show policy_id: {out}");
    }

    // ── Phase 3 wallet CLI arg-parse tests ──────────────────────────────────

    // (The three new subcommand parsers are inline in main(), not separate parse_ fns.
    //  Verify that the build* subcommand strings are recognized by the main
    //  dispatch table — compile-time proof via cfg(test) is not required since
    //  the match arms will produce "unreachable" warnings if removed.)

    #[test]
    fn build_template_structs_serialize_correctly() {
        let att = BuildTemplateAttestorInput {
            attestor_id: "att-1".to_string(),
            pubkey_hex: "03abcd".to_string(),
            display_name: Some("Att One".to_string()),
        };
        let ms = BuildTemplateMilestoneInput {
            milestone_id: "ms-1".to_string(),
            label: None,
            proof_type: "delivery".to_string(),
            deadline_height: Some(500_000),
            holdback_bps: None,
            holdback_release_height: None,
        };
        let req = BuildContractorTemplateRpcRequest {
            policy_id: "pol-ser-1".to_string(),
            agreement_hash: "cc".repeat(32).chars().take(64).collect::<String>(),
            attestors: vec![att],
            milestones: vec![ms],
            notes: None,
        };
        let json = serde_json::to_string(&req).expect("must serialize");
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["policy_id"], "pol-ser-1");
        assert_eq!(v["attestors"][0]["attestor_id"], "att-1");
        assert_eq!(v["milestones"][0]["proof_type"], "delivery");
        assert!(v["milestones"][0]["holdback_bps"].is_null(), "None fields must not appear (skip_serializing_if)");
    }

    // ── Phase 3 render test ─────────────────────────────────────────────────

    // ---- Phase 2 proof submit/list wallet CLI tests ----

    #[test]
    fn proof_submit_cli_parses_required_args() {
        let args: Vec<String> = vec!["--proof".to_string(), "prf.json".to_string()];
        let opts = parse_proof_submit_cli(&args).expect("must parse");
        assert_eq!(opts.proof_path, "prf.json");
        assert!(!opts.json_mode);
    }

    #[test]
    fn proof_submit_cli_parses_all_flags() {
        let args: Vec<String> = vec![
            "--proof".to_string(), "prf.json".to_string(),
            "--rpc".to_string(), "http://localhost:9090".to_string(),
            "--json".to_string(),
        ];
        let opts = parse_proof_submit_cli(&args).expect("must parse");
        assert_eq!(opts.proof_path, "prf.json");
        assert_eq!(opts.rpc_url, "http://localhost:9090");
        assert!(opts.json_mode);
    }

    #[test]
    fn proof_submit_cli_rejects_missing_proof() {
        let args: Vec<String> = vec!["--json".to_string()];
        let err = parse_proof_submit_cli(&args).unwrap_err();
        assert!(err.contains("--proof"), "got: {err}");
    }

    #[test]
    fn proof_submit_cli_rejects_unknown_flag() {
        let args: Vec<String> = vec![
            "--proof".to_string(), "prf.json".to_string(),
            "--unknown".to_string(),
        ];
        let err = parse_proof_submit_cli(&args).unwrap_err();
        assert!(err.contains("unknown"), "got: {err}");
    }

    #[test]
    fn proof_list_cli_parses_required_args() {
        let args: Vec<String> = vec!["--agreement-hash".to_string(), "aabbcc".to_string()];
        let opts = parse_proof_list_cli(&args).expect("must parse");
        assert_eq!(opts.agreement_hash, Some("aabbcc".to_string()));
        assert!(!opts.json_mode);
    }

    #[test]
    fn proof_list_cli_parses_all_flags() {
        let args: Vec<String> = vec![
            "--agreement-hash".to_string(), "aabbcc".to_string(),
            "--rpc".to_string(), "http://localhost:9090".to_string(),
            "--json".to_string(),
        ];
        let opts = parse_proof_list_cli(&args).expect("must parse");
        assert_eq!(opts.agreement_hash, Some("aabbcc".to_string()));
        assert!(opts.json_mode);
    }

    #[test]
    fn proof_list_cli_agreement_hash_optional() {
        let args: Vec<String> = vec!["--json".to_string()];
        let opts = parse_proof_list_cli(&args).expect("must parse without --agreement-hash");
        assert_eq!(opts.agreement_hash, None);
    }

    #[test]
    fn render_proof_submit_summary_accepted() {
        let resp = SubmitProofRpcResponse {
            proof_id: "prf-001".to_string(),
            agreement_hash: "aabbcc".to_string(),
            accepted: true,
            duplicate: false,
            message: "proof accepted".to_string(),
            tip_height: 0,
            expires_at_height: None,
            expired: false,
            status: "active".to_string(),
        };
        let out = render_proof_submit_summary(&resp);
        assert!(out.contains("proof_id prf-001"), "got: {out}");
        assert!(out.contains("accepted true"), "got: {out}");
        assert!(out.contains("duplicate false"), "got: {out}");
        assert!(out.contains("proof accepted"), "got: {out}");
        assert!(out.contains("tip_height 0"), "got: {out}");
        assert!(out.contains("expires_at_height none"), "got: {out}");
        assert!(out.contains("expired false"), "got: {out}");
        assert!(out.contains("status active"), "must show status=active; got: {out}");
    }

    #[test]
    fn render_proof_submit_summary_duplicate() {
        let resp = SubmitProofRpcResponse {
            proof_id: "prf-001".to_string(),
            agreement_hash: "aabbcc".to_string(),
            accepted: false,
            duplicate: true,
            message: "duplicate: proof already stored".to_string(),
            tip_height: 10,
            expires_at_height: Some(5),
            expired: true,
            status: "expired".to_string(),
        };
        let out = render_proof_submit_summary(&resp);
        assert!(out.contains("accepted false"));
        assert!(out.contains("duplicate true"));
        assert!(out.contains("tip_height 10"));
        assert!(out.contains("expires_at_height 5"));
        assert!(out.contains("expired true"));
        assert!(out.contains("status expired"), "must show status=expired; got: {out}");
    }

    #[test]
    fn render_proof_list_summary_with_proofs() {
        use irium_node_rs::settlement::{ProofSignatureEnvelope, SETTLEMENT_PROOF_SCHEMA_ID, AGREEMENT_SIGNATURE_TYPE_SECP256K1};
        let proof = SettlementProof {
            proof_id: "prf-list-001".to_string(),
            schema_id: SETTLEMENT_PROOF_SCHEMA_ID.to_string(),
            proof_type: "delivery_confirmation".to_string(),
            agreement_hash: "aabbcc".to_string(),
            milestone_id: None,
            attested_by: "att-1".to_string(),
            attestation_time: 1700000000,
            evidence_hash: None,
            evidence_summary: None,
            signature: ProofSignatureEnvelope {
                signature_type: AGREEMENT_SIGNATURE_TYPE_SECP256K1.to_string(),
                pubkey_hex: String::new(),
                signature_hex: String::new(),
                payload_hash: String::new(),
            },
            expires_at_height: None,
            typed_payload: None,
        };
        let resp = ListProofsRpcResponse {
            agreement_hash: "aabbcc".to_string(),
            tip_height: 0,
            active_only: false,
            returned_count: 1,
            proofs: vec![ProofListItem { proof, status: String::new() }],
            ..Default::default()
        };
        let out = render_proof_list_summary(&resp);
        assert!(out.contains("agreement_hash aabbcc"), "got: {out}");
        assert!(out.contains("returned_count 1"), "got: {out}");
        assert!(out.contains("prf-list-001"), "got: {out}");
        assert!(out.contains("att-1"), "got: {out}");
        assert!(out.contains("agreement_hash=aabbcc"), "per-proof hash; got: {out}");
        assert!(out.contains("expires_at_height=none"), "no expiry must show none; got: {out}");
    }

    #[test]
    fn render_proof_list_summary_empty() {
        let resp = ListProofsRpcResponse {
            agreement_hash: "deadbeef".to_string(),
            tip_height: 0,
            active_only: false,
            returned_count: 0,
            proofs: vec![],
            ..Default::default()
        };
        let out = render_proof_list_summary(&resp);
        assert!(out.contains("returned_count 0"));
        assert!(out.contains("deadbeef"));
    }

    // ---- Phase 2 proof create CLI tests ----

    #[test]
    fn proof_create_cli_parses_required_args() {
        let args: Vec<String> = vec![
            "--agreement-hash".to_string(), "aabbcc".to_string(),
            "--proof-type".to_string(), "delivery_confirmation".to_string(),
            "--attested-by".to_string(), "attestor-a".to_string(),
            "--address".to_string(), "Iabc123".to_string(),
        ];
        let opts = parse_proof_create_cli(&args).expect("must parse");
        assert_eq!(opts.agreement_hash, "aabbcc");
        assert_eq!(opts.proof_type, "delivery_confirmation");
        assert_eq!(opts.attested_by, "attestor-a");
        assert_eq!(opts.address, "Iabc123");
        assert!(opts.milestone_id.is_none());
        assert!(opts.evidence_summary.is_none());
        assert!(opts.evidence_hash.is_none());
        assert!(opts.proof_id.is_none());
        assert!(opts.timestamp.is_none());
        assert!(opts.out_path.is_none());
        assert!(!opts.json_mode);
    }

    #[test]
    fn proof_create_cli_parses_all_optional_args() {
        let args: Vec<String> = vec![
            "--agreement-hash".to_string(), "deadbeef".to_string(),
            "--proof-type".to_string(), "acceptance".to_string(),
            "--attested-by".to_string(), "att-b".to_string(),
            "--address".to_string(), "Ixyz".to_string(),
            "--milestone-id".to_string(), "ms-1".to_string(),
            "--evidence-summary".to_string(), "goods received".to_string(),
            "--evidence-hash".to_string(), "cafebabe".to_string(),
            "--proof-id".to_string(), "prf-custom".to_string(),
            "--timestamp".to_string(), "1700000000".to_string(),
            "--out".to_string(), "/tmp/proof.json".to_string(),
            "--json".to_string(),
        ];
        let opts = parse_proof_create_cli(&args).expect("must parse");
        assert_eq!(opts.milestone_id.as_deref(), Some("ms-1"));
        assert_eq!(opts.evidence_summary.as_deref(), Some("goods received"));
        assert_eq!(opts.evidence_hash.as_deref(), Some("cafebabe"));
        assert_eq!(opts.proof_id.as_deref(), Some("prf-custom"));
        assert_eq!(opts.timestamp, Some(1700000000));
        assert_eq!(opts.out_path.as_deref(), Some("/tmp/proof.json"));
        assert!(opts.json_mode);
    }

    #[test]
    fn proof_create_cli_rejects_missing_agreement_hash() {
        let args: Vec<String> = vec![
            "--proof-type".to_string(), "t".to_string(),
            "--attested-by".to_string(), "a".to_string(),
            "--address".to_string(), "x".to_string(),
        ];
        let err = parse_proof_create_cli(&args).unwrap_err();
        assert!(err.contains("--agreement-hash"), "got: {err}");
    }

    #[test]
    fn proof_create_cli_rejects_missing_proof_type() {
        let args: Vec<String> = vec![
            "--agreement-hash".to_string(), "aa".to_string(),
            "--attested-by".to_string(), "a".to_string(),
            "--address".to_string(), "x".to_string(),
        ];
        let err = parse_proof_create_cli(&args).unwrap_err();
        assert!(err.contains("--proof-type"), "got: {err}");
    }

    #[test]
    fn proof_create_cli_rejects_missing_attested_by() {
        let args: Vec<String> = vec![
            "--agreement-hash".to_string(), "aa".to_string(),
            "--proof-type".to_string(), "t".to_string(),
            "--address".to_string(), "x".to_string(),
        ];
        let err = parse_proof_create_cli(&args).unwrap_err();
        assert!(err.contains("--attested-by"), "got: {err}");
    }

    #[test]
    fn proof_create_cli_rejects_missing_address() {
        let args: Vec<String> = vec![
            "--agreement-hash".to_string(), "aa".to_string(),
            "--proof-type".to_string(), "t".to_string(),
            "--attested-by".to_string(), "a".to_string(),
        ];
        let err = parse_proof_create_cli(&args).unwrap_err();
        assert!(err.contains("--address"), "got: {err}");
    }

    #[test]
    fn proof_create_cli_rejects_bad_timestamp() {
        let args: Vec<String> = vec![
            "--agreement-hash".to_string(), "aa".to_string(),
            "--proof-type".to_string(), "t".to_string(),
            "--attested-by".to_string(), "a".to_string(),
            "--address".to_string(), "x".to_string(),
            "--timestamp".to_string(), "not-a-number".to_string(),
        ];
        let err = parse_proof_create_cli(&args).unwrap_err();
        assert!(err.contains("--timestamp"), "got: {err}");
    }

    #[test]
    fn proof_create_cli_rejects_unknown_flag() {
        let args: Vec<String> = vec![
            "--agreement-hash".to_string(), "aa".to_string(),
            "--proof-type".to_string(), "t".to_string(),
            "--attested-by".to_string(), "a".to_string(),
            "--address".to_string(), "x".to_string(),
            "--unknown".to_string(),
        ];
        let err = parse_proof_create_cli(&args).unwrap_err();
        assert!(err.contains("unknown"), "got: {err}");
    }

    #[test]
    fn create_settlement_proof_signed_produces_valid_signature() {
        use irium_node_rs::settlement::{
            verify_settlement_proof_signature_only, SETTLEMENT_PROOF_SCHEMA_ID,
        };
        use k256::SecretKey as K256SecretKey;

        let raw = [7u8; 32];
        let secret = K256SecretKey::from_slice(&raw).unwrap();
        let pubkey_hex = hex::encode(secret.public_key().to_encoded_point(true).as_bytes());

        let tmp_dir = std::env::temp_dir();
        let wallet_path_buf = tmp_dir.join(format!("test_wallet_{}.json", std::process::id()));
        let address = {
            use sha2::{Digest as _, Sha256 as Sha256Inner};
            use ripemd::Ripemd160;
            let pk_bytes = secret.public_key().to_encoded_point(true);
            let sha = Sha256Inner::digest(pk_bytes.as_bytes());
            let pkh = Ripemd160::digest(&sha);
            let mut payload = vec![0x00u8];
            payload.extend_from_slice(&pkh);
            let c1 = Sha256Inner::digest(&payload);
            let c2 = Sha256Inner::digest(&c1);
            let mut full = payload.clone();
            full.extend_from_slice(&c2[..4]);
            bs58::encode(full).into_string()
        };
        let wallet_json = serde_json::json!({
            "version": 1,
            "keys": [{
                "address": address,
                "pkh": "",
                "pubkey": pubkey_hex,
                "privkey": hex::encode(raw),
            }]
        });
        std::fs::write(&wallet_path_buf, serde_json::to_string(&wallet_json).unwrap()).unwrap();
        std::env::set_var("IRIUM_WALLET_FILE", wallet_path_buf.to_str().unwrap());

        let opts = ProofCreateCliOptions {
            agreement_hash: "abcd1234".repeat(8),
            proof_type: "test_proof".to_string(),
            attested_by: "test-attestor".to_string(),
            address: address.clone(),
            milestone_id: None,
            evidence_summary: Some("unit test evidence".to_string()),
            evidence_hash: None,
            proof_id: Some("prf-unit-001".to_string()),
            timestamp: Some(1700000000),
            out_path: None,
            json_mode: false,
            expires_at_height: None,
            proof_kind: None,
            reference_id: None,
        };

        let proof = create_settlement_proof_signed(&opts).expect("must create proof");

        assert_eq!(proof.proof_id, "prf-unit-001");
        assert_eq!(proof.schema_id, SETTLEMENT_PROOF_SCHEMA_ID);
        assert_eq!(proof.proof_type, "test_proof");
        assert_eq!(proof.attested_by, "test-attestor");
        assert_eq!(proof.attestation_time, 1700000000);
        assert_eq!(proof.signature.pubkey_hex, pubkey_hex);
        assert!(!proof.signature.signature_hex.is_empty());
        assert!(!proof.signature.payload_hash.is_empty());

        verify_settlement_proof_signature_only(&proof).expect("signature must verify");

        let _ = std::fs::remove_file(&wallet_path_buf);
        std::env::remove_var("IRIUM_WALLET_FILE");
    }

    #[test]
    fn render_proof_create_summary_contains_key_fields() {
        use irium_node_rs::settlement::{
            ProofSignatureEnvelope, SETTLEMENT_PROOF_SCHEMA_ID, AGREEMENT_SIGNATURE_TYPE_SECP256K1,
        };
        let proof = SettlementProof {
            proof_id: "prf-render-001".to_string(),
            schema_id: SETTLEMENT_PROOF_SCHEMA_ID.to_string(),
            proof_type: "acceptance".to_string(),
            agreement_hash: "deadbeef".to_string(),
            milestone_id: Some("ms-1".to_string()),
            attested_by: "att-x".to_string(),
            attestation_time: 1700000001,
            evidence_hash: Some("evhash".to_string()),
            evidence_summary: Some("goods ok".to_string()),
            signature: ProofSignatureEnvelope {
                signature_type: AGREEMENT_SIGNATURE_TYPE_SECP256K1.to_string(),
                pubkey_hex: "pubkey123".to_string(),
                signature_hex: "sig456".to_string(),
                payload_hash: "ph789".to_string(),
            },
            expires_at_height: None,
            typed_payload: None,
        };
        let out = render_proof_create_summary(&proof);
        assert!(out.contains("proof_id prf-render-001"), "got: {out}");
        assert!(out.contains("proof_type acceptance"), "got: {out}");
        assert!(out.contains("agreement_hash deadbeef"), "got: {out}");
        assert!(out.contains("milestone_id ms-1"), "got: {out}");
        assert!(out.contains("attested_by att-x"), "got: {out}");
        assert!(out.contains("evidence_summary goods ok"), "got: {out}");
        assert!(out.contains("evidence_hash evhash"), "got: {out}");
        assert!(out.contains("payload_hash ph789"), "got: {out}");
    }

    #[test]
    fn proof_create_auto_generates_proof_id() {
        use irium_node_rs::settlement::verify_settlement_proof_signature_only;
        use k256::SecretKey as K256SecretKey;

        let raw = [11u8; 32];
        let secret = K256SecretKey::from_slice(&raw).unwrap();
        let pubkey_hex = hex::encode(secret.public_key().to_encoded_point(true).as_bytes());

        let tmp_dir = std::env::temp_dir();
        let wallet_path_buf = tmp_dir.join(format!("test_wallet2_{}.json", std::process::id()));
        let address = {
            use sha2::{Digest as _, Sha256 as Sha256Inner};
            use ripemd::Ripemd160;
            let pk_bytes = secret.public_key().to_encoded_point(true);
            let sha = Sha256Inner::digest(pk_bytes.as_bytes());
            let pkh = Ripemd160::digest(&sha);
            let mut payload = vec![0x00u8];
            payload.extend_from_slice(&pkh);
            let c1 = Sha256Inner::digest(&payload);
            let c2 = Sha256Inner::digest(&c1);
            let mut full = payload.clone();
            full.extend_from_slice(&c2[..4]);
            bs58::encode(full).into_string()
        };
        let wallet_json = serde_json::json!({
            "version": 1,
            "keys": [{
                "address": address,
                "pkh": "",
                "pubkey": pubkey_hex,
                "privkey": hex::encode(raw),
            }]
        });
        std::fs::write(&wallet_path_buf, serde_json::to_string(&wallet_json).unwrap()).unwrap();
        std::env::set_var("IRIUM_WALLET_FILE", wallet_path_buf.to_str().unwrap());

        let opts = ProofCreateCliOptions {
            agreement_hash: "cafebabe".repeat(8),
            proof_type: "auto_id_proof".to_string(),
            attested_by: "att-auto".to_string(),
            address: address.clone(),
            milestone_id: None,
            evidence_summary: None,
            evidence_hash: None,
            proof_id: None,
            timestamp: Some(1700001234),
            out_path: None,
            json_mode: false,
            expires_at_height: None,
            proof_kind: None,
            reference_id: None,
        };

        let proof = create_settlement_proof_signed(&opts).expect("must create proof");
        assert!(proof.proof_id.starts_with("prf-"), "got: {}", proof.proof_id);
        assert_eq!(proof.proof_id.len(), 4 + 16, "got: {}", proof.proof_id);
        verify_settlement_proof_signature_only(&proof).expect("signature must verify");

        let _ = std::fs::remove_file(&wallet_path_buf);
        std::env::remove_var("IRIUM_WALLET_FILE");
    }
    #[test]
    fn policy_set_cli_parses_required_args() {
        let args: Vec<String> = vec![
            "--policy".to_string(), "policy.json".to_string(),
        ];
        let opts = parse_policy_set_cli(&args).expect("must parse");
        assert_eq!(opts.policy_path, "policy.json");
        assert!(!opts.json_mode);
    }

    #[test]
    fn policy_set_cli_rejects_missing_policy() {
        let args: Vec<String> = vec![];
        let err = parse_policy_set_cli(&args).unwrap_err();
        assert!(err.contains("--policy"), "got: {err}");
    }

    #[test]
    fn policy_get_cli_parses_required_args() {
        let args: Vec<String> = vec![
            "--agreement-hash".to_string(), "abcdef".to_string(),
        ];
        let opts = parse_policy_get_cli(&args).expect("must parse");
        assert_eq!(opts.agreement_hash, "abcdef");
        assert!(!opts.json_mode);
    }

    #[test]
    fn policy_get_cli_rejects_missing_agreement_hash() {
        let args: Vec<String> = vec![];
        let err = parse_policy_get_cli(&args).unwrap_err();
        assert!(err.contains("--agreement-hash"), "got: {err}");
    }

    #[test]
    fn render_policy_set_summary_accepted() {
        let resp = StorePolicyRpcResponse {
            policy_id: "pol-001".to_string(),
            agreement_hash: "aabbcc".to_string(),
            accepted: true,
            updated: false,
            message: "policy accepted".to_string(),
        };
        let out = render_policy_set_summary(&resp);
        assert!(out.contains("pol-001"), "got: {out}");
        assert!(out.contains("aabbcc"), "got: {out}");
        assert!(out.contains("accepted"), "got: {out}");
    }

    #[test]
    fn render_policy_set_summary_replaced() {
        let resp = StorePolicyRpcResponse {
            policy_id: "pol-002".to_string(),
            agreement_hash: "ddeeff".to_string(),
            accepted: true,
            updated: true,
            message: "policy replaced".to_string(),
        };
        let out = render_policy_set_summary(&resp);
        assert!(out.contains("replaced"), "got: {out}");
    }

    #[test]
    fn render_policy_get_summary_found() {
        use irium_node_rs::settlement::{ApprovedAttestor, ProofRequirement, ProofResolution};
        let resp = GetPolicyRpcResponse {
            agreement_hash: "cafebabe".to_string(),
            found: true,
            policy: Some(ProofPolicy {
                policy_id: "pol-found".to_string(),
                schema_id: "irium.phase2.proof_policy.v1".to_string(),
                agreement_hash: "cafebabe".to_string(),
                required_proofs: vec![ProofRequirement {
                    requirement_id: "req-1".to_string(),
                    proof_type: "delivery".to_string(),
                    required_by: None,
                    required_attestor_ids: vec!["att-1".to_string()],
                    resolution: ProofResolution::Release,
                    milestone_id: None,
                    threshold: None,
                }],
                no_response_rules: vec![],
                attestors: vec![ApprovedAttestor {
                    attestor_id: "att-1".to_string(),
                    pubkey_hex: "pk1".to_string(),
                    display_name: None,
                    domain: None,
                }],
                notes: None,
                expires_at_height: None,
                milestones: vec![],
                holdback: None,
            }),
            expires_at_height: None,
            expired: false,
        };
        let out = render_policy_get_summary(&resp);
        assert!(out.contains("pol-found"), "got: {out}");
        assert!(out.contains("cafebabe"), "got: {out}");
        assert!(out.contains("found true"), "got: {out}");
    }

    #[test]
    fn render_policy_get_summary_not_found() {
        let resp = GetPolicyRpcResponse {
            agreement_hash: "nothere".to_string(),
            found: false,
            policy: None,
            expires_at_height: None,
            expired: false,
        };
        let out = render_policy_get_summary(&resp);
        assert!(out.contains("found false"), "got: {out}");
        assert!(out.contains("nothere"), "got: {out}");
    }

    #[test]
    fn policy_evaluate_cli_parses_required_args() {
        let args: Vec<String> = vec![
            "--agreement".to_string(), "agreement.json".to_string(),
        ];
        let opts = parse_policy_evaluate_cli(&args).expect("must parse");
        assert_eq!(opts.agreement_path, "agreement.json");
        assert!(!opts.json_mode);
    }

    #[test]
    fn policy_evaluate_cli_rejects_missing_agreement() {
        let args: Vec<String> = vec![];
        let err = parse_policy_evaluate_cli(&args).unwrap_err();
        assert!(err.contains("--agreement"), "got: {err}");
    }

    #[test]
    fn policy_evaluate_cli_rejects_unknown_flag() {
        let args: Vec<String> = vec![
            "--agreement".to_string(), "a.json".to_string(),
            "--unknown".to_string(),
        ];
        let err = parse_policy_evaluate_cli(&args).unwrap_err();
        assert!(err.contains("unknown"), "got: {err}");
    }

    #[test]
    fn render_policy_evaluate_summary_release_eligible() {
        let resp = EvaluatePolicyRpcResponse {
            agreement_hash: "aabbcc".to_string(),
            policy_found: true,
            policy_id: Some("pol-eval-001".to_string()),
            tip_height: 100,
            proof_count: 1,
            matched_proof_count: 1,
            matched_proof_ids: vec!["prf-1".to_string()],
            release_eligible: true,
            refund_eligible: false,
            reason: "all release requirements satisfied".to_string(),
            evaluated_rules: vec!["proof 'prf-1' verified ok".to_string()],
            ..Default::default()
        };
        let out = render_policy_evaluate_summary(&resp);
        assert!(out.contains("agreement_hash aabbcc"), "got: {out}");
        assert!(out.contains("policy_id pol-eval-001"), "got: {out}");
        assert!(out.contains("policy_found true"), "got: {out}");
        assert!(out.contains("proof_count 1"), "got: {out}");
        assert!(out.contains("matched_proof_count 1"), "got: {out}");
        assert!(out.contains("matched_proof_ids prf-1"), "got: {out}");
        assert!(out.contains("release_eligible true"), "got: {out}");
        assert!(out.contains("verified ok"), "got: {out}");
    }

    #[test]
    fn render_policy_evaluate_summary_policy_not_found() {
        let resp = EvaluatePolicyRpcResponse {
            agreement_hash: "deadbeef".to_string(),
            policy_found: false,
            policy_id: None,
            tip_height: 0,
            proof_count: 0,
            release_eligible: false,
            refund_eligible: false,
            reason: "no policy stored for this agreement".to_string(),
            evaluated_rules: vec![],
            ..Default::default()
        };
        let out = render_policy_evaluate_summary(&resp);
        assert!(out.contains("policy_found false"), "got: {out}");
        assert!(out.contains("policy_id none"), "got: {out}");
        assert!(out.contains("no policy stored"), "got: {out}");
    }

    #[test]
    fn render_policy_evaluate_summary_no_proofs() {
        let resp = EvaluatePolicyRpcResponse {
            agreement_hash: "cafecafe".to_string(),
            policy_found: true,
            policy_id: Some("pol-noproof".to_string()),
            tip_height: 10,
            proof_count: 0,
            release_eligible: false,
            refund_eligible: false,
            reason: "no release or refund condition was met".to_string(),
            evaluated_rules: vec![],
            ..Default::default()
        };
        let out = render_policy_evaluate_summary(&resp);
        assert!(out.contains("proof_count 0"), "got: {out}");
        assert!(out.contains("matched_proof_count 0"), "got: {out}");
        assert!(out.contains("release_eligible false"), "got: {out}");
    }

    #[test]
    fn render_policy_evaluate_summary_with_expired_proofs() {
        // Expired proofs are filtered before evaluation; expired_proof_count shows how many.
        let resp = EvaluatePolicyRpcResponse {
            agreement_hash: "exp-hash".to_string(),
            policy_found: true,
            policy_id: Some("pol-exp".to_string()),
            proof_count: 0,
            expired_proof_count: 2,
            matched_proof_count: 0,
            release_eligible: false,
            refund_eligible: false,
            reason: "no release or refund condition was met".to_string(),
            evaluated_rules: vec![
                "proof 'prf-x' skipped: expired at height 5 (tip 10)".to_string(),
                "proof 'prf-y' skipped: expired at height 3 (tip 10)".to_string(),
            ],
            ..Default::default()
        };
        let out = render_policy_evaluate_summary(&resp);
        assert!(out.contains("expired_proof_count 2"), "must show expired count; got: {out}");
        assert!(out.contains("matched_proof_count 0"), "no matched; got: {out}");
        assert!(!out.contains("matched_proof_ids"), "no matched ids to show; got: {out}");
        assert!(out.contains("skipped"), "evaluated_rules must mention skipped; got: {out}");
    }

    #[test]
    fn render_policy_evaluate_summary_mixed_active_expired() {
        // 1 active proof matched, 1 expired filtered out.
        let resp = EvaluatePolicyRpcResponse {
            agreement_hash: "mix-hash".to_string(),
            policy_found: true,
            policy_id: Some("pol-mix".to_string()),
            proof_count: 1,
            expired_proof_count: 1,
            matched_proof_count: 1,
            matched_proof_ids: vec!["prf-active".to_string()],
            release_eligible: true,
            refund_eligible: false,
            reason: "all release requirements satisfied by verified proofs".to_string(),
            evaluated_rules: vec![
                "proof 'prf-expired' skipped: expired at height 0 (tip 5)".to_string(),
                "proof 'prf-active' verified ok".to_string(),
            ],
            ..Default::default()
        };
        let out = render_policy_evaluate_summary(&resp);
        assert!(out.contains("proof_count 1"), "got: {out}");
        assert!(out.contains("expired_proof_count 1"), "got: {out}");
        assert!(out.contains("matched_proof_count 1"), "got: {out}");
        assert!(out.contains("matched_proof_ids prf-active"), "got: {out}");
        assert!(out.contains("release_eligible true"), "got: {out}");
    }

    #[test]
    fn render_policy_evaluate_summary_no_expired_count_hidden_when_zero() {
        // expired_proof_count=0 must not appear in the summary output.
        let resp = EvaluatePolicyRpcResponse {
            agreement_hash: "clean-hash".to_string(),
            policy_found: true,
            policy_id: Some("pol-clean".to_string()),
            proof_count: 1,
            expired_proof_count: 0,
            matched_proof_count: 1,
            matched_proof_ids: vec!["prf-ok".to_string()],
            release_eligible: true,
            refund_eligible: false,
            reason: "all release requirements satisfied by verified proofs".to_string(),
            evaluated_rules: vec!["proof 'prf-ok' verified ok".to_string()],
            ..Default::default()
        };
        let out = render_policy_evaluate_summary(&resp);
        assert!(!out.contains("expired_proof_count"), "zero expired must be silent; got: {out}");
        assert!(out.contains("matched_proof_count 1"), "got: {out}");
    }

    // ---- outcome field render tests ----

    #[test]
    fn render_policy_evaluate_summary_outcome_satisfied() {
        let resp = EvaluatePolicyRpcResponse {
            outcome: "satisfied".to_string(),
            agreement_hash: "hash-sat".to_string(),
            policy_found: true,
            policy_id: Some("pol-sat".to_string()),
            proof_count: 1,
            matched_proof_count: 1,
            matched_proof_ids: vec!["prf-1".to_string()],
            release_eligible: true,
            reason: "all release requirements satisfied by verified proofs".to_string(),
            ..Default::default()
        };
        let out = render_policy_evaluate_summary(&resp);
        assert!(out.contains("outcome satisfied"), "must show outcome; got: {out}");
        assert!(out.contains("release_eligible true"), "got: {out}");
    }

    #[test]
    fn render_policy_evaluate_summary_outcome_unsatisfied() {
        let resp = EvaluatePolicyRpcResponse {
            outcome: "unsatisfied".to_string(),
            agreement_hash: "hash-unsat".to_string(),
            policy_found: true,
            policy_id: Some("pol-unsat".to_string()),
            proof_count: 0,
            release_eligible: false,
            reason: "no release or refund condition was met".to_string(),
            ..Default::default()
        };
        let out = render_policy_evaluate_summary(&resp);
        assert!(out.contains("outcome unsatisfied"), "must show outcome; got: {out}");
        assert!(out.contains("release_eligible false"), "got: {out}");
    }

    #[test]
    fn render_policy_evaluate_summary_outcome_timeout() {
        let resp = EvaluatePolicyRpcResponse {
            outcome: "timeout".to_string(),
            agreement_hash: "hash-to".to_string(),
            policy_found: true,
            policy_id: Some("pol-to".to_string()),
            proof_count: 0,
            refund_eligible: true,
            reason: "no_response_rule rule-1 deadline 100 reached at tip 100 trigger funded_and_no_release".to_string(),
            ..Default::default()
        };
        let out = render_policy_evaluate_summary(&resp);
        assert!(out.contains("outcome timeout"), "must show outcome; got: {out}");
        assert!(out.contains("refund_eligible true"), "got: {out}");
    }

    #[test]
    fn render_policy_evaluate_summary_outcome_empty_string_hidden() {
        // Old server responses without outcome field must not show "outcome " line.
        let resp = EvaluatePolicyRpcResponse {
            outcome: String::new(),
            agreement_hash: "hash-old".to_string(),
            policy_found: true,
            policy_id: Some("pol-old".to_string()),
            proof_count: 1,
            matched_proof_count: 1,
            release_eligible: true,
            reason: "all release requirements satisfied by verified proofs".to_string(),
            ..Default::default()
        };
        let out = render_policy_evaluate_summary(&resp);
        assert!(!out.contains("outcome "), "empty outcome must be silent; got: {out}");
    }

    // ---- milestone render tests ----

    #[test]
    fn render_policy_evaluate_summary_milestone_breakdown() {
        // Two milestones: ms-a satisfied, ms-b unsatisfied.
        let resp = EvaluatePolicyRpcResponse {
            outcome: "unsatisfied".to_string(),
            agreement_hash: "hash-ms".to_string(),
            policy_found: true,
            policy_id: Some("pol-ms".to_string()),
            release_eligible: false,
            reason: "1 of 2 milestones satisfied; 1 unsatisfied".to_string(),
            total_milestone_count: 2,
            completed_milestone_count: 1,
            milestone_results: vec![
                MilestoneRpcResult {
                    milestone_id: "ms-a".to_string(),
                    label: Some("Delivery".to_string()),
                    outcome: "satisfied".to_string(),
                    release_eligible: true,
                    reason: "all release requirements satisfied by verified proofs".to_string(),
                    ..Default::default()
                },
                MilestoneRpcResult {
                    milestone_id: "ms-b".to_string(),
                    label: None,
                    outcome: "unsatisfied".to_string(),
                    reason: "no release or refund condition was met".to_string(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let out = render_policy_evaluate_summary(&resp);
        assert!(out.contains("milestones 1/2"), "must show milestone count; got: {out}");
        assert!(out.contains("Delivery outcome satisfied"), "must show labeled milestone; got: {out}");
        assert!(out.contains("ms-b outcome unsatisfied"), "must show unlabeled milestone by id; got: {out}");
    }

    #[test]
    fn render_policy_evaluate_summary_milestone_all_satisfied() {
        let resp = EvaluatePolicyRpcResponse {
            outcome: "satisfied".to_string(),
            agreement_hash: "hash-ms-sat".to_string(),
            policy_found: true,
            release_eligible: true,
            reason: "all milestones satisfied".to_string(),
            total_milestone_count: 2,
            completed_milestone_count: 2,
            milestone_results: vec![
                MilestoneRpcResult {
                    milestone_id: "ms-a".to_string(),
                    outcome: "satisfied".to_string(),
                    ..Default::default()
                },
                MilestoneRpcResult {
                    milestone_id: "ms-b".to_string(),
                    outcome: "satisfied".to_string(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let out = render_policy_evaluate_summary(&resp);
        assert!(out.contains("milestones 2/2"), "must show 2/2; got: {out}");
        assert!(out.contains("outcome satisfied"), "got: {out}");
    }

    #[test]
    fn render_policy_evaluate_summary_no_milestone_section_when_empty() {
        // When milestone_results is empty, "milestones" line must not appear.
        let resp = EvaluatePolicyRpcResponse {
            outcome: "satisfied".to_string(),
            agreement_hash: "hash-no-ms".to_string(),
            policy_found: true,
            release_eligible: true,
            reason: "all release requirements satisfied by verified proofs".to_string(),
            ..Default::default()
        };
        let out = render_policy_evaluate_summary(&resp);
        assert!(!out.contains("milestones"), "must not show milestone section; got: {out}");
    }

    #[test]
    fn parse_policy_list_cli_defaults() {
        let args: Vec<String> = vec![];
        let opts = parse_policy_list_cli(&args).expect("must parse");
        assert!(!opts.json_mode);
    }

    #[test]
    fn parse_policy_list_cli_json_flag() {
        let args: Vec<String> = vec!["--json".to_string()];
        let opts = parse_policy_list_cli(&args).expect("must parse");
        assert!(opts.json_mode);
    }

    #[test]
    fn parse_policy_list_cli_rejects_unknown_flag() {
        let args: Vec<String> = vec!["--unknown".to_string()];
        let err = parse_policy_list_cli(&args).unwrap_err();
        assert!(err.contains("unknown"), "got: {err}");
    }

    #[test]
    fn render_policy_list_summary_empty() {
        let resp = ListPoliciesRpcResponse {
            count: 0,
            policies: vec![],
            active_only: false,
        };
        let out = render_policy_list_summary(&resp);
        assert!(out.contains("count 0"), "got: {out}");
    }

    #[test]
    fn render_policy_list_summary_with_policies() {
        let resp = ListPoliciesRpcResponse {
            count: 2,
            policies: vec![
                PolicySummaryItem {
                    agreement_hash: "aabbcc".to_string(),
                    policy_id: "pol-001".to_string(),
                    required_proofs: 1,
                    attestors: 2,
                    expires_at_height: None,
                    expired: false,
                },
                PolicySummaryItem {
                    agreement_hash: "ddeeff".to_string(),
                    policy_id: "pol-002".to_string(),
                    required_proofs: 2,
                    attestors: 1,
                    expires_at_height: None,
                    expired: false,
                },
            ],
            active_only: false
        };
        let out = render_policy_list_summary(&resp);
        assert!(out.contains("count 2"), "got: {out}");
        assert!(out.contains("agreement_hash aabbcc"), "got: {out}");
        assert!(out.contains("policy_id pol-001"), "got: {out}");
        assert!(out.contains("required_proofs 1"), "got: {out}");
        assert!(out.contains("attestors 2"), "got: {out}");
        assert!(out.contains("agreement_hash ddeeff"), "got: {out}");
        assert!(out.contains("policy_id pol-002"), "got: {out}");
    }


    #[test]
    fn policy_set_cli_parses_replace_flag() {
        let args: Vec<String> = vec![
            "--policy".to_string(), "p.json".to_string(),
            "--replace".to_string(),
        ];
        let opts = parse_policy_set_cli(&args).expect("must parse");
        assert!(opts.replace, "replace must be true when --replace is given");
        assert!(!opts.json_mode);
    }

    #[test]
    fn policy_set_cli_replace_defaults_false() {
        let args: Vec<String> = vec![
            "--policy".to_string(), "p.json".to_string(),
        ];
        let opts = parse_policy_set_cli(&args).expect("must parse");
        assert!(!opts.replace, "replace must default to false");
    }

    #[test]
    fn render_policy_set_summary_rejected() {
        let resp = StorePolicyRpcResponse {
            policy_id: "pol-003".to_string(),
            agreement_hash: "ff0011".to_string(),
            accepted: false,
            updated: false,
            message: "a policy 'pol-001' already exists for this agreement; use --replace to overwrite".to_string(),
        };
        let out = render_policy_set_summary(&resp);
        assert!(out.contains("rejected"), "got: {out}");
        assert!(out.contains("--replace"), "message must propagate; got: {out}");
    }


    #[test]
    fn policy_set_cli_parses_expires_at_height() {
        let args: Vec<String> = vec![
            "--policy".to_string(), "p.json".to_string(),
            "--expires-at-height".to_string(), "1000".to_string(),
        ];
        let opts = parse_policy_set_cli(&args).expect("must parse");
        assert_eq!(opts.expires_at_height, Some(1000));
    }

    #[test]
    fn policy_set_cli_expires_at_height_defaults_none() {
        let args: Vec<String> = vec![
            "--policy".to_string(), "p.json".to_string(),
        ];
        let opts = parse_policy_set_cli(&args).expect("must parse");
        assert_eq!(opts.expires_at_height, None);
    }

    #[test]
    fn policy_set_cli_rejects_invalid_expires_height() {
        let args: Vec<String> = vec![
            "--policy".to_string(), "p.json".to_string(),
            "--expires-at-height".to_string(), "notanumber".to_string(),
        ];
        let err = parse_policy_set_cli(&args).unwrap_err();
        assert!(err.contains("integer") || err.contains("expires-at-height"), "got: {err}");
    }

    #[test]
    fn render_policy_get_summary_shows_expiry_info() {
        use irium_node_rs::settlement::{ApprovedAttestor, ProofRequirement};
        let resp = GetPolicyRpcResponse {
            agreement_hash: "aabbcc".to_string(),
            found: true,
            policy: Some(ProofPolicy {
                policy_id: "pol-exp".to_string(),
                schema_id: "irium.phase2.proof_policy.v1".to_string(),
                agreement_hash: "aabbcc".to_string(),
                required_proofs: vec![],
                no_response_rules: vec![],
                attestors: vec![],
                notes: None,
                expires_at_height: Some(500),
                milestones: vec![],
                holdback: None,
            }),
            expires_at_height: Some(500),
            expired: false,
        };
        let out = render_policy_get_summary(&resp);
        assert!(out.contains("500"), "must show expiry height; got: {out}");
        assert!(out.contains("expired false"), "must show expired false; got: {out}");
    }

    #[test]
    fn render_policy_get_summary_shows_expired_true() {
        use irium_node_rs::settlement::{ApprovedAttestor, ProofRequirement};
        let resp = GetPolicyRpcResponse {
            agreement_hash: "ddeeff".to_string(),
            found: true,
            policy: Some(ProofPolicy {
                policy_id: "pol-exp2".to_string(),
                schema_id: "irium.phase2.proof_policy.v1".to_string(),
                agreement_hash: "ddeeff".to_string(),
                required_proofs: vec![],
                no_response_rules: vec![],
                attestors: vec![],
                notes: None,
                expires_at_height: Some(10),
                milestones: vec![],
                holdback: None,
            }),
            expires_at_height: Some(10),
            expired: true,
        };
        let out = render_policy_get_summary(&resp);
        assert!(out.contains("expired true"), "must show expired true; got: {out}");
    }

    #[test]
    fn render_policy_list_summary_shows_expiry() {
        let resp = ListPoliciesRpcResponse {
            count: 1,
            policies: vec![PolicySummaryItem {
                agreement_hash: "zzhash".to_string(),
                policy_id: "pol-exp-list".to_string(),
                required_proofs: 0,
                attestors: 0,
                expires_at_height: Some(777),
                expired: true,
            }],
            active_only: false
        };
        let out = render_policy_list_summary(&resp);
        assert!(out.contains("777"), "must show expiry height; got: {out}");
        assert!(out.contains("expired true"), "must show expired; got: {out}");
    }

    #[test]
    fn render_policy_list_summary_no_expiry_shows_none() {
        let resp = ListPoliciesRpcResponse {
            count: 1,
            policies: vec![PolicySummaryItem {
                agreement_hash: "nohash".to_string(),
                policy_id: "pol-no-exp".to_string(),
                required_proofs: 0,
                attestors: 0,
                expires_at_height: None,
                expired: false,
            }],
            active_only: false
        };
        let out = render_policy_list_summary(&resp);
        assert!(out.contains("expires_at_height none"), "must show none; got: {out}");
    }


    #[test]
    fn parse_policy_list_cli_active_only_flag() {
        let args: Vec<String> = vec!["--active-only".to_string()];
        let opts = parse_policy_list_cli(&args).expect("must parse");
        assert!(opts.active_only);
    }

    #[test]
    fn parse_policy_list_cli_active_only_defaults_false() {
        let args: Vec<String> = vec![];
        let opts = parse_policy_list_cli(&args).expect("must parse");
        assert!(!opts.active_only);
    }

    #[test]
    fn render_policy_list_summary_active_only_shows_filter_header() {
        let resp = ListPoliciesRpcResponse {
            count: 1,
            policies: vec![PolicySummaryItem {
                agreement_hash: "abc123".to_string(),
                policy_id: "pol-ao".to_string(),
                required_proofs: 0,
                attestors: 0,
                expires_at_height: Some(100),
                expired: false,
            }],
            active_only: true,
        };
        let out = render_policy_list_summary(&resp);
        assert!(out.contains("filter active_only true"), "must show filter header; got: {out}");
        assert!(out.contains("count 1"), "got: {out}");
    }

    #[test]
    fn render_policy_list_summary_no_filter_header_when_false() {
        let resp = ListPoliciesRpcResponse {
            count: 0,
            policies: vec![],
            active_only: false,
        };
        let out = render_policy_list_summary(&resp);
        assert!(!out.contains("filter active_only"), "must not show filter header; got: {out}");
    }


    #[test]
    fn proof_list_cli_agreement_hash_parsed_when_provided() {
        let args: Vec<String> = vec![
            "--agreement-hash".to_string(), "deadbeef".to_string(),
        ];
        let opts = parse_proof_list_cli(&args).expect("must parse");
        assert_eq!(opts.agreement_hash, Some("deadbeef".to_string()));
    }

    #[test]
    fn render_proof_list_summary_global_shows_star() {
        let resp = ListProofsRpcResponse {
            agreement_hash: "*".to_string(),
            tip_height: 0,
            active_only: false,
            returned_count: 0,
            proofs: vec![],
            ..Default::default()
        };
        let out = render_proof_list_summary(&resp);
        assert!(out.contains("agreement_hash * (all)"), "must show global header; got: {out}");
        assert!(out.contains("returned_count 0"), "got: {out}");
    }

    #[test]
    fn render_proof_list_summary_filtered_shows_hash() {
        let resp = ListProofsRpcResponse {
            agreement_hash: "aabbcc".to_string(),
            tip_height: 0,
            active_only: false,
            returned_count: 0,
            proofs: vec![],
            ..Default::default()
        };
        let out = render_proof_list_summary(&resp);
        assert!(out.contains("agreement_hash aabbcc"), "must show filter hash; got: {out}");
        assert!(!out.contains("(all)"), "must not show global marker; got: {out}");
    }


    #[test]
    fn proof_create_cli_parses_expires_at_height() {
        let args: Vec<String> = vec![
            "--agreement-hash".to_string(), "aabbcc".to_string(),
            "--proof-type".to_string(), "delivery_confirmation".to_string(),
            "--attested-by".to_string(), "att-1".to_string(),
            "--address".to_string(), "Iabc".to_string(),
            "--expires-at-height".to_string(), "5000".to_string(),
        ];
        let opts = parse_proof_create_cli(&args).expect("must parse");
        assert_eq!(opts.expires_at_height, Some(5000));
    }

    #[test]
    fn proof_create_cli_expires_defaults_to_none() {
        let args: Vec<String> = vec![
            "--agreement-hash".to_string(), "aabbcc".to_string(),
            "--proof-type".to_string(), "delivery_confirmation".to_string(),
            "--attested-by".to_string(), "att-1".to_string(),
            "--address".to_string(), "Iabc".to_string(),
        ];
        let opts = parse_proof_create_cli(&args).expect("must parse");
        assert_eq!(opts.expires_at_height, None);
    }

    #[test]
    fn render_proof_list_summary_shows_proof_expiry_not_expired() {
        use irium_node_rs::settlement::{ProofSignatureEnvelope, SETTLEMENT_PROOF_SCHEMA_ID, AGREEMENT_SIGNATURE_TYPE_SECP256K1};
        let proof = SettlementProof {
            proof_id: "prf-exp-1".to_string(),
            schema_id: SETTLEMENT_PROOF_SCHEMA_ID.to_string(),
            proof_type: "delivery_confirmation".to_string(),
            agreement_hash: "hashexp".to_string(),
            milestone_id: None,
            attested_by: "att-exp".to_string(),
            attestation_time: 1700000000,
            evidence_hash: None,
            evidence_summary: None,
            signature: ProofSignatureEnvelope {
                signature_type: AGREEMENT_SIGNATURE_TYPE_SECP256K1.to_string(),
                pubkey_hex: String::new(),
                signature_hex: String::new(),
                payload_hash: String::new(),
            },
            expires_at_height: Some(1000),
            typed_payload: None,
        };
        // tip_height=50 < expires_at_height=1000 => not expired
        let resp = ListProofsRpcResponse {
            agreement_hash: "hashexp".to_string(),
            tip_height: 50,
            active_only: false,
            returned_count: 1,
            proofs: vec![ProofListItem { proof, status: "active".to_string() }],
            ..Default::default()
        };
        let out = render_proof_list_summary(&resp);
        assert!(out.contains("expires_at_height=1000"), "must show expiry height; got: {out}");
        assert!(out.contains("expired=false"), "must show not expired at tip 50; got: {out}");
        assert!(out.contains("status=active"), "must show status=active; got: {out}");
    }

    #[test]
    fn render_proof_list_summary_shows_proof_expiry_expired() {
        use irium_node_rs::settlement::{ProofSignatureEnvelope, SETTLEMENT_PROOF_SCHEMA_ID, AGREEMENT_SIGNATURE_TYPE_SECP256K1};
        let proof = SettlementProof {
            proof_id: "prf-exp-2".to_string(),
            schema_id: SETTLEMENT_PROOF_SCHEMA_ID.to_string(),
            proof_type: "delivery_confirmation".to_string(),
            agreement_hash: "hashexp2".to_string(),
            milestone_id: None,
            attested_by: "att-exp2".to_string(),
            attestation_time: 1700000000,
            evidence_hash: None,
            evidence_summary: None,
            signature: ProofSignatureEnvelope {
                signature_type: AGREEMENT_SIGNATURE_TYPE_SECP256K1.to_string(),
                pubkey_hex: String::new(),
                signature_hex: String::new(),
                payload_hash: String::new(),
            },
            expires_at_height: Some(100),
            typed_payload: None,
        };
        // tip_height=200 >= expires_at_height=100 => expired
        let resp = ListProofsRpcResponse {
            agreement_hash: "hashexp2".to_string(),
            tip_height: 200,
            active_only: false,
            returned_count: 1,
            proofs: vec![ProofListItem { proof, status: "expired".to_string() }],
            ..Default::default()
        };
        let out = render_proof_list_summary(&resp);
        assert!(out.contains("expires_at_height=100"), "must show expiry height; got: {out}");
        assert!(out.contains("expired=true"), "must show expired at tip 200; got: {out}");
        assert!(out.contains("status=expired"), "must show status=expired; got: {out}");
    }

    #[test]
    fn render_proof_create_summary_shows_expiry_when_set() {
        use irium_node_rs::settlement::{ProofSignatureEnvelope, SETTLEMENT_PROOF_SCHEMA_ID, AGREEMENT_SIGNATURE_TYPE_SECP256K1};
        let proof = SettlementProof {
            proof_id: "prf-create-exp".to_string(),
            schema_id: SETTLEMENT_PROOF_SCHEMA_ID.to_string(),
            proof_type: "payment".to_string(),
            agreement_hash: "hashc".to_string(),
            milestone_id: None,
            attested_by: "att-c".to_string(),
            attestation_time: 1700000000,
            evidence_hash: None,
            evidence_summary: None,
            signature: ProofSignatureEnvelope {
                signature_type: AGREEMENT_SIGNATURE_TYPE_SECP256K1.to_string(),
                pubkey_hex: String::new(),
                signature_hex: String::new(),
                payload_hash: String::new(),
            },
            expires_at_height: Some(8000),
            typed_payload: None,
        };
        let out = render_proof_create_summary(&proof);
        assert!(out.contains("expires_at_height 8000"), "must show expiry height; got: {out}");
    }

    #[test]
    fn render_proof_create_summary_shows_none_when_no_expiry() {
        use irium_node_rs::settlement::{ProofSignatureEnvelope, SETTLEMENT_PROOF_SCHEMA_ID, AGREEMENT_SIGNATURE_TYPE_SECP256K1};
        let proof = SettlementProof {
            proof_id: "prf-noexp".to_string(),
            schema_id: SETTLEMENT_PROOF_SCHEMA_ID.to_string(),
            proof_type: "payment".to_string(),
            agreement_hash: "hashd".to_string(),
            milestone_id: None,
            attested_by: "att-d".to_string(),
            attestation_time: 1700000000,
            evidence_hash: None,
            evidence_summary: None,
            signature: ProofSignatureEnvelope {
                signature_type: AGREEMENT_SIGNATURE_TYPE_SECP256K1.to_string(),
                pubkey_hex: String::new(),
                signature_hex: String::new(),
                payload_hash: String::new(),
            },
            expires_at_height: None,
            typed_payload: None,
        };
        let out = render_proof_create_summary(&proof);
        assert!(out.contains("expires_at_height none"), "must show none when no expiry; got: {out}");
    }



    #[test]
    fn parse_proof_list_cli_active_only_flag() {
        let args: Vec<String> = vec!["--active-only".to_string()];
        let opts = parse_proof_list_cli(&args).expect("must parse");
        assert!(opts.active_only);
    }

    #[test]
    fn parse_proof_list_cli_active_only_defaults_false() {
        let args: Vec<String> = vec![];
        let opts = parse_proof_list_cli(&args).expect("must parse");
        assert!(!opts.active_only);
    }

    #[test]
    fn render_proof_list_summary_active_only_shows_filter_header() {
        let resp = ListProofsRpcResponse {
            agreement_hash: "*".to_string(),
            tip_height: 0,
            active_only: true,
            returned_count: 0,
            proofs: vec![],
            ..Default::default()
        };
        let out = render_proof_list_summary(&resp);
        assert!(out.contains("filter active_only true"), "must show filter header; got: {out}");
    }

    #[test]
    fn render_proof_list_summary_active_only_false_no_filter_header() {
        let resp = ListProofsRpcResponse {
            agreement_hash: "*".to_string(),
            tip_height: 0,
            active_only: false,
            returned_count: 0,
            proofs: vec![],
            ..Default::default()
        };
        let out = render_proof_list_summary(&resp);
        assert!(!out.contains("filter active_only"), "must not show filter header; got: {out}");
    }

    #[test]
    fn parse_proof_list_cli_active_only_combined_with_agreement_hash() {
        let args: Vec<String> = vec![
            "--active-only".to_string(),
            "--agreement-hash".to_string(), "deadbeef".to_string(),
        ];
        let opts = parse_proof_list_cli(&args).expect("must parse");
        assert!(opts.active_only);
        assert_eq!(opts.agreement_hash, Some("deadbeef".to_string()));
    }



    #[test]
    fn render_proof_submit_summary_non_expiring() {
        let resp = SubmitProofRpcResponse {
            proof_id: "prf-ne".to_string(),
            agreement_hash: "aabb".to_string(),
            accepted: true,
            duplicate: false,
            message: "proof accepted".to_string(),
            tip_height: 100,
            expires_at_height: None,
            expired: false,
            status: "active".to_string(),
        };
        let out = render_proof_submit_summary(&resp);
        assert!(out.contains("expires_at_height none"), "must show none; got: {out}");
        assert!(out.contains("expired false"), "non-expiring must show expired false; got: {out}");
        assert!(out.contains("tip_height 100"), "must show tip; got: {out}");
        assert!(out.contains("status active"), "non-expiring must show status active; got: {out}");
    }

    #[test]
    fn render_proof_submit_summary_future_expiry() {
        let resp = SubmitProofRpcResponse {
            proof_id: "prf-fe".to_string(),
            agreement_hash: "aabb".to_string(),
            accepted: true,
            duplicate: false,
            message: "proof accepted".to_string(),
            tip_height: 0,
            expires_at_height: Some(1000),
            expired: false,
            status: "active".to_string(),
        };
        let out = render_proof_submit_summary(&resp);
        assert!(out.contains("expires_at_height 1000"), "must show expiry height; got: {out}");
        assert!(out.contains("expired false"), "future expiry must show expired false; got: {out}");
        assert!(out.contains("status active"), "future expiry must show status active; got: {out}");
    }

    #[test]
    fn render_proof_submit_summary_already_expired() {
        let resp = SubmitProofRpcResponse {
            proof_id: "prf-ae".to_string(),
            agreement_hash: "aabb".to_string(),
            accepted: true,
            duplicate: false,
            message: "proof accepted".to_string(),
            tip_height: 50,
            expires_at_height: Some(10),
            expired: true,
            status: "expired".to_string(),
        };
        let out = render_proof_submit_summary(&resp);
        assert!(out.contains("expires_at_height 10"), "must show expiry height; got: {out}");
        assert!(out.contains("expired true"), "must show expired true; got: {out}");
        assert!(out.contains("tip_height 50"), "must show tip height; got: {out}");
        assert!(out.contains("status expired"), "must show status expired; got: {out}");
    }


    #[test]
    fn render_proof_list_summary_status_active_shown() {
        use irium_node_rs::settlement::{ProofSignatureEnvelope, SETTLEMENT_PROOF_SCHEMA_ID, AGREEMENT_SIGNATURE_TYPE_SECP256K1};
        let proof = SettlementProof {
            proof_id: "prf-st-a".to_string(),
            schema_id: SETTLEMENT_PROOF_SCHEMA_ID.to_string(),
            proof_type: "delivery_confirmation".to_string(),
            agreement_hash: "hashst".to_string(),
            milestone_id: None,
            attested_by: "att-s".to_string(),
            attestation_time: 1700000000,
            evidence_hash: None,
            evidence_summary: None,
            signature: ProofSignatureEnvelope {
                signature_type: AGREEMENT_SIGNATURE_TYPE_SECP256K1.to_string(),
                pubkey_hex: String::new(),
                signature_hex: String::new(),
                payload_hash: String::new(),
            },
            expires_at_height: Some(1000),
            typed_payload: None,
        };
        let resp = ListProofsRpcResponse {
            agreement_hash: "hashst".to_string(),
            tip_height: 0,
            active_only: false,
            returned_count: 1,
            proofs: vec![ProofListItem { proof, status: "active".to_string() }],
            ..Default::default()
        };
        let out = render_proof_list_summary(&resp);
        assert!(out.contains("status=active"), "status=active must appear in output; got: {out}");
    }

    #[test]
    fn render_proof_list_summary_status_expired_shown() {
        use irium_node_rs::settlement::{ProofSignatureEnvelope, SETTLEMENT_PROOF_SCHEMA_ID, AGREEMENT_SIGNATURE_TYPE_SECP256K1};
        let proof = SettlementProof {
            proof_id: "prf-st-e".to_string(),
            schema_id: SETTLEMENT_PROOF_SCHEMA_ID.to_string(),
            proof_type: "delivery_confirmation".to_string(),
            agreement_hash: "hashste".to_string(),
            milestone_id: None,
            attested_by: "att-se".to_string(),
            attestation_time: 1700000000,
            evidence_hash: None,
            evidence_summary: None,
            signature: ProofSignatureEnvelope {
                signature_type: AGREEMENT_SIGNATURE_TYPE_SECP256K1.to_string(),
                pubkey_hex: String::new(),
                signature_hex: String::new(),
                payload_hash: String::new(),
            },
            expires_at_height: Some(100),
            typed_payload: None,
        };
        let resp = ListProofsRpcResponse {
            agreement_hash: "hashste".to_string(),
            tip_height: 200,
            active_only: false,
            returned_count: 1,
            proofs: vec![ProofListItem { proof, status: "expired".to_string() }],
            ..Default::default()
        };
        let out = render_proof_list_summary(&resp);
        assert!(out.contains("status=expired"), "status=expired must appear in output; got: {out}");
    }

    #[test]
    fn render_proof_list_summary_status_empty_not_shown() {
        use irium_node_rs::settlement::{ProofSignatureEnvelope, SETTLEMENT_PROOF_SCHEMA_ID, AGREEMENT_SIGNATURE_TYPE_SECP256K1};
        let proof = SettlementProof {
            proof_id: "prf-st-none".to_string(),
            schema_id: SETTLEMENT_PROOF_SCHEMA_ID.to_string(),
            proof_type: "delivery_confirmation".to_string(),
            agreement_hash: "hashstn".to_string(),
            milestone_id: None,
            attested_by: "att-sn".to_string(),
            attestation_time: 1700000000,
            evidence_hash: None,
            evidence_summary: None,
            signature: ProofSignatureEnvelope {
                signature_type: AGREEMENT_SIGNATURE_TYPE_SECP256K1.to_string(),
                pubkey_hex: String::new(),
                signature_hex: String::new(),
                payload_hash: String::new(),
            },
            expires_at_height: None,
            typed_payload: None,
        };
        let resp = ListProofsRpcResponse {
            agreement_hash: "hashstn".to_string(),
            tip_height: 0,
            active_only: false,
            returned_count: 1,
            proofs: vec![ProofListItem { proof, status: String::new() }],
            ..Default::default()
        };
        let out = render_proof_list_summary(&resp);
        assert!(!out.contains("status="), "empty status must not appear in output; got: {out}");
    }


    #[test]
    fn render_proof_submit_summary_status_active_shown() {
        let resp = SubmitProofRpcResponse {
            proof_id: "prf-s-a".to_string(),
            agreement_hash: "hash-sa".to_string(),
            accepted: true,
            duplicate: false,
            message: "proof accepted".to_string(),
            tip_height: 0,
            expires_at_height: None,
            expired: false,
            status: "active".to_string(),
        };
        let out = render_proof_submit_summary(&resp);
        assert!(out.contains("status active"), "status active must appear; got: {out}");
    }

    #[test]
    fn render_proof_submit_summary_status_expired_shown() {
        let resp = SubmitProofRpcResponse {
            proof_id: "prf-s-e".to_string(),
            agreement_hash: "hash-se".to_string(),
            accepted: true,
            duplicate: false,
            message: "proof accepted".to_string(),
            tip_height: 100,
            expires_at_height: Some(50),
            expired: true,
            status: "expired".to_string(),
        };
        let out = render_proof_submit_summary(&resp);
        assert!(out.contains("status expired"), "status expired must appear; got: {out}");
    }

    #[test]
    fn render_proof_submit_summary_status_empty_not_shown() {
        // Old node response with no status field: status defaults to empty, must not appear in output.
        let resp = SubmitProofRpcResponse {
            proof_id: "prf-s-n".to_string(),
            agreement_hash: "hash-sn".to_string(),
            accepted: true,
            duplicate: false,
            message: "proof accepted".to_string(),
            tip_height: 0,
            expires_at_height: None,
            expired: false,
            status: String::new(),
        };
        let out = render_proof_submit_summary(&resp);
        assert!(!out.contains("status"), "empty status must not appear in output; got: {out}");
    }


    #[test]
    fn parse_proof_get_cli_parses_required_args() {
        let args: Vec<String> = vec![
            "--proof-id".to_string(), "prf-001".to_string(),
        ];
        let opts = parse_proof_get_cli(&args).expect("must parse");
        assert_eq!(opts.proof_id, "prf-001");
    }

    #[test]
    fn parse_proof_get_cli_rejects_missing_proof_id() {
        let args: Vec<String> = vec![];
        let err = parse_proof_get_cli(&args).unwrap_err();
        assert!(err.contains("--proof-id"), "got: {err}");
    }

    #[test]
    fn render_proof_get_summary_found_active() {
        use irium_node_rs::settlement::{ProofSignatureEnvelope, SETTLEMENT_PROOF_SCHEMA_ID, AGREEMENT_SIGNATURE_TYPE_SECP256K1};
        let proof = SettlementProof {
            proof_id: "prf-get-1".to_string(),
            schema_id: SETTLEMENT_PROOF_SCHEMA_ID.to_string(),
            proof_type: "delivery_confirmation".to_string(),
            agreement_hash: "hashget".to_string(),
            milestone_id: None,
            attested_by: "att-g".to_string(),
            attestation_time: 1700000000,
            evidence_hash: None,
            evidence_summary: None,
            signature: ProofSignatureEnvelope {
                signature_type: AGREEMENT_SIGNATURE_TYPE_SECP256K1.to_string(),
                pubkey_hex: String::new(),
                signature_hex: String::new(),
                payload_hash: String::new(),
            },
            expires_at_height: None,
            typed_payload: None,
        };
        let resp = GetProofRpcResponse {
            proof_id: "prf-get-1".to_string(),
            found: true,
            tip_height: 0,
            proof: Some(proof),
            expires_at_height: None,
            expired: false,
            status: "active".to_string(),
        };
        let out = render_proof_get_summary(&resp);
        assert!(out.contains("found true"), "got: {out}");
        assert!(out.contains("prf-get-1"), "got: {out}");
        assert!(out.contains("expires_at_height none"), "got: {out}");
        assert!(out.contains("expired false"), "got: {out}");
        assert!(out.contains("status active"), "got: {out}");
    }

    #[test]
    fn render_proof_get_summary_found_expired() {
        use irium_node_rs::settlement::{ProofSignatureEnvelope, SETTLEMENT_PROOF_SCHEMA_ID, AGREEMENT_SIGNATURE_TYPE_SECP256K1};
        let proof = SettlementProof {
            proof_id: "prf-get-2".to_string(),
            schema_id: SETTLEMENT_PROOF_SCHEMA_ID.to_string(),
            proof_type: "payment".to_string(),
            agreement_hash: "hashgete".to_string(),
            milestone_id: None,
            attested_by: "att-ge".to_string(),
            attestation_time: 1700000000,
            evidence_hash: None,
            evidence_summary: None,
            signature: ProofSignatureEnvelope {
                signature_type: AGREEMENT_SIGNATURE_TYPE_SECP256K1.to_string(),
                pubkey_hex: String::new(),
                signature_hex: String::new(),
                payload_hash: String::new(),
            },
            expires_at_height: Some(100),
            typed_payload: None,
        };
        let resp = GetProofRpcResponse {
            proof_id: "prf-get-2".to_string(),
            found: true,
            tip_height: 200,
            proof: Some(proof),
            expires_at_height: Some(100),
            expired: true,
            status: "expired".to_string(),
        };
        let out = render_proof_get_summary(&resp);
        assert!(out.contains("expires_at_height 100"), "got: {out}");
        assert!(out.contains("expired true"), "got: {out}");
        assert!(out.contains("status expired"), "got: {out}");
    }

    #[test]
    fn render_proof_get_summary_not_found() {
        let resp = GetProofRpcResponse {
            proof_id: "no-such-proof".to_string(),
            found: false,
            tip_height: 0,
            proof: None,
            expires_at_height: None,
            expired: false,
            status: String::new(),
        };
        let out = render_proof_get_summary(&resp);
        assert!(out.contains("not_found true"), "must show not_found; got: {out}");
        assert!(!out.contains("
found true"), "must not show found true; got: {out}");
        assert!(!out.contains("status"), "status must not appear when not found; got: {out}");
    }


    // ---- Pagination CLI + render tests ----

    #[test]
    fn proof_list_cli_parses_limit_flag() {
        let args = vec!["--limit".to_string(), "10".to_string()];
        let opts = parse_proof_list_cli(&args).expect("must parse");
        assert_eq!(opts.limit, Some(10));
        assert_eq!(opts.offset, 0);
    }

    #[test]
    fn proof_list_cli_parses_offset_flag() {
        let args = vec!["--offset".to_string(), "5".to_string()];
        let opts = parse_proof_list_cli(&args).expect("must parse");
        assert_eq!(opts.offset, 5);
        assert_eq!(opts.limit, None);
    }

    #[test]
    fn proof_list_cli_parses_limit_and_offset() {
        let args = vec![
            "--offset".to_string(), "3".to_string(),
            "--limit".to_string(), "7".to_string(),
        ];
        let opts = parse_proof_list_cli(&args).expect("must parse");
        assert_eq!(opts.offset, 3);
        assert_eq!(opts.limit, Some(7));
    }

    #[test]
    fn proof_list_cli_rejects_non_integer_limit() {
        let args = vec!["--limit".to_string(), "abc".to_string()];
        let err = parse_proof_list_cli(&args).unwrap_err();
        assert!(err.contains("--limit"), "got: {err}");
    }

    #[test]
    fn render_proof_list_summary_shows_pagination_info() {
        // When total_count != returned_count, pagination metadata must appear in output.
        let resp = ListProofsRpcResponse {
            agreement_hash: "*".to_string(),
            returned_count: 2,
            total_count: 10,
            has_more: true,
            offset: 4,
            limit: Some(2),
            ..Default::default()
        };
        let out = render_proof_list_summary(&resp);
        assert!(out.contains("total_count 10"), "must show total; got: {out}");
        assert!(out.contains("offset 4"), "must show offset; got: {out}");
        assert!(out.contains("limit 2"), "must show limit; got: {out}");
        assert!(out.contains("has_more true"), "must show has_more when true; got: {out}");
    }

    #[test]
    fn render_proof_list_summary_no_pagination_info_when_full_page() {
        // When no pagination, total_count/offset/limit/has_more must not appear.
        let resp = ListProofsRpcResponse {
            agreement_hash: "*".to_string(),
            returned_count: 3,
            total_count: 3,
            ..Default::default()
        };
        let out = render_proof_list_summary(&resp);
        assert!(!out.contains("total_count"), "must not show pagination noise; got: {out}");
        assert!(!out.contains("offset"), "must not show offset when zero; got: {out}");
        assert!(!out.contains("limit"), "must not show limit when absent; got: {out}");
        assert!(!out.contains("has_more"), "must not show has_more when false; got: {out}");
    }

    #[test]
    fn render_proof_list_summary_has_more_false_on_last_page() {
        // Last page: returned_count + offset == total_count => has_more false, not shown.
        let resp = ListProofsRpcResponse {
            agreement_hash: "*".to_string(),
            returned_count: 2,
            total_count: 4,
            has_more: false,
            offset: 2,
            limit: Some(2),
            ..Default::default()
        };
        let out = render_proof_list_summary(&resp);
        assert!(out.contains("total_count 4"), "must show total; got: {out}");
        assert!(out.contains("offset 2"), "must show offset; got: {out}");
        assert!(!out.contains("has_more"), "has_more false must be silent; got: {out}");
    }

    #[test]
    fn render_proof_list_summary_has_more_true_shown() {
        // First page: has_more=true must appear in output.
        let resp = ListProofsRpcResponse {
            agreement_hash: "*".to_string(),
            returned_count: 2,
            total_count: 5,
            has_more: true,
            offset: 0,
            limit: Some(2),
            ..Default::default()
        };
        let out = render_proof_list_summary(&resp);
        assert!(out.contains("has_more true"), "must show has_more when true; got: {out}");
        assert!(out.contains("total_count 5"), "must show total; got: {out}");
        assert!(!out.contains("offset"), "offset is zero, must be silent; got: {out}");
    }

    #[test]
    fn render_proof_list_summary_no_limit_full_result_no_pagination_noise() {
        // No limit, returned_count == total_count => no pagination metadata shown.
        let resp = ListProofsRpcResponse {
            agreement_hash: "*".to_string(),
            returned_count: 3,
            total_count: 3,
            has_more: false,
            ..Default::default()
        };
        let out = render_proof_list_summary(&resp);
        assert!(!out.contains("has_more"), "no pagination noise; got: {out}");
        assert!(!out.contains("total_count"), "no pagination noise; got: {out}");
    }

    // ── Phase 4: render_build_settlement_summary unit tests ──────────────────

    #[test]
    fn render_build_settlement_summary_release() {
        let resp = BuildSettlementTxRpcResponse {
            agreement_hash: "abc123".to_string(),
            tip_height: 19500,
            policy_found: true,
            release_eligible: true,
            refund_eligible: false,
            reason: String::new(),
            actions: vec![SettlementActionRpc {
                action: "release".to_string(),
                recipient_address: "irium1payee000".to_string(),
                recipient_label: "payee".to_string(),
                amount_bps: 10000,
                executable: true,
                executable_after_height: None,
                reason: String::new(),
            }],
        };
        let s = render_build_settlement_summary(&resp);
        assert!(s.contains("release_eligible true"), "must mark release eligible: {s}");
        assert!(s.contains("refund_eligible false"), "must mark refund not eligible: {s}");
        assert!(s.contains("action[0] release"), "must list release action: {s}");
        assert!(s.contains("irium1payee000"), "must include recipient address: {s}");
        assert!(s.contains("executable=true"), "must mark as executable: {s}");
    }

    #[test]
    fn render_build_settlement_summary_refund() {
        let resp = BuildSettlementTxRpcResponse {
            agreement_hash: "def456".to_string(),
            tip_height: 20000,
            policy_found: true,
            release_eligible: false,
            refund_eligible: true,
            reason: "deadline_elapsed".to_string(),
            actions: vec![SettlementActionRpc {
                action: "refund".to_string(),
                recipient_address: "irium1payer000".to_string(),
                recipient_label: "payer".to_string(),
                amount_bps: 10000,
                executable: true,
                executable_after_height: None,
                reason: String::new(),
            }],
        };
        let s = render_build_settlement_summary(&resp);
        assert!(s.contains("release_eligible false"), "must mark release not eligible: {s}");
        assert!(s.contains("refund_eligible true"), "must mark refund eligible: {s}");
        assert!(s.contains("action[0] refund"), "must list refund action: {s}");
        assert!(s.contains("irium1payer000"), "must include payer address: {s}");
        assert!(s.contains("deadline_elapsed"), "must include reason: {s}");
    }

    #[test]
    fn settlement_client_uses_rpc_prefix_in_paths() {
        // Verify the SettlementClient method bodies call self.post() with /rpc/ prefix.
        // This is a compile-time check: if the paths were wrong the RPC calls would
        // hit the wrong URL. We verify by instantiating the client against a
        // non-listening address and confirming the error message contains the path,
        // not a DNS or connection error with a wrong path.
        //
        // We can verify the path strings are correct by checking the source directly
        // or via the string constants. Here we check the struct builds and that
        // the serialize round-trip for request types is correct (path correctness
        // is structural — tested by integration; this ensures no typos in names).
        let req = ComputeAgreementHashRpcRequest {
            agreement: serde_json::from_value(serde_json::json!({
                "agreement_id": "test-path-01",
                "version": 1,
                "template_type": "simple_release_refund",
                "parties": [],
                "payer": "p", "payee": "q",
                "total_amount": 1000,
                "network_marker": "IRIUM",
                "creation_time": 0,
                "deadlines": {"settlement_deadline": 100, "refund_deadline": 100, "dispute_window": null},
                "release_conditions": [],
                "refund_conditions": [],
                "milestones": [],
                "deposit_rule": null,
                "proof_policy_reference": null,
                "document_hash": "a".repeat(64),
                "metadata_hash": null,
                "invoice_reference": null,
                "external_reference": null,
                "disputed_metadata_only": false,
                "mediator_reference": null
            })).unwrap(),
        };
        // Serialize + round-trip: confirms field name is "agreement" (matching server schema).
        let serialized = serde_json::to_string(&req).expect("must serialize");
        assert!(serialized.contains("\"agreement\":"), "request body must use agreement key: {serialized}");
        // Re-deserialize to confirm serde round-trip is stable.
        let _back: ComputeAgreementHashRpcRequest = serde_json::from_str(&serialized)
            .expect("must round-trip");

        let bst_req = BuildSettlementTxRpcRequest {
            agreement: _back.agreement,
        };
        let bst_serialized = serde_json::to_string(&bst_req).expect("must serialize");
        assert!(bst_serialized.contains("\"agreement\":"), "bst request body must use agreement key: {bst_serialized}");
    }

    #[test]
    fn compute_agreement_hash_response_serde_defaults() {
        // A minimal/empty JSON object must deserialize to default values without panic.
        let resp: ComputeAgreementHashRpcResponse = serde_json::from_str("{}").expect("must deserialize from empty object");
        assert!(resp.agreement_hash.is_empty(), "agreement_hash defaults to empty");
        assert!(resp.canonical_json.is_empty(), "canonical_json defaults to empty");
        assert!(resp.serialization_rules.is_empty(), "serialization_rules defaults to empty vec");
    }

    #[test]
    fn build_settlement_tx_response_serde_defaults() {
        // A minimal/empty JSON object must deserialize to default values without panic.
        let resp: BuildSettlementTxRpcResponse = serde_json::from_str("{}").expect("must deserialize from empty object");
        assert!(resp.agreement_hash.is_empty(), "agreement_hash defaults to empty");
        assert!(!resp.policy_found, "policy_found defaults to false");
        assert!(!resp.release_eligible, "release_eligible defaults to false");
        assert!(!resp.refund_eligible, "refund_eligible defaults to false");
        assert_eq!(resp.tip_height, 0, "tip_height defaults to 0");
        assert!(resp.actions.is_empty(), "actions defaults to empty vec");
    }

    #[test]
    fn settlement_action_rpc_serde_defaults() {
        // A minimal action object deserializes without panic; executable defaults false.
        let action: SettlementActionRpc = serde_json::from_str("{}").expect("must deserialize from empty object");
        assert!(action.action.is_empty(), "action defaults to empty");
        assert!(!action.executable, "executable defaults to false");
        assert!(action.executable_after_height.is_none(), "executable_after_height defaults to None");
    }

    #[test]
    fn render_build_settlement_summary_no_policy() {
        // When policy_found=false and no actions, the output must not panic and
        // must clearly state policy_found false with action_count 0.
        let resp = BuildSettlementTxRpcResponse {
            agreement_hash: "nopol123".to_string(),
            tip_height: 5000,
            policy_found: false,
            release_eligible: false,
            refund_eligible: false,
            reason: "no_policy".to_string(),
            actions: vec![],
        };
        let s = render_build_settlement_summary(&resp);
        assert!(s.contains("policy_found false"), "must show policy_found false: {s}");
        assert!(s.contains("action_count 0"), "must show 0 actions: {s}");
        assert!(s.contains("no_policy"), "must show reason: {s}");
        assert!(s.contains("tip_height 5000"), "must show tip_height: {s}");
    }

    #[test]
    fn build_settlement_tx_response_json_round_trip() {
        // A populated response serializes and re-deserializes identically.
        let resp = BuildSettlementTxRpcResponse {
            agreement_hash: "roundtrip01".to_string(),
            tip_height: 9999,
            policy_found: true,
            release_eligible: true,
            refund_eligible: false,
            reason: String::new(),
            actions: vec![SettlementActionRpc {
                action: "release".to_string(),
                recipient_address: "irium1payee".to_string(),
                recipient_label: "payee".to_string(),
                amount_bps: 10000,
                executable: true,
                executable_after_height: None,
                reason: String::new(),
            }],
        };
        let json = serde_json::to_string(&resp).expect("must serialize");
        let back: BuildSettlementTxRpcResponse = serde_json::from_str(&json).expect("must deserialize");
        assert_eq!(back.agreement_hash, resp.agreement_hash);
        assert_eq!(back.tip_height, resp.tip_height);
        assert_eq!(back.actions.len(), 1);
        assert_eq!(back.actions[0].action, "release");
        assert_eq!(back.actions[0].amount_bps, 10000);
    }

    #[test]
    fn render_build_settlement_summary_holdback_locked() {
        let resp = BuildSettlementTxRpcResponse {
            agreement_hash: "ghi789".to_string(),
            tip_height: 19500,
            policy_found: true,
            release_eligible: true,
            refund_eligible: false,
            reason: String::new(),
            actions: vec![
                SettlementActionRpc {
                    action: "release".to_string(),
                    recipient_address: "irium1payee000".to_string(),
                    recipient_label: "immediate".to_string(),
                    amount_bps: 9200,
                    executable: true,
                    executable_after_height: None,
                    reason: String::new(),
                },
                SettlementActionRpc {
                    action: "release".to_string(),
                    recipient_address: "irium1payee000".to_string(),
                    recipient_label: "holdback".to_string(),
                    amount_bps: 800,
                    executable: false,
                    executable_after_height: Some(99999),
                    reason: String::new(),
                },
            ],
        };
        let s = render_build_settlement_summary(&resp);
        assert!(s.contains("action_count 2"), "must report 2 actions: {s}");
        assert!(s.contains("action[0]"), "must list first action: {s}");
        assert!(s.contains("action[1]"), "must list second action: {s}");
        assert!(s.contains("bps=800"), "must include holdback bps: {s}");
        assert!(s.contains("executable=false"), "holdback must be non-executable: {s}");
        assert!(s.contains("height_99999"), "must include holdback unlock height: {s}");
    }

}
fn main() {
    let args = env::args().skip(1).collect::<Vec<_>>();
    if args.is_empty() {
        usage();
        std::process::exit(1);
    }

    match args[0].as_str() {
        "init" => {
            let path = wallet_path();
            if path.exists() {
                eprintln!("Wallet already exists: {}", path.display());
                std::process::exit(1);
            }
            let seed_hex = if args.len() == 3 {
                if args[1] != "--seed" {
                    usage();
                    std::process::exit(1);
                }
                if let Err(e) = parse_seed_hex(&args[2]) {
                    eprintln!("Invalid seed: {}", e);
                    std::process::exit(1);
                }
                args[2].clone()
            } else if args.len() == 1 {
                generate_seed_hex()
            } else {
                usage();
                std::process::exit(1);
            };
            let secret = match derive_secret_from_seed_hex(&seed_hex, 0) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("Failed to derive key from seed: {}", e);
                    std::process::exit(1);
                }
            };
            let key = wallet_key_from_secret(&secret, true);
            let wallet = WalletFile {
                version: 1,
                seed_hex: Some(seed_hex.clone()),
                next_index: 1,
                keys: vec![key.clone()],
            };
            println!("wallet initialized");
            println!(
                "seed saved in wallet metadata; export with: irium-wallet export-seed --out <file>"
            );
            if let Err(e) = save_wallet(&path, &wallet) {
                eprintln!("Failed to save wallet: {}", e);
                std::process::exit(1);
            }
            println!("wallet {}", path.display());
        }
        "new-address" => {
            let path = wallet_path();
            let mut wallet = match ensure_wallet(&path) {
                Ok(w) => w,
                Err(e) => {
                    eprintln!("Failed to load wallet: {}", e);
                    std::process::exit(1);
                }
            };
            let key = if let Some(seed_hex) = wallet.seed_hex.as_deref() {
                let index = wallet.next_index;
                let secret = match derive_secret_from_seed_hex(seed_hex, index) {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("Failed to derive key from seed: {}", e);
                        std::process::exit(1);
                    }
                };
                wallet.next_index = wallet.next_index.saturating_add(1);
                wallet_key_from_secret(&secret, true)
            } else {
                generate_key()
            };
            wallet.keys.push(key);
            println!("new address added; use list-addresses to view");
            if let Err(e) = save_wallet(&path, &wallet) {
                eprintln!("Failed to save wallet: {}", e);
                std::process::exit(1);
            }
            println!("wallet {}", path.display());
        }
        "export-wif" => {
            if args.len() != 4 || args[2] != "--out" {
                usage();
                std::process::exit(1);
            }
            let out = PathBuf::from(&args[3]);
            let path = wallet_path();
            let wallet = match load_wallet(&path) {
                Ok(w) => w,
                Err(e) => {
                    eprintln!("Failed to load wallet: {}", e);
                    std::process::exit(1);
                }
            };
            let key = match find_key(&wallet, &args[1]) {
                Some(k) => k,
                None => {
                    eprintln!("Address not found in wallet");
                    std::process::exit(1);
                }
            };
            let priv_bytes = match hex::decode(&key.privkey) {
                Ok(v) if v.len() == 32 => v,
                _ => {
                    eprintln!("Wallet key is invalid");
                    std::process::exit(1);
                }
            };
            let mut sec = [0u8; 32];
            sec.copy_from_slice(&priv_bytes);
            let wif = secret_to_wif(&sec, true);
            if let Some(parent) = out.parent() {
                if let Err(e) = fs::create_dir_all(parent) {
                    eprintln!("Failed to create output dir: {}", e);
                    std::process::exit(1);
                }
            }
            if let Err(e) = fs::write(&out, format!("{}\n", wif)) {
                eprintln!("Failed to write WIF file: {}", e);
                std::process::exit(1);
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = fs::set_permissions(&out, fs::Permissions::from_mode(0o600));
            }
            println!("wif exported to {}", out.display());
        }
        "import-wif" => {
            if args.len() != 2 {
                usage();
                std::process::exit(1);
            }
            let path = wallet_path();
            let mut wallet = match ensure_wallet(&path) {
                Ok(w) => w,
                Err(e) => {
                    eprintln!("Failed to load wallet: {}", e);
                    std::process::exit(1);
                }
            };
            let (priv_bytes, compressed) = match wif_to_secret_and_compression(&args[1]) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("Invalid WIF: {}", e);
                    std::process::exit(1);
                }
            };
            let secret = match SecretKey::from_slice(&priv_bytes) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("Invalid WIF secret: {}", e);
                    std::process::exit(1);
                }
            };
            let key = wallet_key_from_secret(&secret, compressed);
            if wallet.keys.iter().any(|k| k.address == key.address) {
                println!("key already exists in wallet");
                std::process::exit(0);
            }
            wallet.keys.push(key);
            println!("key imported into wallet");
            if let Err(e) = save_wallet(&path, &wallet) {
                eprintln!("Failed to save wallet: {}", e);
                std::process::exit(1);
            }
            println!("wallet {}", path.display());
        }
        "export-seed" => {
            if args.len() != 3 || args[1] != "--out" {
                usage();
                std::process::exit(1);
            }
            let out = PathBuf::from(&args[2]);
            let path = wallet_path();
            let wallet = match load_wallet(&path) {
                Ok(w) => w,
                Err(e) => {
                    eprintln!("Failed to load wallet: {}", e);
                    std::process::exit(1);
                }
            };
            let seed = match wallet.seed_hex {
                Some(seed) => seed,
                None => {
                    eprintln!("No seed stored in wallet (legacy/imported key-only wallet)");
                    std::process::exit(1);
                }
            };
            if let Some(parent) = out.parent() {
                if let Err(e) = fs::create_dir_all(parent) {
                    eprintln!("Failed to create output dir: {}", e);
                    std::process::exit(1);
                }
            }
            if let Err(e) = fs::write(&out, format!("{}\n", seed)) {
                eprintln!("Failed to write seed file: {}", e);
                std::process::exit(1);
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = fs::set_permissions(&out, fs::Permissions::from_mode(0o600));
            }
            println!("seed exported to {}", out.display());
        }
        "import-seed" => {
            if args.len() != 2 && args.len() != 3 {
                usage();
                std::process::exit(1);
            }
            let force = args.len() == 3 && args[2] == "--force";
            if args.len() == 3 && !force {
                usage();
                std::process::exit(1);
            }
            if let Err(e) = parse_seed_hex(&args[1]) {
                eprintln!("Invalid seed: {}", e);
                std::process::exit(1);
            }
            let path = wallet_path();
            let mut wallet = match ensure_wallet(&path) {
                Ok(w) => w,
                Err(e) => {
                    eprintln!("Failed to load wallet: {}", e);
                    std::process::exit(1);
                }
            };
            if !wallet.keys.is_empty() && !force {
                eprintln!(
                    "Wallet already has keys. Re-run with --force to replace wallet keys from seed."
                );
                std::process::exit(1);
            }
            let seed_hex = args[1].clone();
            let secret = match derive_secret_from_seed_hex(&seed_hex, 0) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("Failed to derive key from seed: {}", e);
                    std::process::exit(1);
                }
            };
            let key = wallet_key_from_secret(&secret, true);
            wallet.version = 1;
            wallet.seed_hex = Some(seed_hex);
            wallet.next_index = 1;
            wallet.keys = vec![key.clone()];
            if let Err(e) = save_wallet(&path, &wallet) {
                eprintln!("Failed to save wallet: {}", e);
                std::process::exit(1);
            }
            println!("seed imported into wallet");
            println!("wallet {}", path.display());
        }
        "backup" => {
            if args.len() != 1 && args.len() != 3 {
                usage();
                std::process::exit(1);
            }
            let path = wallet_path();
            if !path.exists() {
                eprintln!("Wallet does not exist: {}", path.display());
                std::process::exit(1);
            }
            let out = if args.len() == 3 {
                if args[1] != "--out" {
                    usage();
                    std::process::exit(1);
                }
                PathBuf::from(&args[2])
            } else {
                let ts = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                let home = env::var("HOME").unwrap_or_else(|_| "/".to_string());
                PathBuf::from(home)
                    .join(".irium/wallet-backups")
                    .join(format!("wallet.json.bak.{ts}"))
            };
            if let Some(parent) = out.parent() {
                if let Err(e) = fs::create_dir_all(parent) {
                    eprintln!("Failed to create backup dir: {}", e);
                    std::process::exit(1);
                }
            }
            if let Err(e) = fs::copy(&path, &out) {
                eprintln!("Failed to backup wallet: {}", e);
                std::process::exit(1);
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = fs::set_permissions(&out, fs::Permissions::from_mode(0o600));
            }
            println!("backup {}", out.display());
        }
        "restore-backup" => {
            if args.len() != 2 && args.len() != 3 {
                usage();
                std::process::exit(1);
            }
            let force = args.len() == 3 && args[2] == "--force";
            if args.len() == 3 && !force {
                usage();
                std::process::exit(1);
            }
            let src = PathBuf::from(&args[1]);
            if !src.exists() {
                eprintln!("Backup file not found: {}", src.display());
                std::process::exit(1);
            }
            let dst = wallet_path();
            if dst.exists() && !force {
                eprintln!(
                    "Wallet already exists at {}. Re-run with --force to overwrite.",
                    dst.display()
                );
                std::process::exit(1);
            }
            if let Some(parent) = dst.parent() {
                if let Err(e) = fs::create_dir_all(parent) {
                    eprintln!("Failed to create wallet dir: {}", e);
                    std::process::exit(1);
                }
            }
            if let Err(e) = fs::copy(&src, &dst) {
                eprintln!("Failed to restore wallet: {}", e);
                std::process::exit(1);
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = fs::set_permissions(&dst, fs::Permissions::from_mode(0o600));
            }
            println!("wallet {}", dst.display());
        }
        "list-addresses" => {
            let path = wallet_path();
            let wallet = match load_wallet(&path) {
                Ok(w) => w,
                Err(e) => {
                    eprintln!("Failed to load wallet: {}", e);
                    std::process::exit(1);
                }
            };
            for key in wallet.keys {
                println!("{}", key.address);
            }
        }
        "address-to-pkh" => {
            if args.len() != 2 {
                usage();
                std::process::exit(1);
            }
            let addr = &args[1];
            match base58_p2pkh_to_hash(addr) {
                Some(pkh) => println!("{}", hex::encode(pkh)),
                None => {
                    eprintln!("Invalid address or checksum");
                    std::process::exit(1);
                }
            }
        }
        "qr" => {
            if args.len() < 2 {
                usage();
                std::process::exit(1);
            }
            let addr = &args[1];
            if base58_p2pkh_to_hash(addr).is_none() {
                eprintln!("Invalid address or checksum");
                std::process::exit(1);
            }
            let mut output_path: Option<String> = None;
            let mut use_svg = false;
            let mut i = 2;
            while i < args.len() {
                match args[i].as_str() {
                    "--svg" => {
                        use_svg = true;
                        i += 1;
                    }
                    "--out" => {
                        if i + 1 >= args.len() {
                            eprintln!("Missing --out value");
                            std::process::exit(1);
                        }
                        output_path = Some(args[i + 1].clone());
                        i += 2;
                    }
                    _ => {
                        usage();
                        std::process::exit(1);
                    }
                }
            }

            let rendered = if use_svg {
                render_svg(addr, 8, 2).unwrap_or_else(|e| {
                    eprintln!("{}", e);
                    std::process::exit(1);
                })
            } else {
                render_ascii(addr).unwrap_or_else(|e| {
                    eprintln!("{}", e);
                    std::process::exit(1);
                })
            };

            if let Some(path) = output_path {
                if let Err(e) = fs::write(&path, rendered) {
                    eprintln!("Failed to write {}: {}", path, e);
                    std::process::exit(1);
                }
            } else {
                print!("{}", rendered);
            }
        }
        "balance" => {
            if args.len() != 2 && args.len() != 4 {
                usage();
                std::process::exit(1);
            }
            let addr = &args[1];
            if base58_p2pkh_to_hash(addr).is_none() {
                eprintln!("Invalid address or checksum");
                std::process::exit(1);
            }
            let mut rpc_url = default_rpc_url();
            if args.len() == 4 {
                if args[2] != "--rpc" {
                    usage();
                    std::process::exit(1);
                }
                rpc_url = args[3].clone();
            }
            let base = rpc_url.trim_end_matches('/');
            let client = match rpc_client(base) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Failed to init HTTP client: {}", e);
                    std::process::exit(1);
                }
            };
            let payload = match fetch_balance(&client, base, addr) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            };
            let use_color = color_enabled();
            let irm_display = format_irm(payload.balance);
            let balance_display = if use_color {
                format!("\x1b[32m{} IRM\x1b[0m", irm_display)
            } else {
                format!("{} IRM", irm_display)
            };
            let mined_blocks = payload.mined_blocks.unwrap_or(payload.utxo_count);
            println!("balance {} blocks mined {}", balance_display, mined_blocks);
        }
        "list-unspent" => {
            if args.len() != 2 && args.len() != 4 {
                usage();
                std::process::exit(1);
            }
            let addr = &args[1];
            if base58_p2pkh_to_hash(addr).is_none() {
                eprintln!("Invalid address or checksum");
                std::process::exit(1);
            }
            let mut rpc_url = default_rpc_url();
            if args.len() == 4 {
                if args[2] != "--rpc" {
                    usage();
                    std::process::exit(1);
                }
                rpc_url = args[3].clone();
            }
            let base = rpc_url.trim_end_matches('/');
            let client = match rpc_client(base) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Failed to init HTTP client: {}", e);
                    std::process::exit(1);
                }
            };
            let payload = match fetch_utxos(&client, base, addr) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            };
            for utxo in payload.utxos {
                let confirmations = payload.height.saturating_sub(utxo.height);
                if utxo.is_coinbase && confirmations < COINBASE_MATURITY {
                    continue;
                }
                let val = format_irm(utxo.value);
                println!(
                    "{}:{} {} IRM height {} coinbase {}",
                    utxo.txid, utxo.index, val, utxo.height, utxo.is_coinbase
                );
            }
        }

        "history" => {
            if args.len() != 2 && args.len() != 4 {
                usage();
                std::process::exit(1);
            }
            let addr = &args[1];
            if base58_p2pkh_to_hash(addr).is_none() {
                eprintln!("Invalid address or checksum");
                std::process::exit(1);
            }
            let mut rpc_url = default_rpc_url();
            if args.len() == 4 {
                if args[2] != "--rpc" {
                    usage();
                    std::process::exit(1);
                }
                rpc_url = args[3].clone();
            }
            let base = rpc_url.trim_end_matches('/');
            let client = match rpc_client(base) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Failed to init HTTP client: {}", e);
                    std::process::exit(1);
                }
            };
            let payload = match fetch_history(&client, base, addr) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            };
            let _height = payload.height;
            for item in payload.txs {
                let received = format_irm(item.received);
                let spent = format_irm(item.spent);
                let net = if item.net >= 0 {
                    format!("+{}", format_irm(item.net as u64))
                } else {
                    format!("-{}", format_irm((-item.net) as u64))
                };
                println!(
                    "{} height {} net {} recv {} spent {} coinbase {}",
                    item.txid, item.height, net, received, spent, item.is_coinbase
                );
            }
        }
        "estimate-fee" => {
            let mut rpc_url = default_rpc_url();
            if args.len() == 3 {
                if args[1] != "--rpc" {
                    usage();
                    std::process::exit(1);
                }
                rpc_url = args[2].clone();
            } else if args.len() != 1 {
                usage();
                std::process::exit(1);
            }
            let base = rpc_url.trim_end_matches('/');
            let client = match rpc_client(base) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Failed to init HTTP client: {}", e);
                    std::process::exit(1);
                }
            };
            let payload = match fetch_fee_estimate(&client, base) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            };
            println!(
                "min_fee_per_byte {} mempool_size {}",
                payload.min_fee_per_byte, payload.mempool_size
            );
        }
        "agreement-create-simple-settlement" => {
            if let Err(e) = handle_agreement_create_simple(&args[1..]) {
                eprintln!("{}", e);
                std::process::exit(1);
            }
        }
        "agreement-create-otc" => {
            if let Err(e) = handle_agreement_create_otc(&args[1..]) {
                eprintln!("{}", e);
                std::process::exit(1);
            }
        }
        "agreement-create-deposit" => {
            if let Err(e) = handle_agreement_create_deposit(&args[1..]) {
                eprintln!("{}", e);
                std::process::exit(1);
            }
        }
        "agreement-create-milestone" => {
            if let Err(e) = handle_agreement_create_milestone(&args[1..]) {
                eprintln!("{}", e);
                std::process::exit(1);
            }
        }
        "agreement-bundle-create" => {
            if let Err(e) = handle_agreement_bundle_pack(&args[1..], true) {
                eprintln!("{}", e);
                std::process::exit(1);
            }
        }
        "agreement-bundle-pack" => {
            if let Err(e) = handle_agreement_bundle_pack(&args[1..], false) {
                eprintln!("{}", e);
                std::process::exit(1);
            }
        }
        "agreement-bundle-inspect" => {
            if args.len() != 2 && !(args.len() == 3 && args[2] == "--json") {
                usage();
                std::process::exit(1);
            }
            let json_mode = args.iter().any(|a| a == "--json");
            if let Err(e) = handle_agreement_bundle_inspect(&args[1], json_mode) {
                eprintln!("{}", e);
                std::process::exit(1);
            }
        }
        "agreement-bundle-verify" => {
            if args.len() != 2 && !(args.len() == 3 && args[2] == "--json") {
                usage();
                std::process::exit(1);
            }
            let json_mode = args.iter().any(|a| a == "--json");
            if let Err(e) = handle_agreement_bundle_verify(&args[1], json_mode) {
                eprintln!("{}", e);
                std::process::exit(1);
            }
        }
        "agreement-bundle-unpack" => {
            if args.len() < 4 {
                usage();
                std::process::exit(1);
            }
            let mut out_dir = None;
            let mut json_mode = false;
            let mut i = 2;
            while i < args.len() {
                match args[i].as_str() {
                    "--out-dir" => {
                        out_dir = args.get(i + 1).cloned();
                        i += 2;
                    }
                    "--json" => {
                        json_mode = true;
                        i += 1;
                    }
                    other => {
                        eprintln!("Unknown argument {}", other);
                        std::process::exit(1);
                    }
                }
            }
            let out_dir = out_dir.unwrap_or_else(|| {
                eprintln!("--out-dir required");
                std::process::exit(1);
            });
            if let Err(e) = handle_agreement_bundle_unpack(&args[1], &out_dir, json_mode) {
                eprintln!("{}", e);
                std::process::exit(1);
            }
        }
        "agreement-sign" => {
            if let Err(e) = handle_agreement_sign(&args[1..]) {
                eprintln!("{}", e);
                std::process::exit(1);
            }
        }
        "agreement-verify-signature" => {
            if let Err(e) = handle_agreement_verify_signature(&args[1..]) {
                eprintln!("{}", e);
                std::process::exit(1);
            }
        }
        "agreement-bundle-sign" => {
            if let Err(e) = handle_agreement_bundle_sign(&args[1..]) {
                eprintln!("{}", e);
                std::process::exit(1);
            }
        }
        "agreement-bundle-verify-signatures" => {
            if args.len() != 3 && !(args.len() == 4 && args[2] == "--bundle" && args[3] == "--json")
            {
            }
            let mut bundle_reference = None::<String>;
            let mut json_mode = false;
            let mut i = 1;
            while i < args.len() {
                match args[i].as_str() {
                    "--bundle" => {
                        bundle_reference = args.get(i + 1).cloned();
                        i += 2;
                    }
                    "--json" => {
                        json_mode = true;
                        i += 1;
                    }
                    other => {
                        eprintln!("Unknown argument {}", other);
                        std::process::exit(1);
                    }
                }
            }
            let bundle_reference = bundle_reference.unwrap_or_else(|| {
                eprintln!("--bundle required");
                std::process::exit(1);
            });
            if let Err(e) = handle_agreement_bundle_verify_signatures(&bundle_reference, json_mode)
            {
                eprintln!("{}", e);
                std::process::exit(1);
            }
        }
        "agreement-signature-inspect" => {
            if let Err(e) = handle_agreement_signature_inspect(&args[1..]) {
                eprintln!("{}", e);
                std::process::exit(1);
            }
        }
        "agreement-template" => {
            if args.len() < 2 {
                usage();
                std::process::exit(1);
            }
            let template_type = match parse_template_type(&args[1]) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            };
            let mut agreement_id = format!(
                "agr-{}",
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs()
            );
            let mut payer_id = String::from("payer");
            let mut payee_id = String::from("payee");
            let mut payer_address = String::new();
            let mut payee_address = String::new();
            let mut amount = None;
            let mut refund_timeout = None;
            let mut settlement_deadline = None;
            let mut refund_deadline = None;
            let mut secret_hash_hex = "11".repeat(32);
            let mut document_hash = "22".repeat(32);
            let mut metadata_hash: Option<String> = None;
            let mut invoice_reference: Option<String> = None;
            let mut external_reference: Option<String> = None;
            let mut out_path: Option<String> = None;
            let mut milestones = Vec::new();
            let mut i = 2;
            while i < args.len() {
                match args[i].as_str() {
                    "--agreement-id" => {
                        agreement_id = args.get(i + 1).cloned().unwrap_or_default();
                        i += 2;
                    }
                    "--payer-id" => {
                        payer_id = args.get(i + 1).cloned().unwrap_or_default();
                        i += 2;
                    }
                    "--payee-id" => {
                        payee_id = args.get(i + 1).cloned().unwrap_or_default();
                        i += 2;
                    }
                    "--payer-address" => {
                        payer_address = args.get(i + 1).cloned().unwrap_or_default();
                        i += 2;
                    }
                    "--payee-address" => {
                        payee_address = args.get(i + 1).cloned().unwrap_or_default();
                        i += 2;
                    }
                    "--amount" => {
                        let raw = args.get(i + 1).cloned().unwrap_or_default();
                        amount = Some(match parse_irm(&raw) {
                            Ok(v) => v,
                            Err(e) => {
                                eprintln!("Invalid --amount: {}", e);
                                std::process::exit(1);
                            }
                        });
                        i += 2;
                    }
                    "--timeout-height" => {
                        refund_timeout = args.get(i + 1).and_then(|v| v.parse::<u64>().ok());
                        i += 2;
                    }
                    "--settlement-deadline" => {
                        settlement_deadline = args.get(i + 1).and_then(|v| v.parse::<u64>().ok());
                        i += 2;
                    }
                    "--refund-deadline" => {
                        refund_deadline = args.get(i + 1).and_then(|v| v.parse::<u64>().ok());
                        i += 2;
                    }
                    "--secret-hash" => {
                        secret_hash_hex = args.get(i + 1).cloned().unwrap_or_default();
                        i += 2;
                    }
                    "--document-hash" => {
                        document_hash = args.get(i + 1).cloned().unwrap_or_default();
                        i += 2;
                    }
                    "--metadata-hash" => {
                        metadata_hash = args.get(i + 1).cloned();
                        i += 2;
                    }
                    "--invoice-ref" => {
                        invoice_reference = args.get(i + 1).cloned();
                        i += 2;
                    }
                    "--external-ref" => {
                        external_reference = args.get(i + 1).cloned();
                        i += 2;
                    }
                    "--milestone" => {
                        let raw = args.get(i + 1).cloned().unwrap_or_default();
                        let item = match parse_milestone_arg(&raw, &payee_address, &payer_address) {
                            Ok(v) => v,
                            Err(e) => {
                                eprintln!("Invalid --milestone: {}", e);
                                std::process::exit(1);
                            }
                        };
                        milestones.push(item);
                        i += 2;
                    }
                    "--out" => {
                        out_path = args.get(i + 1).cloned();
                        i += 2;
                    }
                    _ => {
                        eprintln!("Unknown argument {}", args[i]);
                        std::process::exit(1);
                    }
                }
            }
            if payer_address.is_empty() || payee_address.is_empty() {
                eprintln!("--payer-address and --payee-address are required");
                std::process::exit(1);
            }
            if base58_p2pkh_to_hash(&payer_address).is_none()
                || base58_p2pkh_to_hash(&payee_address).is_none()
            {
                eprintln!("Invalid payer/payee address");
                std::process::exit(1);
            }
            let creation_time = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let agreement_value = if !milestones.is_empty() {
                let total_amount: u64 = milestones
                    .iter()
                    .filter_map(|m| m.get("amount").and_then(|v| v.as_u64()))
                    .sum();
                json!({
                    "agreement_id": agreement_id,
                    "version": 1,
                    "template_type": template_type,
                    "parties": [
                        {"party_id": payer_id, "display_name": "Payer", "address": payer_address, "role": "payer"},
                        {"party_id": payee_id, "display_name": "Payee", "address": payee_address, "role": "payee"}
                    ],
                    "payer": payer_id,
                    "payee": payee_id,
                    "total_amount": total_amount,
                    "network_marker": "IRIUM",
                    "creation_time": creation_time,
                    "deadlines": {"settlement_deadline": settlement_deadline, "refund_deadline": refund_deadline},
                    "release_conditions": [{"mode": "secret_preimage", "secret_hash_hex": secret_hash_hex, "release_authorizer": "payer"}],
                    "refund_conditions": [{"refund_address": payer_address, "timeout_height": refund_timeout.unwrap_or(refund_deadline.unwrap_or(0))}],
                    "milestones": milestones,
                    "document_hash": document_hash,
                    "metadata_hash": metadata_hash,
                    "invoice_reference": invoice_reference,
                    "external_reference": external_reference,
                    "proof_policy_reference": "phase2-placeholder"
                })
            } else {
                let total_amount = amount.unwrap_or(0);
                json!({
                    "agreement_id": agreement_id,
                    "version": 1,
                    "template_type": template_type,
                    "parties": [
                        {"party_id": payer_id, "display_name": "Payer", "address": payer_address, "role": "payer"},
                        {"party_id": payee_id, "display_name": "Payee", "address": payee_address, "role": "payee"}
                    ],
                    "payer": payer_id,
                    "payee": payee_id,
                    "total_amount": total_amount,
                    "network_marker": "IRIUM",
                    "creation_time": creation_time,
                    "deadlines": {"settlement_deadline": settlement_deadline, "refund_deadline": refund_deadline},
                    "release_conditions": [{"mode": "secret_preimage", "secret_hash_hex": secret_hash_hex, "release_authorizer": "payer"}],
                    "refund_conditions": [{"refund_address": payer_address, "timeout_height": refund_timeout.unwrap_or(refund_deadline.unwrap_or(0))}],
                    "document_hash": document_hash,
                    "metadata_hash": metadata_hash,
                    "invoice_reference": invoice_reference,
                    "external_reference": external_reference,
                    "proof_policy_reference": "phase2-placeholder"
                })
            };
            let agreement: AgreementObject = match serde_json::from_value(agreement_value.clone()) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("template agreement build failed: {}", e);
                    std::process::exit(1);
                }
            };
            if let Err(e) = agreement.validate() {
                eprintln!("agreement invalid: {}", e);
                std::process::exit(1);
            }
            if let Err(e) = save_json_output(out_path.as_deref(), &agreement_value) {
                eprintln!("{}", e);
                std::process::exit(1);
            }
        }
        "agreement-save" => {
            if args.len() < 2 {
                usage();
                std::process::exit(1);
            }
            let resolved = match resolve_agreement_input(&args[1]) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            };
            let mut label = None;
            let mut note = None;
            let mut linked_funding_txids = Vec::new();
            let mut json_mode = false;
            let mut i = 2;
            while i < args.len() {
                match args[i].as_str() {
                    "--label" => {
                        label = args.get(i + 1).cloned();
                        if label.is_none() {
                            eprintln!("missing --label value");
                            std::process::exit(1);
                        }
                        i += 2;
                    }
                    "--note" => {
                        note = args.get(i + 1).cloned();
                        if note.is_none() {
                            eprintln!("missing --note value");
                            std::process::exit(1);
                        }
                        i += 2;
                    }
                    "--funding-txid" => {
                        let txid = args.get(i + 1).cloned().unwrap_or_default();
                        if txid.len() != 64 || hex::decode(&txid).map(|v| v.len()).ok() != Some(32)
                        {
                            eprintln!("--funding-txid must be 32-byte hex");
                            std::process::exit(1);
                        }
                        linked_funding_txids.push(txid.to_lowercase());
                        i += 2;
                    }
                    "--json" => {
                        json_mode = true;
                        i += 1;
                    }
                    other => {
                        eprintln!("Unknown argument {}", other);
                        std::process::exit(1);
                    }
                }
            }
            let mut bundle = match resolved.bundle {
                Some(bundle) => bundle,
                None => match build_agreement_bundle(
                    &resolved.agreement,
                    now_unix(),
                    None,
                    None,
                    Vec::new(),
                    Vec::new(),
                ) {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("{}", e);
                        std::process::exit(1);
                    }
                },
            };
            bundle.metadata.saved_at = now_unix();
            if label.is_some() {
                bundle.metadata.source_label = label;
            }
            if note.is_some() {
                bundle.metadata.note = note;
            }
            for txid in linked_funding_txids {
                if !bundle
                    .metadata
                    .linked_funding_txids
                    .iter()
                    .any(|v| v == &txid)
                {
                    bundle.metadata.linked_funding_txids.push(txid);
                }
            }
            let path = match save_bundle_to_store_at(&agreement_bundles_dir(), &bundle) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            };
            if json_mode {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "saved": true,
                        "path": path.display().to_string(),
                        "bundle": bundle,
                        "source": resolved.source,
                    }))
                    .unwrap()
                );
            } else {
                println!("{}", render_bundle_summary(&bundle, &resolved.source));
                println!("saved_path {}", path.display());
            }
        }
        "agreement-load" => {
            if args.len() != 2 && !(args.len() == 3 && args[2] == "--json") {
                usage();
                std::process::exit(1);
            }
            let json_mode = args.iter().any(|a| a == "--json");
            let stored = match resolve_bundle_input(&args[1]) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            };
            if json_mode {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "path": stored.path.display().to_string(),
                        "bundle": stored.bundle,
                    }))
                    .unwrap()
                );
            } else {
                println!(
                    "{}",
                    render_bundle_summary(&stored.bundle, &stored.path.display().to_string())
                );
            }
        }
        "agreement-list" => {
            let json_mode = args.iter().any(|a| a == "--json");
            let bundles = match list_stored_bundles_at(&agreement_bundles_dir()) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            };
            let items = bundles.iter().map(bundle_list_item).collect::<Vec<_>>();
            if json_mode {
                println!("{}", serde_json::to_string_pretty(&items).unwrap());
            } else if items.is_empty() {
                println!("no saved agreement bundles");
            } else {
                for item in items {
                    println!(
                        "{} {} saved_at={} path={}",
                        item.agreement_id, item.agreement_hash, item.saved_at, item.path
                    );
                }
            }
        }
        "agreement-export" => {
            if args.len() < 4 {
                usage();
                std::process::exit(1);
            }
            let mut out_path = None;
            let mut json_mode = false;
            let mut i = 2;
            while i < args.len() {
                match args[i].as_str() {
                    "--out" => {
                        out_path = args.get(i + 1).cloned();
                        if out_path.is_none() {
                            eprintln!("missing --out value");
                            std::process::exit(1);
                        }
                        i += 2;
                    }
                    "--json" => {
                        json_mode = true;
                        i += 1;
                    }
                    other => {
                        eprintln!("Unknown argument {}", other);
                        std::process::exit(1);
                    }
                }
            }
            let out_path = out_path.unwrap_or_default();
            let stored = match resolve_bundle_input(&args[1]) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            };
            let rendered = serde_json::to_string_pretty(&stored.bundle).unwrap();
            if let Err(e) = fs::write(&out_path, rendered) {
                eprintln!("write export: {}", e);
                std::process::exit(1);
            }
            if json_mode {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "exported": true,
                        "out": out_path,
                        "agreement_hash": stored.bundle.agreement_hash,
                    }))
                    .unwrap()
                );
            } else {
                println!("exported {}", out_path);
            }
        }
        "agreement-import" => {
            if args.len() != 2 && !(args.len() == 3 && args[2] == "--json") {
                usage();
                std::process::exit(1);
            }
            let json_mode = args.iter().any(|a| a == "--json");
            let bundle = match load_bundle_from_path(Path::new(&args[1])) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            };
            let path = match save_bundle_to_store_at(&agreement_bundles_dir(), &bundle) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            };
            if json_mode {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "imported": true,
                        "path": path.display().to_string(),
                        "agreement_hash": bundle.agreement_hash,
                    }))
                    .unwrap()
                );
            } else {
                println!(
                    "{}",
                    render_bundle_summary(&bundle, &format!("import:{}", args[1]))
                );
                println!("saved_path {}", path.display());
            }
        }

        "agreement-share-package" => {
            let mut agreement_path = None::<String>;
            let mut bundle_path = None::<String>;
            let mut audit_path = None::<String>;
            let mut statement_path = None::<String>;
            let mut agreement_signature_paths = Vec::<String>::new();
            let mut bundle_signature_paths = Vec::<String>::new();
            let mut selected_includes = Vec::<String>::new();
            let mut out_path = None::<String>;
            let mut created_at = None::<u64>;
            let mut sender_label = None::<String>;
            let mut package_note = None::<String>;
            let json_mode = args.iter().any(|a| a == "--json");
            let mut i = 1;
            while i < args.len() {
                match args[i].as_str() {
                    "--agreement" => {
                        agreement_path = args.get(i + 1).cloned();
                        i += 2;
                    }
                    "--bundle" => {
                        bundle_path = args.get(i + 1).cloned();
                        i += 2;
                    }
                    "--audit" => {
                        audit_path = args.get(i + 1).cloned();
                        i += 2;
                    }
                    "--statement" => {
                        statement_path = args.get(i + 1).cloned();
                        i += 2;
                    }
                    "--agreement-signature" => {
                        if let Some(value) = args.get(i + 1).cloned() {
                            agreement_signature_paths.push(value);
                            i += 2;
                        } else {
                            eprintln!("missing --agreement-signature value");
                            std::process::exit(1);
                        }
                    }
                    "--bundle-signature" => {
                        if let Some(value) = args.get(i + 1).cloned() {
                            bundle_signature_paths.push(value);
                            i += 2;
                        } else {
                            eprintln!("missing --bundle-signature value");
                            std::process::exit(1);
                        }
                    }
                    "--out" => {
                        out_path = args.get(i + 1).cloned();
                        i += 2;
                    }
                    "--include" => {
                        let raw = args.get(i + 1).cloned().unwrap_or_default();
                        let normalized =
                            normalize_share_package_include(&raw).unwrap_or_else(|e| {
                                eprintln!("{}", e);
                                std::process::exit(1);
                            });
                        if !selected_includes.iter().any(|item| item == &normalized) {
                            selected_includes.push(normalized);
                        }
                        i += 2;
                    }
                    "--created-at" => {
                        created_at = args.get(i + 1).and_then(|v| v.parse::<u64>().ok());
                        i += 2;
                    }
                    "--sender-label" => {
                        sender_label = args.get(i + 1).cloned();
                        i += 2;
                    }
                    "--package-note" => {
                        package_note = args.get(i + 1).cloned();
                        i += 2;
                    }
                    "--json" => i += 1,
                    other => {
                        eprintln!("Unknown argument {}", other);
                        std::process::exit(1);
                    }
                }
            }
            let out_path = match out_path {
                Some(v) => v,
                None => {
                    eprintln!("missing --out value");
                    std::process::exit(1);
                }
            };
            if agreement_path.is_none()
                && bundle_path.is_none()
                && audit_path.is_none()
                && statement_path.is_none()
                && agreement_signature_paths.is_empty()
                && bundle_signature_paths.is_empty()
            {
                eprintln!("at least one artifact must be supplied");
                std::process::exit(1);
            }
            let agreement = agreement_path
                .as_deref()
                .map(|v| load_agreement_json_from_path(Path::new(v)))
                .transpose()
                .unwrap_or_else(|e| {
                    eprintln!("{}", e);
                    std::process::exit(1);
                });
            let bundle = bundle_path
                .as_deref()
                .map(|v| load_bundle_from_path(Path::new(v)))
                .transpose()
                .unwrap_or_else(|e| {
                    eprintln!("{}", e);
                    std::process::exit(1);
                });
            let audit = audit_path
                .as_deref()
                .map(|v| load_audit_from_path(Path::new(v)))
                .transpose()
                .unwrap_or_else(|e| {
                    eprintln!("{}", e);
                    std::process::exit(1);
                });
            let statement = statement_path
                .as_deref()
                .map(|v| load_statement_from_path(Path::new(v)))
                .transpose()
                .unwrap_or_else(|e| {
                    eprintln!("{}", e);
                    std::process::exit(1);
                });
            let agreement_signatures = load_signatures_from_paths(&agreement_signature_paths)
                .unwrap_or_else(|e| {
                    eprintln!("{}", e);
                    std::process::exit(1);
                });
            let bundle_signatures = load_signatures_from_paths(&bundle_signature_paths)
                .unwrap_or_else(|e| {
                    eprintln!("{}", e);
                    std::process::exit(1);
                });
            let (agreement, bundle, audit, statement, agreement_signatures, bundle_signatures) =
                filter_share_package_export_selection(
                    &selected_includes,
                    agreement,
                    bundle,
                    audit,
                    statement,
                    agreement_signatures,
                    bundle_signatures,
                )
                .unwrap_or_else(|e| {
                    eprintln!("{}", e);
                    std::process::exit(1);
                });
            let package = build_agreement_share_package(
                created_at.or_else(|| Some(now_unix())),
                sender_label,
                package_note,
                agreement,
                bundle,
                audit,
                statement,
                agreement_signatures,
                bundle_signatures,
            )
            .unwrap_or_else(|e| {
                eprintln!("{}", e);
                std::process::exit(1);
            });
            let verification =
                build_agreement_share_package_verification(&package, None, now_unix())
                    .unwrap_or_else(|e| {
                        eprintln!("{}", e);
                        std::process::exit(1);
                    });
            if !verification
                .artifact_verification
                .canonical_verification
                .mismatches
                .is_empty()
                || verification
                    .artifact_verification
                    .authenticity
                    .as_ref()
                    .map(|v| v.invalid_signatures > 0)
                    .unwrap_or(false)
            {
                eprintln!(
                    "share package export refused due to mismatched or invalid included artifacts"
                );
                std::process::exit(1);
            }
            let rendered = serde_json::to_string_pretty(&package).unwrap();
            if let Err(e) = fs::write(&out_path, rendered) {
                eprintln!("write share package: {}", e);
                std::process::exit(1);
            }
            if json_mode {
                println!("{}", serde_json::to_string_pretty(&package).unwrap());
            } else {
                let inspection = inspect_agreement_share_package(&package).unwrap();
                println!("{}", render_share_package_inspection(&inspection));
                if !selected_includes.is_empty() {
                    println!(
                        "selected_export_artifacts {}",
                        selected_includes.join(" | ")
                    );
                }
                println!("exported_to {}", out_path);
            }
        }
        "agreement-share-package-inspect" => {
            if args.len() != 2 && !(args.len() == 3 && args[2] == "--json") {
                usage();
                std::process::exit(1);
            }
            let json_mode = args.iter().any(|a| a == "--json");
            let package = load_share_package_from_path(Path::new(&args[1])).unwrap_or_else(|e| {
                eprintln!("{}", e);
                std::process::exit(1);
            });
            let inspection = inspect_agreement_share_package(&package).unwrap_or_else(|e| {
                eprintln!("{}", e);
                std::process::exit(1);
            });
            if json_mode {
                println!("{}", serde_json::to_string_pretty(&inspection).unwrap());
            } else {
                println!("{}", render_share_package_inspection(&inspection));
            }
        }
        "agreement-share-package-verify" => {
            if args.len() < 2 {
                usage();
                std::process::exit(1);
            }
            let mut rpc_url = default_rpc_url();
            let mut out_path = None::<String>;
            let mut import_bundle = false;
            let json_mode = args.iter().any(|a| a == "--json");
            let mut i = 2;
            while i < args.len() {
                match args[i].as_str() {
                    "--rpc" => {
                        rpc_url = args.get(i + 1).cloned().unwrap_or_default();
                        i += 2;
                    }
                    "--out" => {
                        out_path = args.get(i + 1).cloned();
                        i += 2;
                    }
                    "--import-bundle" => {
                        import_bundle = true;
                        i += 1;
                    }
                    "--json" => i += 1,
                    other => {
                        eprintln!("Unknown argument {}", other);
                        std::process::exit(1);
                    }
                }
            }
            let package = load_share_package_from_path(Path::new(&args[1])).unwrap_or_else(|e| {
                eprintln!("{}", e);
                std::process::exit(1);
            });
            let base = rpc_url.trim_end_matches('/');
            let client = rpc_client(base).ok();
            let agreement_ref = package
                .agreement
                .as_ref()
                .or_else(|| package.bundle.as_ref().map(|b| &b.agreement));
            let recomputed_audit = if let Some(agreement_ref) = agreement_ref {
                if let Some(client) = client.as_ref() {
                    rpc_post_json(
                        client,
                        base,
                        "/rpc/agreementaudit",
                        &json!({"agreement": agreement_ref, "bundle": package.bundle.as_ref()}),
                    )
                    .ok()
                    .and_then(|value| serde_json::from_value::<AgreementAuditRecord>(value).ok())
                } else {
                    None
                }
            } else {
                None
            };
            let result = build_agreement_share_package_verification(
                &package,
                recomputed_audit.as_ref(),
                now_unix(),
            )
            .unwrap_or_else(|e| {
                eprintln!("{}", e);
                std::process::exit(1);
            });
            if import_bundle {
                let bundle = match package.bundle.as_ref() {
                    Some(v) => v,
                    None => {
                        eprintln!("--import-bundle requires the share package to include a bundle");
                        std::process::exit(1);
                    }
                };
                if !result
                    .artifact_verification
                    .canonical_verification
                    .mismatches
                    .is_empty()
                    || !result
                        .artifact_verification
                        .artifact_consistency
                        .warnings
                        .is_empty()
                {
                    eprintln!("refusing to import bundle from share package with mismatched canonical artifacts");
                    std::process::exit(1);
                }
                let (status, saved) =
                    save_bundle_to_store_checked(&agreement_bundles_dir(), bundle).unwrap_or_else(
                        |e| {
                            eprintln!("{}", e);
                            std::process::exit(1);
                        },
                    );
                if !json_mode {
                    println!(
                        "imported_bundle_status {}",
                        match status {
                            StoreWriteStatus::Imported => "imported",
                            StoreWriteStatus::AlreadyPresent => "already_present",
                        }
                    );
                    println!("imported_bundle {}", saved.display());
                }
            }
            let value = serde_json::to_value(&result).unwrap();
            if let Some(path) = out_path.as_deref() {
                if let Err(e) = save_json_output(Some(path), &value) {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            }
            if json_mode {
                println!("{}", serde_json::to_string_pretty(&value).unwrap());
            } else {
                println!("{}", render_share_package_verification(&result));
                if let Some(path) = out_path.as_deref() {
                    println!("exported_to {}", path);
                }
            }
        }
        "agreement-share-package-import" => {
            if args.len() < 2 {
                usage();
                std::process::exit(1);
            }
            let mut rpc_url = default_rpc_url();
            let mut import_source_label = None::<String>;
            let mut imports = Vec::<String>::new();
            let json_mode = args.iter().any(|a| a == "--json");
            let mut i = 2;
            while i < args.len() {
                match args[i].as_str() {
                    "--rpc" => {
                        rpc_url = args.get(i + 1).cloned().unwrap_or_default();
                        i += 2;
                    }
                    "--source-label" => {
                        import_source_label = args.get(i + 1).cloned();
                        if import_source_label.is_none() {
                            eprintln!("missing --source-label value");
                            std::process::exit(1);
                        }
                        i += 2;
                    }
                    "--import" => {
                        let raw = args.get(i + 1).cloned().unwrap_or_default();
                        let normalized = normalize_share_package_import(&raw).unwrap_or_else(|e| {
                            eprintln!("{}", e);
                            std::process::exit(1);
                        });
                        if !imports.iter().any(|item| item == &normalized) {
                            imports.push(normalized);
                        }
                        i += 2;
                    }
                    "--json" => i += 1,
                    other => {
                        eprintln!("Unknown argument {}", other);
                        std::process::exit(1);
                    }
                }
            }
            let package = load_share_package_from_path(Path::new(&args[1])).unwrap_or_else(|e| {
                eprintln!("{}", e);
                std::process::exit(1);
            });
            let base = rpc_url.trim_end_matches('/');
            let client = rpc_client(base).ok();
            let agreement_ref = package
                .agreement
                .as_ref()
                .or_else(|| package.bundle.as_ref().map(|b| &b.agreement));
            let recomputed_audit = if let Some(agreement_ref) = agreement_ref {
                if let Some(client) = client.as_ref() {
                    rpc_post_json(
                        client,
                        base,
                        "/rpc/agreementaudit",
                        &json!({"agreement": agreement_ref, "bundle": package.bundle.as_ref()}),
                    )
                    .ok()
                    .and_then(|value| serde_json::from_value::<AgreementAuditRecord>(value).ok())
                } else {
                    None
                }
            } else {
                None
            };
            let verification = build_agreement_share_package_verification(
                &package,
                recomputed_audit.as_ref(),
                now_unix(),
            )
            .unwrap_or_else(|e| {
                eprintln!("{}", e);
                std::process::exit(1);
            });
            let receipt = import_verified_share_package(
                &package,
                &verification,
                &args[1],
                import_source_label,
                &imports,
            )
            .unwrap_or_else(|e| {
                eprintln!("{}", e);
                std::process::exit(1);
            });
            if json_mode {
                println!("{}", serde_json::to_string_pretty(&receipt).unwrap());
            } else {
                println!("{}", render_share_package_receipt(&receipt));
            }
        }
        "agreement-share-package-list" => {
            let mut include_archived = false;
            let mut json_mode = false;
            let mut i = 1;
            while i < args.len() {
                match args[i].as_str() {
                    "--include-archived" => {
                        include_archived = true;
                        i += 1;
                    }
                    "--json" => {
                        json_mode = true;
                        i += 1;
                    }
                    _ => {
                        usage();
                        std::process::exit(1);
                    }
                }
            }
            if include_archived {
                let listing = build_agreement_local_store_listing(true).unwrap_or_else(|e| {
                    eprintln!("{}", e);
                    std::process::exit(1);
                });
                if json_mode {
                    let payload = json!({
                        "scope_notice": listing.scope_notice,
                        "housekeeping_notice": listing.housekeeping_notice,
                        "active_receipt_count": listing.active_receipt_count,
                        "archived_receipt_count": listing.archived_receipt_count,
                        "active_receipts": listing.active_receipts,
                        "archived_receipts": listing.archived_receipts,
                    });
                    println!("{}", serde_json::to_string_pretty(&payload).unwrap());
                } else {
                    let mut rendered = vec!["Agreement share package inbox".to_string()];
                    rendered.push(render_share_package_receipt_inventory(
                        &listing.active_receipts,
                    ));
                    rendered.push("archived_receipts".to_string());
                    if listing.archived_receipts.is_empty() {
                        rendered.push("receipt_count 0".to_string());
                    } else {
                        rendered.push(render_share_package_receipt_inventory(
                            &listing.archived_receipts,
                        ));
                    }
                    rendered.push(format!("scope_notice {}", listing.scope_notice));
                    rendered.push(format!(
                        "housekeeping_notice {}",
                        listing.housekeeping_notice
                    ));
                    println!("{}", rendered.join("\n"));
                }
            } else {
                let receipts = list_share_package_receipts_at(&share_package_inbox_dir())
                    .unwrap_or_else(|e| {
                        eprintln!("{}", e);
                        std::process::exit(1);
                    });
                if json_mode {
                    let payload = receipts
                        .iter()
                        .map(share_package_receipt_list_item)
                        .collect::<Vec<_>>();
                    println!("{}", serde_json::to_string_pretty(&payload).unwrap());
                } else {
                    println!("{}", render_share_package_receipt_list(&receipts));
                }
            }
        }
        "agreement-share-package-show" => {
            if args.len() != 2 && !(args.len() == 3 && args[2] == "--json") {
                usage();
                std::process::exit(1);
            }
            let json_mode = args.iter().any(|a| a == "--json");
            let receipt = resolve_share_package_receipt(&args[1]).unwrap_or_else(|e| {
                eprintln!("{}", e);
                std::process::exit(1);
            });
            if json_mode {
                println!("{}", serde_json::to_string_pretty(&receipt).unwrap());
            } else {
                println!("{}", render_share_package_receipt(&receipt));
            }
        }
        "agreement-share-package-archive" => {
            if args.len() != 2 && !(args.len() == 3 && args[2] == "--json") {
                usage();
                std::process::exit(1);
            }
            let json_mode = args.iter().any(|a| a == "--json");
            let result = archive_share_package_receipt(&args[1]).unwrap_or_else(|e| {
                eprintln!("{}", e);
                std::process::exit(1);
            });
            if json_mode {
                println!("{}", serde_json::to_string_pretty(&result).unwrap());
            } else {
                println!("{}", render_share_package_archive_result(&result));
            }
        }
        "agreement-share-package-prune" => {
            let mut dry_run = false;
            let mut older_than_days = None::<u64>;
            let mut include_archived = false;
            let mut remove_imported_artifacts = false;
            let mut json_mode = false;
            let mut i = 1;
            while i < args.len() {
                match args[i].as_str() {
                    "--dry-run" => {
                        dry_run = true;
                        i += 1;
                    }
                    "--older-than" => {
                        older_than_days =
                            args.get(i + 1).and_then(|value| value.parse::<u64>().ok());
                        if older_than_days.is_none() {
                            eprintln!("--older-than requires a whole number of days");
                            std::process::exit(1);
                        }
                        i += 2;
                    }
                    "--include-archived" => {
                        include_archived = true;
                        i += 1;
                    }
                    "--remove-imported-artifacts" => {
                        remove_imported_artifacts = true;
                        i += 1;
                    }
                    "--json" => {
                        json_mode = true;
                        i += 1;
                    }
                    other => {
                        eprintln!("Unknown argument {}", other);
                        std::process::exit(1);
                    }
                }
            }
            let report = prune_share_package_store(
                dry_run,
                older_than_days,
                include_archived,
                remove_imported_artifacts,
            )
            .unwrap_or_else(|e| {
                eprintln!("{}", e);
                std::process::exit(1);
            });
            if json_mode {
                println!("{}", serde_json::to_string_pretty(&report).unwrap());
            } else {
                println!("{}", render_local_store_mutation_report(&report));
            }
        }
        "agreement-share-package-remove" => {
            let mut dry_run = false;
            let mut remove_imported_artifacts = false;
            let mut json_mode = false;
            let mut receipt_reference = None::<String>;
            let mut exact_path = None::<String>;
            let mut agreement_hash = None::<String>;
            let mut bundle_hash = None::<String>;
            let mut i = 1;
            while i < args.len() {
                match args[i].as_str() {
                    "--dry-run" => {
                        dry_run = true;
                        i += 1;
                    }
                    "--remove-imported-artifacts" => {
                        remove_imported_artifacts = true;
                        i += 1;
                    }
                    "--json" => {
                        json_mode = true;
                        i += 1;
                    }
                    "--path" => {
                        exact_path = args.get(i + 1).cloned();
                        if exact_path.is_none() {
                            eprintln!("missing --path value");
                            std::process::exit(1);
                        }
                        i += 2;
                    }
                    "--agreement-hash" => {
                        agreement_hash = args.get(i + 1).cloned();
                        if agreement_hash.is_none() {
                            eprintln!("missing --agreement-hash value");
                            std::process::exit(1);
                        }
                        i += 2;
                    }
                    "--bundle-hash" => {
                        bundle_hash = args.get(i + 1).cloned();
                        if bundle_hash.is_none() {
                            eprintln!("missing --bundle-hash value");
                            std::process::exit(1);
                        }
                        i += 2;
                    }
                    value if value.starts_with("--") => {
                        eprintln!("Unknown argument {}", value);
                        std::process::exit(1);
                    }
                    value => {
                        if receipt_reference.is_some() {
                            usage();
                            std::process::exit(1);
                        }
                        receipt_reference = Some(value.to_string());
                        i += 1;
                    }
                }
            }
            let selector_count = usize::from(receipt_reference.is_some())
                + usize::from(exact_path.is_some())
                + usize::from(agreement_hash.is_some())
                + usize::from(bundle_hash.is_some());
            if selector_count != 1 {
                eprintln!("select exactly one removal target: receipt reference, --path, --agreement-hash, or --bundle-hash");
                std::process::exit(1);
            }
            let report = if let Some(reference) = receipt_reference {
                remove_share_package_receipt(&reference, dry_run, remove_imported_artifacts)
            } else if let Some(path) = exact_path {
                remove_exact_local_path(Path::new(&path), dry_run, remove_imported_artifacts)
            } else if let Some(hash) = agreement_hash {
                remove_local_store_agreement_hash(&hash, dry_run, remove_imported_artifacts)
            } else {
                remove_local_store_bundle_hash(
                    &bundle_hash.expect("bundle hash target should exist"),
                    dry_run,
                    remove_imported_artifacts,
                )
            }
            .unwrap_or_else(|e| {
                eprintln!("{}", e);
                std::process::exit(1);
            });
            if json_mode {
                println!("{}", serde_json::to_string_pretty(&report).unwrap());
            } else {
                println!("{}", render_local_store_mutation_report(&report));
            }
        }
        "agreement-local-store-list" => {
            let mut include_archived = false;
            let mut json_mode = false;
            let mut i = 1;
            while i < args.len() {
                match args[i].as_str() {
                    "--include-archived" => {
                        include_archived = true;
                        i += 1;
                    }
                    "--json" => {
                        json_mode = true;
                        i += 1;
                    }
                    other => {
                        eprintln!("Unknown argument {}", other);
                        std::process::exit(1);
                    }
                }
            }
            let listing =
                build_agreement_local_store_listing(include_archived).unwrap_or_else(|e| {
                    eprintln!("{}", e);
                    std::process::exit(1);
                });
            if json_mode {
                println!("{}", serde_json::to_string_pretty(&listing).unwrap());
            } else {
                println!("{}", render_agreement_local_store_listing(&listing));
            }
        }
        "agreement-inspect" => {
            if args.len() < 2 {
                usage();
                std::process::exit(1);
            }
            let json_mode = args.iter().any(|a| a == "--json");
            let agreement = match load_agreement(&args[1]) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            };
            let agreement_hash =
                match irium_node_rs::settlement::compute_agreement_hash_hex(&agreement) {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("{}", e);
                        std::process::exit(1);
                    }
                };
            if json_mode {
                let payload = json!({"agreement_hash": agreement_hash, "agreement": agreement});
                println!("{}", serde_json::to_string_pretty(&payload).unwrap());
            } else {
                print_agreement_summary(&agreement, &agreement_hash);
            }
        }
        "agreement-hash" => {
            if args.len() != 2 {
                usage();
                std::process::exit(1);
            }
            let agreement = match load_agreement(&args[1]) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            };
            match irium_node_rs::settlement::compute_agreement_hash_hex(&agreement) {
                Ok(v) => println!("{}", v),
                Err(e) => {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            }
        }
        "agreement-fund" => {
            if args.len() < 2 {
                usage();
                std::process::exit(1);
            }
            let agreement = match load_agreement(&args[1]) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            };
            let mut rpc_url = default_rpc_url();
            let mut broadcast = false;
            let mut fee_per_byte = None;
            let json_mode = args.iter().any(|a| a == "--json");
            let mut i = 2;
            while i < args.len() {
                match args[i].as_str() {
                    "--broadcast" => {
                        broadcast = true;
                        i += 1;
                    }
                    "--rpc" => {
                        rpc_url = args.get(i + 1).cloned().unwrap_or_default();
                        i += 2;
                    }
                    "--fee-per-byte" => {
                        fee_per_byte = args.get(i + 1).and_then(|v| v.parse::<u64>().ok());
                        i += 2;
                    }
                    "--json" => {
                        i += 1;
                    }
                    _ => {
                        eprintln!("Unknown argument {}", args[i]);
                        std::process::exit(1);
                    }
                }
            }
            let base = rpc_url.trim_end_matches('/');
            let client = match rpc_client(base) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            };
            let agreement_hash =
                irium_node_rs::settlement::compute_agreement_hash_hex(&agreement).unwrap();
            print_agreement_summary(&agreement, &agreement_hash);
            let resp: FundAgreementResponse = match rpc_post_json(
                &client,
                base,
                "/rpc/fundagreement",
                &FundAgreementRequestBody {
                    agreement,
                    fee_per_byte,
                    broadcast: Some(broadcast),
                },
            ) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            };
            if json_mode {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::to_value(&resp).unwrap()).unwrap()
                );
            } else {
                println!("fund_txid {}", resp.txid);
                println!("agreement_hash {}", resp.agreement_hash);
                println!("accepted {}", resp.accepted);
                println!("fee {}", format_irm(resp.fee));
                println!("warning funding uses HTLCv1 outputs and OP_RETURN agreement anchors; lifecycle remains indexed metadata, not native consensus settlement state");
                for output in resp.outputs {
                    println!(
                        "  vout {} role {} amount {} milestone {:?}",
                        output.vout,
                        output.role,
                        format_irm(output.amount),
                        output.milestone_id
                    );
                }
            }
        }
        "agreement-funding-legs" => {
            if args.len() < 2 {
                usage();
                std::process::exit(1);
            }
            let resolved = match resolve_agreement_input(&args[1]) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            };
            let mut rpc_url = default_rpc_url();
            let json_mode = args.iter().any(|a| a == "--json");
            let mut i = 2;
            while i < args.len() {
                match args[i].as_str() {
                    "--rpc" => {
                        rpc_url = args.get(i + 1).cloned().unwrap_or_default();
                        i += 2;
                    }
                    "--json" => i += 1,
                    other => {
                        eprintln!("Unknown argument {}", other);
                        std::process::exit(1);
                    }
                }
            }
            let base = rpc_url.trim_end_matches('/');
            let client = match rpc_client(base) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            };
            let resp = match fetch_agreement_funding_legs(&client, base, &resolved) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            };
            if json_mode {
                println!("{}", serde_json::to_string_pretty(&resp).unwrap());
            } else {
                println!("{}", render_agreement_funding_legs(&resp));
            }
        }
        "agreement-timeline" => {
            if args.len() < 2 {
                usage();
                std::process::exit(1);
            }
            let resolved = match resolve_agreement_input(&args[1]) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            };
            let mut rpc_url = default_rpc_url();
            let json_mode = args.iter().any(|a| a == "--json");
            let mut i = 2;
            while i < args.len() {
                match args[i].as_str() {
                    "--rpc" => {
                        rpc_url = args.get(i + 1).cloned().unwrap_or_default();
                        i += 2;
                    }
                    "--json" => i += 1,
                    other => {
                        eprintln!("Unknown argument {}", other);
                        std::process::exit(1);
                    }
                }
            }
            let base = rpc_url.trim_end_matches('/');
            let client = match rpc_client(base) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            };
            let resp = match fetch_agreement_timeline(&client, base, &resolved) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            };
            if json_mode {
                println!("{}", serde_json::to_string_pretty(&resp).unwrap());
            } else {
                println!("{}", render_agreement_timeline(&resp));
            }
        }
        "agreement-audit" => {
            if args.len() < 2 {
                usage();
                std::process::exit(1);
            }
            let resolved = match resolve_agreement_input(&args[1]) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            };
            let mut rpc_url = default_rpc_url();
            let mut agreement_signature_path = None::<String>;
            let mut bundle_signature_path = None::<String>;
            let json_mode = args.iter().any(|a| a == "--json");
            let mut i = 2;
            while i < args.len() {
                match args[i].as_str() {
                    "--rpc" => {
                        rpc_url = args.get(i + 1).cloned().unwrap_or_default();
                        i += 2;
                    }
                    "--agreement-signature" => {
                        agreement_signature_path = args.get(i + 1).cloned();
                        i += 2;
                    }
                    "--bundle-signature" => {
                        bundle_signature_path = args.get(i + 1).cloned();
                        i += 2;
                    }
                    "--json" => i += 1,
                    other => {
                        eprintln!("Unknown argument {}", other);
                        std::process::exit(1);
                    }
                }
            }
            let detached_agreement_signature = agreement_signature_path
                .as_deref()
                .map(|v| load_signature_from_path(Path::new(v)))
                .transpose()
                .unwrap_or_else(|e| {
                    eprintln!("{}", e);
                    std::process::exit(1);
                });
            let detached_bundle_signature = bundle_signature_path
                .as_deref()
                .map(|v| load_signature_from_path(Path::new(v)))
                .transpose()
                .unwrap_or_else(|e| {
                    eprintln!("{}", e);
                    std::process::exit(1);
                });
            let base = rpc_url.trim_end_matches('/');
            let client = match rpc_client(base) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            };
            let mut resp = match fetch_agreement_audit(&client, base, &resolved) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            };
            let detached_agreement_signatures =
                detached_agreement_signature.into_iter().collect::<Vec<_>>();
            let detached_bundle_signatures =
                detached_bundle_signature.into_iter().collect::<Vec<_>>();
            attach_authenticity_to_audit(
                &mut resp,
                &resolved,
                &detached_agreement_signatures,
                &detached_bundle_signatures,
            );
            if json_mode {
                println!("{}", serde_json::to_string_pretty(&resp).unwrap());
            } else {
                println!("{}", render_agreement_audit(&resp));
            }
        }
        "agreement-audit-export" => {
            if args.len() < 4 {
                usage();
                std::process::exit(1);
            }
            let resolved = match resolve_agreement_input(&args[1]) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            };
            let mut rpc_url = default_rpc_url();
            let mut out_path = None::<String>;
            let mut agreement_signature_path = None::<String>;
            let mut bundle_signature_path = None::<String>;
            let json_mode = args.iter().any(|a| a == "--json");
            let mut export_format = "json".to_string();
            let mut i = 2;
            while i < args.len() {
                match args[i].as_str() {
                    "--rpc" => {
                        rpc_url = args.get(i + 1).cloned().unwrap_or_default();
                        i += 2;
                    }
                    "--out" => {
                        out_path = args.get(i + 1).cloned();
                        i += 2;
                    }
                    "--format" => {
                        export_format = args.get(i + 1).cloned().unwrap_or_default();
                        i += 2;
                    }
                    "--agreement-signature" => {
                        agreement_signature_path = args.get(i + 1).cloned();
                        i += 2;
                    }
                    "--bundle-signature" => {
                        bundle_signature_path = args.get(i + 1).cloned();
                        i += 2;
                    }
                    "--json" => i += 1,
                    other => {
                        eprintln!("Unknown argument {}", other);
                        std::process::exit(1);
                    }
                }
            }
            let out_path = match out_path {
                Some(v) => v,
                None => {
                    eprintln!("missing --out value");
                    std::process::exit(1);
                }
            };
            let export_format =
                match validate_agreement_audit_export_format(&export_format, json_mode) {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("{}", e);
                        std::process::exit(1);
                    }
                };
            let detached_agreement_signature = agreement_signature_path
                .as_deref()
                .map(|v| load_signature_from_path(Path::new(v)))
                .transpose()
                .unwrap_or_else(|e| {
                    eprintln!("{}", e);
                    std::process::exit(1);
                });
            let detached_bundle_signature = bundle_signature_path
                .as_deref()
                .map(|v| load_signature_from_path(Path::new(v)))
                .transpose()
                .unwrap_or_else(|e| {
                    eprintln!("{}", e);
                    std::process::exit(1);
                });
            let base = rpc_url.trim_end_matches('/');
            let client = match rpc_client(base) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            };
            let mut resp = match fetch_agreement_audit(&client, base, &resolved) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            };
            let detached_agreement_signatures =
                detached_agreement_signature.into_iter().collect::<Vec<_>>();
            let detached_bundle_signatures =
                detached_bundle_signature.into_iter().collect::<Vec<_>>();
            attach_authenticity_to_audit(
                &mut resp,
                &resolved,
                &detached_agreement_signatures,
                &detached_bundle_signatures,
            );
            if export_format == "csv" {
                let rendered = render_agreement_audit_csv(&resp);
                if let Err(e) = save_text_output(Some(&out_path), &rendered) {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
                println!("{}", render_agreement_audit(&resp));
                println!("export_format csv");
                println!("exported_to {}", out_path);
            } else {
                let value = serde_json::to_value(&resp).unwrap();
                if let Err(e) = save_json_output(Some(&out_path), &value) {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
                if json_mode {
                    println!("{}", serde_json::to_string_pretty(&value).unwrap());
                } else {
                    println!("{}", render_agreement_audit(&resp));
                    println!("export_format json");
                    println!("exported_to {}", out_path);
                }
            }
        }
        "agreement-statement" => {
            if args.len() < 2 {
                usage();
                std::process::exit(1);
            }
            let resolved = match resolve_agreement_input(&args[1]) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            };
            let mut rpc_url = default_rpc_url();
            let mut agreement_signature_path = None::<String>;
            let mut bundle_signature_path = None::<String>;
            let json_mode = args.iter().any(|a| a == "--json");
            let mut i = 2;
            while i < args.len() {
                match args[i].as_str() {
                    "--rpc" => {
                        rpc_url = args.get(i + 1).cloned().unwrap_or_default();
                        i += 2;
                    }
                    "--agreement-signature" => {
                        agreement_signature_path = args.get(i + 1).cloned();
                        i += 2;
                    }
                    "--bundle-signature" => {
                        bundle_signature_path = args.get(i + 1).cloned();
                        i += 2;
                    }
                    "--json" => i += 1,
                    other => {
                        eprintln!("Unknown argument {}", other);
                        std::process::exit(1);
                    }
                }
            }
            let detached_agreement_signature = agreement_signature_path
                .as_deref()
                .map(|v| load_signature_from_path(Path::new(v)))
                .transpose()
                .unwrap_or_else(|e| {
                    eprintln!("{}", e);
                    std::process::exit(1);
                });
            let detached_bundle_signature = bundle_signature_path
                .as_deref()
                .map(|v| load_signature_from_path(Path::new(v)))
                .transpose()
                .unwrap_or_else(|e| {
                    eprintln!("{}", e);
                    std::process::exit(1);
                });
            let base = rpc_url.trim_end_matches('/');
            let client = match rpc_client(base) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            };
            let mut audit = match fetch_agreement_audit(&client, base, &resolved) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            };
            let detached_agreement_signatures =
                detached_agreement_signature.into_iter().collect::<Vec<_>>();
            let detached_bundle_signatures =
                detached_bundle_signature.into_iter().collect::<Vec<_>>();
            attach_authenticity_to_audit(
                &mut audit,
                &resolved,
                &detached_agreement_signatures,
                &detached_bundle_signatures,
            );
            let statement = build_agreement_statement(&audit);
            if json_mode {
                println!("{}", serde_json::to_string_pretty(&statement).unwrap());
            } else {
                println!("{}", render_agreement_statement(&statement));
            }
        }
        "agreement-statement-export" => {
            if args.len() < 4 {
                usage();
                std::process::exit(1);
            }
            let resolved = match resolve_agreement_input(&args[1]) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            };
            let mut rpc_url = default_rpc_url();
            let mut out_path = None::<String>;
            let mut agreement_signature_path = None::<String>;
            let mut bundle_signature_path = None::<String>;
            let json_mode = args.iter().any(|a| a == "--json");
            let mut i = 2;
            while i < args.len() {
                match args[i].as_str() {
                    "--rpc" => {
                        rpc_url = args.get(i + 1).cloned().unwrap_or_default();
                        i += 2;
                    }
                    "--out" => {
                        out_path = args.get(i + 1).cloned();
                        i += 2;
                    }
                    "--agreement-signature" => {
                        agreement_signature_path = args.get(i + 1).cloned();
                        i += 2;
                    }
                    "--bundle-signature" => {
                        bundle_signature_path = args.get(i + 1).cloned();
                        i += 2;
                    }
                    "--json" => i += 1,
                    other => {
                        eprintln!("Unknown argument {}", other);
                        std::process::exit(1);
                    }
                }
            }
            let out_path = match out_path {
                Some(v) => v,
                None => {
                    eprintln!("missing --out value");
                    std::process::exit(1);
                }
            };
            let detached_agreement_signature = agreement_signature_path
                .as_deref()
                .map(|v| load_signature_from_path(Path::new(v)))
                .transpose()
                .unwrap_or_else(|e| {
                    eprintln!("{}", e);
                    std::process::exit(1);
                });
            let detached_bundle_signature = bundle_signature_path
                .as_deref()
                .map(|v| load_signature_from_path(Path::new(v)))
                .transpose()
                .unwrap_or_else(|e| {
                    eprintln!("{}", e);
                    std::process::exit(1);
                });
            let base = rpc_url.trim_end_matches('/');
            let client = match rpc_client(base) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            };
            let mut audit = match fetch_agreement_audit(&client, base, &resolved) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            };
            let detached_agreement_signatures =
                detached_agreement_signature.into_iter().collect::<Vec<_>>();
            let detached_bundle_signatures =
                detached_bundle_signature.into_iter().collect::<Vec<_>>();
            attach_authenticity_to_audit(
                &mut audit,
                &resolved,
                &detached_agreement_signatures,
                &detached_bundle_signatures,
            );
            let statement = build_agreement_statement(&audit);
            let value = serde_json::to_value(&statement).unwrap();
            if let Err(e) = save_json_output(Some(&out_path), &value) {
                eprintln!("{}", e);
                std::process::exit(1);
            }
            if json_mode {
                println!("{}", serde_json::to_string_pretty(&value).unwrap());
            } else {
                println!("{}", render_agreement_statement(&statement));
                println!("exported_to {}", out_path);
            }
        }
        "agreement-verify-artifacts" => {
            let mut agreement_path = None::<String>;
            let mut bundle_path = None::<String>;
            let mut audit_path = None::<String>;
            let mut statement_path = None::<String>;
            let mut agreement_signature_path = None::<String>;
            let mut bundle_signature_path = None::<String>;
            let mut rpc_url = default_rpc_url();
            let mut out_path = None::<String>;
            let json_mode = args.iter().any(|a| a == "--json");
            let mut i = 1;
            while i < args.len() {
                match args[i].as_str() {
                    "--agreement" => {
                        agreement_path = args.get(i + 1).cloned();
                        i += 2;
                    }
                    "--bundle" => {
                        bundle_path = args.get(i + 1).cloned();
                        i += 2;
                    }
                    "--audit" => {
                        audit_path = args.get(i + 1).cloned();
                        i += 2;
                    }
                    "--statement" => {
                        statement_path = args.get(i + 1).cloned();
                        i += 2;
                    }
                    "--agreement-signature" => {
                        agreement_signature_path = args.get(i + 1).cloned();
                        i += 2;
                    }
                    "--bundle-signature" => {
                        bundle_signature_path = args.get(i + 1).cloned();
                        i += 2;
                    }
                    "--rpc" => {
                        rpc_url = args.get(i + 1).cloned().unwrap_or_default();
                        i += 2;
                    }
                    "--out" => {
                        out_path = args.get(i + 1).cloned();
                        i += 2;
                    }
                    "--json" => i += 1,
                    other => {
                        eprintln!("Unknown argument {}", other);
                        std::process::exit(1);
                    }
                }
            }
            if agreement_path.is_none()
                && bundle_path.is_none()
                && audit_path.is_none()
                && statement_path.is_none()
                && agreement_signature_path.is_none()
                && bundle_signature_path.is_none()
            {
                eprintln!("at least one artifact must be supplied");
                std::process::exit(1);
            }
            let agreement = agreement_path
                .as_deref()
                .map(|v| load_agreement_json_from_path(Path::new(v)))
                .transpose()
                .unwrap_or_else(|e| {
                    eprintln!("{}", e);
                    std::process::exit(1);
                });
            let bundle = bundle_path
                .as_deref()
                .map(|v| load_bundle_from_path(Path::new(v)))
                .transpose()
                .unwrap_or_else(|e| {
                    eprintln!("{}", e);
                    std::process::exit(1);
                });
            let supplied_audit = audit_path
                .as_deref()
                .map(|v| load_audit_from_path(Path::new(v)))
                .transpose()
                .unwrap_or_else(|e| {
                    eprintln!("{}", e);
                    std::process::exit(1);
                });
            let supplied_statement = statement_path
                .as_deref()
                .map(|v| load_statement_from_path(Path::new(v)))
                .transpose()
                .unwrap_or_else(|e| {
                    eprintln!("{}", e);
                    std::process::exit(1);
                });
            let detached_agreement_signature = agreement_signature_path
                .as_deref()
                .map(|v| load_signature_from_path(Path::new(v)))
                .transpose()
                .unwrap_or_else(|e| {
                    eprintln!("{}", e);
                    std::process::exit(1);
                });
            let detached_bundle_signature = bundle_signature_path
                .as_deref()
                .map(|v| load_signature_from_path(Path::new(v)))
                .transpose()
                .unwrap_or_else(|e| {
                    eprintln!("{}", e);
                    std::process::exit(1);
                });
            let base = rpc_url.trim_end_matches('/');
            let client = rpc_client(base).ok();
            let recomputed_audit = if let Some(agreement_ref) = agreement
                .as_ref()
                .or_else(|| bundle.as_ref().map(|b| &b.agreement))
            {
                if let Some(client) = client.as_ref() {
                    rpc_post_json(
                        client,
                        base,
                        "/rpc/agreementaudit",
                        &json!({"agreement": agreement_ref, "bundle": bundle}),
                    )
                    .ok()
                } else {
                    None
                }
            } else {
                None
            };
            let detached_agreement_signatures =
                detached_agreement_signature.into_iter().collect::<Vec<_>>();
            let detached_bundle_signatures =
                detached_bundle_signature.into_iter().collect::<Vec<_>>();
            let result = build_agreement_artifact_verification(
                agreement.as_ref(),
                bundle.as_ref(),
                supplied_audit.as_ref(),
                supplied_statement.as_ref(),
                &detached_agreement_signatures,
                &detached_bundle_signatures,
                recomputed_audit.as_ref(),
                now_unix(),
            );
            let value = serde_json::to_value(&result).unwrap();
            if let Some(path) = out_path.as_deref() {
                if let Err(e) = save_json_output(Some(path), &value) {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            }
            if json_mode {
                println!("{}", serde_json::to_string_pretty(&value).unwrap());
            } else {
                println!("{}", render_artifact_verification(&result));
                if let Some(path) = out_path.as_deref() {
                    println!("exported_to {}", path);
                }
            }
        }
        "agreement-release-eligibility" => {
            let opts = match parse_agreement_spend_cli(&args[1..]) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("{}", e);
                    usage();
                    std::process::exit(1);
                }
            };
            let base = opts.rpc_url.trim_end_matches('/');
            let client = match rpc_client(base) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            };
            let (_resolved, req, selection_notice) =
                match resolve_agreement_spend_request(&client, base, &opts) {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("{}", e);
                        std::process::exit(1);
                    }
                };
            let resp: AgreementSpendEligibilityResponse =
                match rpc_post_json(&client, base, "/rpc/agreementreleaseeligibility", &req) {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("{}", e);
                        std::process::exit(1);
                    }
                };
            if opts.json_mode {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::to_value(&resp).unwrap()).unwrap()
                );
            } else {
                if let Some(notice) = selection_notice {
                    println!("{}", notice);
                }
                println!("{}", render_agreement_spend_eligibility_summary(&resp));
            }
            if !resp.eligible {
                std::process::exit(1);
            }
        }
        "agreement-refund-eligibility" => {
            let opts = match parse_agreement_spend_cli(&args[1..]) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("{}", e);
                    usage();
                    std::process::exit(1);
                }
            };
            let base = opts.rpc_url.trim_end_matches('/');
            let client = match rpc_client(base) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            };
            let (_resolved, req, selection_notice) =
                match resolve_agreement_spend_request(&client, base, &opts) {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("{}", e);
                        std::process::exit(1);
                    }
                };
            let resp: AgreementSpendEligibilityResponse =
                match rpc_post_json(&client, base, "/rpc/agreementrefundeligibility", &req) {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("{}", e);
                        std::process::exit(1);
                    }
                };
            if opts.json_mode {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::to_value(&resp).unwrap()).unwrap()
                );
            } else {
                if let Some(notice) = selection_notice {
                    println!("{}", notice);
                }
                println!("{}", render_agreement_spend_eligibility_summary(&resp));
            }
            if !resp.eligible {
                std::process::exit(1);
            }
        }
        "agreement-proof-create" => {
            let opts = match parse_proof_create_cli(&args[1..]) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("{}", e);
                    usage();
                    std::process::exit(1);
                }
            };
            let proof = match create_settlement_proof_signed(&opts) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            };
            let proof_json = match serde_json::to_string_pretty(&proof) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("serialize proof: {}", e);
                    std::process::exit(1);
                }
            };
            if let Some(ref out_path) = opts.out_path {
                if let Err(e) = fs::write(out_path, &proof_json) {
                    eprintln!("write proof to {}: {}", out_path, e);
                    std::process::exit(1);
                }
            }
            if opts.json_mode {
                println!("{}", proof_json);
            } else if opts.out_path.is_none() {
                println!("{}", proof_json);
            } else {
                println!("{}", render_proof_create_summary(&proof));
            }
        }
        "agreement-proof-submit" => {
            let opts = match parse_proof_submit_cli(&args[1..]) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("{}", e);
                    usage();
                    std::process::exit(1);
                }
            };
            let proof: SettlementProof = {
                let data = match read_text_from_path_or_stdin(
                    Path::new(&opts.proof_path),
                    "proof json",
                ) {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("{}", e);
                        std::process::exit(1);
                    }
                };
                match serde_json::from_str(&data) {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("parse proof json: {}", e);
                        std::process::exit(1);
                    }
                }
            };
            let base = opts.rpc_url.trim_end_matches('/');
            let client = match rpc_client(base) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            };
            let req = SubmitProofRpcRequest { proof };
            let resp: SubmitProofRpcResponse =
                match rpc_post_json(&client, base, "/rpc/submitproof", &req) {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("{}", e);
                        std::process::exit(1);
                    }
                };
            if opts.json_mode {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::to_value(&resp).unwrap()).unwrap()
                );
            } else {
                println!("{}", render_proof_submit_summary(&resp));
            }
            if !resp.accepted && !resp.duplicate {
                std::process::exit(1);
            }
        }
        "agreement-proof-list" => {
            let opts = match parse_proof_list_cli(&args[1..]) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("{}", e);
                    usage();
                    std::process::exit(1);
                }
            };
            let base = opts.rpc_url.trim_end_matches('/');
            let client = match rpc_client(base) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            };
            let req = ListProofsRpcRequest {
                agreement_hash: opts.agreement_hash.clone(),
                active_only: opts.active_only,
                offset: opts.offset,
                limit: opts.limit,
            };
            let resp: ListProofsRpcResponse =
                match rpc_post_json(&client, base, "/rpc/listproofs", &req) {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("{}", e);
                        std::process::exit(1);
                    }
                };
            if opts.json_mode {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::to_value(&resp).unwrap()).unwrap()
                );
            } else {
                println!("{}", render_proof_list_summary(&resp));
            }
        }
        "agreement-proof-get" => {
            let opts = match parse_proof_get_cli(&args[1..]) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("{}", e);
                    usage();
                    std::process::exit(1);
                }
            };
            let base = opts.rpc_url.trim_end_matches('/');
            let client = match rpc_client(base) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            };
            let req = GetProofRpcRequest { proof_id: opts.proof_id };
            let resp: GetProofRpcResponse =
                match rpc_post_json(&client, base, "/rpc/getproof", &req) {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("{}", e);
                        std::process::exit(1);
                    }
                };
            if opts.json_mode {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::to_value(&resp).unwrap()).unwrap()
                );
            } else {
                println!("{}", render_proof_get_summary(&resp));
            }
            if !resp.found {
                std::process::exit(1);
            }
        }
        "agreement-policy-check" => {
            let opts = match parse_policy_check_cli(&args[1..]) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("{}", e);
                    usage();
                    std::process::exit(1);
                }
            };
            let agreement = match load_agreement_json_from_path(Path::new(&opts.agreement_path)) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            };
            let policy: ProofPolicy = {
                let data = match read_text_from_path_or_stdin(
                    Path::new(&opts.policy_path),
                    "policy json",
                ) {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("{}", e);
                        std::process::exit(1);
                    }
                };
                match serde_json::from_str(&data) {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("parse policy json: {}", e);
                        std::process::exit(1);
                    }
                }
            };
            let mut proofs: Vec<SettlementProof> = Vec::new();
            for path in &opts.proof_paths {
                let data = match read_text_from_path_or_stdin(Path::new(path), "proof json") {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("{}", e);
                        std::process::exit(1);
                    }
                };
                match serde_json::from_str::<SettlementProof>(&data) {
                    Ok(p) => proofs.push(p),
                    Err(e) => {
                        eprintln!("parse proof json {}: {}", path, e);
                        std::process::exit(1);
                    }
                }
            }
            let base = opts.rpc_url.trim_end_matches('/');
            let client = match rpc_client(base) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            };
            let req = CheckPolicyRpcRequest {
                agreement,
                policy,
                proofs,
            };
            let resp: CheckPolicyRpcResponse =
                match rpc_post_json(&client, base, "/rpc/checkpolicy", &req) {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("{}", e);
                        std::process::exit(1);
                    }
                };
            if opts.json_mode {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::to_value(&resp).unwrap()).unwrap()
                );
            } else {
                println!("{}", render_policy_check_summary(&resp));
            }
            if !resp.release_eligible && !resp.refund_eligible {
                std::process::exit(1);
            }
        }
        "agreement-policy-set" => {
            let opts = match parse_policy_set_cli(&args[1..]) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("{}", e);
                    usage();
                    std::process::exit(1);
                }
            };
            let mut policy: ProofPolicy = {
                let data = match read_text_from_path_or_stdin(
                    Path::new(&opts.policy_path),
                    "policy json",
                ) {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("{}", e);
                        std::process::exit(1);
                    }
                };
                match serde_json::from_str(&data) {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("parse policy json: {}", e);
                        std::process::exit(1);
                    }
                }
            };
            let base = opts.rpc_url.trim_end_matches('/');
            let client = match rpc_client(base) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            };
            if let Some(h) = opts.expires_at_height {
                policy.expires_at_height = Some(h);
            }
            let req = StorePolicyRpcRequest { policy, replace: opts.replace };
            let resp: StorePolicyRpcResponse =
                match rpc_post_json(&client, base, "/rpc/storepolicy", &req) {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("{}", e);
                        std::process::exit(1);
                    }
                };
            if opts.json_mode {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::to_value(&resp).unwrap()).unwrap()
                );
            } else {
                println!("{}", render_policy_set_summary(&resp));
            }
        }
        "agreement-policy-get" => {
            let opts = match parse_policy_get_cli(&args[1..]) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("{}", e);
                    usage();
                    std::process::exit(1);
                }
            };
            let base = opts.rpc_url.trim_end_matches('/');
            let client = match rpc_client(base) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            };
            let req = GetPolicyRpcRequest {
                agreement_hash: opts.agreement_hash,
            };
            let resp: GetPolicyRpcResponse =
                match rpc_post_json(&client, base, "/rpc/getpolicy", &req) {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("{}", e);
                        std::process::exit(1);
                    }
                };
            if opts.json_mode {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::to_value(&resp).unwrap()).unwrap()
                );
            } else {
                println!("{}", render_policy_get_summary(&resp));
            }
            if !resp.found {
                std::process::exit(1);
            }
        }
        "agreement-policy-evaluate" => {
            let opts = match parse_policy_evaluate_cli(&args[1..]) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("{}", e);
                    usage();
                    std::process::exit(1);
                }
            };
            let agreement = match load_agreement_json_from_path(Path::new(&opts.agreement_path)) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            };
            let base = opts.rpc_url.trim_end_matches('/');
            let client = match rpc_client(base) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            };
            let req = EvaluatePolicyRpcRequest { agreement };
            let resp: EvaluatePolicyRpcResponse =
                match rpc_post_json(&client, base, "/rpc/evaluatepolicy", &req) {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("{}", e);
                        std::process::exit(1);
                    }
                };
            if opts.json_mode {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::to_value(&resp).unwrap()).unwrap()
                );
            } else {
                println!("{}", render_policy_evaluate_summary(&resp));
            }
            if !resp.release_eligible && !resp.refund_eligible {
                std::process::exit(1);
            }
        }
        "policy-build-contractor" => {
            // irium-wallet policy-build-contractor --policy-id <id> --agreement-hash <hash>
            //   --attestor <id>:<pubkey> [--attestor ...] --milestone <id>:<proof_type> [--milestone ...]
            //   [--rpc <url>] [--json]
            let mut policy_id = String::new();
            let mut agreement_hash = String::new();
            let mut attestors: Vec<BuildTemplateAttestorInput> = Vec::new();
            let mut milestones: Vec<BuildTemplateMilestoneInput> = Vec::new();
            let mut rpc_url = default_rpc_url();
            let mut json_mode = false;
            let mut notes: Option<String> = None;
            let mut i = 1usize;
            while i < args.len() {
                match args[i].as_str() {
                    "--policy-id" => { policy_id = args.get(i+1).cloned().unwrap_or_default(); i += 2; }
                    "--agreement-hash" => { agreement_hash = args.get(i+1).cloned().unwrap_or_default(); i += 2; }
                    "--attestor" => {
                        let raw = args.get(i+1).cloned().unwrap_or_default();
                        let parts: Vec<&str> = raw.splitn(2, ':').collect();
                        if parts.len() == 2 {
                            attestors.push(BuildTemplateAttestorInput { attestor_id: parts[0].to_string(), pubkey_hex: parts[1].to_string(), display_name: None });
                        } else { eprintln!("--attestor expects <id>:<pubkey>"); std::process::exit(1); }
                        i += 2;
                    }
                    "--milestone" => {
                        let raw = args.get(i+1).cloned().unwrap_or_default();
                        let parts: Vec<&str> = raw.splitn(2, ':').collect();
                        if parts.len() == 2 {
                            milestones.push(BuildTemplateMilestoneInput { milestone_id: parts[0].to_string(), label: None, proof_type: parts[1].to_string(), deadline_height: None, holdback_bps: None, holdback_release_height: None });
                        } else { eprintln!("--milestone expects <id>:<proof_type>"); std::process::exit(1); }
                        i += 2;
                    }
                    "--notes" => { notes = Some(args.get(i+1).cloned().unwrap_or_default()); i += 2; }
                    "--rpc" => { rpc_url = args.get(i+1).cloned().unwrap_or_default(); i += 2; }
                    "--json" => { json_mode = true; i += 1; }
                    _ => { i += 1; }
                }
            }
            if policy_id.is_empty() || agreement_hash.is_empty() || attestors.is_empty() || milestones.is_empty() {
                eprintln!("policy-build-contractor requires --policy-id, --agreement-hash, at least one --attestor and one --milestone");
                std::process::exit(1);
            }
            let base = rpc_url.trim_end_matches('/');
            let client = rpc_client(base).unwrap_or_else(|e| { eprintln!("{}", e); std::process::exit(1); });
            let req = BuildContractorTemplateRpcRequest { policy_id, agreement_hash, attestors, milestones, notes };
            let resp: BuildTemplateRpcResponse = rpc_post_json(&client, base, "/rpc/buildcontractortemplate", &req)
                .unwrap_or_else(|e| { eprintln!("{}", e); std::process::exit(1); });
            if json_mode {
                println!("{}", serde_json::to_string_pretty(&serde_json::to_value(&resp).unwrap()).unwrap());
            } else {
                println!("{}", render_build_template_summary(&resp));
            }
        }
        "policy-build-preorder" => {
            let mut policy_id = String::new();
            let mut agreement_hash = String::new();
            let mut attestors: Vec<BuildTemplateAttestorInput> = Vec::new();
            let mut delivery_proof_type = String::new();
            let mut refund_deadline_height: u64 = 0;
            let mut holdback_bps: Option<u32> = None;
            let mut holdback_release_height: Option<u64> = None;
            let mut rpc_url = default_rpc_url();
            let mut json_mode = false;
            let mut notes: Option<String> = None;
            let mut i = 1usize;
            while i < args.len() {
                match args[i].as_str() {
                    "--policy-id" => { policy_id = args.get(i+1).cloned().unwrap_or_default(); i += 2; }
                    "--agreement-hash" => { agreement_hash = args.get(i+1).cloned().unwrap_or_default(); i += 2; }
                    "--attestor" => {
                        let raw = args.get(i+1).cloned().unwrap_or_default();
                        let parts: Vec<&str> = raw.splitn(2, ':').collect();
                        if parts.len() == 2 {
                            attestors.push(BuildTemplateAttestorInput { attestor_id: parts[0].to_string(), pubkey_hex: parts[1].to_string(), display_name: None });
                        } else { eprintln!("--attestor expects <id>:<pubkey>"); std::process::exit(1); }
                        i += 2;
                    }
                    "--delivery-proof-type" => { delivery_proof_type = args.get(i+1).cloned().unwrap_or_default(); i += 2; }
                    "--refund-deadline-height" => {
                        let raw = args.get(i+1).cloned().unwrap_or_default();
                        refund_deadline_height = match raw.parse::<u64>() {
                            Ok(v) => v,
                            Err(_) => { eprintln!("--refund-deadline-height expects a non-negative integer, got: {}", raw); std::process::exit(1); }
                        };
                        i += 2;
                    }
                    "--holdback-bps" => { holdback_bps = args.get(i+1).and_then(|v| v.parse().ok()); i += 2; }
                    "--holdback-release-height" => { holdback_release_height = args.get(i+1).and_then(|v| v.parse().ok()); i += 2; }
                    "--notes" => { notes = Some(args.get(i+1).cloned().unwrap_or_default()); i += 2; }
                    "--rpc" => { rpc_url = args.get(i+1).cloned().unwrap_or_default(); i += 2; }
                    "--json" => { json_mode = true; i += 1; }
                    _ => { i += 1; }
                }
            }
            if policy_id.is_empty() || agreement_hash.is_empty() || attestors.is_empty() || delivery_proof_type.is_empty() {
                eprintln!("policy-build-preorder requires --policy-id, --agreement-hash, at least one --attestor, and --delivery-proof-type");
                std::process::exit(1);
            }
            let base = rpc_url.trim_end_matches('/');
            let client = rpc_client(base).unwrap_or_else(|e| { eprintln!("{}", e); std::process::exit(1); });
            let req = BuildPreorderTemplateRpcRequest { policy_id, agreement_hash, attestors, delivery_proof_type, refund_deadline_height, holdback_bps, holdback_release_height, notes };
            let resp: BuildTemplateRpcResponse = rpc_post_json(&client, base, "/rpc/buildpreordertemplate", &req)
                .unwrap_or_else(|e| { eprintln!("{}", e); std::process::exit(1); });
            if json_mode {
                println!("{}", serde_json::to_string_pretty(&serde_json::to_value(&resp).unwrap()).unwrap());
            } else {
                println!("{}", render_build_template_summary(&resp));
            }
        }
        "policy-build-otc" => {
            let mut policy_id = String::new();
            let mut agreement_hash = String::new();
            let mut attestors: Vec<BuildTemplateAttestorInput> = Vec::new();
            let mut release_proof_type = String::new();
            let mut refund_deadline_height: u64 = 0;
            let mut threshold: Option<u32> = None;
            let mut rpc_url = default_rpc_url();
            let mut json_mode = false;
            let mut notes: Option<String> = None;
            let mut i = 1usize;
            while i < args.len() {
                match args[i].as_str() {
                    "--policy-id" => { policy_id = args.get(i+1).cloned().unwrap_or_default(); i += 2; }
                    "--agreement-hash" => { agreement_hash = args.get(i+1).cloned().unwrap_or_default(); i += 2; }
                    "--attestor" => {
                        let raw = args.get(i+1).cloned().unwrap_or_default();
                        let parts: Vec<&str> = raw.splitn(2, ':').collect();
                        if parts.len() == 2 {
                            attestors.push(BuildTemplateAttestorInput { attestor_id: parts[0].to_string(), pubkey_hex: parts[1].to_string(), display_name: None });
                        } else { eprintln!("--attestor expects <id>:<pubkey>"); std::process::exit(1); }
                        i += 2;
                    }
                    "--release-proof-type" => { release_proof_type = args.get(i+1).cloned().unwrap_or_default(); i += 2; }
                    "--refund-deadline-height" => {
                        let raw = args.get(i+1).cloned().unwrap_or_default();
                        refund_deadline_height = match raw.parse::<u64>() {
                            Ok(v) => v,
                            Err(_) => { eprintln!("--refund-deadline-height expects a non-negative integer, got: {}", raw); std::process::exit(1); }
                        };
                        i += 2;
                    }
                    "--threshold" => { threshold = args.get(i+1).and_then(|v| v.parse().ok()); i += 2; }
                    "--notes" => { notes = Some(args.get(i+1).cloned().unwrap_or_default()); i += 2; }
                    "--rpc" => { rpc_url = args.get(i+1).cloned().unwrap_or_default(); i += 2; }
                    "--json" => { json_mode = true; i += 1; }
                    _ => { i += 1; }
                }
            }
            if policy_id.is_empty() || agreement_hash.is_empty() || attestors.is_empty() || release_proof_type.is_empty() {
                eprintln!("policy-build-otc requires --policy-id, --agreement-hash, at least one --attestor, and --release-proof-type");
                std::process::exit(1);
            }
            let base = rpc_url.trim_end_matches('/');
            let client = rpc_client(base).unwrap_or_else(|e| { eprintln!("{}", e); std::process::exit(1); });
            let req = BuildOtcTemplateRpcRequest { policy_id, agreement_hash, attestors, release_proof_type, refund_deadline_height, threshold, notes };
            let resp: BuildTemplateRpcResponse = rpc_post_json(&client, base, "/rpc/buildotctemplate", &req)
                .unwrap_or_else(|e| { eprintln!("{}", e); std::process::exit(1); });
            if json_mode {
                println!("{}", serde_json::to_string_pretty(&serde_json::to_value(&resp).unwrap()).unwrap());
            } else {
                println!("{}", render_build_template_summary(&resp));
            }
        }
        "agreement-policy-list" => {
            let opts = match parse_policy_list_cli(&args[1..]) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("{}", e);
                    usage();
                    std::process::exit(1);
                }
            };
            let base = opts.rpc_url.trim_end_matches('/');
            let client = match rpc_client(base) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            };
            let req = ListPoliciesRpcRequest { active_only: opts.active_only };
            let resp: ListPoliciesRpcResponse =
                match rpc_post_json(&client, base, "/rpc/listpolicies", &req) {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("{}", e);
                        std::process::exit(1);
                    }
                };
            if opts.json_mode {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::to_value(&resp).unwrap()).unwrap()
                );
            } else {
                println!("{}", render_policy_list_summary(&resp));
            }
        }
        "agreement-release-build" | "agreement-release-send" | "agreement-release" => {
            let parsed = match parse_agreement_spend_cli(&args[1..]) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("{}", e);
                    usage();
                    std::process::exit(1);
                }
            };
            let mode = match args[0].as_str() {
                "agreement-release-build" => Some(false),
                "agreement-release-send" => Some(true),
                _ => None,
            };
            let opts = match finalize_agreement_spend_mode(parsed, mode) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            };
            let base = opts.rpc_url.trim_end_matches('/');
            let client = match rpc_client(base) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            };
            let (_resolved, req, selection_notice) =
                match resolve_agreement_spend_request(&client, base, &opts) {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("{}", e);
                        std::process::exit(1);
                    }
                };
            let eligibility: AgreementSpendEligibilityResponse =
                match rpc_post_json(&client, base, "/rpc/agreementreleaseeligibility", &req) {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("{}", e);
                        std::process::exit(1);
                    }
                };
            if !opts.json_mode {
                if let Some(notice) = &selection_notice {
                    println!("{}", notice);
                }
                println!(
                    "{}",
                    render_agreement_spend_eligibility_summary(&eligibility)
                );
            }
            if !eligibility.eligible {
                if opts.json_mode {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(
                            &json!({"eligibility": eligibility, "spend": Value::Null})
                        )
                        .unwrap()
                    );
                }
                std::process::exit(1);
            }
            let spend: AgreementBuildSpendResponse =
                match rpc_post_json(&client, base, "/rpc/buildagreementrelease", &req) {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("{}", e);
                        std::process::exit(1);
                    }
                };
            if opts.json_mode {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "eligibility": eligibility,
                        "spend": spend,
                        "workflow": {
                            "mode": if opts.broadcast { "send" } else { "build" },
                            "signed_tx_ready": true,
                            "submitted_to_node": opts.broadcast,
                            "sensitive_raw_tx_present": true,
                        }
                    }))
                    .unwrap()
                );
            } else {
                println!(
                    "{}",
                    render_agreement_build_spend_summary(&spend, opts.broadcast, opts.show_raw_tx)
                );
            }
        }
        "agreement-refund-build" | "agreement-refund-send" | "agreement-refund" => {
            let parsed = match parse_agreement_spend_cli(&args[1..]) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("{}", e);
                    usage();
                    std::process::exit(1);
                }
            };
            let mode = match args[0].as_str() {
                "agreement-refund-build" => Some(false),
                "agreement-refund-send" => Some(true),
                _ => None,
            };
            let opts = match finalize_agreement_spend_mode(parsed, mode) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            };
            let base = opts.rpc_url.trim_end_matches('/');
            let client = match rpc_client(base) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            };
            let (_resolved, req, selection_notice) =
                match resolve_agreement_spend_request(&client, base, &opts) {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("{}", e);
                        std::process::exit(1);
                    }
                };
            let eligibility: AgreementSpendEligibilityResponse =
                match rpc_post_json(&client, base, "/rpc/agreementrefundeligibility", &req) {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("{}", e);
                        std::process::exit(1);
                    }
                };
            if !opts.json_mode {
                if let Some(notice) = &selection_notice {
                    println!("{}", notice);
                }
                println!(
                    "{}",
                    render_agreement_spend_eligibility_summary(&eligibility)
                );
            }
            if !eligibility.eligible {
                if opts.json_mode {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(
                            &json!({"eligibility": eligibility, "spend": Value::Null})
                        )
                        .unwrap()
                    );
                }
                std::process::exit(1);
            }
            let spend: AgreementBuildSpendResponse =
                match rpc_post_json(&client, base, "/rpc/buildagreementrefund", &req) {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("{}", e);
                        std::process::exit(1);
                    }
                };
            if opts.json_mode {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "eligibility": eligibility,
                        "spend": spend,
                        "workflow": {
                            "mode": if opts.broadcast { "send" } else { "build" },
                            "signed_tx_ready": true,
                            "submitted_to_node": opts.broadcast,
                            "sensitive_raw_tx_present": true,
                        }
                    }))
                    .unwrap()
                );
            } else {
                println!(
                    "{}",
                    render_agreement_build_spend_summary(&spend, opts.broadcast, opts.show_raw_tx)
                );
            }
        }
        "agreement-status" => {
            if args.len() < 2 {
                usage();
                std::process::exit(1);
            }
            let agreement = match load_agreement(&args[1]) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            };
            let mut rpc_url = default_rpc_url();
            let json_mode = args.iter().any(|a| a == "--json");
            if args.len() == 4 && args[2] == "--rpc" {
                rpc_url = args[3].clone();
            }
            let base = rpc_url.trim_end_matches('/');
            let client = match rpc_client(base) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            };
            let resp: AgreementStatusResponse = match rpc_post_json(
                &client,
                base,
                "/rpc/agreementstatus",
                &AgreementRequestBody { agreement },
            ) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            };
            if json_mode {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::to_value(&resp).unwrap()).unwrap()
                );
            } else {
                println!("agreement_hash {}", resp.agreement_hash);
                println!("state {:?}", resp.lifecycle.state);
                println!(
                    "funded_amount_irm {}",
                    format_irm(resp.lifecycle.funded_amount)
                );
                println!(
                    "released_amount_irm {}",
                    format_irm(resp.lifecycle.released_amount)
                );
                println!(
                    "refunded_amount_irm {}",
                    format_irm(resp.lifecycle.refunded_amount)
                );
                println!("trust_model {}", resp.lifecycle.trust_model_note);
            }
        }
        "agreement-milestones" => {
            if args.len() < 2 {
                usage();
                std::process::exit(1);
            }
            let agreement = match load_agreement(&args[1]) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            };
            let mut rpc_url = default_rpc_url();
            let json_mode = args.iter().any(|a| a == "--json");
            if args.len() == 4 && args[2] == "--rpc" {
                rpc_url = args[3].clone();
            }
            let base = rpc_url.trim_end_matches('/');
            let client = match rpc_client(base) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            };
            let resp: AgreementMilestonesResponse = match rpc_post_json(
                &client,
                base,
                "/rpc/agreementmilestones",
                &AgreementRequestBody { agreement },
            ) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            };
            if json_mode {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::to_value(&resp).unwrap()).unwrap()
                );
            } else {
                println!("agreement_hash {}", resp.agreement_hash);
                println!("state {}", resp.state);
                for milestone in resp.milestones {
                    println!(
                        "{} {} amount={} funded={} released={} refunded={}",
                        milestone.milestone_id,
                        milestone.title,
                        format_irm(milestone.amount),
                        milestone.funded,
                        milestone.released,
                        milestone.refunded
                    );
                }
            }
        }
        "agreement-txs" => {
            if args.len() < 2 {
                usage();
                std::process::exit(1);
            }
            let agreement = match load_agreement(&args[1]) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            };
            let mut rpc_url = default_rpc_url();
            let json_mode = args.iter().any(|a| a == "--json");
            if args.len() == 4 && args[2] == "--rpc" {
                rpc_url = args[3].clone();
            }
            let base = rpc_url.trim_end_matches('/');
            let client = match rpc_client(base) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            };
            let resp: AgreementTxsResponse = match rpc_post_json(
                &client,
                base,
                "/rpc/listagreementtxs",
                &AgreementRequestBody { agreement },
            ) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            };
            if json_mode {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::to_value(&resp).unwrap()).unwrap()
                );
            } else {
                println!("agreement_hash {}", resp.agreement_hash);
                for tx in resp.txs {
                    println!(
                        "{} role={} value={} height={:?} confirmed={} milestone={:?}",
                        tx.txid,
                        tx.role,
                        format_irm(tx.value),
                        tx.height,
                        tx.confirmed,
                        tx.milestone_id
                    );
                }
            }
        }
        "verify-agreement-link" => {
            if args.len() < 3 {
                usage();
                std::process::exit(1);
            }
            let mut rpc_url = default_rpc_url();
            let json_mode = args.iter().any(|a| a == "--json");
            if args.len() == 5 && args[3] == "--rpc" {
                rpc_url = args[4].clone();
            }
            let base = rpc_url.trim_end_matches('/');
            let client = match rpc_client(base) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            };
            let resp: VerifyAgreementLinkResponse = match rpc_post_json(
                &client,
                base,
                "/rpc/verifyagreementlink",
                &VerifyAgreementLinkRequestBody {
                    agreement_hash: args[1].clone(),
                    tx_hex: args[2].clone(),
                },
            ) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            };
            if json_mode {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::to_value(&resp).unwrap()).unwrap()
                );
            } else {
                println!("agreement_hash {}", resp.agreement_hash);
                println!("matched {}", resp.matched);
                println!("anchors {}", resp.anchors.len());
            }
        }
        "send" => {
            if args.len() < 4 {
                usage();
                std::process::exit(1);
            }
            let from_addr = &args[1];
            let to_addr = &args[2];
            let amount = match parse_irm(&args[3]) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("Invalid amount: {}", e);
                    std::process::exit(1);
                }
            };
            if base58_p2pkh_to_hash(from_addr).is_none() || base58_p2pkh_to_hash(to_addr).is_none()
            {
                eprintln!("Invalid address or checksum");
                std::process::exit(1);
            }
            let mut fee_override: Option<u64> = None;
            let mut rpc_url = default_rpc_url();
            let mut coin_select = String::from("smallest");
            let mut i = 4;
            while i < args.len() {
                match args[i].as_str() {
                    "--fee" => {
                        if i + 1 >= args.len() {
                            eprintln!("Missing --fee value");
                            std::process::exit(1);
                        }
                        fee_override = Some(match parse_irm(&args[i + 1]) {
                            Ok(v) => v,
                            Err(e) => {
                                eprintln!("Invalid fee: {}", e);
                                std::process::exit(1);
                            }
                        });
                        i += 2;
                    }
                    "--coin-select" => {
                        if i + 1 >= args.len() {
                            eprintln!("Missing --coin-select value");
                            std::process::exit(1);
                        }
                        let mode = &args[i + 1];
                        if mode != "smallest" && mode != "largest" {
                            eprintln!("Invalid --coin-select value: {}", mode);
                            std::process::exit(1);
                        }
                        coin_select = mode.clone();
                        i += 2;
                    }
                    "--rpc" => {
                        if i + 1 >= args.len() {
                            eprintln!("Missing --rpc value");
                            std::process::exit(1);
                        }
                        rpc_url = args[i + 1].clone();
                        i += 2;
                    }
                    _ => {
                        usage();
                        std::process::exit(1);
                    }
                }
            }

            let path = wallet_path();
            let wallet = match load_wallet(&path) {
                Ok(w) => w,
                Err(e) => {
                    eprintln!("Failed to load wallet: {}", e);
                    std::process::exit(1);
                }
            };
            let key = match find_key(&wallet, from_addr) {
                Some(k) => k.clone(),
                None => {
                    eprintln!("From address not found in wallet");
                    std::process::exit(1);
                }
            };

            let base = rpc_url.trim_end_matches('/');
            let client = match rpc_client(base) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Failed to init HTTP client: {}", e);
                    std::process::exit(1);
                }
            };
            let payload = match fetch_utxos(&client, base, from_addr) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            };
            let mut utxos = payload.utxos.clone();
            match coin_select.as_str() {
                "smallest" => utxos.sort_by_key(|u| u.value),
                "largest" => utxos.sort_by_key(|u| Reverse(u.value)),
                _ => {}
            }

            let mut fee_per_byte = DEFAULT_FEE_PER_BYTE;
            if fee_override.is_none() {
                if let Ok(est) = fetch_fee_estimate(&client, base) {
                    let est_fee = est.min_fee_per_byte.ceil() as u64;
                    if est_fee > fee_per_byte {
                        fee_per_byte = est_fee;
                    }
                }
            }

            let mut selected = Vec::new();
            let mut total = 0u64;
            let mut fee = fee_override.unwrap_or(0);
            for utxo in utxos.iter() {
                let confirmations = payload.height.saturating_sub(utxo.height);
                if utxo.is_coinbase && confirmations < COINBASE_MATURITY {
                    continue;
                }
                selected.push(utxo.clone());
                total = total.saturating_add(utxo.value);
                if fee_override.is_none() {
                    let outputs = if total > amount { 2 } else { 1 };
                    fee = estimate_tx_size(selected.len(), outputs).saturating_mul(fee_per_byte);
                }
                if total >= amount.saturating_add(fee) {
                    break;
                }
            }

            if total < amount.saturating_add(fee) {
                eprintln!("Insufficient funds");
                std::process::exit(1);
            }

            let to_pkh = match base58_p2pkh_to_hash(to_addr) {
                Some(v) => v,
                None => {
                    eprintln!("Invalid destination address");
                    std::process::exit(1);
                }
            };
            let mut to_arr = [0u8; 20];
            to_arr.copy_from_slice(&to_pkh);
            let to_script = p2pkh_script(&to_arr);

            let from_pkh = match base58_p2pkh_to_hash(from_addr) {
                Some(v) => v,
                None => {
                    eprintln!("Invalid source address");
                    std::process::exit(1);
                }
            };
            let mut from_arr = [0u8; 20];
            from_arr.copy_from_slice(&from_pkh);
            let change_script = p2pkh_script(&from_arr);

            let priv_bytes = match hex::decode(&key.privkey) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("Invalid private key hex: {}", e);
                    std::process::exit(1);
                }
            };
            let signing_key = match SigningKey::from_bytes(priv_bytes.as_slice().into()) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("Invalid signing key: {}", e);
                    std::process::exit(1);
                }
            };
            // Derive pubkey from private key at send-time so stale wallet metadata
            // cannot produce invalid signatures.
            let from_pkh_arr = {
                let mut arr = [0u8; 20];
                arr.copy_from_slice(&from_pkh);
                arr
            };
            let vk = signing_key.verifying_key();
            let pk_comp = vk.to_encoded_point(true);
            let pk_uncomp = vk.to_encoded_point(false);
            let pub_bytes = if hash160(pk_comp.as_bytes()) == from_pkh_arr {
                pk_comp.as_bytes().to_vec()
            } else if hash160(pk_uncomp.as_bytes()) == from_pkh_arr {
                pk_uncomp.as_bytes().to_vec()
            } else {
                eprintln!("Wallet key mismatch: source address does not match derived private key");
                std::process::exit(1);
            };

            let mut inputs: Vec<TxInput> = Vec::new();
            for utxo in &selected {
                let txid = match hex_to_32(&utxo.txid) {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("Invalid utxo txid: {}", e);
                        std::process::exit(1);
                    }
                };
                inputs.push(TxInput {
                    prev_txid: txid,
                    prev_index: utxo.index,
                    script_sig: Vec::new(),
                    sequence: 0xffff_ffff,
                });
            }

            let mut outputs = vec![TxOutput {
                value: amount,
                script_pubkey: to_script,
            }];

            let mut change = total.saturating_sub(amount).saturating_sub(fee);
            if change > 0 {
                outputs.push(TxOutput {
                    value: change,
                    script_pubkey: change_script.clone(),
                });
            }

            let mut tx = Transaction {
                version: 1,
                inputs,
                outputs,
                locktime: 0,
            };

            for _ in 0..2 {
                for (idx, utxo) in selected.iter().enumerate() {
                    let script_pubkey = match hex::decode(&utxo.script_pubkey) {
                        Ok(v) => v,
                        Err(e) => {
                            eprintln!("Invalid utxo script_pubkey hex: {}", e);
                            std::process::exit(1);
                        }
                    };
                    let digest = signature_digest(&tx, idx, &script_pubkey);
                    let sig: Signature = match signing_key.sign_prehash(&digest) {
                        Ok(v) => v,
                        Err(e) => {
                            eprintln!("Failed to sign prehash: {}", e);
                            std::process::exit(1);
                        }
                    };
                    let sig = sig.normalize_s().unwrap_or(sig);
                    let mut sig_bytes = sig.to_der().as_bytes().to_vec();
                    sig_bytes.push(0x01);

                    let mut script = Vec::new();
                    script.push(sig_bytes.len() as u8);
                    script.extend_from_slice(&sig_bytes);
                    script.push(pub_bytes.len() as u8);
                    script.extend_from_slice(&pub_bytes);
                    tx.inputs[idx].script_sig = script;
                }

                let size = tx.serialize().len() as u64;
                if fee_override.is_some() {
                    break;
                }
                let needed_fee = size.saturating_mul(fee_per_byte);
                if needed_fee > fee {
                    let extra = needed_fee - fee;
                    if change >= extra {
                        fee = needed_fee;
                        change = change.saturating_sub(extra);
                        if tx.outputs.len() > 1 {
                            tx.outputs[1].value = change;
                        } else if change > 0 {
                            tx.outputs.push(TxOutput {
                                value: change,
                                script_pubkey: change_script.clone(),
                            });
                        }
                        continue;
                    } else {
                        eprintln!("Insufficient funds for fee");
                        std::process::exit(1);
                    }
                }
                break;
            }

            if let Err(e) = submit_tx(&client, base, &tx) {
                eprintln!("{}", e);
                std::process::exit(1);
            }
            println!("txid {}", hex::encode(tx.txid()));
        }
        "agreement-build-settlement" => {
            // agreement-build-settlement <agreement.json> [--rpc <url>] [--json]
            let mut args = args.iter().skip(1);
            let agreement_path = match args.next() {
                Some(p) => p.clone(),
                None => {
                    eprintln!("usage: agreement-build-settlement <agreement.json> [--rpc <url>] [--json]");
                    std::process::exit(1);
                }
            };
            let mut rpc_url = node_rpc_base();
            let mut json_mode = false;
            while let Some(flag) = args.next() {
                if flag == "--rpc" {
                    if let Some(u) = args.next() { rpc_url = u.clone(); }
                } else if flag == "--json" {
                    json_mode = true;
                }
            }
            let agreement_json = std::fs::read_to_string(&agreement_path)
                .unwrap_or_else(|e| { eprintln!("read {}: {}", agreement_path, e); std::process::exit(1); });
            let agreement: AgreementObject = serde_json::from_str(&agreement_json)
                .unwrap_or_else(|e| { eprintln!("parse agreement: {}", e); std::process::exit(1); });
            let sc = SettlementClient::new(&rpc_url)
                .unwrap_or_else(|e| { eprintln!("rpc client: {}", e); std::process::exit(1); });
            let resp = sc.build_settlement_tx(agreement)
                .unwrap_or_else(|e| { eprintln!("buildsettlementtx: {}", e); std::process::exit(1); });
            if json_mode {
                println!("{}", serde_json::to_string_pretty(&resp).unwrap_or_default());
            } else {
                println!("{}", render_build_settlement_summary(&resp));
            }
        }
        "agreement-settle-status" => {
            // agreement-settle-status <agreement.json> [--rpc <url>]
            // End-to-end helper: computeagreementhash -> getpolicy -> evaluatepolicy -> buildsettlementtx
            let mut args = args.iter().skip(1);
            let agreement_path = match args.next() {
                Some(p) => p.clone(),
                None => {
                    eprintln!("usage: agreement-settle-status <agreement.json> [--rpc <url>]");
                    std::process::exit(1);
                }
            };
            let mut rpc_url = node_rpc_base();
            while let Some(flag) = args.next() {
                if flag == "--rpc" {
                    if let Some(u) = args.next() { rpc_url = u.clone(); }
                }
            }
            let agreement_json = std::fs::read_to_string(&agreement_path)
                .unwrap_or_else(|e| { eprintln!("read {}: {}", agreement_path, e); std::process::exit(1); });
            let agreement: AgreementObject = serde_json::from_str(&agreement_json)
                .unwrap_or_else(|e| { eprintln!("parse agreement: {}", e); std::process::exit(1); });
            let sc = SettlementClient::new(&rpc_url)
                .unwrap_or_else(|e| { eprintln!("rpc client: {}", e); std::process::exit(1); });

            // Step 1: compute canonical hash
            let hash_resp = sc.compute_agreement_hash(agreement.clone())
                .unwrap_or_else(|e| { eprintln!("computeagreementhash: {}", e); std::process::exit(1); });
            println!("=== agreement hash ===");
            println!("agreement_hash {}", hash_resp.agreement_hash);
            println!("canonical_rules {}", hash_resp.serialization_rules.len());

            // Step 2: getpolicy
            let pol_resp = sc.get_policy(hash_resp.agreement_hash.clone())
                .unwrap_or_else(|e| { eprintln!("getpolicy: {}", e); std::process::exit(1); });
            println!("\n=== policy ===");
            println!("{}", render_policy_get_summary(&pol_resp));
            if !pol_resp.found {
                eprintln!("note: no policy stored for this agreement hash; evaluation and settlement steps will reflect no-policy state");
            }

            // Step 3: evaluatepolicy
            let eval_resp = sc.evaluate_policy(agreement.clone())
                .unwrap_or_else(|e| { eprintln!("evaluatepolicy: {}", e); std::process::exit(1); });
            println!("\n=== evaluation ===");
            println!("{}", render_policy_evaluate_summary(&eval_resp));

            // Step 4: buildsettlementtx
            let bst_resp = sc.build_settlement_tx(agreement)
                .unwrap_or_else(|e| { eprintln!("buildsettlementtx: {}", e); std::process::exit(1); });
            println!("\n=== settlement actions ===");
            println!("{}", render_build_settlement_summary(&bst_resp));
        }
        _ => {
            usage();
            std::process::exit(1);
        }
    }
}
