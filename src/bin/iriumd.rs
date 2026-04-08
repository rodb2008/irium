use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, ToSocketAddrs};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use std::sync::{
    atomic::{AtomicU64, AtomicU8, AtomicUsize, Ordering},
    Arc, Mutex, OnceLock,
};
use std::{env, fs};

use axum::{
    extract::{ConnectInfo, DefaultBodyLimit, Json as AxumJson, Query, State},
    http::{
        header::{AUTHORIZATION, CONTENT_TYPE},
        HeaderMap, HeaderValue, Method, StatusCode,
    },
    routing::{get, post},
    Json, Router,
};
use axum_server::tls_rustls::RustlsConfig;
use chrono::Utc;
use hex;
use num_bigint::BigUint;
use num_traits::ToPrimitive;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tower_http::cors::{Any, CorsLayer};

use bs58;
use get_if_addrs::get_if_addrs;
use irium_node_rs::activation::{
    network_kind_from_env, resolved_htlcv1_activation_height, resolved_lwma_activation_height,
    runtime_htlcv1_env_override, runtime_lwma_env_override, NetworkKind,
};
use irium_node_rs::anchors::AnchorManager;
use irium_node_rs::block::{Block, BlockHeader};
use irium_node_rs::chain::{
    block_from_locked, ChainParams, ChainState, HeaderWork, LwmaParams, OutPoint,
};
use irium_node_rs::constants::{block_reward, COINBASE_MATURITY};
use irium_node_rs::genesis::load_locked_genesis;
use irium_node_rs::mempool::MempoolManager;
use irium_node_rs::network::SeedlistManager;
use irium_node_rs::network_era::network_era;
use irium_node_rs::p2p::P2PNode;
use irium_node_rs::pow::{meets_target, sha256d, Target};
use irium_node_rs::rate_limiter::RateLimiter;
use irium_node_rs::reputation::ReputationManager;
use irium_node_rs::settlement::{
    build_agreement_activity_timeline, build_agreement_anchor_output, build_agreement_audit_record,
    build_funding_legs, compute_agreement_hash_hex, derive_lifecycle,
    discover_agreement_funding_leg_candidates, extract_agreement_funding_leg_refs_from_tx,
    parse_agreement_anchor, verify_agreement_bundle, AgreementActivityEvent, AgreementAnchor,
    AgreementAnchorRole, AgreementAuditFundingLegRecord, AgreementAuditRecord, AgreementBundle,
    AgreementFundingLegRef, AgreementLifecycleView, AgreementLinkedTx, AgreementMilestoneStatus,
    AgreementObject, AgreementSummary,
    evaluate_policy,
    ProofPolicy,
    SettlementProof,
    ProofStore,
    PolicyStore,
    StorePolicyOutcome,
};
use irium_node_rs::storage;
use irium_node_rs::tx::{
    decode_full_tx, encode_htlcv1_claim_witness, encode_htlcv1_refund_witness,
    encode_htlcv1_script, parse_htlcv1_script, parse_output_encumbrance, HtlcV1Output,
    OutputEncumbrance, Transaction, TxInput, TxOutput,
};
use irium_node_rs::wallet_store::{WalletKey, WalletManager};
use k256::ecdsa::signature::hazmat::{PrehashSigner, PrehashVerifier};
use k256::ecdsa::{Signature, SigningKey};

const IRIUM_P2PKH_VERSION: u8 = 0x39;
const MAX_SUBMIT_BLOCK_TXS: usize = 10_000;
#[derive(Clone)]
struct AppState {
    chain: Arc<Mutex<ChainState>>,
    genesis_hash: String,
    mempool: Arc<Mutex<MempoolManager>>,
    wallet: Arc<Mutex<WalletManager>>,
    anchors: Option<AnchorManager>,
    p2p: Option<P2PNode>,
    limiter: Arc<Mutex<RateLimiter>>,
    status_height_cache: Arc<AtomicU64>,
    status_peer_count_cache: Arc<AtomicUsize>,
    status_sybil_cache: Arc<AtomicU8>,
    status_persisted_height_cache: Arc<AtomicU64>,
    status_persist_queue_cache: Arc<AtomicUsize>,
    status_persisted_contiguous_cache: Arc<AtomicU64>,
    status_persisted_max_on_disk_cache: Arc<AtomicU64>,
    status_quarantine_count_cache: Arc<AtomicU64>,
    status_persisted_window_tip_cache: Arc<AtomicU64>,
    status_missing_persisted_in_window_cache: Arc<AtomicU64>,
    status_missing_or_mismatch_in_window_cache: Arc<AtomicU64>,
    status_expected_hash_coverage_in_window_cache: Arc<AtomicU64>,
    status_expected_hash_window_span_cache: Arc<AtomicU64>,
    status_best_header_hash_cache: Arc<Mutex<String>>,
    proof_store: Arc<Mutex<ProofStore>>,
    policy_store: Arc<Mutex<PolicyStore>>,
}

#[derive(Serialize)]
struct PeerInfo {
    multiaddr: String,
    agent: Option<String>,
    source: Option<String>,
    height: Option<u64>,
    last_seen: f64,
    dialable: bool,
    last_successful_handshake: Option<f64>,
}

#[derive(Serialize)]
struct PeersResponse {
    peers: Vec<PeerInfo>,
}

#[derive(Serialize)]
struct BestHeaderTipResponse {
    height: u64,
    hash: String,
}

#[derive(Serialize)]
struct StatusResponse {
    height: u64,
    genesis_hash: String,
    network_era: String,
    network_era_description: String,
    network_era_tagline: Option<String>,
    early_participation_signal: bool,
    anchors_digest: Option<String>,
    peer_count: usize,
    anchor_loaded: bool,
    node_id: Option<String>,
    sybil_difficulty: Option<u8>,
    best_header_tip: BestHeaderTipResponse,
    persisted_height: u64,
    persist_queue_len: usize,
    persisted_contiguous_height: u64,
    persisted_max_height_on_disk: u64,
    quarantine_count: u64,
    persisted_window_tip: u64,
    missing_persisted_in_window: u64,
    missing_or_mismatch_in_window: u64,
    expected_hash_coverage_in_window: u64,
    expected_hash_window_span: u64,
    gap_healer_active: bool,
    gap_healer_last_progress_ts: u64,
    gap_healer_last_filled_height: Option<u64>,
    gap_healer_pending_count: u64,
}

#[derive(Serialize)]
struct UtxoResponse {
    value: u64,
    height: u64,
    is_coinbase: bool,
}

#[derive(Deserialize)]
struct NetworkHashrateQuery {
    window: Option<usize>,
}

#[derive(Serialize)]
struct NetworkHashrateResponse {
    tip_height: u64,
    current_network_era: String,
    current_network_era_description: String,
    current_network_era_tagline: Option<String>,
    early_participation_signal: bool,
    difficulty: f64,
    hashrate: Option<f64>,
    avg_block_time: Option<f64>,
    window: usize,
    sample_blocks: usize,
}

#[derive(Deserialize)]
struct MiningMetricsQuery {
    window: Option<usize>,
    series: Option<usize>,
}

#[derive(Serialize, Clone)]
struct MiningMetricsPoint {
    height: u64,
    time: u64,
    difficulty: f64,
}

#[derive(Serialize)]
struct MiningMetricsResponse {
    tip_height: u64,
    tip_time: u64,
    current_network_era: String,
    current_network_era_description: String,
    current_network_era_tagline: Option<String>,
    early_participation_signal: bool,

    difficulty: f64,
    hashrate: Option<f64>,
    avg_block_time: Option<f64>,

    window: usize,
    sample_blocks: usize,

    difficulty_1h: Option<f64>,
    difficulty_24h: Option<f64>,
    difficulty_change_1h_pct: Option<f64>,
    difficulty_change_24h_pct: Option<f64>,

    series: Vec<MiningMetricsPoint>,
}

#[derive(Serialize)]
struct BalanceResponse {
    address: String,
    pkh: String,
    balance: u64,
    mined_balance: u64,
    utxo_count: usize,
    mined_blocks: usize,
    height: u64,
}

#[derive(Serialize)]
struct UtxoItem {
    txid: String,
    index: u32,
    value: u64,
    height: u64,
    is_coinbase: bool,
    script_pubkey: String,
}

#[derive(Serialize)]
struct UtxosResponse {
    address: String,
    pkh: String,
    height: u64,
    utxos: Vec<UtxoItem>,
}

#[derive(Serialize)]
struct HistoryItem {
    txid: String,
    height: u64,
    received: u64,
    spent: u64,
    net: i64,
    is_coinbase: bool,
}

#[derive(Serialize)]
struct HistoryResponse {
    address: String,
    pkh: String,
    height: u64,
    txs: Vec<HistoryItem>,
}

#[derive(Serialize)]
struct FeeEstimateResponse {
    min_fee_per_byte: f64,
    mempool_size: usize,
}

#[derive(Deserialize)]
struct UtxoQuery {
    txid: String,
    index: u32,
}

#[derive(Deserialize)]
struct BalanceQuery {
    address: String,
}

#[derive(Deserialize)]
struct UtxosQuery {
    address: String,
}

#[derive(Deserialize)]
struct BlockQuery {
    height: u64,
}

#[derive(Deserialize)]
struct BlockHashQuery {
    hash: String,
}

#[derive(Deserialize)]
struct TemplateQuery {
    longpoll: Option<u8>,
    poll_secs: Option<u64>,
    max_txs: Option<usize>,
    min_fee: Option<f64>,
}

#[derive(Deserialize)]
struct SubmitTxRequest {
    tx_hex: String,
}

#[derive(Deserialize)]
struct TxQuery {
    txid: String,
}

#[derive(Serialize)]
struct TxLookupResponse {
    txid: String,
    height: u64,
    index: usize,
    block_hash: String,
    inputs: usize,
    outputs: usize,
    output_value: u64,
    is_coinbase: bool,
    tx_hex: String,
}

#[derive(Serialize)]
struct SubmitTxResponse {
    txid: String,
    accepted: bool,
}

#[derive(Deserialize)]
struct WalletCreateRequest {
    passphrase: String,
    seed_hex: Option<String>,
}

#[derive(Deserialize)]
struct WalletUnlockRequest {
    passphrase: String,
}

#[derive(Deserialize)]
struct WalletSendRequest {
    to_address: String,
    amount: String,
    from_address: Option<String>,
    fee_mode: Option<String>,
    fee_per_byte: Option<u64>,
    coin_select: Option<String>,
}

#[derive(Deserialize)]
struct WalletImportWifRequest {
    wif: String,
}

#[derive(Deserialize)]
struct WalletImportSeedRequest {
    seed_hex: String,
    force: Option<bool>,
}

#[derive(Deserialize)]
struct WalletExportWifQuery {
    address: String,
}

#[derive(Serialize)]
struct WalletCreateResponse {
    address: String,
    wallet_path: String,
}

#[derive(Serialize)]
struct WalletUnlockResponse {
    addresses: Vec<String>,
    current_address: String,
}

#[derive(Serialize)]
struct WalletAddressesResponse {
    addresses: Vec<String>,
}

#[derive(Serialize)]
struct WalletReceiveResponse {
    address: String,
}

#[derive(Serialize)]
struct WalletLockResponse {
    locked: bool,
}

#[derive(Serialize)]
struct WalletSendResponse {
    txid: String,
    accepted: bool,
    fee: u64,
    total_input: u64,
    change: u64,
}

#[derive(Deserialize)]
struct CreateHtlcRequest {
    amount: String,
    recipient_address: String,
    refund_address: String,
    secret_hash_hex: String,
    timeout_height: u64,
    fee_per_byte: Option<u64>,
    broadcast: Option<bool>,
}

#[derive(Serialize)]
struct CreateHtlcResponse {
    txid: String,
    accepted: bool,
    raw_tx_hex: String,
    htlc_vout: u32,
    expected_hash: String,
    timeout_height: u64,
    recipient_address: String,
    refund_address: String,
}

#[derive(Deserialize)]
struct DecodeHtlcRequest {
    raw_tx_hex: String,
    vout: Option<u32>,
}

#[derive(Serialize)]
struct DecodeHtlcResponse {
    found: bool,
    vout: Option<u32>,
    output_type: String,
    expected_hash: Option<String>,
    timeout_height: Option<u64>,
    recipient_address: Option<String>,
    refund_address: Option<String>,
}

#[derive(Deserialize)]
struct SpendHtlcRequest {
    funding_txid: String,
    vout: u32,
    destination_address: String,
    fee_per_byte: Option<u64>,
    broadcast: Option<bool>,
    secret_hex: Option<String>,
}

#[derive(Serialize)]
struct SpendHtlcResponse {
    txid: String,
    accepted: bool,
    raw_tx_hex: String,
    fee: u64,
}

#[derive(Deserialize)]
struct InspectHtlcQuery {
    txid: String,
    vout: u32,
}

#[derive(Serialize)]
struct InspectHtlcResponse {
    exists: bool,
    funded: bool,
    unspent: bool,
    spent: bool,
    spend_type: Option<String>,
    claimable_now: bool,
    refundable_now: bool,
    timeout_height: Option<u64>,
    expected_hash: Option<String>,
    recipient_address: Option<String>,
    refund_address: Option<String>,
}

#[derive(Deserialize)]
struct AgreementRequest {
    agreement: AgreementObject,
}

#[derive(Deserialize)]
struct FundAgreementRequest {
    agreement: AgreementObject,
    fee_per_byte: Option<u64>,
    broadcast: Option<bool>,
}

#[derive(Serialize)]
struct AgreementHashResponse {
    agreement_hash: String,
}

#[derive(Serialize)]
struct AgreementInspectResponse {
    agreement_hash: String,
    summary: AgreementSummary,
}

#[derive(Serialize)]
struct AgreementTxsResponse {
    agreement_hash: String,
    txs: Vec<AgreementLinkedTx>,
}

#[derive(Serialize)]
struct AgreementStatusResponse {
    agreement_hash: String,
    lifecycle: AgreementLifecycleView,
}

#[derive(Serialize)]
struct AgreementMilestonesResponse {
    agreement_hash: String,
    state: String,
    milestones: Vec<AgreementMilestoneStatus>,
}

#[derive(Deserialize)]
struct AgreementContextRequest {
    agreement: AgreementObject,
    #[serde(default)]
    bundle: Option<AgreementBundle>,
}

#[derive(Serialize, Deserialize, Clone)]
struct AgreementFundingLegCandidateResponse {
    agreement_hash: String,
    funding_txid: String,
    htlc_vout: u32,
    anchor_vout: u32,
    role: AgreementAnchorRole,
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
struct AgreementTimelineResponse {
    agreement_hash: String,
    lifecycle: AgreementLifecycleView,
    events: Vec<AgreementActivityEvent>,
    trust_model_note: String,
}

#[derive(Deserialize)]
struct VerifyAgreementLinkRequest {
    agreement_hash: String,
    tx_hex: String,
}

#[derive(Serialize)]
struct VerifyAgreementLinkResponse {
    agreement_hash: String,
    matched: bool,
    anchors: Vec<AgreementAnchor>,
}

#[derive(Serialize)]
struct AgreementFundingOutput {
    vout: u32,
    role: AgreementAnchorRole,
    milestone_id: Option<String>,
    amount: u64,
}

#[derive(Serialize)]
struct FundAgreementResponse {
    agreement_hash: String,
    txid: String,
    accepted: bool,
    raw_tx_hex: String,
    outputs: Vec<AgreementFundingOutput>,
    fee: u64,
}

#[derive(Deserialize)]
struct AgreementSpendRequest {
    agreement: AgreementObject,
    funding_txid: String,
    htlc_vout: Option<u32>,
    milestone_id: Option<String>,
    destination_address: Option<String>,
    fee_per_byte: Option<u64>,
    broadcast: Option<bool>,
    secret_hex: Option<String>,
}

#[derive(Serialize)]
struct AgreementSpendEligibilityResponse {
    agreement_hash: String,
    agreement_id: String,
    funding_txid: String,
    htlc_vout: Option<u32>,
    anchor_vout: Option<u32>,
    role: Option<AgreementAnchorRole>,
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

#[derive(Serialize)]
struct AgreementBuildSpendResponse {
    agreement_hash: String,
    agreement_id: String,
    funding_txid: String,
    htlc_vout: u32,
    role: AgreementAnchorRole,
    milestone_id: Option<String>,
    branch: String,
    destination_address: String,
    txid: String,
    accepted: bool,
    raw_tx_hex: String,
    fee: u64,
    trust_model_note: String,
}


#[derive(Deserialize)]
struct SubmitProofRequest {
    proof: SettlementProof,
}

#[derive(Debug, Serialize)]
struct SubmitProofResponse {
    proof_id: String,
    agreement_hash: String,
    accepted: bool,
    duplicate: bool,
    message: String,
}

#[derive(Deserialize)]
struct ListProofsRequest {
    agreement_hash: String,
}

#[derive(Serialize)]
struct ListProofsResponse {
    agreement_hash: String,
    count: usize,
    proofs: Vec<SettlementProof>,
}

#[derive(Deserialize)]
struct CheckPolicyRequest {
    agreement: AgreementObject,
    policy: ProofPolicy,
    #[serde(default)]
    proofs: Vec<SettlementProof>,
}

#[derive(Debug, Serialize)]
struct CheckPolicyResponse {
    agreement_hash: String,
    policy_id: String,
    tip_height: u64,
    release_eligible: bool,
    refund_eligible: bool,
    reason: String,
    evaluated_rules: Vec<String>,
}

#[derive(Deserialize)]
struct EvaluatePolicyRequest {
    agreement: AgreementObject,
}

#[derive(Debug, Serialize)]
struct EvaluatePolicyResponse {
    agreement_hash: String,
    policy_found: bool,
    policy_id: Option<String>,
    tip_height: u64,
    proof_count: usize,
    release_eligible: bool,
    refund_eligible: bool,
    reason: String,
    evaluated_rules: Vec<String>,
}

#[derive(Deserialize)]
struct StorePolicyRequest {
    policy: ProofPolicy,
}

#[derive(Debug, Serialize)]
struct StorePolicyResponse {
    policy_id: String,
    agreement_hash: String,
    accepted: bool,
    updated: bool,
    message: String,
}

#[derive(Deserialize)]
struct GetPolicyRequest {
    agreement_hash: String,
}

#[derive(Debug, Serialize)]
struct GetPolicyResponse {
    agreement_hash: String,
    found: bool,
    policy: Option<ProofPolicy>,
}

#[derive(Serialize)]
struct WalletExportWifResponse {
    address: String,
    wif: String,
}

#[derive(Serialize)]
struct WalletImportWifResponse {
    address: String,
}

#[derive(Serialize)]
struct WalletSeedResponse {
    seed_hex: String,
}

#[derive(Serialize)]
struct WalletImportSeedResponse {
    address: String,
}

#[derive(Clone)]
struct WalletUtxo {
    outpoint: OutPoint,
    output: TxOutput,
    height: u64,
    is_coinbase: bool,
    pkh: [u8; 20],
}

#[derive(Serialize)]
struct TemplateTx {
    hex: String,
    fee: u64,
    relay_addresses: Vec<String>,
}

#[derive(Serialize)]
struct BlockTemplateResponse {
    height: u64,
    prev_hash: String,
    bits: String,
    target: String,
    time: u32,
    txs: Vec<TemplateTx>,
    total_fees: u64,
    coinbase_value: u64,
    mempool_count: usize,
}

#[derive(Deserialize)]
struct SubmitBlockHeader {
    version: u32,
    prev_hash: String,
    merkle_root: String,
    time: u32,
    bits: String,
    nonce: u32,
    hash: String,
}

#[derive(Deserialize)]
struct SubmitBlockRequest {
    height: u64,
    header: SubmitBlockHeader,
    tx_hex: Vec<String>,
    #[serde(default)]
    submit_source: Option<String>,
}

#[derive(Deserialize)]
struct NodeConfig {
    /// Optional P2P bind address, e.g. "0.0.0.0:38291".
    #[serde(default)]
    p2p_bind: Option<String>,
    /// Optional list of manual peers, e.g. ["seed.example.org:38291"].
    #[serde(default)]
    p2p_seeds: Vec<String>,
    /// Optional DNS seed hosts.
    #[serde(default)]
    p2p_dns_seeds: Vec<String>,
    /// Optional relay payout address to advertise to peers.
    #[serde(default)]
    relay_address: Option<String>,
    /// Optional runtime root directory for blocks/state (used by mobile/Termux).
    #[serde(default)]
    data_dir: Option<String>,
}

fn load_node_config_from_env() -> Option<NodeConfig> {
    std::env::var("IRIUM_NODE_CONFIG")
        .ok()
        .and_then(|p| fs::read_to_string(p).ok())
        .and_then(|raw| serde_json::from_str::<NodeConfig>(&raw).ok())
}

fn cors_layer() -> Option<CorsLayer> {
    let raw = env::var("IRIUM_CORS_ORIGINS").ok()?;
    let origins = raw
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>();
    if origins.is_empty() {
        return None;
    }
    let layer = CorsLayer::new()
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
        .allow_headers([AUTHORIZATION, CONTENT_TYPE]);
    if origins.iter().any(|o| *o == "*" || *o == "all") {
        return Some(layer.allow_origin(Any));
    }
    let mut values = Vec::new();
    for origin in origins {
        if let Ok(value) = HeaderValue::from_str(origin) {
            values.push(value);
        }
    }
    if values.is_empty() {
        return None;
    }
    Some(layer.allow_origin(values))
}

fn parse_seed_lines(raw: &str) -> Vec<String> {
    raw.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                None
            } else {
                Some(line.to_string())
            }
        })
        .collect()
}

fn parse_seed_to_socketaddr(seed: &str, default_port: u16) -> Result<std::net::SocketAddr, String> {
    if let Ok(addr) = seed.parse::<std::net::SocketAddr>() {
        return Ok(addr);
    }
    if let Ok(ip) = seed.parse::<std::net::IpAddr>() {
        return format!("{}:{}", ip, default_port)
            .parse::<std::net::SocketAddr>()
            .map_err(|e| e.to_string());
    }
    Err("invalid seed format".to_string())
}
fn local_ip_set(bind: Option<&String>) -> HashSet<IpAddr> {
    let mut ips = HashSet::new();
    if let Some(bind) = bind {
        if let Ok(addr) = bind.parse::<SocketAddr>() {
            ips.insert(addr.ip());
        }
    }
    if let Ok(raw) = env::var("IRIUM_NODE_PUBLIC_IP").or_else(|_| env::var("IRIUM_PUBLIC_IP")) {
        if let Ok(ip) = raw.parse::<IpAddr>() {
            ips.insert(ip);
        }
    }
    if let Ok(ifaces) = get_if_addrs() {
        for iface in ifaces {
            ips.insert(iface.ip());
        }
    }
    // Also query hostname -I so we capture addresses exposed by the OS (e.g., public IPv4 on seeds).
    if let Ok(output) = std::process::Command::new("hostname").arg("-I").output() {
        if output.status.success() {
            if let Ok(list) = String::from_utf8(output.stdout) {
                for part in list.split_whitespace() {
                    if let Ok(ip) = part.parse::<IpAddr>() {
                        ips.insert(ip);
                    }
                }
            }
        }
    }
    // Optional: probe the outbound interface using a user-supplied target.
    if let Ok(target) = env::var("IRIUM_PUBLIC_IP_PROBE_TARGET") {
        if let Ok(sock) = std::net::UdpSocket::bind("0.0.0.0:0") {
            if sock.connect(&target).is_ok() {
                if let Ok(addr) = sock.local_addr() {
                    ips.insert(addr.ip());
                }
            }
        }
    }
    ips.insert(IpAddr::V4(Ipv4Addr::LOCALHOST));
    ips.insert(IpAddr::V6(Ipv6Addr::LOCALHOST));
    ips
}

fn mask_ip(ip: &str) -> String {
    match ip.parse::<IpAddr>() {
        Ok(IpAddr::V4(v4)) => {
            let oct = v4.octets();
            format!("{}.{}.*.*", oct[0], oct[1])
        }
        Ok(IpAddr::V6(v6)) => {
            let seg = v6.segments();
            format!("{:x}:{:x}::*", seg[0], seg[1])
        }
        Err(_) => ip.to_string(),
    }
}

fn mask_seed_label(seed: &str) -> String {
    let (ip, port) = seed.split_once(':').unwrap_or((seed, ""));
    let masked_ip = mask_ip(ip);
    if port.is_empty() {
        masked_ip
    } else {
        format!("{}:{}", masked_ip, port)
    }
}

fn load_runtime_seeds() -> Vec<String> {
    let path = std::path::Path::new("bootstrap/seedlist.runtime");
    std::fs::read_to_string(path)
        .map(|raw| parse_seed_lines(&raw))
        .unwrap_or_default()
}

fn load_persisted_startup_seeds(
    peers: &[irium_node_rs::network::PeerRecord],
    default_seed_port: u16,
) -> Vec<String> {
    let mut seeds = Vec::new();
    let mut persisted_records: Vec<irium_node_rs::network::PeerRecord> = Vec::new();
    let path = storage::state_dir().join("peers.json");
    if let Ok(raw) = std::fs::read_to_string(&path) {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&raw) {
            if let Some(map) = value.as_object() {
                for (multiaddr, entry) in map {
                    if let Some(obj) = entry.as_object() {
                        let dialable = obj
                            .get("dialable")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false);
                        let last_seen = obj
                            .get("last_seen")
                            .and_then(|v| v.as_f64())
                            .unwrap_or(0.0);
                        let first_seen = obj
                            .get("first_seen")
                            .and_then(|v| v.as_f64())
                            .unwrap_or(0.0);
                        persisted_records.push(irium_node_rs::network::PeerRecord {
                            multiaddr: multiaddr.clone(),
                            agent: None,
                            source: obj
                                .get("source")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string()),
                            last_seen,
                            first_seen,
                            seen_days: Vec::new(),
                            relay_address: None,
                            last_height: obj.get("last_height").and_then(|v| v.as_u64()),
                            node_id: None,
                            dialable,
                            last_successful_connect: obj
                                .get("last_successful_connect")
                                .and_then(|v| v.as_f64()),
                            last_successful_handshake: obj
                                .get("last_successful_handshake")
                                .and_then(|v| v.as_f64()),
                        });
                    }
                }
            }
        }
    }
    persisted_records.extend_from_slice(peers);
    for peer in persisted_records {
        if !peer.dialable {
            continue;
        }
        let parts: Vec<&str> = peer
            .multiaddr
            .split('/')
            .filter(|s| !s.is_empty())
            .collect();
        if parts.len() < 4 || parts[2] != "tcp" {
            continue;
        }
        let Some(port) = parts[3].parse::<u16>().ok().filter(|port| *port > 0) else {
            continue;
        };
        let seed = format!("{}:{}", parts[1], port);
        if !seeds.iter().any(|existing| existing == &seed) {
            seeds.push(seed);
        }
    }
    for seed in load_runtime_seeds() {
        let normalized = match parse_seed_to_socketaddr(&seed, default_seed_port) {
            Ok(addr) => addr.to_string(),
            Err(_) => seed,
        };
        if !seeds.iter().any(|existing| existing == &normalized) {
            seeds.push(normalized);
        }
    }
    seeds
}

fn load_manual_seeds(node_cfg: Option<&NodeConfig>) -> Vec<String> {
    let mut seeds = node_cfg
        .map(|cfg| cfg.p2p_seeds.clone())
        .unwrap_or_default();
    for env_name in ["IRIUM_ADDNODE", "IRIUM_MANUAL_PEERS"] {
        if let Ok(raw) = std::env::var(env_name) {
            for token in raw.split([',', ' ', '\n', '\t']) {
                let token = token.trim();
                if token.is_empty() {
                    continue;
                }
                if !seeds.iter().any(|s| s == token) {
                    seeds.push(token.to_string());
                }
            }
        }
    }
    seeds
}

fn load_extra_seeds() -> Vec<String> {
    let path = std::path::Path::new("bootstrap/seedlist.extra");
    std::fs::read_to_string(path)
        .map(|raw| parse_seed_lines(&raw))
        .unwrap_or_default()
}

fn load_static_seeds() -> Vec<String> {
    let path = std::path::Path::new("bootstrap/static_peers.txt");
    let mut seeds = std::fs::read_to_string(path)
        .map(|raw| parse_seed_lines(&raw))
        .unwrap_or_default();
    if let Ok(raw) = std::env::var("IRIUM_STATIC_PEERS") {
        for token in raw.split([',', ' ', '\n', '\t']) {
            let token = token.trim();
            if token.is_empty() {
                continue;
            }
            if !seeds.iter().any(|s| s == token) {
                seeds.push(token.to_string());
            }
        }
    }
    seeds
}

fn load_builtin_fallback_seeds() -> Vec<String> {
    let mut seeds = load_static_seeds();
    for seed in load_extra_seeds() {
        if !seeds.iter().any(|existing| existing == &seed) {
            seeds.push(seed);
        }
    }
    seeds
}

fn load_dns_seed_hosts(node_cfg: Option<&NodeConfig>) -> Vec<String> {
    let mut hosts = node_cfg
        .map(|cfg| cfg.p2p_dns_seeds.clone())
        .unwrap_or_default();
    if let Ok(raw) = std::env::var("IRIUM_DNS_SEEDS") {
        for token in raw.split([',', ' ', '\n', '\t']) {
            let token = token.trim();
            if token.is_empty() {
                continue;
            }
            if !hosts.iter().any(|host| host == token) {
                hosts.push(token.to_string());
            }
        }
    }
    hosts
}

fn resolve_dns_seed_addrs(
    hosts: &[String],
    default_seed_port: u16,
    local_ips: &HashSet<IpAddr>,
) -> (Vec<std::net::SocketAddr>, usize) {
    let mut addrs = Vec::new();
    let mut seen = HashSet::new();
    let mut filtered_local = 0usize;
    for host in hosts {
        match (host.as_str(), default_seed_port).to_socket_addrs() {
            Ok(iter) => {
                for addr in iter {
                    if local_ips.contains(&addr.ip()) {
                        filtered_local += 1;
                        continue;
                    }
                    if seen.insert(addr) {
                        addrs.push(addr);
                    }
                }
            }
            Err(err) => eprintln!(
                "[warn] bootstrap dns seed {} resolution failed: {}",
                host, err
            ),
        }
    }
    (addrs, filtered_local)
}

#[derive(Clone, Copy)]
struct SeedDialInfo {
    total: usize,
    filtered_local: usize,
    persisted: usize,
    manual: usize,
    fallback: usize,
    dns: usize,
    signed: usize,
}

fn load_signed_seeds() -> Vec<String> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let seed_path = std::path::Path::new("bootstrap/seedlist.txt");
    let sig_path = std::path::Path::new("bootstrap/seedlist.txt.sig");
    let allowed = std::path::Path::new("bootstrap/trust/allowed_signers");
    let Ok(seed_data) = std::fs::read_to_string(seed_path) else {
        eprintln!(
            "[warn] bootstrap signed seedlist missing: {}",
            seed_path.display()
        );
        return Vec::new();
    };

    let mut child = match Command::new("ssh-keygen")
        .arg("-Y")
        .arg("verify")
        .arg("-f")
        .arg(allowed)
        .arg("-I")
        .arg("bootstrap-signer")
        .arg("-n")
        .arg("file")
        .arg("-s")
        .arg(sig_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(err) => {
            eprintln!(
                "[warn] bootstrap signed seed verification unavailable: {}",
                err
            );
            return Vec::new();
        }
    };

    if let Some(stdin) = child.stdin.as_mut() {
        if stdin.write_all(seed_data.as_bytes()).is_err() {
            eprintln!("[warn] bootstrap signed seed verification input failed");
            return Vec::new();
        }
    }
    let status = match child.wait() {
        Ok(s) => s,
        Err(err) => {
            eprintln!("[warn] bootstrap signed seed verification failed: {}", err);
            return Vec::new();
        }
    };
    if status.success() {
        parse_seed_lines(&seed_data)
    } else {
        eprintln!("[warn] bootstrap signed seedlist signature invalid; continuing without it");
        Vec::new()
    }
}

fn build_seed_addrs(
    persisted_seeds: &[String],
    manual_seeds: &[String],
    fallback_seeds: &[String],
    dns_seed_addrs: &[std::net::SocketAddr],
    signed_seeds: &[String],
    default_seed_port: u16,
    local_ips: &HashSet<IpAddr>,
) -> (Vec<std::net::SocketAddr>, SeedDialInfo) {
    let mut seeds: Vec<std::net::SocketAddr> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut filtered_local = 0usize;

    for seed in persisted_seeds
        .iter()
        .chain(fallback_seeds.iter())
        .chain(signed_seeds.iter())
    {
        match parse_seed_to_socketaddr(seed, default_seed_port) {
            Ok(addr) => {
                if local_ips.contains(&addr.ip()) {
                    filtered_local += 1;
                    continue;
                }
                if seen.insert(addr) {
                    seeds.push(addr);
                }
            }
            Err(e) => eprintln!("Invalid P2P seed {}: {}", seed, e),
        }
    }
    for addr in dns_seed_addrs {
        if local_ips.contains(&addr.ip()) {
            filtered_local += 1;
            continue;
        }
        if seen.insert(*addr) {
            seeds.push(*addr);
        }
    }
    for seed in manual_seeds {
        match parse_seed_to_socketaddr(seed, default_seed_port) {
            Ok(addr) => {
                if local_ips.contains(&addr.ip()) {
                    filtered_local += 1;
                    continue;
                }
                if seen.insert(addr) {
                    seeds.push(addr);
                }
            }
            Err(e) => eprintln!("Invalid P2P seed {}: {}", seed, e),
        }
    }

    let mut info = SeedDialInfo {
        total: seeds.len(),
        filtered_local,
        persisted: persisted_seeds.len(),
        manual: manual_seeds.len(),
        fallback: fallback_seeds.len(),
        dns: dns_seed_addrs.len(),
        signed: signed_seeds.len(),
    };

    if seeds.is_empty() && filtered_local > 0 {
        let allow = std::env::var("IRIUM_ALLOW_LOCAL_SEED_FALLBACK")
            .ok()
            .map(|v| v == "1" || v.to_lowercase() == "true")
            .unwrap_or(false);
        if allow {
            for seed in persisted_seeds
                .iter()
                .chain(fallback_seeds.iter())
                .chain(signed_seeds.iter())
            {
                if let Ok(addr) = parse_seed_to_socketaddr(seed, default_seed_port) {
                    seeds.push(addr);
                    info.total = seeds.len();
                    break;
                }
            }
        }
    }
    let mut rep_mgr = ReputationManager::new();
    seeds.sort_by(|a, b| {
        rep_mgr
            .score_of(&b.to_string())
            .cmp(&rep_mgr.score_of(&a.to_string()))
    });
    (seeds, info)
}

