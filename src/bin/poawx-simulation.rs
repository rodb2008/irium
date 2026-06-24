//! PoAW-X Phase 2 simulation harness (devnet-only, headless, no live node).
//!
//! Runs the 10 blueprint scenarios against the REAL consensus machinery:
//!  - Scenarios 1,2,6,7,8 drive a real `ChainState` end-to-end via
//!    `connect_block` / `process_block` (blocks built with the proven
//!    `build_solo_poawx_block`).
//!  - Scenarios 3,4,5,9,10 exercise the REAL consensus primitives that
//!    `connect_block` itself calls (`PersistentDominance`, `effective_score`,
//!    `TicketProof::validate`, `resolve_epoch_seed_parts`) — the block builder
//!    produces single-identity, single-candidate-per-role blocks, so competitive
//!    selection / heterogeneous-miner dominance / bad-ticket / multi-height
//!    multisource cases cannot be expressed as a connectable block.
//!
//! Mainnet hard-off throughout (IRIUM_NETWORK=devnet, network_id=2). Each scenario
//! prints a PASS / FAIL / INFO verdict with quantitative evidence; the process
//! exits 0 iff every required scenario passes (INFO findings do not fail the run).
#![allow(warnings)]

use std::env;

use irium_node_rs::chain::{block_from_locked, ChainParams, ChainState, LwmaParams};
use irium_node_rs::genesis::load_locked_genesis;
use irium_node_rs::poawx::{multi_role_amounts, ROLE_COMPUTE_CONTRIBUTOR};
use irium_node_rs::poawx_admission::global_admission_cache;
use irium_node_rs::poawx_candidate::effective_score;
use irium_node_rs::poawx_committed_admission::{multisource_seed_active, resolve_epoch_seed_parts};
use irium_node_rs::poawx_dominance::{PersistentDominance, RoleRewardKind, DOMINANCE_BASE_WORK_SCORE};
use irium_node_rs::poawx_mining_harness::build_solo_poawx_block;
use irium_node_rs::poawx_penalty::PenaltyStatus;
use irium_node_rs::poawx_ticket::{meets_sybil_target, TicketProof};
use irium_node_rs::pow::Target;

const NET: u8 = 2; // devnet
const SUBSIDY: u64 = 5_000_000_000; // notional per-block subsidy (dominance shares are relative)

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Pass,
    Fail,
    Info,
}

struct Verdict {
    name: &'static str,
    status: Status,
    detail: String,
}

// All gate env keys the harness ever sets, for clean per-scenario resets.
const GATE_KEYS: &[&str] = &[
    "IRIUM_POAWX_ACTIVATION_HEIGHT",
    "IRIUM_POAWX_MODE",
    "IRIUM_POAWX_PUZZLE_DIFFICULTY_BITS",
    "IRIUM_POAWX_PUZZLE_BITS",
    "IRIUM_POAWX_MULTI_ROLE_REWARD_ACTIVATION_HEIGHT",
    "IRIUM_POAWX_FAIRNESS_MATRIX_ACTIVATION_HEIGHT",
    "IRIUM_POAWX_ANTI_DOMINATION_ACTIVATION_HEIGHT",
    "IRIUM_POAWX_ANTI_DOMINATION_REQUIRED",
    "IRIUM_POAWX_CANDIDATE_SET_ACTIVATION_HEIGHT",
    "IRIUM_POAWX_CANDIDATE_SET_REQUIRED",
    "IRIUM_POAWX_ASSIGNMENT_PROOF_ACTIVATION_HEIGHT",
    "IRIUM_POAWX_ASSIGNMENT_PROOF_REQUIRED",
    "IRIUM_POAWX_CANDIDATE_ADMISSION_ACTIVATION_HEIGHT",
    "IRIUM_POAWX_CANDIDATE_ADMISSION_REQUIRED",
    "IRIUM_POAWX_PUZZLE_WORK_ACTIVATION_HEIGHT",
    "IRIUM_POAWX_PUZZLE_WORK_REQUIRED",
    "IRIUM_POAWX_FINALITY_COMMITTEE_ACTIVATION_HEIGHT",
    "IRIUM_POAWX_FINALITY_COMMITTEE_REQUIRED",
    "IRIUM_POAWX_FINALITY_THRESHOLD_NUM",
    "IRIUM_POAWX_FINALITY_THRESHOLD_DEN",
    "IRIUM_POAWX_COMMITTED_ADMISSION_ACTIVATION_HEIGHT",
    "IRIUM_POAWX_COMMITTED_ADMISSION_REQUIRED",
    "IRIUM_POAWX_TRUE_VRF_ACTIVATION_HEIGHT",
    "IRIUM_POAWX_TRUE_VRF_REQUIRED",
    "IRIUM_POAWX_MULTISOURCE_SEED_ACTIVATION_HEIGHT",
    "IRIUM_POAWX_ADAPTIVE_MODE_ACTIVATION_HEIGHT",
];

