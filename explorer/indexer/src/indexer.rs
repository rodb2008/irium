
use anyhow::Result;
use sqlx::PgPool;
use tracing::{error, info, warn};

use crate::config::Config;
use crate::db::{read, write};
use crate::rpc::RpcClient;

pub async fn run(pool: PgPool, rpc: RpcClient, cfg: Config) -> Result<()> {
    info!("indexer started");
    loop {
        match sync_once(&pool, &rpc, &cfg).await {
            Ok(indexed) => {
                if indexed == 0 {
                    tokio::time::sleep(
                        std::time::Duration::from_millis(cfg.poll_interval_ms)
                    ).await;
                }
            }
            Err(e) => {
                error!("sync error: {e:#}");
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
        }
    }
}

async fn sync_once(pool: &PgPool, rpc: &RpcClient, cfg: &Config) -> Result<u64> {
    let status = rpc.get_status().await?;
    let chain_height = status.height;

    let (synced_height, synced_hash) = read::get_indexer_state(pool).await?;

    if synced_height >= chain_height { return Ok(0); }

    if synced_height >= 0 {
        let scan_from = (synced_height - cfg.reorg_scan_depth as i64).max(0);
        match detect_reorg(rpc, scan_from, synced_height, &synced_hash).await {
            Ok(None) => {}
            Ok(Some(fork_height)) => {
                warn!("reorg detected at height {fork_height}, rolling back");
                write::rollback_above(pool, fork_height).await?;
                return Ok(0);
            }
            Err(e) => { warn!("reorg check failed: {e:#}"); }
        }
    }

    let from = (synced_height + 1).max(0);
    let count = ((chain_height - from + 1) as u64).min(cfg.batch_size);
    info!("indexing heights {from}..{}", from + count as i64 - 1);

    let resp = rpc.get_blocks(from, count).await?;
    let mut indexed = 0u64;
    for block in &resp.blocks {
        write::index_block(pool, block).await?;
        indexed += 1;
        if indexed % 100 == 0 {
            info!("  indexed {indexed} blocks (height {})", block.height);
        }
    }
    info!("batch done: {indexed} blocks");
    Ok(indexed)
}

async fn detect_reorg(
    rpc: &RpcClient,
    scan_from: i64,
    synced_height: i64,
    synced_hash: &str,
) -> Result<Option<i64>> {
    if synced_hash.is_empty() { return Ok(None); }
    let resp = rpc.get_blocks(synced_height, 1).await?;
    let Some(tip) = resp.blocks.first() else { return Ok(None); };
    if tip.header.hash == synced_hash { return Ok(None); }
    let count = (synced_height - scan_from).max(1) as u64;
    let resp = rpc.get_blocks(scan_from, count).await?;
    let fork = resp.blocks.iter()
        .enumerate()
        .find(|(_, b)| b.header.hash != b.header.prev_hash)
        .map(|(i, _)| scan_from + i as i64)
        .unwrap_or(scan_from);
    Ok(Some(fork))
}