fn json_log_enabled() -> bool {
    static FLAG: OnceLock<bool> = OnceLock::new();
    *FLAG.get_or_init(|| {
        std::env::var("IRIUM_JSON_LOG")
            .ok()
            .map(|v| v == "1" || v.to_lowercase() == "true")
            .unwrap_or(false)
    })
}

fn dial_log_rate_limit_enabled() -> bool {
    static FLAG: OnceLock<bool> = OnceLock::new();
    *FLAG.get_or_init(|| {
        std::env::var("IRIUM_P2P_DIAL_LOG_RATE_LIMIT")
            .ok()
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false)
    })
}

fn dial_seed_log_cooldown_secs() -> u64 {
    static VAL: OnceLock<u64> = OnceLock::new();
    *VAL.get_or_init(|| {
        std::env::var("IRIUM_P2P_DIAL_LOG_COOLDOWN_SECS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .map(|v| v.min(3600))
            .unwrap_or(30)
    })
}

fn dial_seed_log_allowed(kind: u8, ip: IpAddr) -> Option<u64> {
    if !dial_log_rate_limit_enabled() {
        return Some(0);
    }

    let cooldown = dial_seed_log_cooldown_secs();
    if cooldown == 0 {
        return Some(0);
    }

    // kind: 0 = dialing seed, 1 = outbound failed
    static GUARD: OnceLock<Mutex<HashMap<(u8, IpAddr), (Instant, u64)>>> = OnceLock::new();
    let guard = GUARD.get_or_init(|| Mutex::new(HashMap::new()));

    let mut map = guard.lock().unwrap_or_else(|e| e.into_inner());
    let now = Instant::now();
    let entry = map.entry((kind, ip)).or_insert((
        Instant::now() - Duration::from_secs(cooldown.saturating_add(1)),
        0,
    ));

    if now.duration_since(entry.0) < Duration::from_secs(cooldown) {
        entry.1 = entry.1.saturating_add(1);
        return None;
    }

    let suppressed = entry.1;
    entry.0 = now;
    entry.1 = 0;
    Some(suppressed)
}

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
    let payload = &body[1..];
    if payload.len() != 20 {
        return None;
    }
    Some(payload.to_vec())
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

fn hex_to_32(s: &str) -> Result<[u8; 32], String> {
    let bytes = hex::decode(s).map_err(|e| e.to_string())?;
    if bytes.len() != 32 {
        return Err("expected 32-byte hex".to_string());
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes);
    Ok(out)
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

fn miner_address_from_tx(tx: &Transaction) -> Option<String> {
    let output = tx.outputs.first()?;
    let pkh = p2pkh_hash_from_script(&output.script_pubkey)?;
    Some(base58_p2pkh_from_hash(&pkh))
}

fn miner_address_from_block(block: &Block) -> Option<String> {
    block.transactions.first().and_then(miner_address_from_tx)
}

fn p2pkh_hash_from_script(script: &[u8]) -> Option<[u8; 20]> {
    if script.len() != 25 {
        return None;
    }
    if script[0] != 0x76 || script[1] != 0xa9 || script[2] != 0x14 {
        return None;
    }
    if script[23] != 0x88 || script[24] != 0xac {
        return None;
    }
    let mut out = [0u8; 20];
    out.copy_from_slice(&script[3..23]);
    Some(out)
}

fn miner_blocks_dir() -> PathBuf {
    if let Ok(dir) = env::var("IRIUM_MINER_BLOCKS_DIR") {
        PathBuf::from(dir)
    } else {
        let home = env::var("HOME").unwrap_or_else(|_| "/".to_string());
        PathBuf::from(home).join(".irium/miner/blocks")
    }
}

fn same_dir(a: &PathBuf, b: &PathBuf) -> bool {
    if a == b {
        return true;
    }
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

fn persist_window_size() -> u64 {
    std::env::var("IRIUM_PERSIST_WINDOW")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .map(|v| v.clamp(128, 200_000))
        .unwrap_or(2000)
}

fn block_height_from_filename(path: &std::path::Path) -> Option<u64> {
    let name = path.file_name()?.to_str()?;
    let stripped = name.strip_prefix("block_")?;
    let num_part = stripped.strip_suffix(".json")?;
    num_part.parse::<u64>().ok()
}

fn path_contains_orphaned_dir(path: &std::path::Path) -> bool {
    path.components().any(|c| {
        c.as_os_str()
            .to_str()
            .map(|s| s.starts_with("orphaned_"))
            .unwrap_or(false)
    })
}

fn quarantine_single_block_file(path: &std::path::Path, reason: &str) -> bool {
    if !path.exists() || path_contains_orphaned_dir(path) {
        return false;
    }
    let Some(parent) = path.parent() else {
        return false;
    };
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let quarantine_dir = parent.join(format!("orphaned_{}", stamp));
    if fs::create_dir_all(&quarantine_dir).is_err() {
        return false;
    }
    let Some(name) = path.file_name() else {
        return false;
    };
    let mut dest = quarantine_dir.join(name);
    if dest.exists() {
        let mut n = 1u32;
        loop {
            let candidate = quarantine_dir.join(format!("{}.dup{}", name.to_string_lossy(), n));
            if !candidate.exists() {
                dest = candidate;
                break;
            }
            n = n.saturating_add(1);
        }
    }
    match fs::rename(path, &dest) {
        Ok(_) => {
            println!(
                "[🧹] Quarantined persisted block file {} (reason: {}; to={})",
                path.display(),
                reason,
                dest.display()
            );
            true
        }
        Err(_) => false,
    }
}

fn parse_persisted_block_file(
    path: &std::path::Path,
    genesis_hash_lc: &str,
) -> Result<(u64, Block), String> {
    let height =
        block_height_from_filename(path).ok_or_else(|| "invalid block file name".to_string())?;

    let md = fs::metadata(path).map_err(|e| format!("metadata read failed: {}", e))?;
    if md.len() == 0 {
        return Err("file is empty".to_string());
    }

    let data = fs::read_to_string(path).map_err(|e| format!("file read failed: {}", e))?;
    let parsed: serde_json::Value =
        serde_json::from_str(&data).map_err(|e| format!("json parse failed: {}", e))?;
    let header_obj = parsed
        .get("header")
        .ok_or_else(|| "missing header".to_string())?;

    let get_hex32 = |key: &str| -> Result<[u8; 32], String> {
        let s = header_obj
            .get(key)
            .and_then(|v| v.as_str())
            .ok_or_else(|| format!("missing header.{}", key))?;
        let bytes = hex::decode(s).map_err(|e| format!("bad hex in {}: {}", key, e))?;
        if bytes.len() != 32 {
            return Err(format!("{} must be 32 bytes", key));
        }
        let mut out = [0u8; 32];
        out.copy_from_slice(&bytes);
        Ok(out)
    };

    let prev_hash = get_hex32("prev_hash")?;
    let merkle_root = get_hex32("merkle_root")?;
    let version = header_obj
        .get("version")
        .and_then(|v| v.as_u64())
        .unwrap_or(1) as u32;
    let time = header_obj.get("time").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
    let bits_str = header_obj
        .get("bits")
        .and_then(|v| v.as_str())
        .unwrap_or("1d00ffff");
    let bits = u32::from_str_radix(bits_str, 16).map_err(|e| format!("invalid bits: {}", e))?;
    let nonce = header_obj
        .get("nonce")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;

    let txs: Vec<Transaction> = match parsed.get("tx_hex").and_then(|v| v.as_array()) {
        Some(arr) => {
            let mut out = Vec::with_capacity(arr.len());
            for t in arr {
                let s = t
                    .as_str()
                    .ok_or_else(|| "tx_hex entry is not a string".to_string())?;
                let bytes = hex::decode(s).map_err(|e| format!("invalid tx hex: {}", e))?;
                let tx = decode_compact_tx(&bytes)
                    .map_err(|e| format!("failed to decode compact tx: {}", e))?;
                out.push(tx);
            }
            out
        }
        None => Vec::new(),
    };

    let block = Block {
        header: BlockHeader {
            version,
            prev_hash,
            merkle_root,
            time,
            bits,
            nonce,
        },
        transactions: txs,
    };

    if height == 0 {
        let h = hex::encode(block.header.hash()).to_lowercase();
        if h != genesis_hash_lc {
            return Err("genesis hash mismatch".to_string());
        }
    } else {
        if block.transactions.is_empty() {
            return Err("block has no transactions".to_string());
        }
        if block.merkle_root() != block.header.merkle_root {
            return Err("block merkle root mismatch".to_string());
        }
        if block.header.bits == 0 {
            return Err("header bits is zero".to_string());
        }
        if !meets_target(&block.header.hash(), block.header.target()) {
            return Err("header hash does not meet declared target".to_string());
        }
    }

    Ok((height, block))
}

fn collect_block_files_from_dir(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    if !dir.exists() {
        return;
    }
    let mut stack = vec![dir.to_path_buf()];
    while let Some(cur) = stack.pop() {
        let Ok(read_dir) = cur.read_dir() else {
            continue;
        };
        for entry in read_dir.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.is_file() && block_height_from_filename(&path).is_some() {
                out.push(path);
            }
        }
    }
}

fn discover_persist_mismatch_heights(
    expected: &[(u64, [u8; 32])],
    blocks_dir: &std::path::Path,
    genesis_hash_lc: &str,
    current_contiguous: u64,
) -> (Vec<u64>, u64) {
    let mut out = Vec::new();
    let mut contiguous = current_contiguous;

    for (height, expected_hash) in expected.iter().copied() {
        let path = blocks_dir.join(format!("block_{}.json", height));
        let valid_and_matching = match parse_persisted_block_file(&path, genesis_hash_lc) {
            Ok((parsed_h, block)) => parsed_h == height && block.header.hash() == expected_hash,
            Err(_) => false,
        };

        if valid_and_matching {
            if height == contiguous.saturating_add(1) {
                contiguous = height;
            }
        } else {
            out.push(height);
        }
    }

    (out, contiguous)
}

#[derive(Default)]
struct PersistWindowStats {
    max_height_on_disk: u64,
    contiguous_from_zero: u64,
    window_tip: u64,
    missing_in_window: u64,
}

fn compute_persist_window_stats(
    all_heights: &std::collections::HashSet<u64>,
    valid_heights: &std::collections::HashSet<u64>,
) -> PersistWindowStats {
    let max_height_on_disk = all_heights.iter().copied().max().unwrap_or(0);
    let mut contiguous = 0u64;
    while valid_heights.contains(&contiguous) {
        contiguous = contiguous.saturating_add(1);
    }
    let contiguous_from_zero = contiguous.saturating_sub(1);
    let window_tip = valid_heights
        .iter()
        .copied()
        .max()
        .unwrap_or(contiguous_from_zero);
    let window = persist_window_size();
    let window_start = window_tip.saturating_sub(window.saturating_sub(1));
    let mut missing = 0u64;
    for h in window_start..=window_tip {
        if !valid_heights.contains(&h) {
            missing = missing.saturating_add(1);
        }
    }
    PersistWindowStats {
        max_height_on_disk,
        contiguous_from_zero,
        window_tip,
        missing_in_window: missing,
    }
}

fn best_chain_hashes_in_window(
    state: &ChainState,
    window_start: u64,
    window_tip: u64,
) -> std::collections::BTreeMap<u64, [u8; 32]> {
    let mut by_height = std::collections::BTreeMap::new();
    if window_start > window_tip {
        return by_height;
    }

    let mut current = state.best_header_hash();
    let mut guard = 0usize;
    let guard_limit = ((window_tip.saturating_sub(window_start) + 1) as usize)
        .saturating_mul(8)
        .saturating_add(8192);

    while current != [0u8; 32] && guard < guard_limit {
        guard = guard.saturating_add(1);

        if let Some(height) = state.heights.get(&current).copied() {
            let mut h = height;
            loop {
                if h < window_start {
                    break;
                }
                if h > window_tip {
                    if h == 0 {
                        break;
                    }
                    h = h.saturating_sub(1);
                    continue;
                }
                if let Some(block) = state.chain.get(h as usize) {
                    by_height.entry(h).or_insert(block.header.hash());
                }
                if h == 0 {
                    break;
                }
                h = h.saturating_sub(1);
            }
            break;
        }

        if let Some(hw) = state.headers.get(&current) {
            let h = hw.height;
            if h >= window_start && h <= window_tip {
                by_height.entry(h).or_insert(current);
            }
            if hw.header.prev_hash == [0u8; 32] {
                break;
            }
            current = hw.header.prev_hash;
            continue;
        }

        break;
    }

    by_height
}

fn rebuild_startup_header_index(
    state: &mut ChainState,
    candidates: &[(u64, std::path::PathBuf, Block)],
    window_start: u64,
    window_tip: u64,
    missing_in_window: u64,
) {
    let mut bootstrap_blocks: Vec<(u64, Block)> =
        candidates.iter().map(|(h, _, b)| (*h, b.clone())).collect();
    bootstrap_blocks.sort_by_key(|(h, _)| *h);

    let mut pending = bootstrap_blocks;
    let mut inserted = 0usize;
    let mut synthetic_roots = 0usize;
    let mut rounds = 0u8;

    while !pending.is_empty() && rounds < 8 {
        rounds = rounds.saturating_add(1);
        let mut progressed = false;
        let mut next_pending: Vec<(u64, Block)> = Vec::new();

        for (h, block) in pending.into_iter() {
            let hash = block.header.hash();
            if state.headers.contains_key(&hash) || state.heights.contains_key(&hash) {
                continue;
            }

            match state.add_header(block.header.clone()) {
                Ok(_) => {
                    inserted = inserted.saturating_add(1);
                    progressed = true;
                }
                Err(e) => {
                    if e.contains("unknown parent") && synthetic_roots == 0 {
                        if !meets_target(&hash, block.header.target()) {
                            eprintln!(
                                "[warn] startup header index skipped invalid PoW header at h={} hash= {}",
                                h,
                                hex::encode(hash)
                            );
                            continue;
                        }
                        let synthetic_work =
                            state.total_work.clone() + BigUint::from(h.saturating_add(1));
                        state.headers.insert(
                            hash,
                            HeaderWork {
                                header: block.header.clone(),
                                height: h,
                                work: synthetic_work,
                            },
                        );
                        state.header_chain.push(hash);
                        inserted = inserted.saturating_add(1);
                        synthetic_roots = synthetic_roots.saturating_add(1);
                        progressed = true;
                    } else {
                        next_pending.push((h, block));
                    }
                }
            }
        }

        pending = next_pending;
        if !progressed {
            break;
        }
    }

    let best_hash = state.best_header_hash();
    let best_linked_persisted_tip = state
        .headers
        .get(&best_hash)
        .map(|hw| hw.height)
        .or_else(|| state.heights.get(&best_hash).copied())
        .unwrap_or_else(|| state.tip_height());

    let mut unlinked_in_window = 0u64;
    for (h, _, block) in candidates.iter() {
        if *h < window_start || *h > window_tip {
            continue;
        }
        let hash = block.header.hash();
        let linked = state.headers.contains_key(&hash) || state.heights.contains_key(&hash);
        if !linked {
            unlinked_in_window = unlinked_in_window.saturating_add(1);
        }
    }

    println!(
        "[i] startup header index rebuilt: headers_known={} inserted={} synthetic_roots={} best_linked_persisted_tip={}/{} persisted_window_tip={} missing_in_window={} unlinked_in_window={} window=[{}..{}]",
        state.headers.len(),
        inserted,
        synthetic_roots,
        best_linked_persisted_tip,
        hex::encode(best_hash),
        window_tip,
        missing_in_window,
        unlinked_in_window,
        window_start,
        window_tip
    );
    if missing_in_window == 0 && unlinked_in_window > 0 {
        eprintln!(
            "[warn] startup header index has unlinked window headers despite missing_in_window=0 (unlinked_in_window={})",
            unlinked_in_window
        );
    }
    if state.headers.is_empty() {
        eprintln!(
            "[warn] startup header index empty after rebuild (window_tip={} missing_in_window={}); no parsed headers were linkable",
            window_tip,
            missing_in_window
        );
    }
    if missing_in_window == 0 && state.headers.is_empty() {
        eprintln!("[warn] BUG: missing_in_window=0 but headers_known=0 after startup rebuild");
    }
}

fn load_persisted_blocks(state: &mut ChainState, genesis_hash_lc: &str) {
    storage::reset_quarantine_count();
    storage::set_missing_persisted_in_window(0);
    storage::set_missing_or_mismatch_in_window(0);
    storage::set_expected_hash_coverage_in_window(0);
    storage::set_expected_hash_window_span(0);
    storage::set_persisted_window_tip(0);

    let node_dir = storage::blocks_dir();
    let miner_dir = miner_blocks_dir();

    let mut files = Vec::new();
    collect_block_files_from_dir(&node_dir, &mut files);
    if !same_dir(&node_dir, &miner_dir) {
        collect_block_files_from_dir(&miner_dir, &mut files);
    }
    files.sort();
    files.dedup();

    let mut all_heights: std::collections::HashSet<u64> = std::collections::HashSet::new();
    let mut valid_heights: std::collections::HashSet<u64> = std::collections::HashSet::new();
    let mut candidates: Vec<(u64, std::path::PathBuf, Block)> = Vec::new();

    for path in files {
        let Some(h) = block_height_from_filename(&path) else {
            continue;
        };
        all_heights.insert(h);
        match parse_persisted_block_file(&path, genesis_hash_lc) {
            Ok((height, block)) => {
                valid_heights.insert(height);
                candidates.push((height, path, block));
            }
            Err(e) => {
                eprintln!("[⚠️] Invalid persisted block {}: {}", path.display(), e);
                if quarantine_single_block_file(&path, &e) {
                    storage::add_quarantine_count(1);
                }
            }
        }
    }

    let stats = compute_persist_window_stats(&all_heights, &valid_heights);
    storage::set_persisted_max_height_on_disk(stats.max_height_on_disk);
    // contiguous_from_zero from file parsing is informational only; authoritative
    // contiguous height is the replay-connected tip set later in startup.
    storage::force_set_persisted_contiguous_height(0);
    storage::set_persisted_window_tip(stats.window_tip);
    storage::set_missing_persisted_in_window(stats.missing_in_window);

    let window = persist_window_size();
    let window_start = stats.window_tip.saturating_sub(window.saturating_sub(1));
    let mut missing_heights = Vec::new();
    for h in window_start..=stats.window_tip {
        if !valid_heights.contains(&h) {
            missing_heights.push(h);
        }
    }

    // Also track missing persisted files before the continuity window.
    // If this backlog is never healed, restarts can resume from a much lower
    // contiguous height even when near-tip files exist.
    let mut historical_missing_heights = Vec::new();
    if stats.contiguous_from_zero.saturating_add(1) < window_start {
        for h in (stats.contiguous_from_zero.saturating_add(1))..window_start {
            if !valid_heights.contains(&h) {
                historical_missing_heights.push(h);
            }
        }
    }

    println!(
        "[i] persist continuity window: tip={} window_start={} missing_in_window={} contiguous_from_zero={} historical_missing_before_window={}",
        stats.window_tip,
        window_start,
        stats.missing_in_window,
        stats.contiguous_from_zero,
        historical_missing_heights.len()
    );
    if stats.missing_in_window > 0 {
        eprintln!(
            "[warn] persist continuity window has gaps near tip (missing_in_window={}); writer may be behind; will backfill",
            stats.missing_in_window
        );
    }

    rebuild_startup_header_index(
        state,
        &candidates,
        window_start,
        stats.window_tip,
        stats.missing_in_window,
    );

    let mut observed_hashes_by_height: std::collections::BTreeMap<u64, Vec<[u8; 32]>> =
        std::collections::BTreeMap::new();
    for (h, _, block) in candidates.iter() {
        if *h < window_start || *h > stats.window_tip {
            continue;
        }
        observed_hashes_by_height
            .entry(*h)
            .or_default()
            .push(block.header.hash());
    }

    let expected_hashes_by_height =
        best_chain_hashes_in_window(state, window_start, stats.window_tip);
    let expected_hash_coverage_in_window = expected_hashes_by_height.len() as u64;
    let expected_hash_window_span = if window_start <= stats.window_tip {
        stats
            .window_tip
            .saturating_sub(window_start)
            .saturating_add(1)
    } else {
        0
    };
    storage::set_expected_hash_coverage_in_window(expected_hash_coverage_in_window);
    storage::set_expected_hash_window_span(expected_hash_window_span);

    let mut target_heights = historical_missing_heights.clone();

    if expected_hashes_by_height.is_empty() {
        target_heights.extend(missing_heights.iter().copied());
    } else {
        for h in window_start..=stats.window_tip {
            let Some(expected_hash) = expected_hashes_by_height.get(&h) else {
                continue;
            };
            let matched = observed_hashes_by_height
                .get(&h)
                .map(|hashes| hashes.iter().any(|v| v == expected_hash))
                .unwrap_or(false);
            if !matched {
                target_heights.push(h);
            }
        }
    }

    target_heights.sort_unstable();
    target_heights.dedup();

    storage::set_gap_healer_target_heights(&target_heights);
    storage::set_missing_or_mismatch_in_window(target_heights.len() as u64);

    candidates.sort_by_key(|(h, _, _)| *h);
    let mut pending = candidates;
    let mut rounds = 0u32;
    loop {
        rounds = rounds.saturating_add(1);
        let mut progressed = false;
        let mut next_pending: Vec<(u64, std::path::PathBuf, Block)> = Vec::new();

        for (h, path, block) in pending.into_iter() {
            if h == 0 || h <= state.tip_height() {
                continue;
            }
            match state.connect_block(block.clone()) {
                Ok(_) => {
                    progressed = true;
                }
                Err(e) => {
                    let e_lc = e.to_ascii_lowercase();
                    let should_quarantine = e_lc.contains("merkle")
                        || e_lc.contains("proof-of-work")
                        || e_lc.contains("bits mismatch")
                        || e_lc.contains("coinbase")
                        || e_lc.contains("timestamp");
                    if should_quarantine {
                        eprintln!(
                            "[⚠️] Persisted block {} failed validation: {}",
                            path.display(),
                            e
                        );
                        if quarantine_single_block_file(&path, &e) {
                            storage::add_quarantine_count(1);
                        }
                    } else {
                        next_pending.push((h, path, block));
                    }
                }
            }
        }

        pending = next_pending;
        if !progressed || pending.is_empty() || rounds > 4 {
            break;
        }
    }

    if !pending.is_empty() {
        eprintln!(
            "[i] persisted replay deferred {} block files due to missing ancestors; network sync will fill gaps",
            pending.len()
        );
    }

    let tip_height = state.tip_height();
    let tip_hash = hex::encode(state.tip_hash());
    storage::set_persisted_height(tip_height);
    storage::force_set_persisted_contiguous_height(tip_height);
    let queue_len = storage::persist_queue_len();

    println!(
        "[i] Startup source-of-truth: using validated persisted chain data near tip; old historical holes do not force rewind. tip={} hash={}",
        tip_height, tip_hash
    );

    if storage::persisted_max_height_on_disk() > tip_height {
        let gap = storage::persisted_max_height_on_disk().saturating_sub(tip_height);
        if queue_len as u64 >= gap {
            println!(
                "[i] Persisted block gap detected: tip_height={} tip_hash={} highest_persisted_height={} persist_queue_len={}. writer may be behind; will backfill.",
                tip_height,
                tip_hash,
                storage::persisted_max_height_on_disk(),
                queue_len
            );
        } else {
            eprintln!(
                "[warn] Persisted block gap detected: tip_height={} tip_hash={} highest_persisted_height={} persist_queue_len={}. will resync missing continuity from network.",
                tip_height,
                tip_hash,
                storage::persisted_max_height_on_disk(),
                queue_len
            );
        }
    } else {
        println!(
            "[i] Persist continuity OK: tip_height={} tip_hash={} highest_persisted_height={} persist_queue_len={}",
            tip_height,
            tip_hash,
            storage::persisted_max_height_on_disk(),
            queue_len
        );
    }

    if state.height > 1 {
        println!(
            "[↩️] Resumed node height {} from persisted blocks",
            state.height
        );
    }
}

fn dir_is_empty(path: &std::path::Path) -> bool {
    match std::fs::read_dir(path) {
        Ok(mut rd) => rd.next().is_none(),
        Err(_) => true,
    }
}

fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        let ty = entry.file_type()?;
        if ty.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else if ty.is_file() {
            if let Some(parent) = to.parent() {
                std::fs::create_dir_all(parent)?;
            }
            // Do not overwrite any existing new-state files.
            if !to.exists() {
                let _ = std::fs::copy(&from, &to);
            }
        }
    }
    Ok(())
}

fn migrate_legacy_repo_state_dir(state_dir: &std::path::Path) {
    if !dir_is_empty(state_dir) {
        return;
    }

    let mut candidates = Vec::new();
    if let Ok(root) = env::var("IRIUM_REPO_ROOT") {
        candidates.push(PathBuf::from(root).join("state"));
    }
    candidates.push(PathBuf::from("state"));

    for legacy in candidates {
        if legacy.exists() && legacy.is_dir() {
            if let Err(e) = copy_dir_recursive(&legacy, state_dir) {
                eprintln!(
                    "[warn] Legacy state migration failed from {}: {}",
                    legacy.display(),
                    e
                );
            } else {
                println!(
                    "[i] Migrated legacy state from {} -> {}",
                    legacy.display(),
                    state_dir.display()
                );
            }
            break;
        }
    }
}

fn reinit_state_dir(state_dir: &PathBuf, reason: &str) {
    let ts = Utc::now().timestamp();
    if state_dir.exists() && !dir_is_empty(state_dir) {
        let backup = state_dir
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join(format!("state.bad.{ts}"));
        if let Err(e) = fs::rename(state_dir, &backup) {
            eprintln!(
                "[warn] Failed to rename state dir {} -> {}: {}",
                state_dir.display(),
                backup.display(),
                e
            );
        } else {
            println!(
                "[i] State dir reinitialized ({}) -> {}",
                reason,
                backup.display()
            );
        }
    }
    let _ = fs::create_dir_all(state_dir);
}

fn mempool_file() -> PathBuf {
    if let Ok(path) = env::var("IRIUM_MEMPOOL_FILE") {
        PathBuf::from(path)
    } else {
        let path = storage::state_dir().join("mempool/pending.json");
        if !path.exists() {
            let home = env::var("HOME").unwrap_or_else(|_| "/".to_string());
            let legacy = PathBuf::from(home).join(".irium/mempool/pending.json");
            if legacy.exists() {
                if let Some(parent) = path.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                let _ = fs::copy(&legacy, &path);
            }
        }
        path
    }
}

fn rate_limiter() -> RateLimiter {
    let rpm = env::var("IRIUM_RATE_LIMIT_PER_MIN")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(120);
    RateLimiter::new(rpm)
}

fn rpc_body_limit_bytes() -> usize {
    env::var("IRIUM_RPC_BODY_MAX")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(32 * 1024 * 1024)
}