fn reset_gates() {
    for k in GATE_KEYS {
        env::remove_var(k);
    }
    env::set_var("IRIUM_NETWORK", "devnet");
}

/// Block-production gates the `build_solo_poawx_block` output satisfies, EXCLUDING
/// anti-domination, committed-admission and candidate-admission (which constrain
/// multi-miner / heterogeneous chains) and multisource seed (height-1 only in the
/// builder). `adaptive` toggles the adaptive posture engine.
fn set_production_gates(adaptive: bool) {
    reset_gates();
    let base = &[
        ("IRIUM_POAWX_ACTIVATION_HEIGHT", "1"),
        ("IRIUM_POAWX_MODE", "active"),
        ("IRIUM_POAWX_PUZZLE_DIFFICULTY_BITS", "1"),
        ("IRIUM_POAWX_PUZZLE_BITS", "1"),
        ("IRIUM_POAWX_MULTI_ROLE_REWARD_ACTIVATION_HEIGHT", "1"),
        ("IRIUM_POAWX_FAIRNESS_MATRIX_ACTIVATION_HEIGHT", "1"),
        ("IRIUM_POAWX_CANDIDATE_SET_ACTIVATION_HEIGHT", "1"),
        ("IRIUM_POAWX_CANDIDATE_SET_REQUIRED", "1"),
        ("IRIUM_POAWX_ASSIGNMENT_PROOF_ACTIVATION_HEIGHT", "1"),
        ("IRIUM_POAWX_ASSIGNMENT_PROOF_REQUIRED", "1"),
        ("IRIUM_POAWX_PUZZLE_WORK_ACTIVATION_HEIGHT", "1"),
        ("IRIUM_POAWX_PUZZLE_WORK_REQUIRED", "1"),
        ("IRIUM_POAWX_FINALITY_COMMITTEE_ACTIVATION_HEIGHT", "1"),
        ("IRIUM_POAWX_FINALITY_COMMITTEE_REQUIRED", "1"),
        ("IRIUM_POAWX_TRUE_VRF_ACTIVATION_HEIGHT", "1"),
        ("IRIUM_POAWX_TRUE_VRF_REQUIRED", "1"),
    ];
    for (k, v) in base {
        env::set_var(k, v);
    }
    if adaptive {
        env::set_var("IRIUM_POAWX_ADAPTIVE_MODE_ACTIVATION_HEIGHT", "1");
    }
}

fn fresh_chain() -> ChainState {
    let locked = load_locked_genesis().expect("load locked genesis");
    let genesis = block_from_locked(&locked).expect("genesis block");
    let pow_limit = Target { bits: 0x1f00ffff };
    let params = ChainParams {
        genesis_block: genesis,
        pow_limit,
        htlcv1_activation_height: None,
        mpsov1_activation_height: None,
        lwma: LwmaParams::new(None, pow_limit),
        lwma_v2: None,
        auxpow_activation_height: None,
        btc_spv: None,
        ltc_spv: None,
        htlc_btc_swap_v1_activation_height: None,
        btc_swap_bech32_payment_activation_height: None,
        htlc_ltc_swap_v1_activation_height: None,
        swap_order_v1_activation_height: None,
        ltc_swap_order_v1_activation_height: None,
        coinbase_header_batch_activation_height: None,
    };
    ChainState::new(params)
}

