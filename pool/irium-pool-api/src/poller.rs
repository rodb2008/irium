use crate::{AppState, LiveCache, CachedMiner};
use crate::upstream::{get_stratum, get_explorer_blocks, get_node_status};
use crate::db;
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{info, warn};

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

struct PrevWorker {
    accepted: u64,
    ts: u64,
}

pub async fn run(state: AppState) {
    let mut prev: HashMap<String, PrevWorker> = HashMap::new();
    loop {
        if let Err(e) = tick(&state, &mut prev).await {
            warn!("poller tick error: {}", e);
        }
        tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
    }
}

async fn tick(
    state: &AppState,
    prev: &mut HashMap<String, PrevWorker>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let now = now_secs();
    let cfg = &state.config;

    let (asic, cpu, solo, p443) = tokio::join!(
        get_stratum(&state.client, &cfg.stratum_asic),
        get_stratum(&state.client, &cfg.stratum_cpu),
        get_stratum(&state.client, &cfg.stratum_solo),
        get_stratum(&state.client, &cfg.stratum_443),
    );

    let port_for = |profile: &str| -> u16 {
        match profile {
            "asic" => 3333,
            "cpu"  => 3335,
            "solo" => 3336,
            "p443" => 443,
            _      => 0,
        }
    };

    let mut miners: HashMap<String, CachedMiner> = HashMap::new();

    for (metrics, profile) in [(&asic, "asic"), (&cpu, "cpu"), (&solo, "solo"), (&p443, "p443")] {
        for (address, w) in &metrics.miners {
            let key = format!("{}:{}", profile, address);
            let p = prev.entry(key).or_insert(PrevWorker { accepted: 0, ts: 0 });

            let hashrate_hps = if p.ts > 0 && now > p.ts {
                let delta_a = w.accepted.saturating_sub(p.accepted) as f64;
                let delta_t = (now - p.ts) as f64;
                if delta_t > 0.0 && delta_a > 0.0 {
                    (delta_a * w.current_diff * 4_294_967_296.0) / delta_t
                } else {
                    0.0
                }
            } else {
                0.0
            };
            p.accepted = w.accepted;
            p.ts = now;

            let entry = miners.entry(address.clone()).or_insert_with(|| CachedMiner {
                address: address.clone(),
                ..Default::default()
            });
            entry.accepted       += w.accepted;
            entry.rejected       += w.rejected;
            entry.last_share_at   = entry.last_share_at.max(w.last_share_at);
            entry.current_diff    = w.current_diff;
            entry.hashrate_hps   += hashrate_hps;
            entry.port            = port_for(profile);
            entry.profile         = profile.to_string();
            entry.active          = now.saturating_sub(w.last_share_at) < 120;
            for (reason, count) in &w.reject_reasons {
                *entry.reject_reasons.entry(reason.clone()).or_default() += count;
            }
        }
    }

    for m in miners.values_mut() {
        let total = m.accepted + m.rejected;
        m.reject_rate_pct = if total > 0 {
            (m.rejected as f64 / total as f64) * 100.0
        } else {
            0.0
        };
    }

    let total_hashrate: f64 = miners.values().map(|m| m.hashrate_hps).sum();
    let active_count = miners.values().filter(|m| m.active).count() as u64;

    let asic_hr: f64 = miners.values().filter(|m| m.port == 3333).map(|m| m.hashrate_hps).sum();
    let cpu_hr:  f64 = miners.values().filter(|m| m.port == 3335).map(|m| m.hashrate_hps).sum();
    let solo_hr: f64 = miners.values().filter(|m| m.port == 3336).map(|m| m.hashrate_hps).sum();
    let p443_hr: f64 = miners.values().filter(|m| m.port == 443).map(|m| m.hashrate_hps).sum();

    // Sync new blocks from explorer into SQLite
    let db_tip = {
        let conn = state.db.lock().unwrap();
        db::tip_height(&conn).unwrap_or(0)
    };
    let chain_height = get_node_status(&state.client, &cfg.iriumd_rpc).await.height;

    if chain_height > db_tip {
        let fetch_count = (chain_height - db_tip).min(100) + 1;
        let blocks = get_explorer_blocks(&state.client, &cfg.explorer_url, fetch_count).await;
        let conn = state.db.lock().unwrap();
        let mut inserted = 0u64;
        for b in &blocks {
            if b.height > db_tip {
                let row = db::BlockRow {
                    height:        b.height,
                    miner_address: b.miner_address.clone(),
                    block_time:    b.header.time,
                    difficulty:    0.0,
                    reward_sats:   5_000_000_000,
                    hash:          b.header.hash.clone(),
                    found_at_unix: now,
                };
                if db::upsert_block(&conn, &row).is_ok() {
                    inserted += 1;
                }
            }
        }
        if inserted > 0 {
            info!("poller: inserted {} new blocks (chain tip {})", inserted, chain_height);
        }
    }

    let today_start = now - (now % 86400);
    let (today_blocks, total_blocks) = {
        let conn = state.db.lock().unwrap();
        (db::blocks_found_since(&conn, today_start), db::count_blocks(&conn))
    };

    {
        let conn = state.db.lock().unwrap();
        let _ = db::insert_snapshot(&conn, now, total_hashrate, active_count, today_blocks);
    }

    let mut cache = state.cache.lock().unwrap();
    *cache = LiveCache {
        miners,
        total_hashrate_hps: total_hashrate,
        active_miners:      active_count,
        asic_sessions:      asic.active_tcp_sessions,
        cpu_sessions:       cpu.active_tcp_sessions,
        solo_sessions:      solo.active_tcp_sessions,
        p443_sessions:      p443.active_tcp_sessions,
        asic_hashrate:      asic_hr,
        cpu_hashrate:       cpu_hr,
        solo_hashrate:      solo_hr,
        p443_hashrate:      p443_hr,
        asic_accepted:      asic.accepted_shares,
        asic_rejected:      asic.rejected_shares,
        cpu_accepted:       cpu.accepted_shares,
        cpu_rejected:       cpu.rejected_shares,
        solo_accepted:      solo.accepted_shares,
        solo_rejected:      solo.rejected_shares,
        p443_accepted:      p443.accepted_shares,
        p443_rejected:      p443.rejected_shares,
        blocks_found_today: today_blocks,
        blocks_found_total: total_blocks,
        updated_at:         now,
    };

    Ok(())
}