fn require_rpc_auth(headers: &HeaderMap) -> Result<(), StatusCode> {
    let token = match env::var("IRIUM_RPC_TOKEN") {
        Ok(t) => t,
        Err(_) => return Ok(()),
    };
    let expected = format!("Bearer {}", token);
    let header = headers.get(AUTHORIZATION).and_then(|v| v.to_str().ok());
    if header == Some(expected.as_str()) {
        Ok(())
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}

fn rpc_authorized(headers: &HeaderMap) -> bool {
    let token = match env::var("IRIUM_RPC_TOKEN") {
        Ok(t) => t,
        Err(_) => return false,
    };
    let expected = format!("Bearer {}", token);
    let header = headers.get(AUTHORIZATION).and_then(|v| v.to_str().ok());
    header == Some(expected.as_str())
}

fn check_rate_with_auth(
    state: &AppState,
    addr: &SocketAddr,
    headers: &HeaderMap,
) -> Result<(), StatusCode> {
    if rpc_authorized(headers) {
        return Ok(());
    }
    check_rate(state, addr)
}

fn check_rate(state: &AppState, addr: &SocketAddr) -> Result<(), StatusCode> {
    let mut limiter = state.limiter.lock().unwrap_or_else(|e| e.into_inner());
    if limiter.is_allowed(&addr.ip().to_string()) {
        Ok(())
    } else {
        Err(StatusCode::TOO_MANY_REQUESTS)
    }
}

fn difficulty_from_target(pow_limit: Target, target: Target) -> f64 {
    let max_target = pow_limit.to_target();
    let cur_target = target.to_target();
    let max_f = max_target.to_f64().unwrap_or(0.0);
    let cur_f = cur_target.to_f64().unwrap_or(0.0);
    if cur_f <= 0.0 {
        0.0
    } else {
        max_f / cur_f
    }
}

async fn network_hashrate(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    Query(q): Query<NetworkHashrateQuery>,
) -> Result<Json<NetworkHashrateResponse>, StatusCode> {
    check_rate(&state, &addr)?;
    let window = q.window.unwrap_or(120).clamp(1, 2016);
    let (tip_height, difficulty, hashrate, avg_block_time, sample_blocks) = {
        let guard = state.chain.lock().unwrap_or_else(|e| e.into_inner());
        let tip_height = guard.tip_height();
        let tip_target = guard
            .chain
            .last()
            .map(|b| b.header.target())
            .unwrap_or_else(|| guard.params.genesis_block.header.target());
        let difficulty = difficulty_from_target(guard.params.pow_limit, tip_target);

        if guard.chain.len() < 2 {
            (tip_height, difficulty, None, None, 0usize)
        } else {
            let end_index = guard.chain.len() - 1;
            let start_index = if guard.chain.len() > window {
                guard.chain.len() - 1 - window
            } else {
                0
            };
            let blocks = end_index.saturating_sub(start_index);
            if blocks == 0 {
                (tip_height, difficulty, None, None, 0usize)
            } else {
                let start_time = guard.chain[start_index].header.time as i64;
                let end_time = guard.chain[end_index].header.time as i64;
                let elapsed = end_time - start_time;
                if elapsed <= 0 {
                    (tip_height, difficulty, None, None, blocks)
                } else {
                    let avg_time = (elapsed as f64) / (blocks as f64);
                    let hashrate = difficulty * 4294967296.0 / avg_time;
                    (
                        tip_height,
                        difficulty,
                        Some(hashrate),
                        Some(avg_time),
                        blocks,
                    )
                }
            }
        }
    };

    let era = network_era(tip_height);

    Ok(Json(NetworkHashrateResponse {
        tip_height,
        current_network_era: era.era_name.to_string(),
        current_network_era_description: era.era_description.to_string(),
        current_network_era_tagline: era.era_tagline.map(str::to_string),
        early_participation_signal: era.early_participation_signal,
        difficulty,
        hashrate,
        avg_block_time,
        window,
        sample_blocks,
    }))
}

async fn mining_metrics(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    Query(q): Query<MiningMetricsQuery>,
) -> Result<Json<MiningMetricsResponse>, StatusCode> {
    check_rate(&state, &addr)?;
    let window = q.window.unwrap_or(120).clamp(1, 2016);
    let series_len = q.series.unwrap_or(240).clamp(1, 2016);

    let (
        tip_height,
        tip_time,
        difficulty,
        hashrate,
        avg_block_time,
        sample_blocks,
        diff_1h,
        diff_24h,
        diff_1h_pct,
        diff_24h_pct,
        series,
    ) = {
        let guard = state.chain.lock().unwrap_or_else(|e| e.into_inner());
        let tip_height = guard.tip_height();
        let tip_time = guard
            .chain
            .last()
            .map(|b| b.header.time)
            .unwrap_or(guard.params.genesis_block.header.time);

        let tip_target = guard
            .chain
            .last()
            .map(|b| b.header.target())
            .unwrap_or_else(|| guard.params.genesis_block.header.target());
        let difficulty = difficulty_from_target(guard.params.pow_limit, tip_target);

        let (hashrate, avg_block_time, sample_blocks) = if guard.chain.len() < 2 {
            (None, None, 0usize)
        } else {
            let end_index = guard.chain.len() - 1;
            let start_index = if guard.chain.len() > window {
                guard.chain.len() - 1 - window
            } else {
                0
            };
            let blocks = end_index.saturating_sub(start_index);
            if blocks == 0 {
                (None, None, 0usize)
            } else {
                let start_time = guard.chain[start_index].header.time as i64;
                let end_time = guard.chain[end_index].header.time as i64;
                let elapsed = end_time - start_time;
                if elapsed <= 0 {
                    (None, None, blocks)
                } else {
                    let avg_time = (elapsed as f64) / (blocks as f64);
                    let hashrate = difficulty * 4294967296.0 / avg_time;
                    (Some(hashrate), Some(avg_time), blocks)
                }
            }
        };

        let diff_at_age = |age_secs: u64| -> Option<f64> {
            for b in guard.chain.iter().rev() {
                if (tip_time as u64).saturating_sub(b.header.time as u64) >= age_secs {
                    let d = difficulty_from_target(guard.params.pow_limit, b.header.target());
                    return Some(d);
                }
            }
            None
        };

        let diff_1h = diff_at_age(3600);
        let diff_24h = diff_at_age(86400);
        let diff_1h_pct = diff_1h.and_then(|d| {
            if d > 0.0 {
                Some((difficulty - d) / d * 100.0)
            } else {
                None
            }
        });
        let diff_24h_pct = diff_24h.and_then(|d| {
            if d > 0.0 {
                Some((difficulty - d) / d * 100.0)
            } else {
                None
            }
        });

        let mut series = Vec::new();
        if !guard.chain.is_empty() {
            let end_index = guard.chain.len() - 1;
            let start_index = if guard.chain.len() > series_len {
                guard.chain.len() - series_len
            } else {
                0
            };
            let count = end_index + 1 - start_index;
            let step = std::cmp::max(1, count / 120);
            for i in (start_index..=end_index).step_by(step) {
                let b = &guard.chain[i];
                let d = difficulty_from_target(guard.params.pow_limit, b.header.target());
                series.push(MiningMetricsPoint {
                    height: i as u64,
                    time: b.header.time as u64,
                    difficulty: d,
                });
            }
        }

        (
            tip_height,
            tip_time,
            difficulty,
            hashrate,
            avg_block_time,
            sample_blocks,
            diff_1h,
            diff_24h,
            diff_1h_pct,
            diff_24h_pct,
            series,
        )
    };

    let era = network_era(tip_height);

    Ok(Json(MiningMetricsResponse {
        tip_height,
        tip_time: tip_time as u64,
        current_network_era: era.era_name.to_string(),
        current_network_era_description: era.era_description.to_string(),
        current_network_era_tagline: era.era_tagline.map(str::to_string),
        early_participation_signal: era.early_participation_signal,
        difficulty,
        hashrate,
        avg_block_time,
        window,
        sample_blocks,
        difficulty_1h: diff_1h,
        difficulty_24h: diff_24h,
        difficulty_change_1h_pct: diff_1h_pct,
        difficulty_change_24h_pct: diff_24h_pct,
        series,
    }))
}

fn cached_best_header_tip(
    height: u64,
    cached_hash: &str,
    genesis_hash: &str,
) -> BestHeaderTipResponse {
    let hash = if cached_hash.is_empty() {
        if height > 0 {
            genesis_hash.to_string()
        } else {
            String::new()
        }
    } else {
        cached_hash.to_string()
    };
    BestHeaderTipResponse { height, hash }
}

fn compute_best_header_tip_from_chain(
    guard: &ChainState,
    genesis_hash: &str,
) -> BestHeaderTipResponse {
    let h = guard.tip_height();
    let best_hash = guard.best_header_hash();
    let best_height = guard
        .headers
        .get(&best_hash)
        .map(|hw| hw.height)
        .or_else(|| guard.heights.get(&best_hash).copied())
        .unwrap_or(h);
    let best_hash_hex = hex::encode(best_hash);
    if best_height > 0 && best_hash_hex.is_empty() {
        return cached_best_header_tip(best_height, "", genesis_hash);
    }
    BestHeaderTipResponse {
        height: best_height,
        hash: best_hash_hex,
    }
}

async fn status(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
) -> Result<Json<StatusResponse>, StatusCode> {
    check_rate(&state, &addr)?;

    // Keep /status responsive under heavy sync/P2P load by using short timeouts
    // and cached values instead of waiting indefinitely.
    let (peer_count, node_id, sybil_diff) = match state.p2p {
        Some(ref p2p) => {
            let peer_count =
                match tokio::time::timeout(Duration::from_millis(250), p2p.peer_count()).await {
                    Ok(v) => {
                        state.status_peer_count_cache.store(v, Ordering::Relaxed);
                        v
                    }
                    Err(_) => state.status_peer_count_cache.load(Ordering::Relaxed),
                };
            let sybil = match tokio::time::timeout(
                Duration::from_millis(250),
                p2p.current_sybil_difficulty(),
            )
            .await
            {
                Ok(v) => {
                    state.status_sybil_cache.store(v, Ordering::Relaxed);
                    Some(v)
                }
                Err(_) => Some(state.status_sybil_cache.load(Ordering::Relaxed)),
            };
            (peer_count, Some(p2p.node_id_hex()), sybil)
        }
        None => (0, None, None),
    };

    let anchors_digest = state
        .anchors
        .as_ref()
        .map(|a| a.payload_digest().to_string());

    let (height, best_header_tip) = match state.chain.try_lock() {
        Ok(guard) => {
            let h = guard.tip_height();
            state.status_height_cache.store(h, Ordering::Relaxed);
            let best = compute_best_header_tip_from_chain(&guard, &state.genesis_hash);
            if let Ok(mut cached) = state.status_best_header_hash_cache.lock() {
                if !best.hash.is_empty() {
                    *cached = best.hash.clone();
                }
            }
            (h, best)
        }
        Err(_) => {
            let h = state.status_height_cache.load(Ordering::Relaxed);
            let cached_hash = state
                .status_best_header_hash_cache
                .lock()
                .map(|v| v.clone())
                .unwrap_or_default();
            (
                h,
                cached_best_header_tip(h, &cached_hash, &state.genesis_hash),
            )
        }
    };

    let era = network_era(height);

    let persisted_height = storage::persisted_height();
    state
        .status_persisted_height_cache
        .store(persisted_height, Ordering::Relaxed);
    let persist_queue_len = storage::persist_queue_len();
    state
        .status_persist_queue_cache
        .store(persist_queue_len, Ordering::Relaxed);
    let persisted_contiguous_height = storage::persisted_contiguous_height();
    state
        .status_persisted_contiguous_cache
        .store(persisted_contiguous_height, Ordering::Relaxed);
    let persisted_max_height_on_disk = storage::persisted_max_height_on_disk();
    state
        .status_persisted_max_on_disk_cache
        .store(persisted_max_height_on_disk, Ordering::Relaxed);
    let quarantine_count = storage::quarantine_count();
    state
        .status_quarantine_count_cache
        .store(quarantine_count, Ordering::Relaxed);
    let persisted_window_tip = storage::persisted_window_tip();
    state
        .status_persisted_window_tip_cache
        .store(persisted_window_tip, Ordering::Relaxed);
    let missing_persisted_in_window = storage::missing_persisted_in_window();
    state
        .status_missing_persisted_in_window_cache
        .store(missing_persisted_in_window, Ordering::Relaxed);
    let missing_or_mismatch_in_window = storage::missing_or_mismatch_in_window();
    state
        .status_missing_or_mismatch_in_window_cache
        .store(missing_or_mismatch_in_window, Ordering::Relaxed);
    let expected_hash_coverage_in_window = storage::expected_hash_coverage_in_window();
    state
        .status_expected_hash_coverage_in_window_cache
        .store(expected_hash_coverage_in_window, Ordering::Relaxed);
    let expected_hash_window_span = storage::expected_hash_window_span();
    state
        .status_expected_hash_window_span_cache
        .store(expected_hash_window_span, Ordering::Relaxed);
    let gap_healer_active = storage::gap_healer_active();
    let gap_healer_last_progress_ts = storage::gap_healer_last_progress_ts();
    let gap_healer_last_filled_height = storage::gap_healer_last_filled_height();
    let gap_healer_pending_count = storage::gap_healer_pending_count();

    Ok(Json(StatusResponse {
        height,
        genesis_hash: state.genesis_hash.clone(),
        network_era: era.era_name.to_string(),
        network_era_description: era.era_description.to_string(),
        network_era_tagline: era.era_tagline.map(str::to_string),
        early_participation_signal: era.early_participation_signal,
        anchors_digest,
        peer_count,
        anchor_loaded: state.anchors.is_some(),
        node_id,
        sybil_difficulty: sybil_diff,
        best_header_tip,
        persisted_height,
        persist_queue_len,
        persisted_contiguous_height,
        persisted_max_height_on_disk,
        quarantine_count,
        persisted_window_tip,
        missing_persisted_in_window,
        missing_or_mismatch_in_window,
        expected_hash_coverage_in_window,
        expected_hash_window_span,
        gap_healer_active,
        gap_healer_last_progress_ts,
        gap_healer_last_filled_height,
        gap_healer_pending_count,
    }))
}

async fn peers(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
) -> Result<Json<PeersResponse>, StatusCode> {
    check_rate(&state, &addr)?;
    if let Some(ref p2p) = state.p2p {
        let list = p2p
            .peers_snapshot()
            .await
            .into_iter()
            .map(|p| PeerInfo {
                multiaddr: p.multiaddr,
                agent: p.agent,
                source: p.source,
                height: p.last_height,
                last_seen: p.last_seen,
                dialable: p.dialable,
                last_successful_handshake: p.last_successful_handshake,
            })
            .collect();
        Ok(Json(PeersResponse { peers: list }))
    } else {
        Ok(Json(PeersResponse { peers: Vec::new() }))
    }
}

async fn metrics(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
) -> Result<String, StatusCode> {
    check_rate(&state, &addr)?;
    let (height, anchor_loaded, tip_hash, anchor_digest) = {
        let g = state.chain.lock().unwrap_or_else(|e| e.into_inner());
        let tip_hash = g
            .chain
            .last()
            .map(|b| hex::encode(b.header.hash()))
            .unwrap_or_else(|| state.genesis_hash.clone());
        let digest = state
            .anchors
            .as_ref()
            .map(|a| a.payload_digest().to_string())
            .unwrap_or_default();
        (g.tip_height(), state.anchors.is_some(), tip_hash, digest)
    };
    let era = network_era(height);
    let relay = P2PNode::relay_telemetry_snapshot();
    let (peer_count, node_id_hex, sybil_diff, peer_telemetry) = match state.p2p {
        Some(ref p2p) => {
            let peers = p2p.peer_count().await;
            let node_id = p2p.node_id_hex();
            let diff = p2p.current_sybil_difficulty().await;
            let peer_telemetry = p2p.peer_telemetry_snapshot().await;
            (peers, node_id, diff, peer_telemetry)
        }
        None => (0usize, String::new(), 0u8, Default::default()),
    };
    let seeds = SeedlistManager::new(128).merged_seedlist();
    let mempool_sz = state
        .mempool
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .len();
    Ok(format!(
        "irium_height {}
irium_peers {}
irium_anchor_loaded {}
irium_tip_hash {}
irium_mempool_size {}
irium_anchor_digest {}
irium_node_id {}
irium_sybil_difficulty {}
irium_seed_count {}
irium_early_participation_signal {}
irium_new_tip_announced_count {}
irium_block_announce_peers_count_total {}
irium_block_request_count {}
irium_duplicate_block_suppressed_count {}
irium_avg_block_processing_ms {}
irium_avg_block_announce_delay_ms {}
irium_outbound_dial_attempts_total {}
irium_outbound_dial_success_total {}
irium_outbound_dial_failure_total {}
irium_outbound_dial_failure_timeout_total {}
irium_outbound_dial_failure_refused_total {}
irium_outbound_dial_failure_no_route_total {}
irium_outbound_dial_failure_banned_total {}
irium_outbound_dial_failure_backoff_total {}
irium_outbound_dial_failure_other_total {}
irium_inbound_accepted_total {}
irium_handshake_failures_total {}
irium_temp_bans_total {}
irium_unique_connected_peer_ips {}
irium_attempted_peer_ips {}
irium_banned_peers {}
",
        height,
        peer_count,
        anchor_loaded as u8,
        tip_hash,
        mempool_sz,
        anchor_digest,
        node_id_hex,
        sybil_diff,
        seeds.len(),
        era.early_participation_signal as u8,
        relay.new_tip_announced_count,
        relay.block_announce_peers_count_total,
        relay.block_request_count,
        relay.duplicate_block_suppressed_count,
        relay.avg_block_processing_ms,
        relay.avg_block_announce_delay_ms,
        peer_telemetry.outbound_dial_attempts_total,
        peer_telemetry.outbound_dial_success_total,
        peer_telemetry.outbound_dial_failure_total,
        peer_telemetry.outbound_dial_failure_timeout_total,
        peer_telemetry.outbound_dial_failure_refused_total,
        peer_telemetry.outbound_dial_failure_no_route_total,
        peer_telemetry.outbound_dial_failure_banned_total,
        peer_telemetry.outbound_dial_failure_backoff_total,
        peer_telemetry.outbound_dial_failure_other_total,
        peer_telemetry.inbound_accepted_total,
        peer_telemetry.handshake_failures_total,
        peer_telemetry.temp_bans_total,
        peer_telemetry.unique_connected_peer_ips,
        peer_telemetry.attempted_peer_ips,
        peer_telemetry.banned_peers
    ))
}

async fn get_utxo(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<UtxoQuery>,
) -> Result<Json<UtxoResponse>, StatusCode> {
    check_rate_with_auth(&state, &addr, &headers)?;
    let bytes = match hex::decode(&q.txid) {
        Ok(b) => b,
        Err(_) => return Err(StatusCode::BAD_REQUEST),
    };
    if bytes.len() != 32 {
        return Err(StatusCode::BAD_REQUEST);
    }
    let mut txid = [0u8; 32];
    txid.copy_from_slice(&bytes);

    let guard = state.chain.lock().unwrap_or_else(|e| e.into_inner());
    let key = OutPoint {
        txid,
        index: q.index,
    };
    if let Some(utxo) = guard.utxos.get(&key) {
        Ok(Json(UtxoResponse {
            value: utxo.output.value,
            height: utxo.height,
            is_coinbase: utxo.is_coinbase,
        }))
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

async fn get_balance(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<BalanceQuery>,
) -> Result<Json<BalanceResponse>, StatusCode> {
    check_rate_with_auth(&state, &addr, &headers)?;
    let pkh = base58_p2pkh_to_hash(&q.address).ok_or(StatusCode::BAD_REQUEST)?;
    if pkh.len() != 20 {
        return Err(StatusCode::BAD_REQUEST);
    }
    let mut pkh_arr = [0u8; 20];
    pkh_arr.copy_from_slice(&pkh);

    let (balance, utxo_count, mined_balance, mined_blocks, height) = {
        let guard = state.chain.lock().unwrap_or_else(|e| e.into_inner());
        let mut balance = 0u64;
        let mut utxo_count = 0usize;
        let mut mined_balance = 0u64;
        let mut mined_blocks = 0usize;
        for utxo in guard.utxos.values() {
            if let Some(script_pkh) = p2pkh_hash_from_script(&utxo.output.script_pubkey) {
                if script_pkh == pkh_arr {
                    balance = balance.saturating_add(utxo.output.value);
                    utxo_count += 1;
                    if utxo.is_coinbase {
                        mined_balance = mined_balance.saturating_add(utxo.output.value);
                        mined_blocks += 1;
                    }
                }
            }
        }
        (
            balance,
            utxo_count,
            mined_balance,
            mined_blocks,
            guard.tip_height(),
        )
    };

    Ok(Json(BalanceResponse {
        address: q.address,
        pkh: hex::encode(pkh_arr),
        balance,
        mined_balance,
        utxo_count,
        mined_blocks,
        height,
    }))
}

async fn get_utxos(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<UtxosQuery>,
) -> Result<Json<UtxosResponse>, StatusCode> {
    check_rate_with_auth(&state, &addr, &headers)?;
    let pkh = base58_p2pkh_to_hash(&q.address).ok_or(StatusCode::BAD_REQUEST)?;
    if pkh.len() != 20 {
        return Err(StatusCode::BAD_REQUEST);
    }
    let mut pkh_arr = [0u8; 20];
    pkh_arr.copy_from_slice(&pkh);

    let (utxos, height) = {
        let guard = state.chain.lock().unwrap_or_else(|e| e.into_inner());
        let mut items = Vec::new();
        for (outpoint, utxo) in guard.utxos.iter() {
            if let Some(script_pkh) = p2pkh_hash_from_script(&utxo.output.script_pubkey) {
                if script_pkh == pkh_arr {
                    items.push(UtxoItem {
                        txid: hex::encode(outpoint.txid),
                        index: outpoint.index,
                        value: utxo.output.value,
                        height: utxo.height,
                        is_coinbase: utxo.is_coinbase,
                        script_pubkey: hex::encode(&utxo.output.script_pubkey),
                    });
                }
            }
        }
        (items, guard.tip_height())
    };

    Ok(Json(UtxosResponse {
        address: q.address,
        pkh: hex::encode(pkh_arr),
        height,
        utxos,
    }))
}

fn agreement_party_address<'a>(
    agreement: &'a AgreementObject,
    party_id: &str,
) -> Result<&'a str, StatusCode> {
    agreement
        .parties
        .iter()
        .find(|p| p.party_id == party_id)
        .map(|p| p.address.as_str())
        .ok_or(StatusCode::BAD_REQUEST)
}

fn agreement_anchor_value(
    agreement: &AgreementObject,
    role: AgreementAnchorRole,
    milestone_id: Option<&str>,
) -> u64 {
    if let Some(mid) = milestone_id {
        if let Some(ms) = agreement.milestones.iter().find(|m| m.milestone_id == mid) {
            return ms.amount;
        }
    }
    match role {
        AgreementAnchorRole::DepositLock | AgreementAnchorRole::CollateralLock => agreement
            .deposit_rule
            .as_ref()
            .map(|r| r.amount)
            .unwrap_or(agreement.total_amount),
        _ => agreement.total_amount,
    }
}

fn scan_agreement_linked_txs(
    chain: &ChainState,
    agreement: &AgreementObject,
    agreement_hash: &str,
) -> Vec<AgreementLinkedTx> {
    let mut txs = Vec::new();
    for (height, block) in chain.chain.iter().enumerate() {
        for tx in &block.transactions {
            let txid = hex::encode(tx.txid());
            for output in &tx.outputs {
                if let Some(anchor) = parse_agreement_anchor(&output.script_pubkey) {
                    if anchor.agreement_hash == agreement_hash {
                        txs.push(AgreementLinkedTx {
                            txid: txid.clone(),
                            role: anchor.role,
                            milestone_id: anchor.milestone_id.clone(),
                            height: Some(height as u64),
                            confirmed: true,
                            value: agreement_anchor_value(
                                agreement,
                                anchor.role,
                                anchor.milestone_id.as_deref(),
                            ),
                        });
                    }
                }
            }
        }
    }
    txs.sort_by(|a, b| {
        b.height
            .cmp(&a.height)
            .then_with(|| a.txid.cmp(&b.txid))
            .then_with(|| a.milestone_id.cmp(&b.milestone_id))
    });
    txs
}

fn find_confirmed_tx_by_id(chain: &ChainState, txid_hex: &str) -> Result<Transaction, String> {
    let target = hex_to_32(txid_hex.trim()).map_err(|e| format!("invalid funding_txid: {e}"))?;
    for block in &chain.chain {
        for tx in &block.transactions {
            if tx.txid() == target {
                return Ok(tx.clone());
            }
        }
    }
    Err("funding_tx_not_found_on_chain".to_string())
}

fn resolve_agreement_leg_ref(
    chain: &ChainState,
    agreement: &AgreementObject,
    agreement_hash: &str,
    funding_txid: &str,
    htlc_vout: Option<u32>,
    milestone_id: Option<&str>,
) -> Result<AgreementFundingLegRef, String> {
    let tx = find_confirmed_tx_by_id(chain, funding_txid)?;
    let mut refs = extract_agreement_funding_leg_refs_from_tx(&tx, agreement_hash);
    refs.retain(|r| {
        matches!(
            r.role,
            AgreementAnchorRole::Funding
                | AgreementAnchorRole::DepositLock
                | AgreementAnchorRole::OtcSettlement
                | AgreementAnchorRole::MerchantSettlement
        )
    });
    if let Some(vout) = htlc_vout {
        refs.retain(|r| r.htlc_vout == vout);
    }
    if let Some(mid) = milestone_id {
        refs.retain(|r| r.milestone_id.as_deref() == Some(mid));
    }
    if refs.is_empty() {
        return Err("agreement_funding_leg_not_found_or_not_htlc_backed".to_string());
    }
    if refs.len() > 1 {
        return Err("agreement_funding_leg_ambiguous".to_string());
    }
    let resolved = refs.remove(0);
    let expected_amount = if let Some(mid) = resolved.milestone_id.as_deref() {
        agreement
            .milestones
            .iter()
            .find(|m| m.milestone_id == mid)
            .map(|m| m.amount)
            .unwrap_or(agreement.total_amount)
    } else {
        match resolved.role {
            AgreementAnchorRole::DepositLock | AgreementAnchorRole::CollateralLock => agreement
                .deposit_rule
                .as_ref()
                .map(|r| r.amount)
                .unwrap_or(agreement.total_amount),
            _ => agreement.total_amount,
        }
    };
    if resolved.amount != expected_amount {
        return Err("agreement_leg_amount_mismatch".to_string());
    }
    Ok(resolved)
}

fn agreement_observation_trust_note() -> String {
    "Phase 1 agreement funding leg discovery and activity timelines are reconstructed from the supplied canonical agreement object, optional local bundle hints, on-chain anchor observations, and HTLCv1 branch checks. They do not create native consensus agreement state.".to_string()
}

fn verify_agreement_context_bundle(
    agreement: &AgreementObject,
    bundle: Option<&AgreementBundle>,
    agreement_hash: &str,
) -> Result<Option<AgreementBundle>, String> {
    let Some(bundle) = bundle else {
        return Ok(None);
    };
    verify_agreement_bundle(bundle)?;
    if bundle.agreement_hash != agreement_hash {
        return Err("bundle agreement hash does not match supplied agreement".to_string());
    }
    if bundle.agreement != *agreement {
        return Err("bundle agreement object does not match supplied agreement".to_string());
    }
    Ok(Some(bundle.clone()))
}

fn collect_agreement_funding_leg_refs(
    chain: &ChainState,
    agreement: &AgreementObject,
    agreement_hash: &str,
) -> Vec<AgreementFundingLegRef> {
    let linked = scan_agreement_linked_txs(chain, agreement, agreement_hash);
    let mut refs = Vec::new();
    let mut seen = Vec::<String>::new();
    for tx in linked.iter().filter(|tx| {
        matches!(
            tx.role,
            AgreementAnchorRole::Funding
                | AgreementAnchorRole::DepositLock
                | AgreementAnchorRole::OtcSettlement
                | AgreementAnchorRole::MerchantSettlement
        )
    }) {
        if seen.iter().any(|existing| existing == &tx.txid) {
            continue;
        }
        seen.push(tx.txid.clone());
        if let Ok(observed_tx) = find_confirmed_tx_by_id(chain, &tx.txid) {
            refs.extend(extract_agreement_funding_leg_refs_from_tx(
                &observed_tx,
                agreement_hash,
            ));
        }
    }
    refs.sort_by(|a, b| {
        a.milestone_id
            .cmp(&b.milestone_id)
            .then_with(|| a.funding_txid.cmp(&b.funding_txid))
            .then_with(|| a.htlc_vout.cmp(&b.htlc_vout))
    });
    refs
}

fn build_agreement_funding_leg_candidate_views(
    chain: &ChainState,
    agreement: &AgreementObject,
    agreement_hash: &str,
    bundle: Option<&AgreementBundle>,
) -> Result<Vec<AgreementFundingLegCandidateResponse>, String> {
    let linked = scan_agreement_linked_txs(chain, agreement, agreement_hash);
    let refs = collect_agreement_funding_leg_refs(chain, agreement, agreement_hash);
    let candidates = discover_agreement_funding_leg_candidates(
        agreement_hash,
        &linked,
        &refs,
        bundle.map(|b| &b.metadata),
    )?;
    let mut out = Vec::new();
    for candidate in candidates {
        let release_eval = evaluate_agreement_spend_eligibility(
            true,
            chain,
            agreement,
            &AgreementSpendRequest {
                agreement: agreement.clone(),
                funding_txid: candidate.funding_txid.clone(),
                htlc_vout: Some(candidate.htlc_vout),
                milestone_id: candidate.milestone_id.clone(),
                destination_address: None,
                fee_per_byte: None,
                broadcast: Some(false),
                secret_hex: None,
            },
        )?;
        let refund_eval = evaluate_agreement_spend_eligibility(
            false,
            chain,
            agreement,
            &AgreementSpendRequest {
                agreement: agreement.clone(),
                funding_txid: candidate.funding_txid.clone(),
                htlc_vout: Some(candidate.htlc_vout),
                milestone_id: candidate.milestone_id.clone(),
                destination_address: None,
                fee_per_byte: None,
                broadcast: Some(false),
                secret_hex: None,
            },
        )?;
        out.push(AgreementFundingLegCandidateResponse {
            agreement_hash: candidate.agreement_hash,
            funding_txid: candidate.funding_txid,
            htlc_vout: candidate.htlc_vout,
            anchor_vout: candidate.anchor_vout,
            role: candidate.role,
            milestone_id: candidate.milestone_id,
            amount: candidate.amount,
            htlc_backed: candidate.htlc_backed,
            timeout_height: candidate.timeout_height,
            recipient_address: candidate.recipient_address,
            refund_address: candidate.refund_address,
            source_notes: candidate.source_notes,
            release_eligible: release_eval.eligible,
            release_reasons: release_eval.reasons,
            refund_eligible: refund_eval.eligible,
            refund_reasons: refund_eval.reasons,
        });
    }
    Ok(out)
}

fn agreement_spend_trust_note() -> String {
    "Phase 1 agreement release/refund uses existing HTLCv1 spend rules only. Agreement lifecycle, milestone meaning, and settlement context remain metadata/indexed and require off-chain agreement exchange.".to_string()
}

fn evaluate_agreement_spend_eligibility(
    claim: bool,
    chain: &ChainState,
    agreement: &AgreementObject,
    req: &AgreementSpendRequest,
) -> Result<AgreementSpendEligibilityResponse, String> {
    agreement.validate()?;
    let agreement_hash = compute_agreement_hash_hex(agreement)?;
    let leg = resolve_agreement_leg_ref(
        chain,
        agreement,
        &agreement_hash,
        &req.funding_txid,
        req.htlc_vout,
        req.milestone_id.as_deref(),
    )?;
    let outpoint = OutPoint {
        txid: hex_to_32(req.funding_txid.trim())
            .map_err(|e| format!("invalid funding_txid: {e}"))?,
        index: leg.htlc_vout,
    };
    let maybe_utxo = chain.utxos.get(&outpoint).cloned();
    let unspent = maybe_utxo.is_some();
    let tip_height = chain.tip_height();
    let timeout_reached = tip_height >= leg.timeout_height;
    let mut reasons = Vec::new();
    let preimage_required = claim;
    if !unspent {
        reasons.push("funding_leg_already_spent_or_missing".to_string());
    }
    if claim {
        match req.secret_hex.as_deref() {
            Some(secret_hex) => {
                let preimage = hex::decode(secret_hex.trim())
                    .map_err(|_| "secret_hex_invalid_hex".to_string())?;
                let digest = sha256d(&preimage)[..32].to_vec();
                if hex::encode(digest) != leg.expected_hash {
                    reasons.push("secret_hash_mismatch".to_string());
                }
            }
            None => reasons.push("secret_hex_required_for_release".to_string()),
        }
    } else if !timeout_reached {
        reasons.push("refund_timeout_not_reached".to_string());
    }
    let destination_address = req.destination_address.clone().or_else(|| {
        if claim {
            Some(leg.recipient_address.clone())
        } else {
            Some(leg.refund_address.clone())
        }
    });
    let eligible = reasons.is_empty() && destination_address.is_some();
    Ok(AgreementSpendEligibilityResponse {
        agreement_hash,
        agreement_id: agreement.agreement_id.clone(),
        funding_txid: req.funding_txid.clone(),
        htlc_vout: Some(leg.htlc_vout),
        anchor_vout: Some(leg.anchor_vout),
        role: Some(leg.role),
        milestone_id: leg.milestone_id.clone(),
        amount: Some(leg.amount),
        branch: if claim {
            "release".to_string()
        } else {
            "refund".to_string()
        },
        htlc_backed: true,
        funded: true,
        unspent,
        preimage_required,
        timeout_height: Some(leg.timeout_height),
        timeout_reached,
        destination_address,
        expected_hash: Some(leg.expected_hash.clone()),
        recipient_address: Some(leg.recipient_address.clone()),
        refund_address: Some(leg.refund_address.clone()),
        eligible,
        reasons,
        trust_model_note: agreement_spend_trust_note(),
    })
}

fn spend_htlc_from_params(
    claim: bool,
    state: &AppState,
    funding_txid: &str,
    vout: u32,
    destination_address: &str,
    fee_per_byte: Option<u64>,
    broadcast: Option<bool>,
    secret_hex: Option<&str>,
) -> Result<SpendHtlcResponse, StatusCode> {
    {
        let chain = state.chain.lock().unwrap_or_else(|e| e.into_inner());
        let active = chain
            .params
            .htlcv1_activation_height
            .map(|h| chain.height >= h)
            .unwrap_or(false);
        if !active {
            return Err(StatusCode::BAD_REQUEST);
        }
    }
    let txid_arr = hex_to_32(funding_txid.trim()).map_err(|_| StatusCode::BAD_REQUEST)?;
    let (funding_out, tip_height) = {
        let chain = state.chain.lock().unwrap_or_else(|e| e.into_inner());
        let key = OutPoint {
            txid: txid_arr,
            index: vout,
        };
        let utxo = chain
            .utxos
            .get(&key)
            .cloned()
            .ok_or(StatusCode::BAD_REQUEST)?;
        (utxo, chain.tip_height())
    };
    let htlc =
        parse_htlcv1_script(&funding_out.output.script_pubkey).ok_or(StatusCode::BAD_REQUEST)?;
    let signer_pkh = if claim {
        htlc.recipient_pkh
    } else {
        htlc.refund_pkh
    };
    if !claim && tip_height < htlc.timeout_height {
        return Err(StatusCode::BAD_REQUEST);
    }
    let mut wallet = state.wallet.lock().unwrap_or_else(|e| e.into_inner());
    let keys = wallet.keys().map_err(|_| StatusCode::BAD_REQUEST)?;
    let mut key: Option<WalletKey> = None;
    for k in keys {
        let b = hex::decode(&k.pkh).map_err(|_| StatusCode::BAD_REQUEST)?;
        if b.len() != 20 {
            continue;
        }
        let mut pkh = [0u8; 20];
        pkh.copy_from_slice(&b);
        if pkh == signer_pkh {
            key = Some(k);
            break;
        }
    }
    let key = key.ok_or(StatusCode::FORBIDDEN)?;
    let dest = base58_p2pkh_to_hash(destination_address).ok_or(StatusCode::BAD_REQUEST)?;
    if dest.len() != 20 {
        return Err(StatusCode::BAD_REQUEST);
    }
    let mut dest_pkh = [0u8; 20];
    dest_pkh.copy_from_slice(&dest);
    let fee_per_byte = fee_per_byte.unwrap_or(1).max(1);
    let fee = estimate_tx_size(1, 1).saturating_mul(fee_per_byte);
    if funding_out.output.value <= fee {
        return Err(StatusCode::BAD_REQUEST);
    }
    let mut tx = Transaction {
        version: 1,
        inputs: vec![TxInput {
            prev_txid: txid_arr,
            prev_index: vout,
            script_sig: Vec::new(),
            sequence: 0xffff_fffe,
        }],
        outputs: vec![TxOutput {
            value: funding_out.output.value - fee,
            script_pubkey: p2pkh_script(&dest_pkh),
        }],
        locktime: 0,
    };
    let digest = signature_digest(&tx, 0, &funding_out.output.script_pubkey);
    let priv_bytes = hex::decode(&key.privkey).map_err(|_| StatusCode::BAD_REQUEST)?;
    if priv_bytes.len() != 32 {
        return Err(StatusCode::BAD_REQUEST);
    }
    let mut sk_bytes = [0u8; 32];
    sk_bytes.copy_from_slice(&priv_bytes);
    let signing_key =
        SigningKey::from_bytes((&sk_bytes).into()).map_err(|_| StatusCode::BAD_REQUEST)?;
    let sig: Signature = signing_key
        .sign_prehash(&digest)
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    let sig = sig.normalize_s().unwrap_or(sig);
    let mut sig_bytes = sig.to_der().as_bytes().to_vec();
    sig_bytes.push(0x01);
    let pubkey = signing_key
        .verifying_key()
        .to_encoded_point(true)
        .as_bytes()
        .to_vec();
    tx.inputs[0].script_sig = if claim {
        let secret_hex = secret_hex.ok_or(StatusCode::BAD_REQUEST)?;
        let preimage = hex::decode(secret_hex.trim()).map_err(|_| StatusCode::BAD_REQUEST)?;
        encode_htlcv1_claim_witness(&sig_bytes, &pubkey, &preimage)
            .ok_or(StatusCode::BAD_REQUEST)?
    } else {
        encode_htlcv1_refund_witness(&sig_bytes, &pubkey).ok_or(StatusCode::BAD_REQUEST)?
    };
    let fee_checked = {
        let chain = state.chain.lock().unwrap_or_else(|e| e.into_inner());
        chain
            .calculate_fees(&tx)
            .map_err(|_| StatusCode::BAD_REQUEST)?
    };
    let raw = tx.serialize();
    let txid = tx.txid();
    let txid_hex = hex::encode(txid);
    let mut accepted = false;
    if broadcast.unwrap_or(false) {
        let mut mempool = state.mempool.lock().unwrap_or_else(|e| e.into_inner());
        if !mempool.contains(&txid) {
            accepted = mempool
                .add_transaction(tx, raw.clone(), fee_checked)
                .is_ok();
        }
    }
    Ok(SpendHtlcResponse {
        txid: txid_hex,
        accepted,
        raw_tx_hex: hex::encode(raw),
        fee: fee_checked,
    })
}

async fn create_agreement(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumJson(req): AxumJson<AgreementRequest>,
) -> Result<Json<AgreementInspectResponse>, StatusCode> {
    check_rate_with_auth(&state, &addr, &headers)?;
    require_rpc_auth(&headers)?;
    let summary = req
        .agreement
        .summary()
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    Ok(Json(AgreementInspectResponse {
        agreement_hash: summary.agreement_hash.clone(),
        summary,
    }))
}

async fn inspect_agreement(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumJson(req): AxumJson<AgreementRequest>,
) -> Result<Json<AgreementInspectResponse>, StatusCode> {
    create_agreement(ConnectInfo(addr), State(state), headers, AxumJson(req)).await
}

async fn compute_agreement_hash_rpc(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumJson(req): AxumJson<AgreementRequest>,
) -> Result<Json<AgreementHashResponse>, StatusCode> {
    check_rate_with_auth(&state, &addr, &headers)?;
    require_rpc_auth(&headers)?;
    let agreement_hash =
        compute_agreement_hash_hex(&req.agreement).map_err(|_| StatusCode::BAD_REQUEST)?;
    Ok(Json(AgreementHashResponse { agreement_hash }))
}

async fn list_agreement_txs(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumJson(req): AxumJson<AgreementRequest>,
) -> Result<Json<AgreementTxsResponse>, StatusCode> {
    check_rate_with_auth(&state, &addr, &headers)?;
    require_rpc_auth(&headers)?;
    let agreement_hash =
        compute_agreement_hash_hex(&req.agreement).map_err(|_| StatusCode::BAD_REQUEST)?;
    let txs = {
        let chain = state.chain.lock().unwrap_or_else(|e| e.into_inner());
        scan_agreement_linked_txs(&chain, &req.agreement, &agreement_hash)
    };
    Ok(Json(AgreementTxsResponse {
        agreement_hash,
        txs,
    }))
}

async fn agreement_funding_legs(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumJson(req): AxumJson<AgreementContextRequest>,
) -> Result<Json<AgreementFundingLegsResponse>, StatusCode> {
    check_rate_with_auth(&state, &addr, &headers)?;
    require_rpc_auth(&headers)?;
    let agreement_hash =
        compute_agreement_hash_hex(&req.agreement).map_err(|_| StatusCode::BAD_REQUEST)?;
    let bundle =
        verify_agreement_context_bundle(&req.agreement, req.bundle.as_ref(), &agreement_hash)
            .map_err(|_| StatusCode::BAD_REQUEST)?;
    let candidates = {
        let chain = state.chain.lock().unwrap_or_else(|e| e.into_inner());
        build_agreement_funding_leg_candidate_views(
            &chain,
            &req.agreement,
            &agreement_hash,
            bundle.as_ref(),
        )
        .map_err(|_| StatusCode::BAD_REQUEST)?
    };
    Ok(Json(AgreementFundingLegsResponse {
        agreement_hash,
        selection_required: candidates.len() != 1,
        candidates,
        trust_model_note: agreement_observation_trust_note(),
    }))
}

async fn agreement_timeline(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumJson(req): AxumJson<AgreementContextRequest>,
) -> Result<Json<AgreementTimelineResponse>, StatusCode> {
    check_rate_with_auth(&state, &addr, &headers)?;
    require_rpc_auth(&headers)?;
    let agreement_hash =
        compute_agreement_hash_hex(&req.agreement).map_err(|_| StatusCode::BAD_REQUEST)?;
    let bundle =
        verify_agreement_context_bundle(&req.agreement, req.bundle.as_ref(), &agreement_hash)
            .map_err(|_| StatusCode::BAD_REQUEST)?;
    let (lifecycle, events) = {
        let chain = state.chain.lock().unwrap_or_else(|e| e.into_inner());
        let linked = scan_agreement_linked_txs(&chain, &req.agreement, &agreement_hash);
        let lifecycle = derive_lifecycle(
            &req.agreement,
            &agreement_hash,
            linked.clone(),
            chain.tip_height(),
        );
        let refs = collect_agreement_funding_leg_refs(&chain, &req.agreement, &agreement_hash);
        let candidates = discover_agreement_funding_leg_candidates(
            &agreement_hash,
            &linked,
            &refs,
            bundle.as_ref().map(|b| &b.metadata),
        )
        .map_err(|_| StatusCode::BAD_REQUEST)?;
        let candidate_views = build_agreement_funding_leg_candidate_views(
            &chain,
            &req.agreement,
            &agreement_hash,
            bundle.as_ref(),
        )
        .map_err(|_| StatusCode::BAD_REQUEST)?;
        let mut events = build_agreement_activity_timeline(
            &agreement_hash,
            &lifecycle,
            &linked,
            &candidates,
            bundle.as_ref(),
        );
        for candidate in &candidate_views {
            if candidate.release_eligible {
                events.push(AgreementActivityEvent {
                    event_type: "release_eligible".to_string(),
                    source: irium_node_rs::settlement::AgreementActivitySource::HtlcEligibility,
                    txid: Some(candidate.funding_txid.clone()),
                    height: None,
                    timestamp: None,
                    milestone_id: candidate.milestone_id.clone(),
                    note: Some(
                        "HTLC release branch is currently eligible with the provided default context"
                            .to_string(),
                    ),
                });
            }
            if candidate.refund_eligible {
                events.push(AgreementActivityEvent {
                    event_type: "refund_eligible".to_string(),
                    source: irium_node_rs::settlement::AgreementActivitySource::HtlcEligibility,
                    txid: Some(candidate.funding_txid.clone()),
                    height: None,
                    timestamp: None,
                    milestone_id: candidate.milestone_id.clone(),
                    note: Some(
                        "HTLC refund branch is currently eligible with the observed timeout state"
                            .to_string(),
                    ),
                });
            }
        }
        (lifecycle, events)
    };
    Ok(Json(AgreementTimelineResponse {
        agreement_hash,
        lifecycle,
        events,
        trust_model_note: agreement_observation_trust_note(),
    }))
}

async fn agreement_status(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumJson(req): AxumJson<AgreementRequest>,
) -> Result<Json<AgreementStatusResponse>, StatusCode> {
    check_rate_with_auth(&state, &addr, &headers)?;
    require_rpc_auth(&headers)?;
    let agreement_hash =
        compute_agreement_hash_hex(&req.agreement).map_err(|_| StatusCode::BAD_REQUEST)?;
    let lifecycle = {
        let chain = state.chain.lock().unwrap_or_else(|e| e.into_inner());
        let linked = scan_agreement_linked_txs(&chain, &req.agreement, &agreement_hash);
        derive_lifecycle(&req.agreement, &agreement_hash, linked, chain.tip_height())
    };
    Ok(Json(AgreementStatusResponse {
        agreement_hash,
        lifecycle,
    }))
}

async fn agreement_milestones(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumJson(req): AxumJson<AgreementRequest>,
) -> Result<Json<AgreementMilestonesResponse>, StatusCode> {
    check_rate_with_auth(&state, &addr, &headers)?;
    require_rpc_auth(&headers)?;
    let agreement_hash =
        compute_agreement_hash_hex(&req.agreement).map_err(|_| StatusCode::BAD_REQUEST)?;
    let lifecycle = {
        let chain = state.chain.lock().unwrap_or_else(|e| e.into_inner());
        let linked = scan_agreement_linked_txs(&chain, &req.agreement, &agreement_hash);
        derive_lifecycle(&req.agreement, &agreement_hash, linked, chain.tip_height())
    };
    Ok(Json(AgreementMilestonesResponse {
        agreement_hash,
        state: format!("{:?}", lifecycle.state).to_lowercase(),
        milestones: lifecycle.milestones,
    }))
}

async fn verify_agreement_link(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumJson(req): AxumJson<VerifyAgreementLinkRequest>,
) -> Result<Json<VerifyAgreementLinkResponse>, StatusCode> {
    check_rate_with_auth(&state, &addr, &headers)?;
    require_rpc_auth(&headers)?;
    let raw = hex::decode(req.tx_hex.trim()).map_err(|_| StatusCode::BAD_REQUEST)?;
    let tx = decode_full_tx(&raw).map_err(|_| StatusCode::BAD_REQUEST)?;
    let anchors: Vec<AgreementAnchor> = tx
        .outputs
        .iter()
        .filter_map(|o| parse_agreement_anchor(&o.script_pubkey))
        .filter(|a| a.agreement_hash == req.agreement_hash)
        .collect();
    Ok(Json(VerifyAgreementLinkResponse {
        agreement_hash: req.agreement_hash,
        matched: !anchors.is_empty(),
        anchors,
    }))
}

async fn fund_agreement(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumJson(req): AxumJson<FundAgreementRequest>,
) -> Result<Json<FundAgreementResponse>, (StatusCode, String)> {
    let bad =
        |reason: &str| -> (StatusCode, String) { (StatusCode::BAD_REQUEST, reason.to_string()) };
    check_rate_with_auth(&state, &addr, &headers)
        .map_err(|sc| (sc, "rate_limit_or_auth_failed".to_string()))?;
    require_rpc_auth(&headers).map_err(|sc| (sc, format!("rpc_auth_failed:{sc}")))?;
    {
        let chain = state.chain.lock().unwrap_or_else(|e| e.into_inner());
        let active = chain
            .params
            .htlcv1_activation_height
            .map(|h| chain.height >= h)
            .unwrap_or(false);
        if !active {
            return Err(bad("htlcv1_not_active_at_current_height"));
        }
    }

    req.agreement
        .validate()
        .map_err(|_| bad("agreement_invalid"))?;
    let agreement_hash =
        compute_agreement_hash_hex(&req.agreement).map_err(|_| bad("agreement_hash_failed"))?;
    let payer_addr = agreement_party_address(&req.agreement, &req.agreement.payer)
        .map_err(|_| bad("payer_party_missing"))?;
    let payee_addr = agreement_party_address(&req.agreement, &req.agreement.payee)
        .map_err(|_| bad("payee_party_missing"))?;
    let payer_vec =
        base58_p2pkh_to_hash(payer_addr).ok_or_else(|| bad("payer_address_decode_failed"))?;
    let payee_vec =
        base58_p2pkh_to_hash(payee_addr).ok_or_else(|| bad("payee_address_decode_failed"))?;
    if payer_vec.len() != 20 || payee_vec.len() != 20 {
        return Err(bad("party_address_hash_len_invalid"));
    }
    let mut payer_pkh = [0u8; 20];
    payer_pkh.copy_from_slice(&payer_vec);
    let mut payee_pkh = [0u8; 20];
    payee_pkh.copy_from_slice(&payee_vec);
    let legs = build_funding_legs(&req.agreement, payer_pkh, payee_pkh)
        .map_err(|_| bad("build_funding_legs_failed"))?;
    if legs.is_empty() {
        return Err(bad("agreement_has_no_funding_legs"));
    }

    let mut key_map: HashMap<[u8; 20], WalletKey> = HashMap::new();
    {
        let mut wallet = state.wallet.lock().unwrap_or_else(|e| e.into_inner());
        let keys = wallet.keys().map_err(|_| bad("wallet_keys_unavailable"))?;
        for key in keys {
            let bytes = hex::decode(&key.pkh).map_err(|_| bad("wallet_key_pkh_decode_failed"))?;
            if bytes.len() != 20 {
                continue;
            }
            let mut arr = [0u8; 20];
            arr.copy_from_slice(&bytes);
            key_map.insert(arr, key);
        }
    }
    if key_map.is_empty() {
        return Err(bad("wallet_key_map_empty"));
    }

    let (mut utxos, tip_height) = {
        let chain = state.chain.lock().unwrap_or_else(|e| e.into_inner());
        let mut collected = Vec::new();
        for (outpoint, utxo) in chain.utxos.iter() {
            if let Some(script_pkh) = p2pkh_hash_from_script(&utxo.output.script_pubkey) {
                if key_map.contains_key(&script_pkh) {
                    collected.push(WalletUtxo {
                        outpoint: outpoint.clone(),
                        output: utxo.output.clone(),
                        height: utxo.height,
                        is_coinbase: utxo.is_coinbase,
                        pkh: script_pkh,
                    });
                }
            }
        }
        (collected, chain.tip_height())
    };
    if utxos.is_empty() {
        return Err(bad("wallet_utxo_set_empty"));
    }
    utxos.sort_by(|a, b| b.output.value.cmp(&a.output.value));

    let total_required: u64 = legs.iter().map(|l| l.amount).sum();
    let mut fee_per_byte = req.fee_per_byte.unwrap_or(1).max(1);
    if fee_per_byte == 0 {
        fee_per_byte = 1;
    }

    let mut selected: Vec<WalletUtxo> = Vec::new();
    let mut total = 0u64;
    let mut fee = 0u64;
    let base_outputs = legs.len().saturating_mul(2);
    for utxo in &utxos {
        let confirmations = tip_height.saturating_sub(utxo.height);
        if utxo.is_coinbase && confirmations < COINBASE_MATURITY {
            continue;
        }
        selected.push(utxo.clone());
        total = total.saturating_add(utxo.output.value);
        fee = estimate_tx_size(selected.len(), base_outputs + 1).saturating_mul(fee_per_byte);
        if total >= total_required.saturating_add(fee) {
            break;
        }
    }
    if total < total_required.saturating_add(fee) {
        return Err(bad("insufficient_spendable_funds_or_immature_coinbase"));
    }

    let inputs: Vec<TxInput> = selected
        .iter()
        .map(|u| TxInput {
            prev_txid: u.outpoint.txid,
            prev_index: u.outpoint.index,
            script_sig: Vec::new(),
            sequence: 0xffff_ffff,
        })
        .collect();

    let mut outputs = Vec::new();
    let mut funding_outputs = Vec::new();
    for leg in &legs {
        let vout = outputs.len() as u32;
        outputs.push(leg.output.clone());
        outputs.push(
            build_agreement_anchor_output(&AgreementAnchor {
                agreement_hash: agreement_hash.clone(),
                role: leg.role,
                milestone_id: leg.milestone_id.clone(),
            })
            .map_err(|_| bad("build_agreement_anchor_failed"))?,
        );
        funding_outputs.push(AgreementFundingOutput {
            vout,
            role: leg.role,
            milestone_id: leg.milestone_id.clone(),
            amount: leg.amount,
        });
    }

    let change_script = selected
        .first()
        .map(|u| p2pkh_script(&u.pkh))
        .ok_or_else(|| bad("change_output_missing_selected_input"))?;
    let mut change = total.saturating_sub(total_required).saturating_sub(fee);
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
        sign_wallet_inputs(&mut tx, &selected, &key_map)
            .map_err(|_| bad("sign_wallet_inputs_failed"))?;
        let needed_fee = (tx.serialize().len() as u64).saturating_mul(fee_per_byte);
        if needed_fee > fee {
            let extra = needed_fee - fee;
            if change >= extra {
                fee = needed_fee;
                change = change.saturating_sub(extra);
                if let Some(last) = tx.outputs.last_mut() {
                    if p2pkh_hash_from_script(&last.script_pubkey).is_some() {
                        last.value = change;
                    }
                }
                continue;
            } else {
                return Err(bad("fee_recalculation_exceeded_change"));
            }
        }
        break;
    }

    let fee_checked = {
        let chain = state.chain.lock().unwrap_or_else(|e| e.into_inner());
        chain
            .calculate_fees(&tx)
            .map_err(|_| bad("chain_fee_calculation_failed"))?
    };

    let raw = tx.serialize();
    let txid = tx.txid();
    let txid_hex = hex::encode(txid);
    let mut accepted = false;
    if req.broadcast.unwrap_or(false) {
        let mut mempool = state.mempool.lock().unwrap_or_else(|e| e.into_inner());
        if !mempool.contains(&txid) {
            accepted = mempool
                .add_transaction(tx.clone(), raw.clone(), fee_checked)
                .is_ok();
        }
    }

    Ok(Json(FundAgreementResponse {
        agreement_hash,
        txid: txid_hex,
        accepted,
        raw_tx_hex: hex::encode(raw),
        outputs: funding_outputs,
        fee: fee_checked,
    }))
}