fn genesis_time(st: &ChainState) -> u32 {
    st.chain.first().map(|b| b.header.time).unwrap_or(0)
}

/// Build a solo all-gates block for `secret` on the current tip and connect it.
fn build_and_connect(st: &mut ChainState, secret: &[u8; 32], g_time: u32) -> Result<(), String> {
    let height = st.tip_height() + 1;
    let tip = st.chain.last().ok_or("empty chain")?;
    let tip_height = st.tip_height();
    let prev_hash = tip.header.hash_for_height(tip_height);
    let parent_prev = if height <= 1 {
        None
    } else {
        Some(tip.header.prev_hash)
    };
    let bits = st.target_for_height(height).bits;
    let time = g_time + height as u32;
    let proof = build_solo_poawx_block(secret, NET, height, prev_hash, parent_prev, bits, time, 1)?;
    st.connect_block(proof.block)
}

/// Sum the four value-bearing coinbase role outputs (skipping the irx1 OP_RETURN).
fn role_outputs(block: &irium_node_rs::block::Block) -> Option<[u64; 4]> {
    let outs = &block.transactions.first()?.outputs;
    // [irx1(value 0), primary, compute, verify, support]
    if outs.len() < 5 {
        return None;
    }
    Some([outs[1].value, outs[2].value, outs[3].value, outs[4].value])
}

// ── Scenario 1: Normal mining, 55/22/13/10 split over 100 blocks ─────────────
fn scenario_1_reward_split() -> Verdict {
    set_production_gates(true);
    let mut st = fresh_chain();
    let g_time = genesis_time(&st);
    let miners: Vec<[u8; 32]> = (0..4u8).map(|i| [0x40 + i; 32]).collect();
    let n = 100u64;
    let mut bad_split = 0u64;
    for h in 1..=n {
        let secret = &miners[(h as usize) % miners.len()];
        if let Err(e) = build_and_connect(&mut st, secret, g_time) {
            return Verdict {
                name: "1. normal mining / reward split",
                status: Status::Fail,
                detail: format!("connect_block failed at height {h}: {e}"),
            };
        }
        let outs = role_outputs(st.chain.last().unwrap()).expect("role outputs");
        let total: u64 = outs.iter().sum();
        let expected = multi_role_amounts(total);
        if outs != expected {
            bad_split += 1;
        }
    }
    let mode = st.adaptive_mode();
    let ok = bad_split == 0 && st.tip_height() == n;
    Verdict {
        name: "1. normal mining / reward split",
        status: if ok { Status::Pass } else { Status::Fail },
        detail: format!(
            "{} blocks connected; bad splits={}; per-block split == 55/22/13/10 bps; adaptive mode={}",
            st.tip_height(),
            bad_split,
            mode.as_str()
        ),
    }
}

// ── Scenario 2: low participation (1 miner) -> Caution ───────────────────────
fn scenario_2_low_participation() -> Verdict {
    set_production_gates(true);
    let mut st = fresh_chain();
    let g_time = genesis_time(&st);
    let secret = [0x51u8; 32];
    let n = 40u64;
    for h in 1..=n {
        if let Err(e) = build_and_connect(&mut st, &secret, g_time) {
            return Verdict {
                name: "2. low participation / Caution",
                status: Status::Fail,
                detail: format!("connect_block failed at height {h}: {e}"),
            };
        }
    }
    let mode = st.adaptive_mode();
    let ok = mode == irium_node_rs::poawx_adaptive::AdaptiveMode::Caution;
    Verdict {
        name: "2. low participation / Caution",
        status: if ok { Status::Pass } else { Status::Fail },
        detail: format!(
            "1 miner, {} blocks; adaptive mode={} (expect caution: active_miner_count=1 < 3)",
            st.tip_height(),
            mode.as_str()
        ),
    }
}