async fn get_history(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<BalanceQuery>,
) -> Result<Json<HistoryResponse>, StatusCode> {
    check_rate_with_auth(&state, &addr, &headers)?;
    let pkh = base58_p2pkh_to_hash(&q.address).ok_or(StatusCode::BAD_REQUEST)?;
    if pkh.len() != 20 {
        return Err(StatusCode::BAD_REQUEST);
    }
    let mut pkh_arr = [0u8; 20];
    pkh_arr.copy_from_slice(&pkh);

    let (height, txs) = {
        let guard = state.chain.lock().unwrap_or_else(|e| e.into_inner());
        let mut owned: HashMap<OutPoint, u64> = HashMap::new();
        let mut map: HashMap<[u8; 32], HistoryItem> = HashMap::new();

        for (h, block) in guard.chain.iter().enumerate() {
            let height = h as u64;
            for tx in &block.transactions {
                let txid = tx.txid();
                let is_coinbase = tx.inputs.len() == 1
                    && tx.inputs[0].prev_txid == [0u8; 32]
                    && tx.inputs[0].prev_index == 0xffff_ffff;

                let mut received = 0u64;
                let mut spent = 0u64;

                if !is_coinbase {
                    for input in &tx.inputs {
                        let outpoint = OutPoint {
                            txid: input.prev_txid,
                            index: input.prev_index,
                        };
                        if let Some(value) = owned.remove(&outpoint) {
                            spent = spent.saturating_add(value);
                        }
                    }
                }

                for (idx, output) in tx.outputs.iter().enumerate() {
                    if let Some(script_pkh) = p2pkh_hash_from_script(&output.script_pubkey) {
                        if script_pkh == pkh_arr {
                            received = received.saturating_add(output.value);
                            owned.insert(
                                OutPoint {
                                    txid,
                                    index: idx as u32,
                                },
                                output.value,
                            );
                        }
                    }
                }

                if received > 0 || spent > 0 {
                    let entry = map.entry(txid).or_insert(HistoryItem {
                        txid: hex::encode(txid),
                        height,
                        received: 0,
                        spent: 0,
                        net: 0,
                        is_coinbase,
                    });
                    entry.received = entry.received.saturating_add(received);
                    entry.spent = entry.spent.saturating_add(spent);
                    entry.net = entry.received as i64 - entry.spent as i64;
                }
            }
        }

        let mut txs: Vec<HistoryItem> = map.into_values().collect();
        txs.sort_by(|a, b| b.height.cmp(&a.height));
        (guard.tip_height(), txs)
    };

    Ok(Json(HistoryResponse {
        address: q.address,
        pkh: hex::encode(pkh_arr),
        height,
        txs,
    }))
}