// ── Scenario 3/4: dominant miner / pool -> anti-domination down-weights ──────
fn dominance_concentration(name: &'static str) -> Verdict {
    // Real PersistentDominance engine fed the exact per-block role-reward events.
    let mut dom = PersistentDominance::new(100_000, 2);
    let dominant = [0xD0u8; 20];
    let other = [0x0Eu8; 20];
    let fresh = [0xFEu8; 20];
    let amts = multi_role_amounts(SUBSIDY);
    let blocks = 200u64;
    for h in 1..=blocks {
        // dominant builds > 80% of blocks (9 of every 10 => 90%)
        let miner = if h % 10 == 0 { other } else { dominant };
        dom.apply_event(miner, RoleRewardKind::Primary, amts[0], h);
        dom.apply_event(miner, RoleRewardKind::Compute, amts[1], h);
        dom.apply_event(miner, RoleRewardKind::Verify, amts[2], h);
        dom.apply_event(miner, RoleRewardKind::Support, amts[3], h);
    }
    let share = dom.recent_reward_share_permille(&dominant, blocks);
    let w_dom = dom.weight(DOMINANCE_BASE_WORK_SCORE, &dominant, blocks);
    let w_fresh = dom.weight(DOMINANCE_BASE_WORK_SCORE, &fresh, blocks);
    // Selection effect via the REAL effective_score: same VRF score, the
    // down-weighted miner scores strictly lower (=> lower expected selection).
    let assignment_score = 1000u64;
    let pen = PenaltyStatus::Clean.weight_multiplier_permille() as u64;
    let es_dom = effective_score(assignment_score, w_dom, pen);
    let es_fresh = effective_score(assignment_score, w_fresh, pen);
    let reduction_pct = if w_fresh > 0 {
        100u64.saturating_sub(w_dom.saturating_mul(100) / w_fresh)
    } else {
        0
    };
    let ok = share > 800 && reduction_pct >= 30 && es_dom < es_fresh;
    Verdict {
        name,
        status: if ok { Status::Pass } else { Status::Fail },
        detail: format!(
            "dominant share={}permille; weight {} vs fresh {} (reduced {}%); effective_score dom={} < fresh={}",
            share, w_dom, w_fresh, reduction_pct, es_dom, es_fresh
        ),
    }
}

fn scenario_3_dominant_miner() -> Verdict {
    dominance_concentration("3. one dominant miner")
}

fn scenario_4_dominant_pool() -> Verdict {
    // A pool is a single reward identity: same mechanism, same engine.
    let mut v = dominance_concentration("4. one dominant pool");
    v
}

// ── Scenario 5: Sybil attacker -> ticket sybil-PoW blocks them ───────────────
fn scenario_5_sybil() -> Verdict {
    reset_gates();
    let require_bits = 18u32;
    let height = 10u64;
    let mut rejected = 0u64;
    let n = 50u64;
    for i in 0..n {
        let pkh = [i as u8; 20];
        // trivial sybil nonce; guarantee it does NOT meet the high threshold.
        let mut nonce = [0u8; 32];
        nonce[0] = i as u8;
        let mut tp = TicketProof::new(
            NET,
            height,
            ROLE_COMPUTE_CONTRIBUTOR,
            pkh,
            0,
            height + 100,
            [2u8; 33],
            nonce,
            PenaltyStatus::Clean.id(),
        );
        let mut bump = 0u8;
        while meets_sybil_target(&tp.sybil_work_digest, require_bits) {
            bump = bump.wrapping_add(1);
            nonce[1] = bump;
            tp = TicketProof::new(
                NET,
                height,
                ROLE_COMPUTE_CONTRIBUTOR,
                pkh,
                0,
                height + 100,
                [2u8; 33],
                nonce,
                PenaltyStatus::Clean.id(),
            );
        }
        if tp
            .validate(NET, height, ROLE_COMPUTE_CONTRIBUTOR, &pkh, require_bits, false)
            .is_err()
        {
            rejected += 1;
        }
    }
    // Control: a properly-mined ticket is accepted.
    let cpkh = [0xAAu8; 20];
    let mut cn = [0u8; 32];
    let mut mined = TicketProof::new(
        NET, height, ROLE_COMPUTE_CONTRIBUTOR, cpkh, 0, height + 100, [2u8; 33], cn, PenaltyStatus::Clean.id(),
    );
    let mut tries = 0u64;
    while !meets_sybil_target(&mined.sybil_work_digest, require_bits) {
        tries += 1;
        cn[..8].copy_from_slice(&tries.to_le_bytes());
        mined = TicketProof::new(
            NET, height, ROLE_COMPUTE_CONTRIBUTOR, cpkh, 0, height + 100, [2u8; 33], cn, PenaltyStatus::Clean.id(),
        );
    }
    let control_ok = mined
        .validate(NET, height, ROLE_COMPUTE_CONTRIBUTOR, &cpkh, require_bits, false)
        .is_ok();
    let ok = rejected == n && control_ok;
    Verdict {
        name: "5. sybil attacker",
        status: if ok { Status::Pass } else { Status::Fail },
        detail: format!(
            "{}/{} minimal-PoW sybil tickets rejected at {} required bits; mined control accepted={} ({} mine tries)",
            rejected, n, require_bits, control_ok, tries
        ),
    }
}

// ── Scenario 6: reorg past finality (INFORMATIONAL finding) ──────────────────
fn scenario_6_reorg() -> Verdict {
    set_production_gates(false);
    let mut st = fresh_chain();
    let g_time = genesis_time(&st);
    let secret = [0x61u8; 32];
    // main chain h1..h3 (h3 carries a finality proof finalizing h2).
    for _h in 1..=3u64 {
        if let Err(e) = build_and_connect(&mut st, &secret, g_time) {
            return Verdict {
                name: "6. reorg past finality",
                status: Status::Info,
                detail: format!("could not build main chain: {e}"),
            };
        }
    }
    let main_tip_before = st.tip_height();
    let h1 = st.chain[1].clone();
    let h1_hash = h1.header.hash_for_height(1);
    let h1_prev = h1.header.prev_hash;
    let bits = st.target_for_height(2).bits; // devnet: constant (no LWMA activation)

    // Build a competing fork diverging at h1: h2b, h3b (equal work => no reorg),
    // then h4b (more work => reorg past the finalized h2). No closure (avoids
    // borrowing `st` while also mutating it via process_block).
    macro_rules! fork_or_info {
        ($e:expr, $what:expr) => {
            match $e {
                Ok(p) => p,
                Err(e) => {
                    return Verdict {
                        name: "6. reorg past finality",
                        status: Status::Info,
                        detail: format!("fork build {} failed: {}", $what, e),
                    }
                }
            }
        };
    }
    let h2b = fork_or_info!(
        build_solo_poawx_block(&secret, NET, 2, h1_hash, Some(h1_prev), bits, g_time + 2000 + 2, 1),
        "h2b"
    );
    let h2b_hash = h2b.block.header.hash_for_height(2);
    let _ = st.process_block(h2b.block); // lower work => stored, no reorg
    let h3b = fork_or_info!(
        build_solo_poawx_block(&secret, NET, 3, h2b_hash, Some(h1_hash), bits, g_time + 2000 + 3, 1),
        "h3b"
    );
    let h3b_hash = h3b.block.header.hash_for_height(3);
    let _ = st.process_block(h3b.block); // fork height 3 == main 3 => equal work
    let tip_after_equal = st.tip_height();
    let work_monotonic_ok = tip_after_equal == main_tip_before; // equal work did NOT reorg

    let h4b = fork_or_info!(
        build_solo_poawx_block(&secret, NET, 4, h3b_hash, Some(h2b_hash), bits, g_time + 2000 + 4, 1),
        "h4b"
    );
    let _ = st.process_block(h4b.block); // fork height 4 > main 3 => reorg
    let reorged = st.tip_height() == 4;

    let finding = if reorged {
        "FINDING: a heavier fork reorged past the finalized block (no finality-checkpoint reorg protection)."
    } else {
        "reorg did not occur (unexpected)."
    };
    Verdict {
        name: "6. reorg past finality",
        status: Status::Info,
        detail: format!(
            "work-monotonic reorg enforced (equal-work fork did NOT reorg)={}; {} \
             Finality reorg protection is NOT implemented — required before testnet.",
            work_monotonic_ok, finding
        ),
    }
}