async fn get_fee_estimate(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<FeeEstimateResponse>, StatusCode> {
    check_rate_with_auth(&state, &addr, &headers)?;
    let (min_fee_per_byte, mempool_size) = {
        let mempool = state.mempool.lock().unwrap_or_else(|e| e.into_inner());
        (mempool.min_fee_per_byte(), mempool.len())
    };
    Ok(Json(FeeEstimateResponse {
        min_fee_per_byte,
        mempool_size,
    }))
}

fn sign_wallet_inputs(
    tx: &mut Transaction,
    utxos: &[WalletUtxo],
    key_map: &HashMap<[u8; 20], WalletKey>,
) -> Result<(), StatusCode> {
    for (idx, utxo) in utxos.iter().enumerate() {
        let key = key_map.get(&utxo.pkh).ok_or(StatusCode::BAD_REQUEST)?;
        let priv_bytes = hex::decode(&key.privkey).map_err(|_| StatusCode::BAD_REQUEST)?;
        let signing_key = SigningKey::from_bytes(priv_bytes.as_slice().into())
            .map_err(|_| StatusCode::BAD_REQUEST)?;
        let pub_bytes = signing_key
            .verifying_key()
            .to_encoded_point(true)
            .as_bytes()
            .to_vec();
        let digest = signature_digest(tx, idx, &utxo.output.script_pubkey);
        let verify_key = signing_key.verifying_key();
        let mut sig_opt: Option<Signature> = None;
        for _ in 0..4 {
            let sig_try: Signature = signing_key
                .sign_prehash(&digest)
                .map_err(|_| StatusCode::BAD_REQUEST)?;
            let sig_try = sig_try.normalize_s().unwrap_or(sig_try);
            if verify_key.verify_prehash(&digest, &sig_try).is_ok() {
                sig_opt = Some(sig_try);
                break;
            }
        }
        let sig = sig_opt.ok_or(StatusCode::BAD_REQUEST)?;
        let mut sig_bytes = sig.to_der().as_bytes().to_vec();
        sig_bytes.push(0x01);

        let mut script = Vec::new();
        script.push(sig_bytes.len() as u8);
        script.extend_from_slice(&sig_bytes);
        script.push(pub_bytes.len() as u8);
        script.extend_from_slice(&pub_bytes);
        tx.inputs[idx].script_sig = script;
    }
    Ok(())
}

async fn wallet_create(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumJson(req): AxumJson<WalletCreateRequest>,
) -> Result<Json<WalletCreateResponse>, StatusCode> {
    check_rate_with_auth(&state, &addr, &headers)?;
    require_rpc_auth(&headers)?;

    let mut wallet = state.wallet.lock().unwrap_or_else(|e| e.into_inner());
    if wallet.exists() {
        return Err(StatusCode::CONFLICT);
    }
    let key = wallet
        .create_with_seed(&req.passphrase, req.seed_hex.as_deref())
        .map_err(|_| StatusCode::BAD_REQUEST)?;

    Ok(Json(WalletCreateResponse {
        address: key.address,
        wallet_path: wallet.path().display().to_string(),
    }))
}

async fn wallet_unlock(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumJson(req): AxumJson<WalletUnlockRequest>,
) -> Result<Json<WalletUnlockResponse>, StatusCode> {
    check_rate_with_auth(&state, &addr, &headers)?;
    require_rpc_auth(&headers)?;

    let mut wallet = state.wallet.lock().unwrap_or_else(|e| e.into_inner());
    wallet
        .unlock(&req.passphrase)
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    let addresses = wallet.addresses().map_err(|_| StatusCode::BAD_REQUEST)?;
    let current = wallet
        .current_address()
        .map_err(|_| StatusCode::BAD_REQUEST)?;

    Ok(Json(WalletUnlockResponse {
        addresses,
        current_address: current,
    }))
}

async fn wallet_lock(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<WalletLockResponse>, StatusCode> {
    check_rate_with_auth(&state, &addr, &headers)?;
    require_rpc_auth(&headers)?;

    let mut wallet = state.wallet.lock().unwrap_or_else(|e| e.into_inner());
    wallet.lock();

    Ok(Json(WalletLockResponse { locked: true }))
}

async fn wallet_addresses(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<WalletAddressesResponse>, StatusCode> {
    check_rate_with_auth(&state, &addr, &headers)?;
    require_rpc_auth(&headers)?;

    let mut wallet = state.wallet.lock().unwrap_or_else(|e| e.into_inner());
    let addresses = wallet.addresses().map_err(|_| StatusCode::BAD_REQUEST)?;

    Ok(Json(WalletAddressesResponse { addresses }))
}

async fn wallet_receive(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<WalletReceiveResponse>, StatusCode> {
    check_rate_with_auth(&state, &addr, &headers)?;
    require_rpc_auth(&headers)?;

    let mut wallet = state.wallet.lock().unwrap_or_else(|e| e.into_inner());
    let address = wallet
        .current_address()
        .map_err(|_| StatusCode::BAD_REQUEST)?;

    Ok(Json(WalletReceiveResponse { address }))
}

async fn wallet_new_address(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<WalletReceiveResponse>, StatusCode> {
    check_rate_with_auth(&state, &addr, &headers)?;
    require_rpc_auth(&headers)?;

    let mut wallet = state.wallet.lock().unwrap_or_else(|e| e.into_inner());
    let key = wallet.new_address().map_err(|_| StatusCode::BAD_REQUEST)?;

    Ok(Json(WalletReceiveResponse {
        address: key.address,
    }))
}

async fn wallet_export_wif(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<WalletExportWifQuery>,
) -> Result<Json<WalletExportWifResponse>, StatusCode> {
    check_rate_with_auth(&state, &addr, &headers)?;
    require_rpc_auth(&headers)?;

    let mut wallet = state.wallet.lock().unwrap_or_else(|e| e.into_inner());
    let wif = wallet
        .export_wif(&q.address)
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    Ok(Json(WalletExportWifResponse {
        address: q.address,
        wif,
    }))
}

async fn wallet_import_wif(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumJson(req): AxumJson<WalletImportWifRequest>,
) -> Result<Json<WalletImportWifResponse>, StatusCode> {
    check_rate_with_auth(&state, &addr, &headers)?;
    require_rpc_auth(&headers)?;

    let mut wallet = state.wallet.lock().unwrap_or_else(|e| e.into_inner());
    let key = wallet
        .import_wif(&req.wif)
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    Ok(Json(WalletImportWifResponse {
        address: key.address,
    }))
}

async fn wallet_export_seed(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<WalletSeedResponse>, StatusCode> {
    check_rate_with_auth(&state, &addr, &headers)?;
    require_rpc_auth(&headers)?;

    let mut wallet = state.wallet.lock().unwrap_or_else(|e| e.into_inner());
    let seed_hex = wallet.export_seed().map_err(|_| StatusCode::BAD_REQUEST)?;
    Ok(Json(WalletSeedResponse { seed_hex }))
}

async fn wallet_import_seed(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumJson(req): AxumJson<WalletImportSeedRequest>,
) -> Result<Json<WalletImportSeedResponse>, StatusCode> {
    check_rate_with_auth(&state, &addr, &headers)?;
    require_rpc_auth(&headers)?;

    let mut wallet = state.wallet.lock().unwrap_or_else(|e| e.into_inner());
    let key = wallet
        .import_seed(&req.seed_hex, req.force.unwrap_or(false))
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    Ok(Json(WalletImportSeedResponse {
        address: key.address,
    }))
}

async fn wallet_send(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumJson(req): AxumJson<WalletSendRequest>,
) -> Result<Json<WalletSendResponse>, StatusCode> {
    check_rate_with_auth(&state, &addr, &headers)?;
    require_rpc_auth(&headers)?;

    let amount = parse_irm(&req.amount).map_err(|_| StatusCode::BAD_REQUEST)?;
    if amount == 0 {
        return Err(StatusCode::BAD_REQUEST);
    }

    let (keys, change_address) = {
        let mut wallet = state.wallet.lock().unwrap_or_else(|e| e.into_inner());
        let keys = wallet.keys().map_err(|_| StatusCode::BAD_REQUEST)?;
        let change = if let Some(ref from) = req.from_address {
            from.clone()
        } else {
            wallet
                .current_address()
                .map_err(|_| StatusCode::BAD_REQUEST)?
        };
        (keys, change)
    };

    if keys.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    let mut key_map: HashMap<[u8; 20], WalletKey> = HashMap::new();
    for key in keys {
        let bytes = hex::decode(&key.pkh).map_err(|_| StatusCode::BAD_REQUEST)?;
        if bytes.len() != 20 {
            continue;
        }
        let mut arr = [0u8; 20];
        arr.copy_from_slice(&bytes);
        key_map.insert(arr, key);
    }

    if key_map.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    let mut allowed: HashSet<[u8; 20]> = HashSet::new();
    if let Some(ref from_addr) = req.from_address {
        let pkh = base58_p2pkh_to_hash(from_addr).ok_or(StatusCode::BAD_REQUEST)?;
        if pkh.len() != 20 {
            return Err(StatusCode::BAD_REQUEST);
        }
        let mut arr = [0u8; 20];
        arr.copy_from_slice(&pkh);
        if !key_map.contains_key(&arr) {
            return Err(StatusCode::FORBIDDEN);
        }
        allowed.insert(arr);
    } else {
        for key in key_map.keys() {
            allowed.insert(*key);
        }
    }

    let change_vec = base58_p2pkh_to_hash(&change_address).ok_or(StatusCode::BAD_REQUEST)?;
    if change_vec.len() != 20 {
        return Err(StatusCode::BAD_REQUEST);
    }
    let mut change_pkh = [0u8; 20];
    change_pkh.copy_from_slice(&change_vec);
    if !key_map.contains_key(&change_pkh) {
        return Err(StatusCode::FORBIDDEN);
    }

    let (mut utxos, tip_height) = {
        let chain = state.chain.lock().unwrap_or_else(|e| e.into_inner());
        let mut collected = Vec::new();
        for (outpoint, utxo) in chain.utxos.iter() {
            if let Some(script_pkh) = p2pkh_hash_from_script(&utxo.output.script_pubkey) {
                if allowed.contains(&script_pkh) {
                    collected.push(WalletUtxo {
                        outpoint: outpoint.clone(),
                        output: utxo.output.clone(),
                        height: utxo.height,
                        is_coinbase: utxo.is_coinbase,
                        pkh: script_pkh,
                    });
                }
            }
        }
        (collected, chain.tip_height())
    };

    if utxos.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    let coin_select = req.coin_select.as_deref().unwrap_or("largest");
    match coin_select {
        "smallest" => utxos.sort_by_key(|u| u.output.value),
        _ => utxos.sort_by(|a, b| b.output.value.cmp(&a.output.value)),
    }

    let mut fee_per_byte = {
        let mempool = state.mempool.lock().unwrap_or_else(|e| e.into_inner());
        mempool.min_fee_per_byte().ceil() as u64
    };
    if fee_per_byte == 0 {
        fee_per_byte = 1;
    }
    if let Some(override_fee) = req.fee_per_byte {
        if override_fee > 0 {
            fee_per_byte = override_fee;
        }
    } else if let Some(mode) = req.fee_mode.as_deref() {
        match mode.to_lowercase().as_str() {
            "low" => {}
            "normal" => fee_per_byte = fee_per_byte.saturating_mul(2),
            "high" => fee_per_byte = fee_per_byte.saturating_mul(4),
            _ => {}
        }
    }
    if fee_per_byte == 0 {
        fee_per_byte = 1;
    }

    let mut selected: Vec<WalletUtxo> = Vec::new();
    let mut total = 0u64;
    let mut fee = 0u64;
    for utxo in utxos.iter() {
        let confirmations = tip_height.saturating_sub(utxo.height);
        if utxo.is_coinbase && confirmations < COINBASE_MATURITY {
            continue;
        }
        selected.push(utxo.clone());
        total = total.saturating_add(utxo.output.value);
        let outputs = if total > amount { 2 } else { 1 };
        fee = estimate_tx_size(selected.len(), outputs).saturating_mul(fee_per_byte);
        if total >= amount.saturating_add(fee) {
            break;
        }
    }

    if total < amount.saturating_add(fee) {
        return Err(StatusCode::BAD_REQUEST);
    }

    let to_vec = base58_p2pkh_to_hash(&req.to_address).ok_or(StatusCode::BAD_REQUEST)?;
    if to_vec.len() != 20 {
        return Err(StatusCode::BAD_REQUEST);
    }
    let mut to_pkh = [0u8; 20];
    to_pkh.copy_from_slice(&to_vec);
    let to_script = p2pkh_script(&to_pkh);
    let change_script = p2pkh_script(&change_pkh);

    let mut inputs: Vec<TxInput> = Vec::new();
    for utxo in &selected {
        inputs.push(TxInput {
            prev_txid: utxo.outpoint.txid,
            prev_index: utxo.outpoint.index,
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
        sign_wallet_inputs(&mut tx, &selected, &key_map)?;
        let size = tx.serialize().len() as u64;
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
                return Err(StatusCode::BAD_REQUEST);
            }
        }
        break;
    }

    let fee_checked = {
        let chain = state.chain.lock().unwrap_or_else(|e| e.into_inner());
        chain
            .calculate_fees(&tx)
            .map_err(|_| StatusCode::BAD_REQUEST)?
    };

    let raw = tx.serialize();
    let txid = tx.txid();
    let hex_txid = hex::encode(txid);

    let mut mempool = state.mempool.lock().unwrap_or_else(|e| e.into_inner());
    if mempool.contains(&txid) {
        return Ok(Json(WalletSendResponse {
            txid: hex_txid,
            accepted: false,
            fee: fee_checked,
            total_input: total,
            change,
        }));
    }

    let accepted = match mempool.add_transaction(tx, raw, fee_checked) {
        Ok(_) => true,
        Err(e) => {
            eprintln!("Failed to add tx to mempool: {}", e);
            false
        }
    };

    Ok(Json(WalletSendResponse {
        txid: hex_txid,
        accepted,
        fee: fee_checked,
        total_input: total,
        change,
    }))
}

async fn create_htlc(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumJson(req): AxumJson<CreateHtlcRequest>,
) -> Result<Json<CreateHtlcResponse>, (StatusCode, String)> {
    let bad = |reason: &str| -> (StatusCode, String) {
        eprintln!("[create_htlc] reject reason={}", reason);
        (StatusCode::BAD_REQUEST, reason.to_string())
    };

    check_rate_with_auth(&state, &addr, &headers)
        .map_err(|sc| (sc, "rate_limit_or_auth_failed".to_string()))?;
    require_rpc_auth(&headers).map_err(|sc| (sc, format!("rpc_auth_failed:{sc}")))?;
    {
        let chain = state.chain.lock().unwrap_or_else(|e| e.into_inner());
        let active = chain
            .params
            .htlcv1_activation_height
            .map(|h| chain.height >= h)
            .unwrap_or(false);
        if !active {
            return Err(bad("htlcv1_not_active_at_current_height"));
        }
    }

    let amount = parse_irm(&req.amount).map_err(|_| bad("amount_parse_failed"))?;
    if amount == 0 {
        return Err(bad("amount_zero"));
    }

    let recipient_vec = base58_p2pkh_to_hash(&req.recipient_address)
        .ok_or_else(|| bad("recipient_address_decode_failed"))?;
    let refund_vec = base58_p2pkh_to_hash(&req.refund_address)
        .ok_or_else(|| bad("refund_address_decode_failed"))?;
    if recipient_vec.len() != 20 || refund_vec.len() != 20 {
        return Err(bad("address_hash_len_invalid"));
    }
    let mut recipient_pkh = [0u8; 20];
    recipient_pkh.copy_from_slice(&recipient_vec);
    let mut refund_pkh = [0u8; 20];
    refund_pkh.copy_from_slice(&refund_vec);

    let hash_bytes = hex::decode(req.secret_hash_hex.trim())
        .map_err(|_| bad("secret_hash_hex_decode_failed"))?;
    if hash_bytes.len() != 32 {
        return Err(bad("secret_hash_len_invalid"));
    }
    let mut expected_hash = [0u8; 32];
    expected_hash.copy_from_slice(&hash_bytes);

    let mut key_map: HashMap<[u8; 20], WalletKey> = HashMap::new();
    {
        let mut wallet = state.wallet.lock().unwrap_or_else(|e| e.into_inner());
        let keys = wallet.keys().map_err(|_| bad("wallet_keys_unavailable"))?;
        for key in keys {
            let bytes = hex::decode(&key.pkh).map_err(|_| bad("wallet_key_pkh_decode_failed"))?;
            if bytes.len() != 20 {
                continue;
            }
            let mut arr = [0u8; 20];
            arr.copy_from_slice(&bytes);
            key_map.insert(arr, key);
        }
    }
    if key_map.is_empty() {
        return Err(bad("wallet_key_map_empty"));
    }

    let (mut utxos, tip_height) = {
        let chain = state.chain.lock().unwrap_or_else(|e| e.into_inner());
        let mut collected = Vec::new();
        for (outpoint, utxo) in chain.utxos.iter() {
            if let Some(script_pkh) = p2pkh_hash_from_script(&utxo.output.script_pubkey) {
                if key_map.contains_key(&script_pkh) {
                    collected.push(WalletUtxo {
                        outpoint: outpoint.clone(),
                        output: utxo.output.clone(),
                        height: utxo.height,
                        is_coinbase: utxo.is_coinbase,
                        pkh: script_pkh,
                    });
                }
            }
        }
        (collected, chain.tip_height())
    };

    if utxos.is_empty() {
        return Err(bad("wallet_utxo_set_empty"));
    }

    utxos.sort_by(|a, b| b.output.value.cmp(&a.output.value));

    let mut fee_per_byte = req.fee_per_byte.unwrap_or(1).max(1);
    if fee_per_byte == 0 {
        fee_per_byte = 1;
    }

    let mut selected: Vec<WalletUtxo> = Vec::new();
    let mut total = 0u64;
    let mut fee = 0u64;
    for utxo in utxos.iter() {
        let confirmations = tip_height.saturating_sub(utxo.height);
        if utxo.is_coinbase && confirmations < COINBASE_MATURITY {
            continue;
        }
        selected.push(utxo.clone());
        total = total.saturating_add(utxo.output.value);
        let outputs = if total > amount { 2 } else { 1 };
        fee = estimate_tx_size(selected.len(), outputs).saturating_mul(fee_per_byte);
        if total >= amount.saturating_add(fee) {
            break;
        }
    }

    if total < amount.saturating_add(fee) {
        return Err(bad("insufficient_spendable_funds_or_immature_coinbase"));
    }

    let htlc = HtlcV1Output {
        expected_hash,
        recipient_pkh,
        refund_pkh,
        timeout_height: req.timeout_height,
    };

    let inputs: Vec<TxInput> = selected
        .iter()
        .map(|u| TxInput {
            prev_txid: u.outpoint.txid,
            prev_index: u.outpoint.index,
            script_sig: Vec::new(),
            sequence: 0xffff_ffff,
        })
        .collect();

    let mut outputs = vec![TxOutput {
        value: amount,
        script_pubkey: encode_htlcv1_script(&htlc),
    }];

    let mut change = total.saturating_sub(amount).saturating_sub(fee);
    if change > 0 {
        let change_pkh = selected
            .first()
            .map(|u| u.pkh)
            .ok_or_else(|| bad("change_output_missing_selected_input"))?;
        outputs.push(TxOutput {
            value: change,
            script_pubkey: p2pkh_script(&change_pkh),
        });
    }

    let mut tx = Transaction {
        version: 1,
        inputs,
        outputs,
        locktime: 0,
    };

    for _ in 0..2 {
        sign_wallet_inputs(&mut tx, &selected, &key_map)
            .map_err(|_| bad("sign_wallet_inputs_failed"))?;
        let needed_fee = (tx.serialize().len() as u64).saturating_mul(fee_per_byte);
        if needed_fee > fee {
            let extra = needed_fee - fee;
            if change >= extra {
                fee = needed_fee;
                change = change.saturating_sub(extra);
                if tx.outputs.len() > 1 {
                    tx.outputs[1].value = change;
                }
                continue;
            } else {
                return Err(bad("fee_recalculation_exceeded_change"));
            }
        }
        break;
    }

    let fee_checked = {
        let chain = state.chain.lock().unwrap_or_else(|e| e.into_inner());
        chain
            .calculate_fees(&tx)
            .map_err(|_| bad("chain_fee_calculation_failed"))?
    };

    let raw = tx.serialize();
    let txid = tx.txid();
    let txid_hex = hex::encode(txid);
    let mut accepted = false;

    if req.broadcast.unwrap_or(false) {
        let mut mempool = state.mempool.lock().unwrap_or_else(|e| e.into_inner());
        if !mempool.contains(&txid) {
            match mempool.add_transaction(tx.clone(), raw.clone(), fee_checked) {
                Ok(_) => accepted = true,
                Err(e) => {
                    eprintln!("[create_htlc] mempool_reject reason={}", e);
                    accepted = false;
                }
            }
        }
    }

    Ok(Json(CreateHtlcResponse {
        txid: txid_hex,
        accepted,
        raw_tx_hex: hex::encode(raw),
        htlc_vout: 0,
        expected_hash: hex::encode(expected_hash),
        timeout_height: req.timeout_height,
        recipient_address: req.recipient_address,
        refund_address: req.refund_address,
    }))
}

async fn decode_htlc(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumJson(req): AxumJson<DecodeHtlcRequest>,
) -> Result<Json<DecodeHtlcResponse>, StatusCode> {
    check_rate_with_auth(&state, &addr, &headers)?;
    require_rpc_auth(&headers)?;

    let raw = hex::decode(req.raw_tx_hex.trim()).map_err(|_| StatusCode::BAD_REQUEST)?;
    let tx = decode_full_tx(&raw).map_err(|_| StatusCode::BAD_REQUEST)?;

    if tx.outputs.is_empty() {
        return Ok(Json(DecodeHtlcResponse {
            found: false,
            vout: None,
            output_type: "none".to_string(),
            expected_hash: None,
            timeout_height: None,
            recipient_address: None,
            refund_address: None,
        }));
    }

    let idx = req.vout.unwrap_or(0) as usize;
    if idx >= tx.outputs.len() {
        return Err(StatusCode::BAD_REQUEST);
    }

    let out = &tx.outputs[idx];
    match parse_output_encumbrance(&out.script_pubkey) {
        OutputEncumbrance::HtlcV1(htlc) => Ok(Json(DecodeHtlcResponse {
            found: true,
            vout: Some(idx as u32),
            output_type: "htlcv1".to_string(),
            expected_hash: Some(hex::encode(htlc.expected_hash)),
            timeout_height: Some(htlc.timeout_height),
            recipient_address: Some(base58_p2pkh_from_hash(&htlc.recipient_pkh)),
            refund_address: Some(base58_p2pkh_from_hash(&htlc.refund_pkh)),
        })),
        OutputEncumbrance::P2pkh(_) => Ok(Json(DecodeHtlcResponse {
            found: false,
            vout: Some(idx as u32),
            output_type: "p2pkh".to_string(),
            expected_hash: None,
            timeout_height: None,
            recipient_address: None,
            refund_address: None,
        })),
        OutputEncumbrance::Unknown => Ok(Json(DecodeHtlcResponse {
            found: false,
            vout: Some(idx as u32),
            output_type: "unknown".to_string(),
            expected_hash: None,
            timeout_height: None,
            recipient_address: None,
            refund_address: None,
        })),
    }
}

async fn claim_htlc(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumJson(req): AxumJson<SpendHtlcRequest>,
) -> Result<Json<SpendHtlcResponse>, StatusCode> {
    spend_htlc_internal(true, addr, state, headers, req).await
}

async fn refund_htlc(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumJson(req): AxumJson<SpendHtlcRequest>,
) -> Result<Json<SpendHtlcResponse>, StatusCode> {
    spend_htlc_internal(false, addr, state, headers, req).await
}

async fn agreement_audit(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumJson(req): AxumJson<AgreementContextRequest>,
) -> Result<Json<AgreementAuditRecord>, StatusCode> {
    check_rate_with_auth(&state, &addr, &headers)?;
    require_rpc_auth(&headers)?;
    let agreement_hash =
        compute_agreement_hash_hex(&req.agreement).map_err(|_| StatusCode::BAD_REQUEST)?;
    let bundle =
        verify_agreement_context_bundle(&req.agreement, req.bundle.as_ref(), &agreement_hash)
            .map_err(|_| StatusCode::BAD_REQUEST)?;
    let record = {
        let chain = state.chain.lock().unwrap_or_else(|e| e.into_inner());
        let linked = scan_agreement_linked_txs(&chain, &req.agreement, &agreement_hash);
        let lifecycle = derive_lifecycle(
            &req.agreement,
            &agreement_hash,
            linked.clone(),
            chain.tip_height(),
        );
        let refs = collect_agreement_funding_leg_refs(&chain, &req.agreement, &agreement_hash);
        let candidates = discover_agreement_funding_leg_candidates(
            &agreement_hash,
            &linked,
            &refs,
            bundle.as_ref().map(|b| &b.metadata),
        )
        .map_err(|_| StatusCode::BAD_REQUEST)?;
        let candidate_views = build_agreement_funding_leg_candidate_views(
            &chain,
            &req.agreement,
            &agreement_hash,
            bundle.as_ref(),
        )
        .map_err(|_| StatusCode::BAD_REQUEST)?;
        let mut events = build_agreement_activity_timeline(
            &agreement_hash,
            &lifecycle,
            &linked,
            &candidates,
            bundle.as_ref(),
        );
        for candidate in &candidate_views {
            if candidate.release_eligible {
                events.push(AgreementActivityEvent {
                    event_type: "release_eligible".to_string(),
                    source: irium_node_rs::settlement::AgreementActivitySource::HtlcEligibility,
                    txid: Some(candidate.funding_txid.clone()),
                    height: None,
                    timestamp: None,
                    milestone_id: candidate.milestone_id.clone(),
                    note: Some(
                        "HTLC release branch is currently eligible with the provided default context"
                            .to_string(),
                    ),
                });
            }
            if candidate.refund_eligible {
                events.push(AgreementActivityEvent {
                    event_type: "refund_eligible".to_string(),
                    source: irium_node_rs::settlement::AgreementActivitySource::HtlcEligibility,
                    txid: Some(candidate.funding_txid.clone()),
                    height: None,
                    timestamp: None,
                    milestone_id: candidate.milestone_id.clone(),
                    note: Some(
                        "HTLC refund branch is currently eligible with the observed timeout state"
                            .to_string(),
                    ),
                });
            }
        }
        let funding_legs = candidate_views
            .iter()
            .map(|candidate| AgreementAuditFundingLegRecord {
                funding_txid: candidate.funding_txid.clone(),
                htlc_vout: candidate.htlc_vout,
                anchor_vout: candidate.anchor_vout,
                role: candidate.role,
                milestone_id: candidate.milestone_id.clone(),
                amount: candidate.amount,
                htlc_backed: candidate.htlc_backed,
                timeout_height: candidate.timeout_height,
                recipient_address: candidate.recipient_address.clone(),
                refund_address: candidate.refund_address.clone(),
                source_notes: candidate.source_notes.clone(),
                release_eligible: Some(candidate.release_eligible),
                release_reasons: candidate.release_reasons.clone(),
                refund_eligible: Some(candidate.refund_eligible),
                refund_reasons: candidate.refund_reasons.clone(),
            })
            .collect::<Vec<_>>();
        let selected_leg = if funding_legs.len() == 1 {
            funding_legs.first()
        } else {
            None
        };
        build_agreement_audit_record(
            &req.agreement,
            &agreement_hash,
            bundle.as_ref(),
            &lifecycle,
            &linked,
            &funding_legs,
            selected_leg,
            &events,
            Utc::now().timestamp().max(0) as u64,
            "iriumd_rpc",
        )
    };
    Ok(Json(record))
}

async fn agreement_release_eligibility(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumJson(req): AxumJson<AgreementSpendRequest>,
) -> Result<Json<AgreementSpendEligibilityResponse>, StatusCode> {
    check_rate_with_auth(&state, &addr, &headers)?;
    require_rpc_auth(&headers)?;
    let chain = state.chain.lock().unwrap_or_else(|e| e.into_inner());
    let resp = evaluate_agreement_spend_eligibility(true, &chain, &req.agreement, &req)
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    Ok(Json(resp))
}

async fn agreement_refund_eligibility(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumJson(req): AxumJson<AgreementSpendRequest>,
) -> Result<Json<AgreementSpendEligibilityResponse>, StatusCode> {
    check_rate_with_auth(&state, &addr, &headers)?;
    require_rpc_auth(&headers)?;
    let chain = state.chain.lock().unwrap_or_else(|e| e.into_inner());
    let resp = evaluate_agreement_spend_eligibility(false, &chain, &req.agreement, &req)
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    Ok(Json(resp))
}



async fn submit_proof_rpc(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumJson(req): AxumJson<SubmitProofRequest>,
) -> Result<Json<SubmitProofResponse>, (StatusCode, String)> {
    let bad = |reason: &str| (StatusCode::BAD_REQUEST, reason.to_string());
    check_rate_with_auth(&state, &addr, &headers)
        .map_err(|sc| (sc, format!("rate_limit_or_auth_failed:{sc}")))?;
    require_rpc_auth(&headers).map_err(|sc| (sc, format!("rpc_auth_failed:{sc}")))?;
    let mut store = state.proof_store.lock().unwrap_or_else(|e| e.into_inner());
    let outcome = store.submit(req.proof).map_err(|e| bad(&e))?;
    Ok(Json(SubmitProofResponse {
        proof_id: outcome.proof_id,
        agreement_hash: outcome.agreement_hash,
        accepted: outcome.accepted,
        duplicate: outcome.duplicate,
        message: outcome.message,
    }))
}

async fn list_proofs_rpc(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumJson(req): AxumJson<ListProofsRequest>,
) -> Result<Json<ListProofsResponse>, (StatusCode, String)> {
    check_rate_with_auth(&state, &addr, &headers)
        .map_err(|sc| (sc, format!("rate_limit_or_auth_failed:{sc}")))?;
    require_rpc_auth(&headers).map_err(|sc| (sc, format!("rpc_auth_failed:{sc}")))?;
    let store = state.proof_store.lock().unwrap_or_else(|e| e.into_inner());
    let proofs: Vec<SettlementProof> = store
        .list_by_agreement(&req.agreement_hash)
        .into_iter()
        .cloned()
        .collect();
    let count = proofs.len();
    Ok(Json(ListProofsResponse {
        agreement_hash: req.agreement_hash,
        count,
        proofs,
    }))
}

async fn store_policy_rpc(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumJson(req): AxumJson<StorePolicyRequest>,
) -> Result<Json<StorePolicyResponse>, (StatusCode, String)> {
    let bad = |reason: &str| (StatusCode::BAD_REQUEST, reason.to_string());
    check_rate_with_auth(&state, &addr, &headers)
        .map_err(|sc| (sc, format!("rate_limit_or_auth_failed:{sc}")))?;
    require_rpc_auth(&headers).map_err(|sc| (sc, format!("rpc_auth_failed:{sc}")))?;
    let mut store = state.policy_store.lock().unwrap_or_else(|e| e.into_inner());
    let outcome = store.store(req.policy).map_err(|e| bad(&e))?;
    Ok(Json(StorePolicyResponse {
        policy_id: outcome.policy_id,
        agreement_hash: outcome.agreement_hash,
        accepted: outcome.accepted,
        updated: outcome.updated,
        message: outcome.message,
    }))
}

async fn get_policy_rpc(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumJson(req): AxumJson<GetPolicyRequest>,
) -> Result<Json<GetPolicyResponse>, (StatusCode, String)> {
    check_rate_with_auth(&state, &addr, &headers)
        .map_err(|sc| (sc, format!("rate_limit_or_auth_failed:{sc}")))?;
    require_rpc_auth(&headers).map_err(|sc| (sc, format!("rpc_auth_failed:{sc}")))?;
    let store = state.policy_store.lock().unwrap_or_else(|e| e.into_inner());
    let policy = store.get(&req.agreement_hash).cloned();
    let found = policy.is_some();
    Ok(Json(GetPolicyResponse {
        agreement_hash: req.agreement_hash,
        found,
        policy,
    }))
}

async fn evaluate_policy_rpc(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumJson(req): AxumJson<EvaluatePolicyRequest>,
) -> Result<Json<EvaluatePolicyResponse>, (StatusCode, String)> {
    let bad = |reason: &str| (StatusCode::BAD_REQUEST, reason.to_string());
    check_rate_with_auth(&state, &addr, &headers)
        .map_err(|sc| (sc, format!("rate_limit_or_auth_failed:{sc}")))?;
    require_rpc_auth(&headers).map_err(|sc| (sc, format!("rpc_auth_failed:{sc}")))?;
    let tip_height = {
        let chain = state.chain.lock().unwrap_or_else(|e| e.into_inner());
        chain.tip_height()
    };
    let agreement_hash = compute_agreement_hash_hex(&req.agreement)
        .map_err(|e| bad(&format!("agreement_hash_failed:{e}")))?;
    let policy = {
        let store = state.policy_store.lock().unwrap_or_else(|e| e.into_inner());
        store.get(&agreement_hash).cloned()
    };
    let policy = match policy {
        None => {
            return Ok(Json(EvaluatePolicyResponse {
                agreement_hash,
                policy_found: false,
                policy_id: None,
                tip_height,
                proof_count: 0,
                release_eligible: false,
                refund_eligible: false,
                reason: "no policy stored for this agreement".to_string(),
                evaluated_rules: Vec::new(),
            }));
        }
        Some(p) => p,
    };
    let proofs = {
        let store = state.proof_store.lock().unwrap_or_else(|e| e.into_inner());
        store
            .list_by_agreement(&agreement_hash)
            .into_iter()
            .cloned()
            .collect::<Vec<_>>()
    };
    let proof_count = proofs.len();
    let policy_id = policy.policy_id.clone();
    let result = evaluate_policy(&req.agreement, &policy, &proofs, tip_height)
        .map_err(|e| bad(&format!("policy_eval_failed:{e}")))?;
    Ok(Json(EvaluatePolicyResponse {
        agreement_hash,
        policy_found: true,
        policy_id: Some(policy_id),
        tip_height,
        proof_count,
        release_eligible: result.release_eligible,
        refund_eligible: result.refund_eligible,
        reason: result.reason,
        evaluated_rules: result.evaluated_rules,
    }))
}

async fn check_policy_rpc(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumJson(req): AxumJson<CheckPolicyRequest>,
) -> Result<Json<CheckPolicyResponse>, (StatusCode, String)> {
    let bad = |reason: &str| (StatusCode::BAD_REQUEST, reason.to_string());
    check_rate_with_auth(&state, &addr, &headers)
        .map_err(|sc| (sc, format!("rate_limit_or_auth_failed:{sc}")))?;
    require_rpc_auth(&headers).map_err(|sc| (sc, format!("rpc_auth_failed:{sc}")))?;
    let tip_height = {
        let chain = state.chain.lock().unwrap_or_else(|e| e.into_inner());
        chain.tip_height()
    };
    let agreement_hash = compute_agreement_hash_hex(&req.agreement)
        .map_err(|e| bad(&format!("agreement_hash_failed:{e}")))?;
    let result = evaluate_policy(&req.agreement, &req.policy, &req.proofs, tip_height)
        .map_err(|e| bad(&format!("policy_eval_failed:{e}")))?;
    Ok(Json(CheckPolicyResponse {
        agreement_hash,
        policy_id: req.policy.policy_id.clone(),
        tip_height,
        release_eligible: result.release_eligible,
        refund_eligible: result.refund_eligible,
        reason: result.reason,
        evaluated_rules: result.evaluated_rules,
    }))
}

async fn build_agreement_release(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumJson(req): AxumJson<AgreementSpendRequest>,
) -> Result<Json<AgreementBuildSpendResponse>, (StatusCode, String)> {
    build_agreement_spend_internal(true, addr, state, headers, req).await
}

async fn build_agreement_refund(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumJson(req): AxumJson<AgreementSpendRequest>,
) -> Result<Json<AgreementBuildSpendResponse>, (StatusCode, String)> {
    build_agreement_spend_internal(false, addr, state, headers, req).await
}

async fn build_agreement_spend_internal(
    claim: bool,
    addr: SocketAddr,
    state: AppState,
    headers: HeaderMap,
    req: AgreementSpendRequest,
) -> Result<Json<AgreementBuildSpendResponse>, (StatusCode, String)> {
    let bad =
        |reason: &str| -> (StatusCode, String) { (StatusCode::BAD_REQUEST, reason.to_string()) };
    check_rate_with_auth(&state, &addr, &headers)
        .map_err(|sc| (sc, format!("rate_limit_or_auth_failed:{sc}")))?;
    require_rpc_auth(&headers).map_err(|sc| (sc, format!("rpc_auth_failed:{sc}")))?;
    let eligibility = {
        let chain = state.chain.lock().unwrap_or_else(|e| e.into_inner());
        evaluate_agreement_spend_eligibility(claim, &chain, &req.agreement, &req)
            .map_err(|e| bad(&e))?
    };
    if !eligibility.eligible {
        return Err(bad(&format!(
            "ineligible:{}",
            eligibility.reasons.join(",")
        )));
    }
    let dest = eligibility
        .destination_address
        .clone()
        .ok_or_else(|| bad("destination_address_missing"))?;
    let spend = spend_htlc_from_params(
        claim,
        &state,
        &req.funding_txid,
        eligibility
            .htlc_vout
            .ok_or_else(|| bad("htlc_vout_missing"))?,
        &dest,
        req.fee_per_byte,
        req.broadcast,
        req.secret_hex.as_deref(),
    )
    .map_err(|_| bad("build_htlc_spend_failed"))?;
    Ok(Json(AgreementBuildSpendResponse {
        agreement_hash: eligibility.agreement_hash,
        agreement_id: req.agreement.agreement_id,
        funding_txid: req.funding_txid,
        htlc_vout: eligibility.htlc_vout.unwrap_or(0),
        role: eligibility.role.unwrap_or(AgreementAnchorRole::Funding),
        milestone_id: eligibility.milestone_id,
        branch: if claim {
            "release".to_string()
        } else {
            "refund".to_string()
        },
        destination_address: dest,
        txid: spend.txid,
        accepted: spend.accepted,
        raw_tx_hex: spend.raw_tx_hex,
        fee: spend.fee,
        trust_model_note: agreement_spend_trust_note(),
    }))
}

async fn spend_htlc_internal(
    claim: bool,
    addr: SocketAddr,
    state: AppState,
    headers: HeaderMap,
    req: SpendHtlcRequest,
) -> Result<Json<SpendHtlcResponse>, StatusCode> {
    check_rate_with_auth(&state, &addr, &headers)?;
    require_rpc_auth(&headers)?;
    let resp = spend_htlc_from_params(
        claim,
        &state,
        &req.funding_txid,
        req.vout,
        &req.destination_address,
        req.fee_per_byte,
        req.broadcast,
        req.secret_hex.as_deref(),
    )?;
    Ok(Json(resp))
}

async fn inspect_htlc(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<InspectHtlcQuery>,
) -> Result<Json<InspectHtlcResponse>, StatusCode> {
    check_rate_with_auth(&state, &addr, &headers)?;
    require_rpc_auth(&headers)?;

    let txid = hex_to_32(q.txid.trim()).map_err(|_| StatusCode::BAD_REQUEST)?;
    let key = OutPoint {
        txid,
        index: q.vout,
    };

    let (tip_height, maybe_utxo) = {
        let chain = state.chain.lock().unwrap_or_else(|e| e.into_inner());
        (chain.tip_height(), chain.utxos.get(&key).cloned())
    };

    let Some(utxo) = maybe_utxo else {
        return Ok(Json(InspectHtlcResponse {
            exists: false,
            funded: false,
            unspent: false,
            spent: true,
            spend_type: None,
            claimable_now: false,
            refundable_now: false,
            timeout_height: None,
            expected_hash: None,
            recipient_address: None,
            refund_address: None,
        }));
    };

    let htlc = match parse_htlcv1_script(&utxo.output.script_pubkey) {
        Some(v) => v,
        None => {
            return Ok(Json(InspectHtlcResponse {
                exists: false,
                funded: false,
                unspent: false,
                spent: false,
                spend_type: None,
                claimable_now: false,
                refundable_now: false,
                timeout_height: None,
                expected_hash: None,
                recipient_address: None,
                refund_address: None,
            }))
        }
    };

    Ok(Json(InspectHtlcResponse {
        exists: true,
        funded: true,
        unspent: true,
        spent: false,
        spend_type: None,
        claimable_now: true,
        refundable_now: tip_height >= htlc.timeout_height,
        timeout_height: Some(htlc.timeout_height),
        expected_hash: Some(hex::encode(htlc.expected_hash)),
        recipient_address: Some(base58_p2pkh_from_hash(&htlc.recipient_pkh)),
        refund_address: Some(base58_p2pkh_from_hash(&htlc.refund_pkh)),
    }))
}

async fn get_block_template(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<TemplateQuery>,
) -> Result<Json<BlockTemplateResponse>, StatusCode> {
    check_rate_with_auth(&state, &addr, &headers)?;
    require_rpc_auth(&headers)?;

    let longpoll = q.longpoll.unwrap_or(0) == 1;
    let poll_secs = q.poll_secs.unwrap_or(25).max(1).min(120);
    let max_txs = q.max_txs;
    let min_fee = q.min_fee;

    if longpoll {
        let last_tip = {
            let guard = state.chain.lock().unwrap_or_else(|e| e.into_inner());
            guard
                .chain
                .last()
                .map(|b| hex::encode(b.header.hash()))
                .unwrap_or_else(|| state.genesis_hash.clone())
        };
        let last_mempool = state
            .mempool
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .len();

        let start = std::time::Instant::now();
        while start.elapsed().as_secs() < poll_secs {
            tokio::time::sleep(Duration::from_secs(1)).await;
            let current_tip = {
                let guard = state.chain.lock().unwrap_or_else(|e| e.into_inner());
                guard
                    .chain
                    .last()
                    .map(|b| hex::encode(b.header.hash()))
                    .unwrap_or_else(|| state.genesis_hash.clone())
            };
            let current_mempool = state
                .mempool
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .len();
            if current_tip != last_tip || current_mempool != last_mempool {
                break;
            }
        }
    }

    let (height, prev_hash, bits, target, time) = {
        let guard = state.chain.lock().unwrap_or_else(|e| e.into_inner());
        let tip = guard.chain.last();
        let prev_hash = tip
            .map(|b| hex::encode(b.header.hash()))
            .unwrap_or_else(|| "00".repeat(32));
        let height = guard.height;
        let target = guard.target_for_height(height);
        let bits = target.bits;
        let prev_time = tip.map(|b| b.header.time).unwrap_or(0);
        let now = Utc::now().timestamp() as u32;
        let time = now.max(prev_time.saturating_add(1));
        (height, prev_hash, bits, target_hex(bits), time)
    };

    let mut mempool_entries = state
        .mempool
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .ordered_entries();
    if let Some(min_fee) = min_fee {
        mempool_entries.retain(|e| e.fee_per_byte >= min_fee);
    }
    if let Some(max) = max_txs {
        if mempool_entries.len() > max {
            mempool_entries.truncate(max);
        }
    }
    let mempool_count = mempool_entries.len();
    let mut total_fees = 0u64;
    let txs = mempool_entries
        .into_iter()
        .map(|entry| {
            total_fees = total_fees.saturating_add(entry.fee);
            TemplateTx {
                hex: hex::encode(entry.raw),
                fee: entry.fee,
                relay_addresses: entry.relay_addresses,
            }
        })
        .collect();

    let coinbase_value = block_reward(height).saturating_add(total_fees);

    Ok(Json(BlockTemplateResponse {
        height,
        prev_hash,
        bits: format!("{:08x}", bits),
        target,
        time,
        txs,
        total_fees,
        coinbase_value,
        mempool_count,
    }))
}

fn block_json_for(height: u64, block: &Block) -> Value {
    let header = &block.header;
    serde_json::json!({
        "height": height,
        "header": {
            "version": header.version,
            "prev_hash": hex::encode(header.prev_hash),
            "merkle_root": hex::encode(header.merkle_root),
            "time": header.time,
            "bits": format!("{:08x}", header.bits),
            "nonce": header.nonce,
            "hash": hex::encode(header.hash()),
        },
        "tx_hex": block.transactions.iter().map(|tx| hex::encode(tx.serialize())).collect::<Vec<_>>(),
        "miner_address": miner_address_from_block(block),
        "submit_source": storage::read_block_submit_source(height),
    })
}
async fn get_block(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<BlockQuery>,
) -> Result<Json<Value>, StatusCode> {
    check_rate_with_auth(&state, &addr, &headers)?;
    let guard = state.chain.lock().unwrap_or_else(|e| e.into_inner());
    let idx = q.height as usize;
    if idx >= guard.chain.len() {
        return Err(StatusCode::NOT_FOUND);
    }
    let block = &guard.chain[idx];
    Ok(Json(block_json_for(q.height, block)))
}

async fn get_block_by_hash(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<BlockHashQuery>,
) -> Result<Json<Value>, StatusCode> {
    check_rate_with_auth(&state, &addr, &headers)?;
    let bytes = hex::decode(&q.hash).map_err(|_| StatusCode::BAD_REQUEST)?;
    if bytes.len() != 32 {
        return Err(StatusCode::BAD_REQUEST);
    }
    let mut target = [0u8; 32];
    target.copy_from_slice(&bytes);

    let guard = state.chain.lock().unwrap_or_else(|e| e.into_inner());
    let height = match guard.heights.get(&target) {
        Some(h) => *h,
        None => return Err(StatusCode::NOT_FOUND),
    };
    let block = guard
        .block_store
        .get(&target)
        .or_else(|| guard.chain.get(height as usize))
        .ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(block_json_for(height, block)))
}

async fn get_tx(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<TxQuery>,
) -> Result<Json<TxLookupResponse>, StatusCode> {
    check_rate_with_auth(&state, &addr, &headers)?;
    let bytes = hex::decode(&q.txid).map_err(|_| StatusCode::BAD_REQUEST)?;
    if bytes.len() != 32 {
        return Err(StatusCode::BAD_REQUEST);
    }
    let mut target = [0u8; 32];
    target.copy_from_slice(&bytes);

    let guard = state.chain.lock().unwrap_or_else(|e| e.into_inner());
    for (height, block) in guard.chain.iter().enumerate() {
        for (idx, tx) in block.transactions.iter().enumerate() {
            if tx.txid() == target {
                let output_value: u64 = tx.outputs.iter().map(|o| o.value).sum();
                let is_coinbase = tx.inputs.len() == 1 && tx.inputs[0].prev_txid == [0u8; 32];
                let response = TxLookupResponse {
                    txid: hex::encode(target),
                    height: height as u64,
                    index: idx,
                    block_hash: hex::encode(block.header.hash()),
                    inputs: tx.inputs.len(),
                    outputs: tx.outputs.len(),
                    output_value,
                    is_coinbase,
                    tx_hex: hex::encode(tx.serialize()),
                };
                return Ok(Json(response));
            }
        }
    }
    Err(StatusCode::NOT_FOUND)
}

fn decode_compact_tx(raw: &[u8]) -> Result<Transaction, String> {
    let mut offset = 0usize;

    let read_u8 = |buf: &[u8], off: &mut usize| -> Result<u8, String> {
        if *off >= buf.len() {
            return Err("unexpected EOF".to_string());
        }
        let v = buf[*off];
        *off += 1;
        Ok(v)
    };
    let read_u32 = |buf: &[u8], off: &mut usize| -> Result<u32, String> {
        if *off + 4 > buf.len() {
            return Err("unexpected EOF".to_string());
        }
        let mut bytes = [0u8; 4];
        bytes.copy_from_slice(&buf[*off..*off + 4]);
        *off += 4;
        Ok(u32::from_le_bytes(bytes))
    };
    let read_u64 = |buf: &[u8], off: &mut usize| -> Result<u64, String> {
        if *off + 8 > buf.len() {
            return Err("unexpected EOF".to_string());
        }
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(&buf[*off..*off + 8]);
        *off += 8;
        Ok(u64::from_le_bytes(bytes))
    };
    let read_bytes = |buf: &[u8], off: &mut usize, len: usize| -> Result<Vec<u8>, String> {
        if *off + len > buf.len() {
            return Err("unexpected EOF".to_string());
        }
        let out = buf[*off..*off + len].to_vec();
        *off += len;
        Ok(out)
    };

    let version = read_u32(raw, &mut offset)?;
    let input_count = read_u8(raw, &mut offset)? as usize;
    let mut inputs = Vec::with_capacity(input_count);
    for _ in 0..input_count {
        let prev_len = read_u8(raw, &mut offset)? as usize;
        let prev_txid_bytes = read_bytes(raw, &mut offset, prev_len)?;
        let mut prev_txid = [0u8; 32];
        if prev_txid_bytes.len() == 32 {
            prev_txid.copy_from_slice(&prev_txid_bytes);
        } else {
            let start = 32 - prev_txid_bytes.len();
            prev_txid[start..].copy_from_slice(&prev_txid_bytes);
        }
        let prev_index = read_u32(raw, &mut offset)?;
        let script_sig_len = read_u8(raw, &mut offset)? as usize;
        let script_sig = read_bytes(raw, &mut offset, script_sig_len)?;
        let sequence = read_u32(raw, &mut offset)?;
        inputs.push(TxInput {
            prev_txid,
            prev_index,
            script_sig,
            sequence,
        });
    }

    let output_count = read_u8(raw, &mut offset)? as usize;
    let mut outputs = Vec::with_capacity(output_count);
    for _ in 0..output_count {
        let value = read_u64(raw, &mut offset)?;
        let script_len = read_u8(raw, &mut offset)? as usize;
        let script_pubkey = read_bytes(raw, &mut offset, script_len)?;
        outputs.push(TxOutput {
            value,
            script_pubkey,
        });
    }

    let locktime = read_u32(raw, &mut offset)?;

    Ok(Transaction {
        version,
        inputs,
        outputs,
        locktime,
    })
}

fn target_hex(bits: u32) -> String {
    let target = Target { bits }.to_target();
    let mut bytes = target.to_bytes_be();
    if bytes.len() < 32 {
        let mut padded = vec![0u8; 32 - bytes.len()];
        padded.extend_from_slice(&bytes);
        bytes = padded;
    }
    hex::encode(bytes)
}

fn parse_header_bits(bits_str: &str) -> Result<u32, String> {
    let trimmed = bits_str.trim_start_matches("0x");
    u32::from_str_radix(trimmed, 16).map_err(|e| format!("invalid bits field: {e}"))
}

async fn submit_block(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumJson(req): AxumJson<SubmitBlockRequest>,
) -> Result<Json<Value>, StatusCode> {
    check_rate_with_auth(&state, &addr, &headers)?;
    require_rpc_auth(&headers)?;
    // Rebuild header from JSON.
    let header = &req.header;

    let prev_bytes = hex::decode(&header.prev_hash).map_err(|_| StatusCode::BAD_REQUEST)?;
    let merkle_bytes = hex::decode(&header.merkle_root).map_err(|_| StatusCode::BAD_REQUEST)?;
    let hash_bytes = hex::decode(&header.hash).map_err(|_| StatusCode::BAD_REQUEST)?;
    if prev_bytes.len() != 32 || merkle_bytes.len() != 32 || hash_bytes.len() != 32 {
        return Err(StatusCode::BAD_REQUEST);
    }

    let bits = parse_header_bits(&header.bits).map_err(|_| StatusCode::BAD_REQUEST)?;

    let mut prev_hash = [0u8; 32];
    prev_hash.copy_from_slice(&prev_bytes);
    let mut merkle_root = [0u8; 32];
    merkle_root.copy_from_slice(&merkle_bytes);

    let block_header = BlockHeader {
        version: header.version,
        prev_hash,
        merkle_root,
        time: header.time,
        bits,
        nonce: header.nonce,
    };

    // Sanity-check header hash matches payload.
    let derived_hash = block_header.hash();
    if derived_hash[..] != hash_bytes[..] {
        eprintln!(
            "[submit_block] reject branch=hash_mismatch derived={} provided={}",
            hex::encode(derived_hash),
            hex::encode(&hash_bytes)
        );
        return Err(StatusCode::BAD_REQUEST);
    }

    if req.tx_hex.is_empty() || req.tx_hex.len() > MAX_SUBMIT_BLOCK_TXS {
        return Err(StatusCode::PAYLOAD_TOO_LARGE);
    }

    // Decode full transactions from hex payload.
    let mut txs: Vec<Transaction> = Vec::new();
    for tx_hex in &req.tx_hex {
        let raw = hex::decode(tx_hex).map_err(|_| StatusCode::BAD_REQUEST)?;
        let tx = decode_full_tx(&raw).map_err(|_| StatusCode::BAD_REQUEST)?;
        txs.push(tx);
    }

    let block = Block {
        header: block_header,
        transactions: txs,
    };

    // Apply to chain state under lock, enforcing consensus rules.
    let (new_height, new_tip_hash) = {
        let mut chain = state.chain.lock().unwrap_or_else(|e| e.into_inner());

        // Height must match the next expected block height.
        if req.height != chain.height {
            eprintln!(
                "[submit_block] reject branch=height_mismatch req_height={} chain_height={}",
                req.height, chain.height
            );
            return Err(StatusCode::BAD_REQUEST);
        }

        if let Err(e) = chain.connect_block(block.clone()) {
            eprintln!(
                "[submit_block] reject branch=connect_block_failed err={}",
                e
            );
            return Err(StatusCode::BAD_REQUEST);
        }

        let tip_hash = block.header.hash();
        (chain.tip_height(), hex::encode(tip_hash))
    };

    // If anchors are loaded, enforce anchor consistency on the new tip.
    if let Some(ref anchors) = state.anchors {
        if !anchors.is_chain_valid(new_height, &new_tip_hash) {
            eprintln!(
                "[submit_block] reject branch=anchor_reject height={} tip={}",
                new_height, new_tip_hash
            );
            return Err(StatusCode::BAD_REQUEST);
        }
    }

    // Persist JSON representation alongside miner-written blocks.
    if let Err(_e) = storage::write_block_json(req.height, &block) {
        // The block is already in memory; surface a server error if disk write fails.
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }

    if let Err(_e) =
        storage::write_block_json_with_source(req.height, &block, req.submit_source.as_deref())
    {
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }

    // Remove any included transactions from the HTTP mempool.
    {
        let mut mempool = state.mempool.lock().unwrap_or_else(|e| e.into_inner());
        for tx in block.transactions.iter().skip(1) {
            let txid = tx.txid();
            mempool.remove(&txid);
        }
    }

    // Broadcast the newly accepted block over P2P if enabled.
    if let Some(ref p2p) = state.p2p {
        let mut bytes = Vec::new();
        // Serialize header + transactions using the canonical Rust format.
        //
        // For now we reuse Transaction::serialize() and BlockHeader::serialize()
        // and simply concatenate them; remote peers can interpret this as needed.
        bytes.extend_from_slice(&block.header.serialize());
        for tx in &block.transactions {
            bytes.extend_from_slice(&tx.serialize());
        }
        if let Err(e) = p2p.broadcast_block(&bytes).await {
            eprintln!("Failed to broadcast accepted block over P2P: {}", e);
        }
    }

    Ok(Json(json!({
        "accepted": true,
        "height": req.height,
        "hash": header.hash,
    })))
}

async fn submit_tx(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumJson(req): AxumJson<SubmitTxRequest>,
) -> Result<Json<SubmitTxResponse>, StatusCode> {
    check_rate_with_auth(&state, &addr, &headers)?;
    require_rpc_auth(&headers)?;
    let bytes = match hex::decode(&req.tx_hex) {
        Ok(b) => b,
        Err(_) => return Err(StatusCode::BAD_REQUEST),
    };
    // A compact wallet tx payload may be ambiguously parseable by the full decoder.
    // Try both decoders and select the candidate that passes fee/signature checks.
    let mut candidates: Vec<(&'static str, Transaction)> = Vec::new();
    if let Ok(tx) = decode_compact_tx(&bytes) {
        candidates.push(("compact", tx));
    }
    if let Ok(tx) = decode_full_tx(&bytes) {
        candidates.push(("full", tx));
    }
    if candidates.is_empty() {
        eprintln!("submit_tx decode failed: no valid decoder for payload");
        return Err(StatusCode::BAD_REQUEST);
    }

    let (tx, fee) = {
        let chain = state.chain.lock().unwrap_or_else(|e| e.into_inner());
        let mut last_err: Option<String> = None;
        let mut selected: Option<(Transaction, u64)> = None;

        for (kind, cand) in candidates.into_iter() {
            if cand.inputs.is_empty() || cand.outputs.is_empty() {
                last_err = Some(format!("{} decode yielded empty tx", kind));
                continue;
            }
            match chain.calculate_fees(&cand) {
                Ok(f) => {
                    selected = Some((cand, f));
                    break;
                }
                Err(e) => {
                    last_err = Some(format!("{} decode: {}", kind, e));
                }
            }
        }

        match selected {
            Some(v) => v,
            None => {
                eprintln!(
                    "submit_tx fee validation failed: {}",
                    last_err.unwrap_or_else(|| "no valid decoded transaction".to_string())
                );
                return Err(StatusCode::BAD_REQUEST);
            }
        }
    };

    let txid = tx.txid();

    let mut mempool = state.mempool.lock().unwrap_or_else(|e| e.into_inner());
    let hex_txid = hex::encode(txid);
    if mempool.contains(&txid) {
        return Ok(Json(SubmitTxResponse {
            txid: hex_txid,
            accepted: false,
        }));
    }

    let raw = bytes;
    if let Err(e) = mempool.add_transaction(tx, raw, fee) {
        eprintln!("Failed to add tx to mempool: {}", e);
        return Ok(Json(SubmitTxResponse {
            txid: hex_txid,
            accepted: false,
        }));
    }

    Ok(Json(SubmitTxResponse {
        txid: hex_txid,
        accepted: true,
    }))
}

#[tokio::main(flavor = "multi_thread", worker_threads = 8)]
async fn main() {
    // Load config first so data_dir can influence runtime path selection.
    let node_cfg: Option<NodeConfig> = load_node_config_from_env();
    if let Some(data_dir) = node_cfg
        .as_ref()
        .and_then(|cfg| cfg.data_dir.as_ref())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        if std::env::var_os("IRIUM_DATA_DIR").is_none() {
            std::env::set_var("IRIUM_DATA_DIR", data_dir);
            println!("Using config data_dir via IRIUM_DATA_DIR={}", data_dir);
        }
    }

    let (blocks_dir, state_dir) = storage::ensure_runtime_dirs().unwrap_or_else(|e| {
        eprintln!("Failed to init runtime dirs: {e}");
        std::process::exit(1);
    });
    migrate_legacy_repo_state_dir(&state_dir);
    println!("Using blocks dir: {}", blocks_dir.display());
    println!("Using state dir: {}", state_dir.display());
    println!(
        "To resync, delete ONLY state dir: {} (keep blocks dir: {})",
        state_dir.display(),
        blocks_dir.display()
    );
    storage::init_persist_writer();
    // Initialize chain state with locked genesis.
    let locked = load_locked_genesis().expect("load locked genesis");
    let genesis_hash = locked.header.hash.clone();
    let genesis_hash_lc = genesis_hash.to_lowercase();
    let genesis_block = match block_from_locked(&locked) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("Failed to build genesis block from locked config: {e}");
            std::process::exit(1);
        }
    };

    // Ensure genesis (block 0) exists and matches the locked genesis.
    // If a persisted genesis is corrupt/mismatched, quarantine it and reset volatile state.
    let mut load_persisted = true;
    let block0_path = blocks_dir.join("block_0.json");
    if block0_path.exists() {
        let mut bad = false;
        match fs::read_to_string(&block0_path) {
            Ok(raw) => match serde_json::from_str::<Value>(&raw) {
                Ok(v) => {
                    let file_hash = v
                        .get("header")
                        .and_then(|h| h.get("hash"))
                        .and_then(|h| h.as_str())
                        .unwrap_or("");
                    if file_hash.to_lowercase() != genesis_hash_lc {
                        bad = true;
                    }
                }
                Err(_) => bad = true,
            },
            Err(_) => bad = true,
        }
        if bad {
            eprintln!(
                "[error] Genesis block file (block_0.json) is corrupt or mismatched at {}",
                block0_path.display()
            );
            let ts = Utc::now().timestamp();
            let quarantine = blocks_dir.join(format!("block_0.bad.{ts}.json"));
            let _ = fs::rename(&block0_path, &quarantine);
            eprintln!(
                "[error] Quarantined bad genesis to {}. Reinitializing state dir and resyncing headers from genesis.",
                quarantine.display()
            );
            reinit_state_dir(&state_dir, "genesis mismatch");
            load_persisted = false;
        }
    }
    if !block0_path.exists() {
        if let Err(e) = storage::write_block_json(0, &genesis_block) {
            eprintln!(
                "[warn] Failed to write genesis block_0.json to {}: {}",
                block0_path.display(),
                e
            );
        }
    }

    let pow_limit = Target { bits: 0x1d00_ffff };
    let network = network_kind_from_env();
    let env_override = runtime_htlcv1_env_override();
    let lwma_env_override = runtime_lwma_env_override();
    let htlc_activation = resolved_htlcv1_activation_height(network);
    let lwma_activation = resolved_lwma_activation_height(network);
    match (network, htlc_activation) {
        (NetworkKind::Mainnet, Some(h)) => {
            println!("HTLCv1 mainnet activation height (code-defined): {}", h)
        }
        (NetworkKind::Mainnet, None) => {
            println!("HTLCv1 mainnet activation disabled in code (no activation height set)")
        }
        (_, Some(h)) => println!("HTLCv1 non-mainnet activation height from env: {}", h),
        (_, None) => println!("HTLCv1 non-mainnet activation unset (env not provided)"),
    }
    match (network, lwma_activation) {
        (NetworkKind::Mainnet, Some(h)) => {
            println!("LWMA active on mainnet since height {}", h)
        }
        (NetworkKind::Mainnet, None) => {
            println!("LWMA mainnet activation disabled in code (no activation height set)")
        }
        (_, Some(h)) => println!("LWMA non-mainnet activation height from env: {}", h),
        (_, None) => println!("LWMA non-mainnet activation unset (env not provided)"),
    }
    if network == NetworkKind::Mainnet && env_override.is_some() {
        eprintln!("[warn] Ignoring IRIUM_HTLCV1_ACTIVATION_HEIGHT on mainnet; activation source is code-defined");
    }
    if network == NetworkKind::Mainnet && lwma_env_override.is_some() {
        eprintln!("[warn] Ignoring IRIUM_LWMA_ACTIVATION_HEIGHT on mainnet; activation source is code-defined");
    }
    let params = ChainParams {
        genesis_block: genesis_block.clone(),
        pow_limit,
        htlcv1_activation_height: htlc_activation,
        lwma: LwmaParams::new(lwma_activation, pow_limit),
    };
    let mut state = ChainState::new(params);
    if load_persisted {
        load_persisted_blocks(&mut state, &genesis_hash_lc);
    }
    let shared_state = Arc::new(Mutex::new(state));
    {
        let guard = shared_state.lock().unwrap_or_else(|e| e.into_inner());
        let era = network_era(guard.tip_height());
        println!(
            "[Irium] Network Era: {} — {}",
            era.era_name, era.era_description
        );
    }
    let mempool = Arc::new(Mutex::new(MempoolManager::new(mempool_file(), 1000, 1.0)));
    let limiter = Arc::new(Mutex::new(rate_limiter()));
    let wallet = Arc::new(Mutex::new(
        WalletManager::new(WalletManager::default_path()),
    ));

    // Attempt to load anchors from the repo root if present. On mainnet,
    // the anchors file is shipped and verified out-of-band.
    let anchors = match AnchorManager::from_default_repo_root(PathBuf::from(".")) {
        Ok(a) => Some(a),
        Err(e) => {
            eprintln!("Failed to load anchors: {}", e);
            std::process::exit(1);
        }
    };
    if let Some(a) = anchors.clone() {
        let mut guard = shared_state.lock().unwrap_or_else(|e| e.into_inner());
        guard.set_anchors(a);
    }

    // Enforce anchor consistency if anchors are present
    if let Some(ref a) = anchors {
        if let Some(latest) = a.get_latest_anchor() {
            let expected = latest.hash.to_lowercase();
            let tip_hash = genesis_hash.to_lowercase();
            if latest.height <= 1 && expected != tip_hash {
                eprintln!(
                    "Anchors mismatch: latest anchor hash {} != genesis hash {}",
                    expected, tip_hash
                );
                std::process::exit(1);
            }
        }
    }

    // Validate anchors against genesis if available.
    if let Some(ref a) = anchors {
        if let Some(latest) = a.get_latest_anchor() {
            if latest.height <= 1 && latest.hash.to_lowercase() != genesis_hash.to_lowercase() {
                eprintln!(
                    "Anchors mismatch: latest anchor hash {} != genesis hash {}",
                    latest.hash, genesis_hash
                );
                std::process::exit(1);
            }
        }
    }

    let agent_string =
        std::env::var("IRIUM_NODE_AGENT").unwrap_or_else(|_| "Irium-Node".to_string());
    let relay_address = node_cfg
        .as_ref()
        .and_then(|c| c.relay_address.clone())
        .or_else(|| std::env::var("IRIUM_RELAY_ADDRESS").ok());

    // Set up P2P node if configured.
    let p2p: Option<P2PNode> = if let Some(ref cfg) = node_cfg {
        if let Some(bind) = &cfg.p2p_bind {
            match bind.parse::<SocketAddr>() {
                Ok(addr) => {
                    let node = P2PNode::new(
                        addr,
                        agent_string.clone(),
                        Some(shared_state.clone()),
                        Some(mempool.clone()),
                        relay_address.clone(),
                    );
                    // Start listener in the background.
                    if let Err(e) = node.start().await {
                        eprintln!("Failed to start P2P listener on {}: {}", addr, e);
                        None
                    } else {
                        Some(node)
                    }
                }
                Err(e) => {
                    eprintln!("Invalid P2P bind address {}: {}", bind, e);
                    None
                }
            }
        } else {
            None
        }
    } else {
        None
    };

    // Build seed list: merge config, signed, and runtime seeds; filter locals.
    let default_seed_port: u16 = node_cfg
        .as_ref()
        .and_then(|cfg| cfg.p2p_bind.as_ref())
        .and_then(|b| b.split(":").last())
        .and_then(|p| p.parse().ok())
        .unwrap_or(38291);

    let manual_seeds = load_manual_seeds(node_cfg.as_ref());
    let fallback_seeds = load_builtin_fallback_seeds();
    let dns_seed_hosts = load_dns_seed_hosts(node_cfg.as_ref());
    let signed_seeds = load_signed_seeds();
    let local_ips = local_ip_set(node_cfg.as_ref().and_then(|cfg| cfg.p2p_bind.as_ref()));

    let startup_missing_window = storage::missing_persisted_in_window();

    // Connect to seed peers using a basic handshake and keep retrying in background.
    if let Some(node) = p2p.clone() {
        let manual_seeds = manual_seeds.clone();
        let fallback_seeds = fallback_seeds.clone();
        let dns_seed_hosts = dns_seed_hosts.clone();
        let signed_seeds = signed_seeds.clone();
        let local_ips = local_ips.clone();
        let agent_clone = agent_string.clone();
        let shared_clone = shared_state.clone();
        tokio::spawn(async move {
            let node = node;
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
            let mut no_seed_logged = false;
            let mut bootstrap_logged = false;

            loop {
                let persisted_peers = node.peers_snapshot().await;
                let persisted_seeds =
                    load_persisted_startup_seeds(&persisted_peers, default_seed_port);
                let _ = tokio::time::timeout(
                    std::time::Duration::from_secs(2),
                    node.connect_known_peers(5),
                )
                .await;
                let (dns_seed_addrs, dns_filtered_local) =
                    resolve_dns_seed_addrs(&dns_seed_hosts, default_seed_port, &local_ips);
                let (seeds, mut seed_info) = build_seed_addrs(
                    &persisted_seeds,
                    &manual_seeds,
                    &fallback_seeds,
                    &dns_seed_addrs,
                    &signed_seeds,
                    default_seed_port,
                    &local_ips,
                );
                seed_info.filtered_local += dns_filtered_local;
                if !bootstrap_logged {
                    eprintln!(
                        "[{}] [bootstrap] persisted={} manual={} fallback={} dns_hosts={} dns_addrs={} signed={} filtered_local={}",
                        Utc::now().format("%H:%M:%S"),
                        seed_info.persisted,
                        seed_info.manual,
                        seed_info.fallback,
                        dns_seed_hosts.len(),
                        seed_info.dns,
                        seed_info.signed,
                        seed_info.filtered_local,
                    );
                    bootstrap_logged = true;
                }
                if seeds.is_empty() {
                    if !no_seed_logged {
                        if seed_info.filtered_local > 0 {
                            // All configured bootstrap targets resolved to local addresses.
                        } else {
                            println!(
                                "[{}] no bootstrap seeds resolved; continuing with persisted peers and inbound discovery",
                                Utc::now().format("%H:%M:%S")
                            );
                        }
                        no_seed_logged = true;
                    }
                    interval.tick().await;
                    continue;
                }
                no_seed_logged = false;

                // Dedup seeds to avoid churn when the seed list contains duplicates.
                let mut seeds_seen: std::collections::HashSet<std::net::SocketAddr> =
                    std::collections::HashSet::new();
                let mut seeds_ip_seen: std::collections::HashSet<std::net::IpAddr> =
                    std::collections::HashSet::new();

                for addr in &seeds {
                    if !seeds_seen.insert(*addr) {
                        continue;
                    }
                    if !seeds_ip_seen.insert(addr.ip()) {
                        continue;
                    }
                    if node.is_connected(addr).await {
                        continue;
                    }
                    if node.is_self_ip(addr.ip()).await {
                        continue;
                    }
                    if node.is_ip_connected(addr.ip()).await {
                        continue;
                    }

                    if !node.outbound_dial_allowed(addr).await {
                        continue;
                    }

                    let height = {
                        let chain = shared_clone.lock().unwrap_or_else(|e| e.into_inner());
                        chain.tip_height()
                    };
                    if let Some(suppressed) = dial_seed_log_allowed(0, addr.ip()) {
                        let mut line = format!(
                            "[{}] dialing seed {} (h={})",
                            Utc::now().format("%H:%M:%S"),
                            addr,
                            height
                        );
                        if suppressed > 0 {
                            line.push_str(&format!(" (suppressed {} repeats)", suppressed));
                        }
                        println!("{}", line);
                    }
                    if let Err(e) = node
                        .connect_and_handshake(*addr, height, &agent_clone)
                        .await
                    {
                        let msg = format!("{}", e);
                        if msg.contains("dial backoff") || msg.contains("dial in progress") {
                            continue;
                        }
                        if let Some(suppressed) = dial_seed_log_allowed(1, addr.ip()) {
                            let mut line = format!(
                                "[{}] outbound {} failed: {}",
                                Utc::now().format("%H:%M:%S"),
                                addr,
                                msg
                            );
                            if suppressed > 0 {
                                line.push_str(&format!(" (suppressed {} repeats)", suppressed));
                            }
                            eprintln!("{}", line);
                        }
                    }
                }
                interval.tick().await;
            }
        });
    }

    if startup_missing_window > 0 {
        if let Some(node) = p2p.clone() {
            let shared_for_gap = shared_state.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_secs(2)).await;
                let start_hash = {
                    let guard = shared_for_gap.lock().unwrap_or_else(|e| e.into_inner());
                    guard.tip_hash()
                };
                eprintln!(
                    "[i] persist gap healer: requesting network sync burst for missing persisted window blocks (missing_in_window={})",
                    startup_missing_window
                );
                let _ = node.force_sync_burst_from_tip(start_hash).await;
            });
        }
    }

    let status_height_cache = Arc::new(AtomicU64::new({
        let g = shared_state.lock().unwrap_or_else(|e| e.into_inner());
        g.tip_height()
    }));
    let status_peer_count_cache = Arc::new(AtomicUsize::new(0));
    let status_sybil_cache = Arc::new(AtomicU8::new(0));
    let status_persisted_height_cache = Arc::new(AtomicU64::new(storage::persisted_height()));
    let status_persist_queue_cache = Arc::new(AtomicUsize::new(storage::persist_queue_len()));
    let status_persisted_contiguous_cache =
        Arc::new(AtomicU64::new(storage::persisted_contiguous_height()));
    let status_persisted_max_on_disk_cache =
        Arc::new(AtomicU64::new(storage::persisted_max_height_on_disk()));
    let status_quarantine_count_cache = Arc::new(AtomicU64::new(storage::quarantine_count()));
    let status_persisted_window_tip_cache =
        Arc::new(AtomicU64::new(storage::persisted_window_tip()));
    let status_missing_persisted_in_window_cache =
        Arc::new(AtomicU64::new(storage::missing_persisted_in_window()));
    let status_missing_or_mismatch_in_window_cache =
        Arc::new(AtomicU64::new(storage::missing_or_mismatch_in_window()));
    let status_expected_hash_coverage_in_window_cache =
        Arc::new(AtomicU64::new(storage::expected_hash_coverage_in_window()));
    let status_expected_hash_window_span_cache =
        Arc::new(AtomicU64::new(storage::expected_hash_window_span()));
    let status_best_header_hash_cache = Arc::new(Mutex::new({
        let g = shared_state.lock().unwrap_or_else(|e| e.into_inner());
        let best = compute_best_header_tip_from_chain(&g, &genesis_hash);
        best.hash
    }));

    // Periodic heartbeat logging to surface peers and seedlist.
    if let Some(ref node) = p2p {
        let node_clone = node.clone();
        let chain_clone = shared_state.clone();
        let mempool_clone = mempool.clone();
        let genesis_hex = genesis_hash.clone();
        let status_height = status_height_cache.clone();
        let status_peer_count = status_peer_count_cache.clone();
        let status_sybil = status_sybil_cache.clone();
        tokio::spawn(async move {
            let seed_mgr = SeedlistManager::new(128);
            let mut hb_ticks: u64 = 0;
            let mut maintenance_ticks: u64 = 0;
            let mut last_progress_height: u64 = 0;
            let mut stalled_ticks: u32 = 0;
            let mut last_tip_hash: String = genesis_hex.clone();
            let mut last_tip_bytes: [u8; 32] = [0u8; 32];
            let mut last_best_header_height: u64 = 0;
            let mut last_sync_burst_at: Option<std::time::Instant> = None;
            let mut last_mempool_size: usize = 0;
            let mut last_peer_summary = None;
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                let peers = tokio::time::timeout(
                    std::time::Duration::from_secs(2),
                    node_clone.peers_snapshot(),
                )
                .await
                .unwrap_or_default();
                let current_peer_count = tokio::time::timeout(
                    std::time::Duration::from_millis(250),
                    node_clone.peer_count(),
                )
                .await
                .unwrap_or_else(|_| peers.len());
                maintenance_ticks = maintenance_ticks.wrapping_add(1);
                if maintenance_ticks % 6 == 0 {
                    let maintenance_node = node_clone.clone();
                    tokio::spawn(async move {
                        let _ = tokio::time::timeout(
                            std::time::Duration::from_secs(2),
                            maintenance_node.refresh_seedlist(),
                        )
                        .await;
                        let _ = tokio::time::timeout(
                            std::time::Duration::from_secs(2),
                            maintenance_node.connect_known_peers(3),
                        )
                        .await;
                    });
                }
                let seeds = seed_mgr.merged_seedlist();

                let mut peer_ips = std::collections::HashSet::new();
                let mut peer_list: Vec<String> = Vec::new();
                for p in peers.iter() {
                    let parts: Vec<&str> = p.multiaddr.split("/").collect();
                    if parts.len() >= 5 {
                        let ip = parts[2];
                        if peer_ips.insert(ip.to_string()) {
                            let label = p.agent.clone().unwrap_or_else(|| "peer".to_string());
                            peer_list.push(label);
                        }
                    } else if peer_ips.insert(p.multiaddr.clone()) {
                        let label = p.agent.clone().unwrap_or_else(|| "peer".to_string());
                        peer_list.push(label);
                    }
                }
                if peer_list.is_empty() {
                    peer_list.push("-".to_string());
                }

                let best_peer_height = peers.iter().filter_map(|p| p.last_height).max();

                let mut seed_ips = std::collections::HashSet::new();
                let mut seed_list: Vec<String> = Vec::new();
                for s in seeds.iter() {
                    let parts: Vec<&str> = s.split('/').collect();
                    if parts.len() >= 5 {
                        let ip = parts[2];
                        if seed_ips.insert(ip.to_string()) {
                            seed_list.push(mask_seed_label(ip));
                        }
                    } else if seed_ips.insert(s.clone()) {
                        seed_list.push(mask_seed_label(s));
                    }
                }
                if seed_list.is_empty() {
                    seed_list.push("-".to_string());
                }

                let (local_height, tip_hash, tip_bytes, best_header_height) =
                    match chain_clone.try_lock() {
                        Ok(g) => {
                            let local_height = g.tip_height();
                            let tip_bytes =
                                g.chain.last().map(|b| b.header.hash()).unwrap_or([0u8; 32]);
                            let tip = hex::encode(tip_bytes);
                            let best_hash = g.best_header_hash();
                            let best_header_height = g
                                .headers
                                .get(&best_hash)
                                .map(|hw| hw.height)
                                .or_else(|| g.heights.get(&best_hash).copied())
                                .unwrap_or(local_height)
                                .max(local_height);
                            (local_height, tip, tip_bytes, best_header_height)
                        }
                        Err(_) => (
                            status_height.load(Ordering::Relaxed),
                            last_tip_hash.clone(),
                            last_tip_bytes,
                            last_best_header_height.max(status_height.load(Ordering::Relaxed)),
                        ),
                    };
                last_tip_hash = tip_hash.clone();
                last_tip_bytes = tip_bytes;
                last_best_header_height = best_header_height;

                let mempool_size = match mempool_clone.try_lock() {
                    Ok(g) => g.len(),
                    Err(_) => last_mempool_size,
                };
                last_mempool_size = mempool_size;

                status_height.store(local_height, Ordering::Relaxed);
                status_peer_count.store(current_peer_count, Ordering::Relaxed);
                let sybil_now = match tokio::time::timeout(
                    std::time::Duration::from_millis(250),
                    node_clone.current_sybil_difficulty(),
                )
                .await
                {
                    Ok(v) => v,
                    Err(_) => status_sybil.load(Ordering::Relaxed),
                };
                status_sybil.store(sybil_now, Ordering::Relaxed);

                let next_height = local_height.saturating_add(1);
                let peer_height = best_peer_height.unwrap_or(0);
                let sync_target_height = best_header_height.max(local_height);
                // Keep local height authoritative for validation, but expose the
                // best known header height separately in heartbeat logs so sync
                // lag is visible instead of being flattened into local height.
                let chain_height = sync_target_height;

                hb_ticks = hb_ticks.wrapping_add(1);

                let dbg = node_clone.sync_debug_snapshot().await;
                let peer_dbg = node_clone.peer_telemetry_snapshot().await;
                let seed_count = seed_mgr.merged_seedlist().len();

                // Use validated best-header progress for sync decisions (peer-advertised
                // heights are untrusted and can cause false stall churn).
                let behind = sync_target_height >= local_height.saturating_add(3);
                let header_only_stall = dbg.sync_requests > 0 && dbg.getblocks_inflight == 0;
                let need_sync_burst = behind && (dbg.getblocks_inflight == 0 || header_only_stall);
                if need_sync_burst {
                    let burst_ok = last_sync_burst_at
                        .map(|t| t.elapsed() >= std::time::Duration::from_secs(10))
                        .unwrap_or(true);
                    if burst_ok {
                        let burst_node = node_clone.clone();
                        let start = tip_bytes;
                        tokio::spawn(async move {
                            let _ = burst_node.force_sync_burst_from_tip(start).await;
                        });
                        last_sync_burst_at = Some(std::time::Instant::now());
                    }
                }

                // Periodic sync status line to diagnose stalls quickly.
                if hb_ticks % 6 == 0 {
                    let ahead = sync_target_height.saturating_sub(local_height);
                    eprintln!(
                        "[{}] [🔁 sync] status local={} best_header={} best_peer={} ahead={} peers={} inflight(getheaders)={} inflight(getblocks)={} handshake_failures={}",
                        Utc::now().format("%H:%M:%S"),
                        local_height,
                        sync_target_height,
                        peer_height,
                        ahead,
                        current_peer_count,
                        dbg.sync_requests,
                        dbg.getblocks_inflight,
                        dbg.handshake_failures
                    );
                }

                if hb_ticks % 12 == 0 {
                    let prev = last_peer_summary.unwrap_or(peer_dbg);
                    let delta_attempts = peer_dbg
                        .outbound_dial_attempts_total
                        .saturating_sub(prev.outbound_dial_attempts_total);
                    let delta_success = peer_dbg
                        .outbound_dial_success_total
                        .saturating_sub(prev.outbound_dial_success_total);
                    let delta_fail = peer_dbg
                        .outbound_dial_failure_total
                        .saturating_sub(prev.outbound_dial_failure_total);
                    let delta_timeout = peer_dbg
                        .outbound_dial_failure_timeout_total
                        .saturating_sub(prev.outbound_dial_failure_timeout_total);
                    let delta_refused = peer_dbg
                        .outbound_dial_failure_refused_total
                        .saturating_sub(prev.outbound_dial_failure_refused_total);
                    let delta_no_route = peer_dbg
                        .outbound_dial_failure_no_route_total
                        .saturating_sub(prev.outbound_dial_failure_no_route_total);
                    let delta_banned = peer_dbg
                        .outbound_dial_failure_banned_total
                        .saturating_sub(prev.outbound_dial_failure_banned_total);
                    let delta_other = peer_dbg
                        .outbound_dial_failure_other_total
                        .saturating_sub(prev.outbound_dial_failure_other_total);
                    let delta_handshake = peer_dbg
                        .handshake_failures_total
                        .saturating_sub(prev.handshake_failures_total);
                    let delta_temp_bans = peer_dbg
                        .temp_bans_total
                        .saturating_sub(prev.temp_bans_total);
                    let delta_inbound = peer_dbg
                        .inbound_accepted_total
                        .saturating_sub(prev.inbound_accepted_total);
                    eprintln!(
                        "[{}] [peer_mgr] summary peers={} unique_ips={} attempted={} outbound_attempts={} success={} failure={} timeout={} refused={} no_route={} banned={} other={} inbound_accepted={} handshake_failures={} temp_bans={} banned_peers={} seedlist={}",
                        Utc::now().format("%H:%M:%S"),
                        current_peer_count,
                        peer_dbg.unique_connected_peer_ips,
                        peer_dbg.attempted_peer_ips,
                        delta_attempts,
                        delta_success,
                        delta_fail,
                        delta_timeout,
                        delta_refused,
                        delta_no_route,
                        delta_banned,
                        delta_other,
                        delta_inbound,
                        delta_handshake,
                        delta_temp_bans,
                        peer_dbg.banned_peers,
                        seed_count,
                    );
                    last_peer_summary = Some(peer_dbg);
                }

                // If we're behind OR stuck in header-only mode and not making progress, clear
                // throttles and reconnect peers to kick block body sync.
                if behind || header_only_stall {
                    if local_height == last_progress_height {
                        stalled_ticks = stalled_ticks.saturating_add(1);
                    } else {
                        last_progress_height = local_height;
                        stalled_ticks = 0;
                    }

                    if stalled_ticks >= 12 {
                        eprintln!(
                            "[{}] [🔁 sync] WARN stalled (local={}, best_header={}, best_peer={}, headers_inflight={}, getblocks_inflight={}); clearing sync throttles and reconnecting",
                            Utc::now().format("%H:%M:%S"),
                            local_height,
                            sync_target_height,
                            peer_height,
                            dbg.sync_requests,
                            dbg.getblocks_inflight
                        );
                        let stalled_node = node_clone.clone();
                        tokio::spawn(async move {
                            stalled_node.clear_sync_throttles().await;
                            stalled_node.clear_transient_headers().await;
                            let _ = tokio::time::timeout(
                                std::time::Duration::from_secs(2),
                                stalled_node.connect_known_peers(5),
                            )
                            .await;
                        });
                        stalled_ticks = 0;
                    }
                } else {
                    last_progress_height = local_height;
                    stalled_ticks = 0;
                }

                let peer_sample = peer_list
                    .iter()
                    .take(5)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ");
                let seed_count = seed_list.len();

                if json_log_enabled() {
                    eprintln!(
                        "{}",
                        json!({
                            "ts": Utc::now().format("%H:%M:%S").to_string(),
                            "level": "info",
                            "event": "heartbeat",
                            "height": local_height,
                            "local_height": local_height,
                            "chain_height": chain_height,
                            "peer_height": peer_height,
                            "next_height": next_height,
                            "peers": current_peer_count,
                            "peer_sample": peer_sample,
                            "seed_count": seed_count,
                            "agent": std::env::var("IRIUM_NODE_AGENT").unwrap_or_else(|_| "Irium-Node".to_string()),
                            "tip": tip_hash,
                            "mempool": mempool_size,
                        })
                    );
                } else {
                    let short_tip = tip_hash.chars().take(12).collect::<String>();
                    eprintln!(
                        "[{}] ❤️ heartbeat Irium best height={} 🏠 local height={} 🧱 next height={} ⛏ tip={} 👥 peers={} 🌱 seedlist={} 🧺 mempool={}",
                        Utc::now().format("%H:%M:%S"),
                        chain_height,
                        local_height,
                        next_height,
                        short_tip,
                        current_peer_count,
                        seed_count,
                        mempool_size
                    );
                }
            }
        });
    }

    {
        let chain_for_gap_healer = shared_state.clone();
        let p2p_for_gap_healer = p2p.clone();
        let genesis_hash_for_gap_healer = genesis_hash_lc.clone();
        tokio::spawn(async move {
            let interval_secs = std::env::var("IRIUM_GAP_HEALER_SECS")
                .ok()
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(30)
                .clamp(10, 600);
            let batch_size = std::env::var("IRIUM_GAP_HEALER_BATCH")
                .ok()
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(100)
                .clamp(1, 500);

            loop {
                tokio::time::sleep(Duration::from_secs(interval_secs)).await;

                let mut pending = storage::gap_healer_pending_count();
                if pending == 0 {
                    // Opportunistically scan the next segment above contiguous persisted
                    // height and queue missing/mismatched files for repair.
                    let expected_segment = {
                        let guard = chain_for_gap_healer
                            .lock()
                            .unwrap_or_else(|e| e.into_inner());
                        let tip = guard.tip_height();
                        let start = storage::persisted_contiguous_height().saturating_add(1);
                        if start > tip {
                            Vec::new()
                        } else {
                            let end = (start.saturating_add(799)).min(tip);
                            let mut v =
                                Vec::with_capacity((end.saturating_sub(start) + 1) as usize);
                            for h in start..=end {
                                if let Some(block) = guard.chain.get(h as usize) {
                                    v.push((h, block.header.hash()));
                                }
                            }
                            v
                        }
                    };

                    if !expected_segment.is_empty() {
                        let blocks_dir = storage::blocks_dir();
                        let current_contiguous = storage::persisted_contiguous_height();
                        let (discovered, contiguous_end) = discover_persist_mismatch_heights(
                            &expected_segment,
                            &blocks_dir,
                            &genesis_hash_for_gap_healer,
                            current_contiguous,
                        );

                        if contiguous_end > current_contiguous {
                            storage::force_set_persisted_contiguous_height(contiguous_end);
                        }

                        if !discovered.is_empty() {
                            storage::set_gap_healer_target_heights(&discovered);
                            pending = storage::gap_healer_pending_count();
                            eprintln!(
                                "[i] gap healer discovered backlog: queued={} contiguous={} tip={}",
                                pending,
                                storage::persisted_contiguous_height(),
                                expected_segment.last().map(|(h, _)| *h).unwrap_or(0)
                            );
                        }
                    }
                }

                if pending == 0 {
                    storage::set_gap_healer_active(false);
                    continue;
                }
                storage::set_gap_healer_active(true);

                let adaptive_batch = if pending > 5_000 {
                    batch_size.max(300)
                } else if pending > 2_000 {
                    batch_size.max(200)
                } else if pending > 500 {
                    batch_size.max(120)
                } else {
                    batch_size
                };
                let batch = storage::gap_healer_batch(adaptive_batch);
                if batch.is_empty() {
                    continue;
                }

                let (tip_height, tip_bytes) = {
                    let guard = chain_for_gap_healer
                        .lock()
                        .unwrap_or_else(|e| e.into_inner());
                    let tip_bytes = guard
                        .chain
                        .last()
                        .map(|b| b.header.hash())
                        .unwrap_or([0u8; 32]);
                    (guard.tip_height(), tip_bytes)
                };

                let mut filled: usize = 0;
                for h in batch.iter().copied() {
                    if h > tip_height {
                        continue;
                    }

                    let block_opt = {
                        let guard = chain_for_gap_healer
                            .lock()
                            .unwrap_or_else(|e| e.into_inner());
                        guard.chain.get(h as usize).cloned()
                    };

                    let Some(block) = block_opt else {
                        continue;
                    };

                    match storage::write_block_json(h, &block) {
                        Ok(_) => {
                            if storage::gap_healer_mark_filled(h) {
                                filled = filled.saturating_add(1);
                                // Per-height progress logs are intentionally suppressed to avoid
                                // flooding journals during large backfills; batch summary is logged below.
                            }
                        }
                        Err(e) => {
                            eprintln!("[warn] gap healer persist failed for height {}: {}", h, e);
                        }
                    }
                }

                let remaining = storage::gap_healer_pending_count();
                eprintln!(
                    "[i] gap healer batch: requested={} filled={} remaining={}",
                    batch.len(),
                    filled,
                    remaining
                );

                if filled == 0 {
                    if let Some(node) = p2p_for_gap_healer.clone() {
                        let _ = node.force_sync_burst_from_tip(tip_bytes).await;
                    }
                }
            }
        });
    }
    let app_state = AppState {
        chain: shared_state.clone(),
        genesis_hash: genesis_hash.clone(),
        mempool: mempool.clone(),
        wallet: wallet.clone(),
        anchors,
        p2p,
        limiter: limiter.clone(),
        status_height_cache,
        status_peer_count_cache,
        status_sybil_cache,
        status_persisted_height_cache,
        status_persist_queue_cache,
        status_persisted_contiguous_cache,
        status_persisted_max_on_disk_cache,
        status_quarantine_count_cache,
        status_persisted_window_tip_cache,
        status_missing_persisted_in_window_cache,
        status_missing_or_mismatch_in_window_cache,
        status_expected_hash_coverage_in_window_cache,
        status_expected_hash_window_span_cache,
        status_best_header_hash_cache,
        proof_store: Arc::new(Mutex::new(ProofStore::new(
            storage::state_dir().join("proofs.json"),
        ))),
        policy_store: Arc::new(Mutex::new(PolicyStore::new(
            storage::state_dir().join("policies.json"),
        ))),
    };

    let persist_drain_secs = std::env::var("IRIUM_PERSIST_DRAIN_SECS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(15)
        .clamp(0, 20);
    if persist_drain_secs > 0 {
        #[cfg(unix)]
        {
            tokio::spawn(async move {
                use tokio::signal::unix::{signal, SignalKind};
                let mut sigterm = match signal(SignalKind::terminate()) {
                    Ok(s) => s,
                    Err(_) => return,
                };
                let _ = sigterm.recv().await;
                let ok = storage::drain_persist_queue(Duration::from_secs(persist_drain_secs));
                if ok {
                    eprintln!("[i] persist queue drained on shutdown");
                } else {
                    eprintln!(
                        "[warn] persist queue drain timeout on shutdown; remaining_queue_len={}",
                        storage::persist_queue_len()
                    );
                }
            });
        }
    }

    let mut app = Router::new()
        .route("/status", get(status))
        .route("/peers", get(peers))
        .route("/metrics", get(metrics))
        .route("/rpc/network_hashrate", get(network_hashrate))
        .route("/rpc/mining_metrics", get(mining_metrics))
        .route("/rpc/balance", get(get_balance))
        .route("/rpc/utxos", get(get_utxos))
        .route("/rpc/history", get(get_history))
        .route("/rpc/fee_estimate", get(get_fee_estimate))
        .route("/rpc/utxo", get(get_utxo))
        .route("/rpc/getblocktemplate", get(get_block_template))
        .route("/rpc/block", get(get_block))
        .route("/rpc/block_by_hash", get(get_block_by_hash))
        .route("/rpc/tx", get(get_tx))
        .route("/rpc/submit_block", post(submit_block))
        .route("/rpc/submit_tx", post(submit_tx))
        .route("/rpc/createagreement", post(create_agreement))
        .route("/rpc/inspectagreement", post(inspect_agreement))
        .route(
            "/rpc/computeagreementhash",
            post(compute_agreement_hash_rpc),
        )
        .route("/rpc/fundagreement", post(fund_agreement))
        .route("/rpc/listagreementtxs", post(list_agreement_txs))
        .route("/rpc/agreementfundinglegs", post(agreement_funding_legs))
        .route("/rpc/agreementtimeline", post(agreement_timeline))
        .route("/rpc/agreementaudit", post(agreement_audit))
        .route("/rpc/agreementstatus", post(agreement_status))
        .route("/rpc/agreementmilestones", post(agreement_milestones))
        .route("/rpc/verifyagreementlink", post(verify_agreement_link))
        .route(
            "/rpc/agreementreleaseeligibility",
            post(agreement_release_eligibility),
        )
        .route(
            "/rpc/agreementrefundeligibility",
            post(agreement_refund_eligibility),
        )
        .route("/rpc/buildagreementrelease", post(build_agreement_release))
        .route("/rpc/buildagreementrefund", post(build_agreement_refund))
        .route("/rpc/checkpolicy", post(check_policy_rpc))
        .route("/rpc/submitproof", post(submit_proof_rpc))
        .route("/rpc/listproofs", post(list_proofs_rpc))
        .route("/rpc/storepolicy", post(store_policy_rpc))
        .route("/rpc/getpolicy", post(get_policy_rpc))
        .route("/rpc/evaluatepolicy", post(evaluate_policy_rpc))
        .route("/rpc/createhtlc", post(create_htlc))
        .route("/rpc/decodehtlc", post(decode_htlc))
        .route("/rpc/claimhtlc", post(claim_htlc))
        .route("/rpc/refundhtlc", post(refund_htlc))
        .route("/rpc/inspecthtlc", get(inspect_htlc))
        .route("/wallet/create", post(wallet_create))
        .route("/wallet/unlock", post(wallet_unlock))
        .route("/wallet/lock", post(wallet_lock))
        .route("/wallet/addresses", get(wallet_addresses))
        .route("/wallet/receive", get(wallet_receive))
        .route("/wallet/new_address", post(wallet_new_address))
        .route("/wallet/export_wif", get(wallet_export_wif))
        .route("/wallet/import_wif", post(wallet_import_wif))
        .route("/wallet/export_seed", get(wallet_export_seed))
        .route("/wallet/import_seed", post(wallet_import_seed))
        .route("/wallet/send", post(wallet_send))
        .layer(DefaultBodyLimit::max(rpc_body_limit_bytes()))
        .with_state(app_state.clone());

    if let Some(cors) = cors_layer() {
        app = app.layer(cors);
    }

    let app = app.into_make_service_with_connect_info::<SocketAddr>();

    let status_host =
        std::env::var("IRIUM_STATUS_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let status_port: u16 = std::env::var("IRIUM_STATUS_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8080);
    let status_addr: SocketAddr = format!("{}:{}", status_host, status_port)
        .parse()
        .expect("valid status bind address");

    let status_app = Router::new()
        .route("/status", get(status))
        .with_state(app_state.clone())
        .into_make_service_with_connect_info::<SocketAddr>();

    tokio::spawn(async move {
        match tokio::net::TcpListener::bind(status_addr).await {
            Ok(listener) => {
                if let Err(e) = axum::serve(listener, status_app).await {
                    eprintln!("[warn] HTTP status server exited: {}", e);
                }
            }
            Err(e) => {
                eprintln!(
                    "[warn] failed to bind HTTP status listener on {}: {}",
                    status_addr, e
                );
            }
        }
    });

    let host = std::env::var("IRIUM_NODE_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let port: u16 = std::env::var("IRIUM_NODE_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(38300);

    let addr: SocketAddr = format!("{}:{}", host, port)
        .parse()
        .expect("valid bind address");

    println!("[i] RPC status: https://{}:{}/status", host, port);
    println!(
        "[i] HTTP status: http://{}:{}/status",
        status_host, status_port
    );

    let tls_cert = std::env::var("IRIUM_TLS_CERT").ok();
    let tls_key = std::env::var("IRIUM_TLS_KEY").ok();
    if let (Some(cert_path), Some(key_path)) = (tls_cert, tls_key) {
        let config = RustlsConfig::from_pem_file(cert_path, key_path)
            .await
            .expect("failed to load TLS cert/key");
        if json_log_enabled() {
            println!(
                "{}",
                json!({"ts": Utc::now().format("%H:%M:%S").to_string(), "level": "info", "event": "http_listen", "host": host, "port": port, "scheme": "https"})
            );
        } else {
            println!(
                "Irium Rust node HTTPS listening on https://{}:{}",
                host, port
            );
        }
        axum_server::bind_rustls(addr, config)
            .serve(app)
            .await
            .expect("server error");
    } else {
        if json_log_enabled() {
            println!(
                "{}",
                json!({"ts": Utc::now().format("%H:%M:%S").to_string(), "level": "info", "event": "http_listen", "host": host, "port": port, "scheme": "http"})
            );
        } else {
            println!("Irium Rust node HTTP listening on http://{}:{}", host, port);
        }

        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .expect("bind failed");

        axum::serve(listener, app).await.expect("server error");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::extract::{ConnectInfo, Query, State};
    use axum::http::HeaderMap;
    use axum::Json;
    use irium_node_rs::chain::{block_from_locked, ChainParams, ChainState, OutPoint, UtxoEntry};
    use irium_node_rs::genesis::load_locked_genesis;
    use irium_node_rs::mempool::MempoolManager;
    use irium_node_rs::settlement::{
        AgreementDeadlines, AgreementMilestone, AgreementObject, AgreementParty,
        AgreementRefundCondition, AgreementReleaseCondition, AgreementTemplateType,
        AGREEMENT_SIGNATURE_TYPE_SECP256K1, ApprovedAttestor,
        NoResponseRule, NoResponseTrigger, ProofPolicy, ProofRequirement,
        ProofResolution, ProofSignatureEnvelope, PROOF_POLICY_SCHEMA_ID,
        SETTLEMENT_PROOF_SCHEMA_ID, SettlementProof,
        settlement_proof_payload_bytes,
    };
    use irium_node_rs::wallet_store::WalletManager;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, AtomicU8, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_socket() -> SocketAddr {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 38000)
    }

    static TEST_PATH_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn unique_path(prefix: &str, ext: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let seq = TEST_PATH_COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "{}_{}_{}_{}.{}",
            prefix,
            std::process::id(),
            nanos,
            seq,
            ext
        ))
    }

    fn create_test_state(activation: Option<u64>) -> (AppState, String, String, String) {
        std::env::remove_var("IRIUM_RPC_TOKEN");

        let locked = load_locked_genesis().expect("locked genesis");
        let genesis_block = block_from_locked(&locked).expect("genesis block");
        let pow_limit = genesis_block.header.target();
        let params = ChainParams {
            pow_limit,
            genesis_block,
            htlcv1_activation_height: activation,
            lwma: LwmaParams::new(None, pow_limit),
        };
        let chain = Arc::new(Mutex::new(ChainState::new(params)));

        let mempool_path = unique_path("irium_htlc_mempool", "json");
        let mempool = Arc::new(Mutex::new(MempoolManager::new(mempool_path, 1024, 0.0)));

        let wallet_path = unique_path("irium_htlc_wallet", "json");
        let wallet = Arc::new(Mutex::new(WalletManager::new(wallet_path)));

        let (sender, recipient, refund) = {
            let mut w = wallet.lock().unwrap_or_else(|e| e.into_inner());
            let sender = w
                .create_with_seed(
                    "test-pass",
                    Some("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"),
                )
                .expect("wallet create")
                .address;
            let recipient = w.new_address().expect("recipient address").address;
            let refund = w.new_address().expect("refund address").address;
            (sender, recipient, refund)
        };

        let state = AppState {
            chain,
            genesis_hash: "00".repeat(32),
            mempool,
            wallet,
            anchors: None,
            p2p: None,
            limiter: Arc::new(Mutex::new(rate_limiter())),
            status_height_cache: Arc::new(AtomicU64::new(0)),
            status_peer_count_cache: Arc::new(AtomicUsize::new(0)),
            status_sybil_cache: Arc::new(AtomicU8::new(0)),
            status_persisted_height_cache: Arc::new(AtomicU64::new(0)),
            status_persist_queue_cache: Arc::new(AtomicUsize::new(0)),
            status_persisted_contiguous_cache: Arc::new(AtomicU64::new(0)),
            status_persisted_max_on_disk_cache: Arc::new(AtomicU64::new(0)),
            status_quarantine_count_cache: Arc::new(AtomicU64::new(0)),
            status_persisted_window_tip_cache: Arc::new(AtomicU64::new(0)),
            status_missing_persisted_in_window_cache: Arc::new(AtomicU64::new(0)),
            status_missing_or_mismatch_in_window_cache: Arc::new(AtomicU64::new(0)),
            status_expected_hash_coverage_in_window_cache: Arc::new(AtomicU64::new(0)),
            status_expected_hash_window_span_cache: Arc::new(AtomicU64::new(0)),
            status_best_header_hash_cache: Arc::new(Mutex::new(String::new())),
            proof_store: Arc::new(Mutex::new(ProofStore::new(
                unique_path("irium_proofs", "json"),
            ))),
            policy_store: Arc::new(Mutex::new(PolicyStore::new(
                unique_path("irium_policies", "json"),
            ))),
        };

        (state, sender, recipient, refund)
    }

    fn add_wallet_utxo(state: &AppState, address: &str, value: u64) {
        let pkh = base58_p2pkh_to_hash(address).expect("address decode");
        let mut pkh20 = [0u8; 20];
        pkh20.copy_from_slice(&pkh);
        let mut chain = state.chain.lock().unwrap_or_else(|e| e.into_inner());
        let tip = chain.tip_height();
        chain.utxos.insert(
            OutPoint {
                txid: [0x55; 32],
                index: 0,
            },
            UtxoEntry {
                output: TxOutput {
                    value,
                    script_pubkey: p2pkh_script(&pkh20),
                },
                height: tip,
                is_coinbase: false,
            },
        );
    }

    fn apply_tx_to_chain_for_test(state: &AppState, tx: &Transaction) {
        let mut chain = state.chain.lock().unwrap_or_else(|e| e.into_inner());
        for input in &tx.inputs {
            let _ = chain.utxos.remove(&OutPoint {
                txid: input.prev_txid,
                index: input.prev_index,
            });
        }
        let txid = tx.txid();
        let h = chain.tip_height();
        for (idx, out) in tx.outputs.iter().cloned().enumerate() {
            chain.utxos.insert(
                OutPoint {
                    txid,
                    index: idx as u32,
                },
                UtxoEntry {
                    output: out,
                    height: h,
                    is_coinbase: false,
                },
            );
        }
    }

    fn htlc_create_request(
        recipient: &str,
        refund: &str,
        secret_hash_hex: String,
        timeout_height: u64,
    ) -> CreateHtlcRequest {
        CreateHtlcRequest {
            amount: "5.00000000".to_string(),
            recipient_address: recipient.to_string(),
            refund_address: refund.to_string(),
            secret_hash_hex,
            timeout_height,
            fee_per_byte: Some(1),
            broadcast: Some(false),
        }
    }

    #[tokio::test]
    async fn rpc_createhtlc_rejected_before_activation() {
        let (state, sender, recipient, refund) = create_test_state(Some(50));
        add_wallet_utxo(&state, &sender, 20_000_000_000);

        {
            let mut chain = state.chain.lock().unwrap_or_else(|e| e.into_inner());
            chain.height = 49;
        }

        let req = htlc_create_request(&recipient, &refund, "11".repeat(32), 100);
        let res = create_htlc(
            ConnectInfo(test_socket()),
            State(state),
            HeaderMap::new(),
            Json(req),
        )
        .await;

        assert!(matches!(res, Err((StatusCode::BAD_REQUEST, _))));
    }

    #[tokio::test]
    async fn template_includes_htlc_at_activation_boundary() {
        let (state, sender, recipient, refund) = create_test_state(Some(50));
        add_wallet_utxo(&state, &sender, 20_000_000_000);

        {
            let mut chain = state.chain.lock().unwrap_or_else(|e| e.into_inner());
            chain.height = 50; // candidate block height == activation height
        }

        let create = create_htlc(
            ConnectInfo(test_socket()),
            State(state.clone()),
            HeaderMap::new(),
            Json(CreateHtlcRequest {
                amount: "5.00000000".to_string(),
                recipient_address: recipient,
                refund_address: refund,
                secret_hash_hex: "33".repeat(32),
                timeout_height: 120,
                fee_per_byte: Some(1),
                broadcast: Some(true),
            }),
        )
        .await
        .expect("createhtlc")
        .0;

        let tpl = get_block_template(
            ConnectInfo(test_socket()),
            State(state.clone()),
            HeaderMap::new(),
            Query(TemplateQuery {
                longpoll: None,
                poll_secs: None,
                max_txs: None,
                min_fee: None,
            }),
        )
        .await
        .expect("template")
        .0;

        assert!(
            tpl.txs.iter().any(|t| t.hex == create.raw_tx_hex),
            "HTLC tx should be in template at activation height"
        );

        {
            let mut chain = state.chain.lock().unwrap_or_else(|e| e.into_inner());
            chain.height = 51; // activation + 1 should also include
        }

        let tpl_after = get_block_template(
            ConnectInfo(test_socket()),
            State(state),
            HeaderMap::new(),
            Query(TemplateQuery {
                longpoll: None,
                poll_secs: None,
                max_txs: None,
                min_fee: None,
            }),
        )
        .await
        .expect("template after")
        .0;

        assert!(
            tpl_after.txs.iter().any(|t| t.hex == create.raw_tx_hex),
            "HTLC tx should remain template-eligible after activation"
        );
    }

    #[tokio::test]
    async fn rpc_create_decode_inspect_and_claim_flow() {
        let (state, sender, recipient, refund) = create_test_state(Some(0));
        add_wallet_utxo(&state, &sender, 20_000_000_000);

        let secret = b"swap-secret";
        let secret_hash_hex = hex::encode(Sha256::digest(secret));

        let create = create_htlc(
            ConnectInfo(test_socket()),
            State(state.clone()),
            HeaderMap::new(),
            Json(htlc_create_request(
                &recipient,
                &refund,
                secret_hash_hex.clone(),
                10,
            )),
        )
        .await
        .expect("createhtlc")
        .0;

        let decode = decode_htlc(
            ConnectInfo(test_socket()),
            State(state.clone()),
            HeaderMap::new(),
            Json(DecodeHtlcRequest {
                raw_tx_hex: create.raw_tx_hex.clone(),
                vout: Some(0),
            }),
        )
        .await
        .expect("decodehtlc")
        .0;

        assert!(decode.found);
        assert_eq!(decode.output_type, "htlcv1");
        assert_eq!(
            decode.expected_hash.as_deref(),
            Some(secret_hash_hex.as_str())
        );

        let funding_tx =
            decode_full_tx(&hex::decode(&create.raw_tx_hex).expect("hex")).expect("tx decode");
        apply_tx_to_chain_for_test(&state, &funding_tx);

        let inspect_funded = inspect_htlc(
            ConnectInfo(test_socket()),
            State(state.clone()),
            HeaderMap::new(),
            Query(InspectHtlcQuery {
                txid: create.txid.clone(),
                vout: 0,
            }),
        )
        .await
        .expect("inspect funded")
        .0;

        assert!(inspect_funded.exists);
        assert!(inspect_funded.unspent);

        let claim = claim_htlc(
            ConnectInfo(test_socket()),
            State(state.clone()),
            HeaderMap::new(),
            Json(SpendHtlcRequest {
                funding_txid: create.txid.clone(),
                vout: 0,
                destination_address: recipient.clone(),
                fee_per_byte: Some(1),
                broadcast: Some(false),
                secret_hex: Some(hex::encode(secret)),
            }),
        )
        .await
        .expect("claim")
        .0;

        let claim_tx =
            decode_full_tx(&hex::decode(&claim.raw_tx_hex).expect("hex")).expect("claim decode");
        apply_tx_to_chain_for_test(&state, &claim_tx);

        let inspect_spent = inspect_htlc(
            ConnectInfo(test_socket()),
            State(state),
            HeaderMap::new(),
            Query(InspectHtlcQuery {
                txid: create.txid,
                vout: 0,
            }),
        )
        .await
        .expect("inspect spent")
        .0;

        assert!(!inspect_spent.exists);
        assert!(inspect_spent.spent);
    }

    #[tokio::test]
    async fn rpc_claim_wrong_preimage_rejected() {
        let (state, sender, recipient, refund) = create_test_state(Some(0));
        add_wallet_utxo(&state, &sender, 20_000_000_000);
        {
            let mut chain = state.chain.lock().unwrap_or_else(|e| e.into_inner());
            chain.height = 19;
        }

        let secret_hash_hex = hex::encode(Sha256::digest(b"right-secret"));
        let create = create_htlc(
            ConnectInfo(test_socket()),
            State(state.clone()),
            HeaderMap::new(),
            Json(htlc_create_request(
                &recipient,
                &refund,
                secret_hash_hex,
                40,
            )),
        )
        .await
        .expect("createhtlc")
        .0;

        let funding_tx =
            decode_full_tx(&hex::decode(&create.raw_tx_hex).expect("hex")).expect("tx decode");
        apply_tx_to_chain_for_test(&state, &funding_tx);

        let wrong = claim_htlc(
            ConnectInfo(test_socket()),
            State(state),
            HeaderMap::new(),
            Json(SpendHtlcRequest {
                funding_txid: create.txid,
                vout: 0,
                destination_address: recipient,
                fee_per_byte: Some(1),
                broadcast: Some(true),
                secret_hex: Some(hex::encode("wrong-secret")),
            }),
        )
        .await;

        assert!(matches!(wrong, Err(StatusCode::BAD_REQUEST)));
    }

    #[tokio::test]
    async fn rpc_refund_before_and_after_timeout() {
        let (state, sender, recipient, refund) = create_test_state(Some(0));
        add_wallet_utxo(&state, &sender, 20_000_000_000);

        let timeout = 20u64;
        {
            let mut chain = state.chain.lock().unwrap_or_else(|e| e.into_inner());
            chain.height = 19;
        }

        let create = create_htlc(
            ConnectInfo(test_socket()),
            State(state.clone()),
            HeaderMap::new(),
            Json(htlc_create_request(
                &recipient,
                &refund,
                "22".repeat(32),
                timeout,
            )),
        )
        .await
        .expect("createhtlc")
        .0;

        let funding_tx =
            decode_full_tx(&hex::decode(&create.raw_tx_hex).expect("hex")).expect("tx decode");
        apply_tx_to_chain_for_test(&state, &funding_tx);

        let early = refund_htlc(
            ConnectInfo(test_socket()),
            State(state.clone()),
            HeaderMap::new(),
            Json(SpendHtlcRequest {
                funding_txid: create.txid.clone(),
                vout: 0,
                destination_address: refund.clone(),
                fee_per_byte: Some(1),
                broadcast: Some(false),
                secret_hex: None,
            }),
        )
        .await;
        assert!(matches!(early, Err(StatusCode::BAD_REQUEST)));

        {
            let mut chain = state.chain.lock().unwrap_or_else(|e| e.into_inner());
            chain.height = timeout + 1;
        }

        let late = refund_htlc(
            ConnectInfo(test_socket()),
            State(state),
            HeaderMap::new(),
            Json(SpendHtlcRequest {
                funding_txid: create.txid,
                vout: 0,
                destination_address: refund,
                fee_per_byte: Some(1),
                broadcast: Some(false),
                secret_hex: None,
            }),
        )
        .await;

        assert!(late.is_ok());
    }

    #[tokio::test]
    async fn rpc_decodehtlc_reports_non_htlc_output() {
        let mut tx = Transaction {
            version: 1,
            inputs: vec![TxInput {
                prev_txid: [0u8; 32],
                prev_index: 0,
                script_sig: vec![],
                sequence: 0xffff_ffff,
            }],
            outputs: vec![TxOutput {
                value: 1,
                script_pubkey: p2pkh_script(&[1u8; 20]),
            }],
            locktime: 0,
        };
        tx.inputs[0].script_sig = vec![1, 0, 1, 2];

        let (state, _, _, _) = create_test_state(None);
        let decode = decode_htlc(
            ConnectInfo(test_socket()),
            State(state),
            HeaderMap::new(),
            Json(DecodeHtlcRequest {
                raw_tx_hex: hex::encode(tx.serialize()),
                vout: Some(0),
            }),
        )
        .await
        .expect("decode")
        .0;

        assert!(!decode.found);
        assert_eq!(decode.output_type, "p2pkh");
    }

    fn confirm_tx_for_agreement_scan(state: &AppState, tx: &Transaction) {
        let mut chain = state.chain.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(block) = chain.chain.last_mut() {
            block.transactions.push(tx.clone());
        }
    }

    fn sample_agreement_for_test(
        payer_address: &str,
        payee_address: &str,
        secret_hash_hex: String,
        timeout_height: u64,
    ) -> AgreementObject {
        AgreementObject {
            schema_id: Some(irium_node_rs::settlement::AGREEMENT_SCHEMA_ID_V1.to_string()),
            agreement_id: "agr-node-1".to_string(),
            version: 1,
            template_type: AgreementTemplateType::SimpleReleaseRefund,
            parties: vec![
                AgreementParty {
                    party_id: "payer".to_string(),
                    display_name: "Payer".to_string(),
                    address: payer_address.to_string(),
                    role: Some("payer".to_string()),
                },
                AgreementParty {
                    party_id: "payee".to_string(),
                    display_name: "Payee".to_string(),
                    address: payee_address.to_string(),
                    role: Some("payee".to_string()),
                },
            ],
            payer: "payer".to_string(),
            payee: "payee".to_string(),
            mediator_reference: None,
            total_amount: 500_000_000,
            network_marker: "IRIUM".to_string(),
            creation_time: 1_700_000_000,
            deadlines: AgreementDeadlines {
                settlement_deadline: Some(timeout_height.saturating_sub(10)),
                refund_deadline: Some(timeout_height),
                dispute_window: None,
            },
            release_conditions: vec![AgreementReleaseCondition {
                mode: "secret_preimage".to_string(),
                secret_hash_hex: Some(secret_hash_hex),
                release_authorizer: Some("payer".to_string()),
                notes: None,
            }],
            refund_conditions: vec![AgreementRefundCondition {
                refund_address: payer_address.to_string(),
                timeout_height,
                notes: None,
            }],
            milestones: vec![],
            deposit_rule: None,
            proof_policy_reference: Some("phase2-placeholder".to_string()),
            document_hash: "22".repeat(32),
            metadata_hash: None,
            invoice_reference: None,
            external_reference: None,
            disputed_metadata_only: false,
            asset_reference: None,
            payment_reference: None,
            purpose_reference: None,
            release_summary: Some("Release when the payer reveals the preimage".to_string()),
            refund_summary: Some("Refund after timeout to the payer".to_string()),
            attestor_reference: None,
            resolver_reference: None,
            notes: Some("fixture".to_string()),
        }
    }

    fn milestone_agreement_for_test(
        payer_address: &str,
        payee_address: &str,
        timeout_height: u64,
    ) -> (AgreementObject, Vec<Vec<u8>>) {
        let s1 = b"milestone-one".to_vec();
        let s2 = b"milestone-two".to_vec();
        let agreement = AgreementObject {
            schema_id: Some(irium_node_rs::settlement::AGREEMENT_SCHEMA_ID_V1.to_string()),
            agreement_id: "agr-node-ms".to_string(),
            version: 1,
            template_type: AgreementTemplateType::MilestoneSettlement,
            parties: vec![
                AgreementParty {
                    party_id: "payer".to_string(),
                    display_name: "Payer".to_string(),
                    address: payer_address.to_string(),
                    role: Some("payer".to_string()),
                },
                AgreementParty {
                    party_id: "payee".to_string(),
                    display_name: "Payee".to_string(),
                    address: payee_address.to_string(),
                    role: Some("payee".to_string()),
                },
            ],
            payer: "payer".to_string(),
            payee: "payee".to_string(),
            mediator_reference: None,
            total_amount: 700_000_000,
            network_marker: "IRIUM".to_string(),
            creation_time: 1_700_000_000,
            deadlines: AgreementDeadlines {
                settlement_deadline: Some(timeout_height.saturating_sub(10)),
                refund_deadline: Some(timeout_height),
                dispute_window: None,
            },
            release_conditions: vec![AgreementReleaseCondition {
                mode: "secret_preimage".to_string(),
                secret_hash_hex: Some(hex::encode(sha256d(&s1))),
                release_authorizer: Some("payer".to_string()),
                notes: None,
            }],
            refund_conditions: vec![AgreementRefundCondition {
                refund_address: payer_address.to_string(),
                timeout_height,
                notes: None,
            }],
            milestones: vec![
                AgreementMilestone {
                    milestone_id: "ms1".to_string(),
                    title: "Kickoff".to_string(),
                    amount: 300_000_000,
                    recipient_address: payee_address.to_string(),
                    refund_address: payer_address.to_string(),
                    secret_hash_hex: hex::encode(sha256d(&s1)),
                    timeout_height,
                    metadata_hash: None,
                },
                AgreementMilestone {
                    milestone_id: "ms2".to_string(),
                    title: "Delivery".to_string(),
                    amount: 400_000_000,
                    recipient_address: payee_address.to_string(),
                    refund_address: payer_address.to_string(),
                    secret_hash_hex: hex::encode(sha256d(&s2)),
                    timeout_height: timeout_height + 5,
                    metadata_hash: None,
                },
            ],
            deposit_rule: None,
            proof_policy_reference: Some("phase2-placeholder".to_string()),
            document_hash: "22".repeat(32),
            metadata_hash: None,
            invoice_reference: None,
            external_reference: None,
            disputed_metadata_only: false,
            asset_reference: None,
            payment_reference: None,
            purpose_reference: None,
            release_summary: Some("Milestone releases remain off-chain coordination unless a specific HTLC branch is exercised".to_string()),
            refund_summary: Some("Refund each milestone after its timeout back to the payer".to_string()),
            attestor_reference: None,
            resolver_reference: None,
            notes: Some("fixture".to_string()),
        };
        (agreement, vec![s1, s2])
    }

    async fn fund_agreement_for_test(
        state: AppState,
        agreement: AgreementObject,
    ) -> FundAgreementResponse {
        fund_agreement(
            ConnectInfo(test_socket()),
            State(state),
            HeaderMap::new(),
            Json(FundAgreementRequest {
                agreement,
                fee_per_byte: Some(1),
                broadcast: Some(false),
            }),
        )
        .await
        .expect("fund agreement")
        .0
    }

    #[tokio::test]
    async fn agreement_release_eligibility_when_preimage_available() {
        let (state, sender, recipient, _) = create_test_state(Some(0));
        add_wallet_utxo(&state, &sender, 20_000_000_000);
        let (agreement, secrets) = milestone_agreement_for_test(&sender, &recipient, 120);
        let funded = fund_agreement_for_test(state.clone(), agreement.clone()).await;
        let funding_tx = decode_full_tx(&hex::decode(&funded.raw_tx_hex).unwrap()).unwrap();
        apply_tx_to_chain_for_test(&state, &funding_tx);
        confirm_tx_for_agreement_scan(&state, &funding_tx);
        let target = funded
            .outputs
            .iter()
            .find(|o| o.milestone_id.as_deref() == Some("ms1"))
            .expect("ms1 output")
            .vout;
        let resp = agreement_release_eligibility(
            ConnectInfo(test_socket()),
            State(state),
            HeaderMap::new(),
            Json(AgreementSpendRequest {
                agreement,
                funding_txid: funded.txid,
                htlc_vout: Some(target),
                milestone_id: Some("ms1".to_string()),
                destination_address: Some(recipient),
                fee_per_byte: None,
                broadcast: Some(false),
                secret_hex: Some(hex::encode(&secrets[0])),
            }),
        )
        .await
        .expect("eligibility")
        .0;
        assert!(resp.eligible);
        assert!(resp.preimage_required);
        assert_eq!(resp.branch, "release");
    }

    #[tokio::test]
    async fn agreement_release_ineligible_without_required_preimage() {
        let (state, sender, recipient, _) = create_test_state(Some(0));
        add_wallet_utxo(&state, &sender, 20_000_000_000);
        let (agreement, _) = milestone_agreement_for_test(&sender, &recipient, 120);
        let funded = fund_agreement_for_test(state.clone(), agreement.clone()).await;
        let funding_tx = decode_full_tx(&hex::decode(&funded.raw_tx_hex).unwrap()).unwrap();
        apply_tx_to_chain_for_test(&state, &funding_tx);
        confirm_tx_for_agreement_scan(&state, &funding_tx);
        let target = funded
            .outputs
            .iter()
            .find(|o| o.milestone_id.as_deref() == Some("ms1"))
            .expect("ms1 output")
            .vout;
        let resp = agreement_release_eligibility(
            ConnectInfo(test_socket()),
            State(state),
            HeaderMap::new(),
            Json(AgreementSpendRequest {
                agreement,
                funding_txid: funded.txid,
                htlc_vout: Some(target),
                milestone_id: Some("ms1".to_string()),
                destination_address: Some(recipient),
                fee_per_byte: None,
                broadcast: Some(false),
                secret_hex: None,
            }),
        )
        .await
        .expect("eligibility")
        .0;
        assert!(!resp.eligible);
        assert!(resp
            .reasons
            .iter()
            .any(|r| r == "secret_hex_required_for_release"));
    }

    #[tokio::test]
    async fn agreement_refund_eligibility_when_timeout_matured() {
        let (state, sender, recipient, _) = create_test_state(Some(0));
        add_wallet_utxo(&state, &sender, 20_000_000_000);
        {
            let mut chain = state.chain.lock().unwrap_or_else(|e| e.into_inner());
            chain.height = 140;
        }
        let (agreement, _) = milestone_agreement_for_test(&sender, &recipient, 120);
        let funded = fund_agreement_for_test(state.clone(), agreement.clone()).await;
        let funding_tx = decode_full_tx(&hex::decode(&funded.raw_tx_hex).unwrap()).unwrap();
        apply_tx_to_chain_for_test(&state, &funding_tx);
        confirm_tx_for_agreement_scan(&state, &funding_tx);
        let target = funded
            .outputs
            .iter()
            .find(|o| o.milestone_id.as_deref() == Some("ms1"))
            .expect("ms1 output")
            .vout;
        let resp = agreement_refund_eligibility(
            ConnectInfo(test_socket()),
            State(state),
            HeaderMap::new(),
            Json(AgreementSpendRequest {
                agreement,
                funding_txid: funded.txid,
                htlc_vout: Some(target),
                milestone_id: Some("ms1".to_string()),
                destination_address: Some(sender),
                fee_per_byte: None,
                broadcast: Some(false),
                secret_hex: None,
            }),
        )
        .await
        .expect("eligibility")
        .0;
        assert!(resp.eligible);
        assert!(resp.timeout_reached);
        assert_eq!(resp.branch, "refund");
    }

    #[tokio::test]
    async fn agreement_refund_ineligible_before_timeout() {
        let (state, sender, recipient, _) = create_test_state(Some(0));
        add_wallet_utxo(&state, &sender, 20_000_000_000);
        {
            let mut chain = state.chain.lock().unwrap_or_else(|e| e.into_inner());
            chain.height = 100;
        }
        let (agreement, _) = milestone_agreement_for_test(&sender, &recipient, 120);
        let funded = fund_agreement_for_test(state.clone(), agreement.clone()).await;
        let funding_tx = decode_full_tx(&hex::decode(&funded.raw_tx_hex).unwrap()).unwrap();
        apply_tx_to_chain_for_test(&state, &funding_tx);
        confirm_tx_for_agreement_scan(&state, &funding_tx);
        let target = funded
            .outputs
            .iter()
            .find(|o| o.milestone_id.as_deref() == Some("ms1"))
            .expect("ms1 output")
            .vout;
        let resp = agreement_refund_eligibility(
            ConnectInfo(test_socket()),
            State(state),
            HeaderMap::new(),
            Json(AgreementSpendRequest {
                agreement,
                funding_txid: funded.txid,
                htlc_vout: Some(target),
                milestone_id: Some("ms1".to_string()),
                destination_address: Some(sender),
                fee_per_byte: None,
                broadcast: Some(false),
                secret_hex: None,
            }),
        )
        .await
        .expect("eligibility")
        .0;
        assert!(!resp.eligible);
        assert!(resp
            .reasons
            .iter()
            .any(|r| r == "refund_timeout_not_reached"));
    }

    #[tokio::test]
    async fn agreement_release_rejects_non_htlc_backed_funding_path() {
        let (state, sender, recipient, _) = create_test_state(Some(0));
        let plain_tx = Transaction {
            version: 1,
            inputs: vec![],
            outputs: vec![TxOutput {
                value: 5_000_000,
                script_pubkey: p2pkh_script(&[9u8; 20]),
            }],
            locktime: 0,
        };
        confirm_tx_for_agreement_scan(&state, &plain_tx);
        let agreement =
            sample_agreement_for_test(&sender, &recipient, hex::encode(sha256d(b"x")), 120);
        let res = agreement_release_eligibility(
            ConnectInfo(test_socket()),
            State(state),
            HeaderMap::new(),
            Json(AgreementSpendRequest {
                agreement,
                funding_txid: hex::encode(plain_tx.txid()),
                htlc_vout: Some(0),
                milestone_id: None,
                destination_address: Some(sender),
                fee_per_byte: None,
                broadcast: Some(false),
                secret_hex: Some(hex::encode(b"x")),
            }),
        )
        .await;
        assert!(matches!(res, Err(StatusCode::BAD_REQUEST)));
    }

    #[tokio::test]
    async fn agreement_release_eligibility_resolves_requested_milestone_leg() {
        let (state, sender, recipient, _) = create_test_state(Some(0));
        add_wallet_utxo(&state, &sender, 30_000_000_000);
        let (agreement, secrets) = milestone_agreement_for_test(&sender, &recipient, 140);
        let funded = fund_agreement_for_test(state.clone(), agreement.clone()).await;
        let funding_tx = decode_full_tx(&hex::decode(&funded.raw_tx_hex).unwrap()).unwrap();
        apply_tx_to_chain_for_test(&state, &funding_tx);
        confirm_tx_for_agreement_scan(&state, &funding_tx);
        let target = funded
            .outputs
            .iter()
            .find(|o| o.milestone_id.as_deref() == Some("ms2"))
            .expect("ms2 output")
            .vout;
        let resp = agreement_release_eligibility(
            ConnectInfo(test_socket()),
            State(state),
            HeaderMap::new(),
            Json(AgreementSpendRequest {
                agreement,
                funding_txid: funded.txid,
                htlc_vout: Some(target),
                milestone_id: Some("ms2".to_string()),
                destination_address: Some(recipient),
                fee_per_byte: None,
                broadcast: Some(false),
                secret_hex: Some(hex::encode(&secrets[1])),
            }),
        )
        .await
        .expect("eligibility")
        .0;
        assert!(resp.eligible);
        assert_eq!(resp.milestone_id.as_deref(), Some("ms2"));
        assert_eq!(resp.htlc_vout, Some(target));
    }

    #[test]
    fn status_best_header_tip_hash_non_empty_when_height_positive() {
        let locked = load_locked_genesis().expect("locked genesis");
        let genesis_block = block_from_locked(&locked).expect("genesis block");
        let pow_limit = genesis_block.header.target();
        let params = ChainParams {
            pow_limit,
            genesis_block,
            htlcv1_activation_height: None,
            lwma: LwmaParams::new(None, pow_limit),
        };
        let chain = ChainState::new(params);
        let genesis_hash = hex::encode(chain.tip_hash());

        let tip = compute_best_header_tip_from_chain(&chain, &genesis_hash);
        assert!(!tip.hash.is_empty(), "best header hash should not be empty");

        let fallback = cached_best_header_tip(1, "", &genesis_hash);
        assert!(
            !fallback.hash.is_empty(),
            "fallback best_header_tip.hash must not be empty when height > 0"
        );
        assert_eq!(fallback.height, 1);
    }

    // ---- Phase 2 RPC tests ----

    fn make_rpc_policy(agreement_hash: &str, pubkey_hex: &str) -> ProofPolicy {
        ProofPolicy {
            policy_id: "rpc-pol-001".to_string(),
            schema_id: PROOF_POLICY_SCHEMA_ID.to_string(),
            agreement_hash: agreement_hash.to_string(),
            required_proofs: vec![ProofRequirement {
                requirement_id: "req-rpc-001".to_string(),
                proof_type: "delivery_confirmation".to_string(),
                required_by: None,
                required_attestor_ids: vec!["rpc-attestor".to_string()],
                resolution: ProofResolution::Release,
                milestone_id: None,
            }],
            no_response_rules: vec![],
            attestors: vec![ApprovedAttestor {
                attestor_id: "rpc-attestor".to_string(),
                pubkey_hex: pubkey_hex.to_string(),
                display_name: None,
                domain: None,
            }],
            notes: None,
        }
    }

    fn rpc_signing_key() -> SigningKey {
        SigningKey::from_bytes((&[11u8; 32]).into()).expect("static signing key")
    }

    fn rpc_pubkey_hex(sk: &SigningKey) -> String {
        hex::encode(sk.verifying_key().to_encoded_point(false).as_bytes())
    }

    fn sign_rpc_proof(
        proof: &SettlementProof,
        sk: &SigningKey,
    ) -> ProofSignatureEnvelope {
        use sha2::{Digest, Sha256};
        let payload = settlement_proof_payload_bytes(proof).unwrap();
        let digest = Sha256::digest(&payload);
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&digest);
        let sig: k256::ecdsa::Signature = sk.sign_prehash(&arr).unwrap();
        ProofSignatureEnvelope {
            signature_type: AGREEMENT_SIGNATURE_TYPE_SECP256K1.to_string(),
            pubkey_hex: rpc_pubkey_hex(sk),
            signature_hex: hex::encode(sig.to_bytes()),
            payload_hash: hex::encode(digest),
        }
    }

    fn make_rpc_proof(
        agreement_hash: &str,
        sk: &SigningKey,
    ) -> SettlementProof {
        let mut proof = SettlementProof {
            proof_id: "rpc-prf-001".to_string(),
            schema_id: SETTLEMENT_PROOF_SCHEMA_ID.to_string(),
            proof_type: "delivery_confirmation".to_string(),
            agreement_hash: agreement_hash.to_string(),
            milestone_id: None,
            attested_by: "rpc-attestor".to_string(),
            attestation_time: 1_700_000_000,
            evidence_hash: None,
            evidence_summary: Some("rpc test delivery".to_string()),
            signature: ProofSignatureEnvelope {
                signature_type: AGREEMENT_SIGNATURE_TYPE_SECP256K1.to_string(),
                pubkey_hex: rpc_pubkey_hex(sk),
                signature_hex: String::new(),
                payload_hash: String::new(),
            },
        };
        proof.signature = sign_rpc_proof(&proof, sk);
        proof
    }

    #[tokio::test]
    async fn check_policy_returns_release_eligible_when_requirements_met() {
        use irium_node_rs::settlement::agreement_canonical_bytes;
        let (state, sender, recipient, _) = create_test_state(Some(0));
        let (agreement, _) = milestone_agreement_for_test(&sender, &recipient, 200);

        let bytes = agreement_canonical_bytes(&agreement).unwrap();
        let agreement_hash = hex::encode(Sha256::digest(&bytes));

        let sk = rpc_signing_key();
        let pubkey_hex = rpc_pubkey_hex(&sk);
        let policy = make_rpc_policy(&agreement_hash, &pubkey_hex);
        let proof = make_rpc_proof(&agreement_hash, &sk);

        let req = CheckPolicyRequest {
            agreement,
            policy,
            proofs: vec![proof],
        };

        let resp = check_policy_rpc(
            ConnectInfo(test_socket()),
            State(state),
            HeaderMap::new(),
            Json(req),
        )
        .await
        .expect("check_policy_rpc must succeed")
        .0;

        assert!(resp.release_eligible, "valid proof must yield release_eligible");
        assert!(!resp.refund_eligible);
        assert_eq!(resp.policy_id, "rpc-pol-001");
        assert_eq!(resp.agreement_hash, agreement_hash);
    }

    #[tokio::test]
    async fn check_policy_rejects_agreement_hash_mismatch() {
        let (state, sender, recipient, _) = create_test_state(Some(0));
        let (agreement, _) = milestone_agreement_for_test(&sender, &recipient, 200);

        let sk = rpc_signing_key();
        let pubkey_hex = rpc_pubkey_hex(&sk);
        // policy references a wrong hash
        let policy = make_rpc_policy("deadbeef_wrong", &pubkey_hex);

        let req = CheckPolicyRequest {
            agreement,
            policy,
            proofs: vec![],
        };

        let result = check_policy_rpc(
            ConnectInfo(test_socket()),
            State(state),
            HeaderMap::new(),
            Json(req),
        )
        .await;

        assert!(result.is_err(), "mismatched hash must return error");
        let (status, body) = result.unwrap_err();
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body.contains("policy_eval_failed"), "got: {body}");
    }

    #[tokio::test]
    async fn check_policy_no_proofs_with_no_response_rule_at_tip() {
        use irium_node_rs::settlement::agreement_canonical_bytes;
        let (state, sender, recipient, _) = create_test_state(Some(0));
        let (agreement, _) = milestone_agreement_for_test(&sender, &recipient, 200);

        let bytes = agreement_canonical_bytes(&agreement).unwrap();
        let agreement_hash = hex::encode(Sha256::digest(&bytes));

        let sk = rpc_signing_key();
        let pubkey_hex = rpc_pubkey_hex(&sk);
        let mut policy = make_rpc_policy(&agreement_hash, &pubkey_hex);
        // deadline at height 0, so tip (0) >= deadline (0) → triggers immediately
        policy.no_response_rules.push(NoResponseRule {
            rule_id: "rpc-rule-refund-0".to_string(),
            deadline_height: 0,
            trigger: NoResponseTrigger::FundedAndNoRelease,
            resolution: ProofResolution::Refund,
            milestone_id: None,
            notes: None,
        });
        // remove required proofs so the only path is no-response
        policy.required_proofs.clear();

        let req = CheckPolicyRequest {
            agreement,
            policy,
            proofs: vec![],
        };

        let resp = check_policy_rpc(
            ConnectInfo(test_socket()),
            State(state),
            HeaderMap::new(),
            Json(req),
        )
        .await
        .expect("check_policy_rpc must succeed")
        .0;

        assert!(resp.refund_eligible, "no-response rule must yield refund_eligible");
        assert!(!resp.release_eligible);
        assert!(
            resp.reason.contains("rpc-rule-refund-0"),
            "reason must mention rule: {}",
            resp.reason
        );
    }


    // ---- Phase 2 proof store RPC tests ----

    fn make_signed_proof_for_rpc(agreement_hash: &str, signing_key: &SigningKey) -> SettlementProof {
        use irium_node_rs::settlement::{
            settlement_proof_payload_bytes, AGREEMENT_SIGNATURE_TYPE_SECP256K1,
            SETTLEMENT_PROOF_SCHEMA_ID,
        };
        let pubkey_hex = hex::encode(signing_key.verifying_key().to_encoded_point(false).as_bytes());
        let mut proof = SettlementProof {
            proof_id: "rpc-store-prf-001".to_string(),
            schema_id: SETTLEMENT_PROOF_SCHEMA_ID.to_string(),
            proof_type: "delivery_confirmation".to_string(),
            agreement_hash: agreement_hash.to_string(),
            milestone_id: None,
            attested_by: "att-node-test".to_string(),
            attestation_time: 1_700_200_000,
            evidence_hash: None,
            evidence_summary: Some("rpc store test".to_string()),
            signature: ProofSignatureEnvelope {
                signature_type: AGREEMENT_SIGNATURE_TYPE_SECP256K1.to_string(),
                pubkey_hex: pubkey_hex.clone(),
                signature_hex: String::new(),
                payload_hash: String::new(),
            },
        };
        let payload = settlement_proof_payload_bytes(&proof).unwrap();
        let digest = sha2::Sha256::digest(&payload);
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&digest);
        let sig: k256::ecdsa::Signature = signing_key.sign_prehash(&arr).unwrap();
        proof.signature.signature_hex = hex::encode(sig.to_bytes());
        proof.signature.payload_hash = hex::encode(digest);
        proof
    }

    #[tokio::test]
    async fn submit_proof_rpc_accepts_valid_proof() {
        let (state, _, _, _) = create_test_state(Some(0));
        let sk = SigningKey::from_bytes((&[17u8; 32]).into()).unwrap();
        let proof = make_signed_proof_for_rpc("cafecafe", &sk);
        let resp = submit_proof_rpc(
            ConnectInfo(test_socket()),
            State(state.clone()),
            HeaderMap::new(),
            Json(SubmitProofRequest { proof }),
        )
        .await
        .expect("submit must succeed")
        .0;
        assert!(resp.accepted);
        assert!(!resp.duplicate);
        assert_eq!(resp.proof_id, "rpc-store-prf-001");
    }

    #[tokio::test]
    async fn submit_proof_rpc_rejects_invalid_signature() {
        let (state, _, _, _) = create_test_state(Some(0));
        let sk = SigningKey::from_bytes((&[17u8; 32]).into()).unwrap();
        let mut proof = make_signed_proof_for_rpc("cafecafe", &sk);
        proof.signature.signature_hex = "00".repeat(64);
        let result = submit_proof_rpc(
            ConnectInfo(test_socket()),
            State(state),
            HeaderMap::new(),
            Json(SubmitProofRequest { proof }),
        )
        .await;
        assert!(result.is_err());
        let (status, _) = result.unwrap_err();
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn submit_proof_rpc_deduplicates() {
        let (state, _, _, _) = create_test_state(Some(0));
        let sk = SigningKey::from_bytes((&[17u8; 32]).into()).unwrap();
        let proof = make_signed_proof_for_rpc("cafecafe", &sk);
        submit_proof_rpc(
            ConnectInfo(test_socket()),
            State(state.clone()),
            HeaderMap::new(),
            Json(SubmitProofRequest { proof: proof.clone() }),
        )
        .await
        .expect("first submit")
        .0;
        let second = submit_proof_rpc(
            ConnectInfo(test_socket()),
            State(state),
            HeaderMap::new(),
            Json(SubmitProofRequest { proof }),
        )
        .await
        .expect("second submit")
        .0;
        assert!(!second.accepted);
        assert!(second.duplicate);
    }

    #[tokio::test]
    async fn list_proofs_rpc_returns_submitted_proofs() {
        let (state, _, _, _) = create_test_state(Some(0));
        let sk = SigningKey::from_bytes((&[17u8; 32]).into()).unwrap();
        let proof = make_signed_proof_for_rpc("listtest", &sk);
        let _ = submit_proof_rpc(
            ConnectInfo(test_socket()),
            State(state.clone()),
            HeaderMap::new(),
            Json(SubmitProofRequest { proof }),
        )
        .await
        .expect("submit");
        let list_resp = list_proofs_rpc(
            ConnectInfo(test_socket()),
            State(state),
            HeaderMap::new(),
            Json(ListProofsRequest {
                agreement_hash: "listtest".to_string(),
            }),
        )
        .await
        .expect("list")
        .0;
        assert_eq!(list_resp.count, 1);
        assert_eq!(list_resp.proofs[0].proof_id, "rpc-store-prf-001");
    }

    #[tokio::test]
    async fn store_policy_rpc_accepts_valid_policy() {
        let (state, _, _, _) = create_test_state(Some(0));
        let policy = make_rpc_policy("storepol-hash", "pk-abc");
        let resp = store_policy_rpc(
            ConnectInfo(test_socket()),
            State(state),
            HeaderMap::new(),
            Json(StorePolicyRequest { policy }),
        )
        .await
        .expect("must accept")
        .0;
        assert!(resp.accepted);
        assert!(!resp.updated);
        assert_eq!(resp.agreement_hash, "storepol-hash");
        assert!(resp.message.contains("accepted"), "got: {}", resp.message);
    }

    #[tokio::test]
    async fn store_policy_rpc_rejects_empty_agreement_hash() {
        let (state, _, _, _) = create_test_state(Some(0));
        let mut policy = make_rpc_policy("", "pk");
        policy.agreement_hash = "".to_string();
        let result = store_policy_rpc(
            ConnectInfo(test_socket()),
            State(state),
            HeaderMap::new(),
            Json(StorePolicyRequest { policy }),
        )
        .await;
        assert!(result.is_err(), "must reject empty agreement_hash");
    }

    #[tokio::test]
    async fn get_policy_rpc_returns_stored_policy() {
        let (state, _, _, _) = create_test_state(Some(0));
        let policy = make_rpc_policy("getpol-hash-001", "pk-get");
        store_policy_rpc(
            ConnectInfo(test_socket()),
            State(state.clone()),
            HeaderMap::new(),
            Json(StorePolicyRequest {
                policy: policy.clone(),
            }),
        )
        .await
        .expect("store must succeed");
        let resp = get_policy_rpc(
            ConnectInfo(test_socket()),
            State(state),
            HeaderMap::new(),
            Json(GetPolicyRequest {
                agreement_hash: "getpol-hash-001".to_string(),
            }),
        )
        .await
        .expect("get must succeed")
        .0;
        assert!(resp.found);
        assert_eq!(resp.policy.as_ref().unwrap().policy_id, policy.policy_id);
        assert_eq!(resp.policy.as_ref().unwrap().agreement_hash, "getpol-hash-001");
    }

    #[tokio::test]
    async fn get_policy_rpc_returns_not_found_for_missing() {
        let (state, _, _, _) = create_test_state(Some(0));
        let resp = get_policy_rpc(
            ConnectInfo(test_socket()),
            State(state),
            HeaderMap::new(),
            Json(GetPolicyRequest {
                agreement_hash: "nosuchpolicy".to_string(),
            }),
        )
        .await
        .expect("get must not error")
        .0;
        assert!(!resp.found);
        assert!(resp.policy.is_none());
    }



    #[tokio::test]
    async fn evaluate_policy_rpc_success_with_stored_policy_and_proof() {
        use irium_node_rs::settlement::agreement_canonical_bytes;
        let (state, sender, recipient, _) = create_test_state(Some(0));
        let (agreement, _) = milestone_agreement_for_test(&sender, &recipient, 200);
        let bytes = agreement_canonical_bytes(&agreement).unwrap();
        let agreement_hash = hex::encode(Sha256::digest(&bytes));

        let sk = rpc_signing_key();
        let pubkey_hex = rpc_pubkey_hex(&sk);

        // Store policy
        let policy = make_rpc_policy(&agreement_hash, &pubkey_hex);
        store_policy_rpc(
            ConnectInfo(test_socket()),
            State(state.clone()),
            HeaderMap::new(),
            Json(StorePolicyRequest { policy }),
        )
        .await
        .expect("store policy");

        // Store proof (attested_by matches rpc-attestor in policy)
        let proof = make_rpc_proof(&agreement_hash, &sk);
        submit_proof_rpc(
            ConnectInfo(test_socket()),
            State(state.clone()),
            HeaderMap::new(),
            Json(SubmitProofRequest { proof }),
        )
        .await
        .expect("submit proof");

        // Evaluate using stored artifacts
        let resp = evaluate_policy_rpc(
            ConnectInfo(test_socket()),
            State(state),
            HeaderMap::new(),
            Json(EvaluatePolicyRequest { agreement }),
        )
        .await
        .expect("evaluate must succeed")
        .0;

        assert!(resp.policy_found);
        assert_eq!(resp.policy_id.as_deref(), Some("rpc-pol-001"));
        assert_eq!(resp.proof_count, 1);
        assert!(resp.release_eligible, "expected release eligible; reason: {}", resp.reason);
        assert!(!resp.refund_eligible);
        assert!(
            resp.evaluated_rules.iter().any(|r| r.contains("verified ok")),
            "got rules: {:?}",
            resp.evaluated_rules
        );
    }

    #[tokio::test]
    async fn evaluate_policy_rpc_missing_policy_returns_not_found() {
        let (state, sender, recipient, _) = create_test_state(Some(0));
        let (agreement, _) = milestone_agreement_for_test(&sender, &recipient, 200);

        let resp = evaluate_policy_rpc(
            ConnectInfo(test_socket()),
            State(state),
            HeaderMap::new(),
            Json(EvaluatePolicyRequest { agreement }),
        )
        .await
        .expect("evaluate must not error")
        .0;

        assert!(!resp.policy_found);
        assert!(resp.policy_id.is_none());
        assert_eq!(resp.proof_count, 0);
        assert!(!resp.release_eligible);
        assert!(!resp.refund_eligible);
        assert!(
            resp.reason.contains("no policy stored"),
            "got reason: {}",
            resp.reason
        );
    }

    #[tokio::test]
    async fn evaluate_policy_rpc_no_proofs_not_eligible() {
        use irium_node_rs::settlement::agreement_canonical_bytes;
        let (state, sender, recipient, _) = create_test_state(Some(0));
        let (agreement, _) = milestone_agreement_for_test(&sender, &recipient, 200);
        let bytes = agreement_canonical_bytes(&agreement).unwrap();
        let agreement_hash = hex::encode(Sha256::digest(&bytes));

        let sk = rpc_signing_key();
        let pubkey_hex = rpc_pubkey_hex(&sk);
        let policy = make_rpc_policy(&agreement_hash, &pubkey_hex);
        store_policy_rpc(
            ConnectInfo(test_socket()),
            State(state.clone()),
            HeaderMap::new(),
            Json(StorePolicyRequest { policy }),
        )
        .await
        .expect("store policy");

        let resp = evaluate_policy_rpc(
            ConnectInfo(test_socket()),
            State(state),
            HeaderMap::new(),
            Json(EvaluatePolicyRequest { agreement }),
        )
        .await
        .expect("evaluate must not error")
        .0;

        assert!(resp.policy_found);
        assert_eq!(resp.proof_count, 0);
        assert!(!resp.release_eligible);
    }

    #[tokio::test]
    async fn evaluate_policy_rpc_cross_agreement_proof_not_fetched() {
        use irium_node_rs::settlement::agreement_canonical_bytes;
        let (state, sender, recipient, _) = create_test_state(Some(0));
        let (agreement_b, _) = milestone_agreement_for_test(&sender, &recipient, 200);
        let bytes_b = agreement_canonical_bytes(&agreement_b).unwrap();
        let hash_b = hex::encode(Sha256::digest(&bytes_b));

        let sk = rpc_signing_key();
        let pubkey_hex = rpc_pubkey_hex(&sk);

        let policy_b = make_rpc_policy(&hash_b, &pubkey_hex);
        store_policy_rpc(
            ConnectInfo(test_socket()),
            State(state.clone()),
            HeaderMap::new(),
            Json(StorePolicyRequest { policy: policy_b }),
        )
        .await
        .expect("store policy b");

        // Proof stored for a different hash
        let proof_for_a = make_rpc_proof(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            &sk,
        );
        submit_proof_rpc(
            ConnectInfo(test_socket()),
            State(state.clone()),
            HeaderMap::new(),
            Json(SubmitProofRequest { proof: proof_for_a }),
        )
        .await
        .expect("submit proof for a");

        let resp = evaluate_policy_rpc(
            ConnectInfo(test_socket()),
            State(state),
            HeaderMap::new(),
            Json(EvaluatePolicyRequest { agreement: agreement_b }),
        )
        .await
        .expect("evaluate must not error")
        .0;

        assert!(resp.policy_found);
        assert_eq!(resp.proof_count, 0, "proof for A must not be fetched for B");
        assert!(!resp.release_eligible);
    }


}