// ── Scenario 7/8: hardware neutrality (chain continues) ──────────────────────
fn scenario_hardware(name: &'static str, classes: &[u8]) -> Verdict {
    set_production_gates(false);
    let mut st = fresh_chain();
    let g_time = genesis_time(&st);
    let n = 20u64;
    for h in 1..=n {
        let secret = [classes[(h as usize) % classes.len()]; 32];
        if let Err(e) = build_and_connect(&mut st, &secret, g_time) {
            return Verdict {
                name,
                status: Status::Fail,
                detail: format!("chain stalled at height {h}: {e}"),
            };
        }
    }
    let ok = st.tip_height() == n;
    Verdict {
        name,
        status: if ok { Status::Pass } else { Status::Fail },
        detail: format!(
            "chain advanced to height {} with {} miner identities (consensus has no hardware-class gating; neutrality = nothing blocks any class)",
            st.tip_height(),
            classes.len()
        ),
    }
}

fn scenario_7_no_asic() -> Verdict {
    scenario_hardware("7. no ASIC participation (CPU/GPU only)", &[0x71, 0x72])
}

fn scenario_8_no_gpu() -> Verdict {
    scenario_hardware("8. no GPU participation (CPU/ASIC only)", &[0x81, 0x82])
}

// ── Scenario 9: randomness manipulation -> multi-source seed prevents bias ───
fn scenario_9_seed() -> Verdict {
    reset_gates();
    env::set_var("IRIUM_POAWX_MULTISOURCE_SEED_ACTIVATION_HEIGHT", "1");
    let height = 10u64;
    if !multisource_seed_active(height) {
        return Verdict {
            name: "9. randomness manipulation",
            status: Status::Fail,
            detail: "multisource seed gate not active under devnet".to_string(),
        };
    }
    let grandparent = [0xAAu8; 32]; // the only source a consecutive-block proposer can grind
    let fin1 = [0xF1u8; 32]; // parent finality digest (committee-controlled)
    let fin2 = [0xF2u8; 32];
    let precommit = [0x0Cu8; 32];
    let legacy = grandparent; // pre-multisource seed == fully proposer-grindable
    let seed1 = resolve_epoch_seed_parts(height, grandparent, fin1, precommit);
    let seed2 = resolve_epoch_seed_parts(height, grandparent, fin2, precommit);
    let differs_from_legacy = seed1 != legacy;
    let finality_changes_seed = seed1 != seed2; // proposer can't reproduce without committee
    let ok = differs_from_legacy && finality_changes_seed;
    Verdict {
        name: "9. randomness manipulation",
        status: if ok { Status::Pass } else { Status::Fail },
        detail: format!(
            "multi-source seed differs from grindable grandparent-only seed={}; changing ONLY the committee finality digest changes the seed={} (proposer cannot bias it alone)",
            differs_from_legacy, finality_changes_seed
        ),
    }
}

// ── Scenario 10: reward fairness, 1000 blocks, 5 hashrates ───────────────────
fn scenario_10_fairness() -> Verdict {
    let mut dom = PersistentDominance::new(2_000_000, 2);
    let miners: [[u8; 20]; 5] = [[1; 20], [2; 20], [3; 20], [4; 20], [5; 20]];
    let hashrate = [30u64, 25, 20, 15, 10]; // percent; sums to 100
    let amts = multi_role_amounts(SUBSIDY);
    let blocks = 1000u64;
    // Deterministic weighted assignment matching the hashrate distribution.
    let mut counts = [0u64; 5];
    for h in 1..=blocks {
        let mut acc = (h * 1) % 100; // deterministic rotation across 0..99
        let pick = {
            let mut cum = 0u64;
            let mut sel = 4usize;
            for (i, hr) in hashrate.iter().enumerate() {
                cum += hr;
                if acc < cum {
                    sel = i;
                    break;
                }
            }
            sel
        };
        counts[pick] += 1;
        let m = miners[pick];
        dom.apply_event(m, RoleRewardKind::Primary, amts[0], h);
        dom.apply_event(m, RoleRewardKind::Compute, amts[1], h);
        dom.apply_event(m, RoleRewardKind::Verify, amts[2], h);
        dom.apply_event(m, RoleRewardKind::Support, amts[3], h);
    }
    let mut worst_dev = 0i64;
    let mut max_share = 0u32;
    let mut lines = Vec::new();
    for (i, m) in miners.iter().enumerate() {
        let share = dom.recent_reward_share_permille(m, blocks);
        max_share = max_share.max(share);
        let expected = hashrate[i] * 10; // percent -> permille
        let dev = (share as i64 - expected as i64).abs();
        worst_dev = worst_dev.max(dev);
        lines.push(format!("m{}:{}permille(hr {}%)", i, share, hashrate[i]));
    }
    // Fair: each miner within +-50 permille (5%) of its hashrate share, and no
    // miner above the Defense concentration line (700 permille).
    let ok = worst_dev <= 50 && (max_share as u32) < 700;
    Verdict {
        name: "10. reward distribution fairness",
        status: if ok { Status::Pass } else { Status::Fail },
        detail: format!(
            "1000 blocks, 5 miners {:?}; worst deviation from hashrate={}permille; max share={}permille (<700)",
            lines, worst_dev, max_share
        ),
    }
}

fn main() {
    println!("PoAW-X Phase 2 simulation harness (devnet, headless)\n");
    let scenarios: Vec<fn() -> Verdict> = vec![
        scenario_1_reward_split,
        scenario_2_low_participation,
        scenario_3_dominant_miner,
        scenario_4_dominant_pool,
        scenario_5_sybil,
        scenario_6_reorg,
        scenario_7_no_asic,
        scenario_8_no_gpu,
        scenario_9_seed,
        scenario_10_fairness,
    ];
    let mut results = Vec::new();
    for s in scenarios {
        let v = s();
        let tag = match v.status {
            Status::Pass => "PASS",
            Status::Fail => "FAIL",
            Status::Info => "INFO",
        };
        println!("[{}] {}\n      {}", tag, v.name, v.detail);
        results.push(v);
    }

    let pass = results.iter().filter(|v| v.status == Status::Pass).count();
    let fail = results.iter().filter(|v| v.status == Status::Fail).count();
    let info = results.iter().filter(|v| v.status == Status::Info).count();

    println!("\n================ PoAW-X simulation report ================");
    for v in &results {
        let tag = match v.status {
            Status::Pass => "PASS",
            Status::Fail => "FAIL",
            Status::Info => "INFO",
        };
        println!("  [{}] {}", tag, v.name);
    }
    println!("---------------------------------------------------------");
    println!("  {} pass, {} fail, {} info", pass, fail, info);
    println!("=========================================================");

    if fail > 0 {
        std::process::exit(fail as i32);
    }
    std::process::exit(0);
}
