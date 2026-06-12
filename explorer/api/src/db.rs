
use anyhow::Result;
use sqlx::{PgPool, Row};
use crate::models::*;

// ─── Status ──────────────────────────────────────────────────────────────────

pub async fn get_status(pool: &PgPool) -> Result<ExplorerStatus> {
    let row = sqlx::query(
        "SELECT synced_height, synced_block_hash FROM indexer_state WHERE id=1"
    )
    .fetch_one(pool)
    .await?;
    Ok(ExplorerStatus {
        synced_height: row.get("synced_height"),
        synced_block_hash: row.get("synced_block_hash"),
    })
}

// ─── Blocks ──────────────────────────────────────────────────────────────────

pub async fn get_blocks(pool: &PgPool, limit: i64, offset: i64) -> Result<Vec<BlockSummary>> {
    let rows = sqlx::query(
        "SELECT height,hash,timestamp,tx_count,miner_address,total_reward,coinbase_tag \
         FROM blocks ORDER BY height DESC LIMIT $1 OFFSET $2"
    )
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await?;
    Ok(rows.iter().map(|r| BlockSummary {
        height: r.get("height"),
        hash: r.get("hash"),
        timestamp: r.get("timestamp"),
        tx_count: r.get("tx_count"),
        miner_address: r.get("miner_address"),
        total_reward: r.get("total_reward"),
        coinbase_tag: r.get("coinbase_tag"),
    }).collect())
}

pub async fn get_block_by_height(pool: &PgPool, height: i64) -> Result<Option<BlockDetail>> {
    let row = sqlx::query(
        "SELECT height,hash,prev_hash,merkle_root,timestamp,difficulty,nonce,tx_count,miner_address,total_reward,coinbase_tag \
         FROM blocks WHERE height=$1"
    )
    .bind(height)
    .fetch_optional(pool)
    .await?;

    let Some(row) = row else { return Ok(None); };
    let txids = get_block_txids(pool, height).await?;
    Ok(Some(BlockDetail {
        height: row.get("height"),
        hash: row.get("hash"),
        prev_hash: row.get("prev_hash"),
        merkle_root: row.get("merkle_root"),
        timestamp: row.get("timestamp"),
        difficulty: row.get("difficulty"),
        nonce: row.get("nonce"),
        tx_count: row.get("tx_count"),
        miner_address: row.get("miner_address"),
        total_reward: row.get("total_reward"),
        txids,
        coinbase_tag: row.get("coinbase_tag"),
    }))
}

pub async fn get_block_by_hash(pool: &PgPool, hash: &str) -> Result<Option<BlockDetail>> {
    let row = sqlx::query(
        "SELECT height,hash,prev_hash,merkle_root,timestamp,difficulty,nonce,tx_count,miner_address,total_reward,coinbase_tag \
         FROM blocks WHERE hash=$1"
    )
    .bind(hash)
    .fetch_optional(pool)
    .await?;

    let Some(row) = row else { return Ok(None); };
    let height: i64 = row.get("height");
    let txids = get_block_txids(pool, height).await?;
    Ok(Some(BlockDetail {
        height,
        hash: row.get("hash"),
        prev_hash: row.get("prev_hash"),
        merkle_root: row.get("merkle_root"),
        timestamp: row.get("timestamp"),
        difficulty: row.get("difficulty"),
        nonce: row.get("nonce"),
        tx_count: row.get("tx_count"),
        miner_address: row.get("miner_address"),
        total_reward: row.get("total_reward"),
        txids,
        coinbase_tag: row.get("coinbase_tag"),
    }))
}

async fn get_block_txids(pool: &PgPool, height: i64) -> Result<Vec<String>> {
    let rows = sqlx::query(
        "SELECT txid FROM txs WHERE block_height=$1 ORDER BY tx_index"
    )
    .bind(height)
    .fetch_all(pool)
    .await?;
    Ok(rows.iter().map(|r| r.get("txid")).collect())
}

// ─── Transactions ────────────────────────────────────────────────────────────

pub async fn get_tx(pool: &PgPool, txid: &str) -> Result<Option<TxDetail>> {
    let row = sqlx::query(
        "SELECT txid,block_height,block_hash,tx_index,is_coinbase,input_count,output_count,total_out,fee \
         FROM txs WHERE txid=$1"
    )
    .bind(txid)
    .fetch_optional(pool)
    .await?;
    let Some(row) = row else { return Ok(None); };
    let block_height: i64 = row.get("block_height");

    let inp_rows = sqlx::query(
        "SELECT prev_txid,prev_vout,script_sig_hex,is_coinbase FROM tx_inputs WHERE txid=$1 ORDER BY vin_index"
    ).bind(txid).fetch_all(pool).await?;

    let out_rows = sqlx::query(
        "SELECT vout,value,script_type,address,spent_by_txid FROM tx_outputs WHERE txid=$1 ORDER BY vout"
    ).bind(txid).fetch_all(pool).await?;

    Ok(Some(TxDetail {
        txid: row.get("txid"),
        block_height,
        block_hash: row.get("block_hash"),
        tx_index: row.get("tx_index"),
        is_coinbase: row.get("is_coinbase"),
        input_count: row.get("input_count"),
        output_count: row.get("output_count"),
        total_out: row.get("total_out"),
        fee: row.get("fee"),
        inputs: inp_rows.iter().map(|r| TxInput {
            prev_txid: r.get("prev_txid"),
            prev_vout: r.get("prev_vout"),
            script_sig_hex: r.get("script_sig_hex"),
            is_coinbase: r.get("is_coinbase"),
        }).collect(),
        outputs: out_rows.iter().map(|r| TxOutput {
            vout: r.get("vout"),
            value: r.get("value"),
            script_type: r.get("script_type"),
            address: r.get("address"),
            spent_by_txid: r.get("spent_by_txid"),
        }).collect(),
    }))
}

// ─── Address ─────────────────────────────────────────────────────────────────

pub async fn get_address(pool: &PgPool, address: &str) -> Result<Option<AddressStats>> {
    let row = sqlx::query(
        "SELECT address,balance,total_received,total_sent,tx_count FROM address_stats WHERE address=$1"
    )
    .bind(address)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| AddressStats {
        address: r.get("address"),
        balance: r.get("balance"),
        total_received: r.get("total_received"),
        total_sent: r.get("total_sent"),
        tx_count: r.get("tx_count"),
    }))
}

pub async fn get_address_txs(pool: &PgPool, address: &str, limit: i64) -> Result<Vec<AddressTx>> {
    let rows = sqlx::query(
        "SELECT DISTINCT t.txid, t.block_height, t.total_out \
         FROM txs t \
         JOIN tx_outputs o ON o.txid = t.txid \
         WHERE o.address=$1 \
         ORDER BY t.block_height DESC LIMIT $2"
    )
    .bind(address).bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows.iter().map(|r| AddressTx {
        txid: r.get("txid"),
        block_height: r.get("block_height"),
        total_out: r.get("total_out"),
    }).collect())
}

// ─── HTLCs ───────────────────────────────────────────────────────────────────

pub async fn get_htlcs(pool: &PgPool, address: &str, limit: i64) -> Result<Vec<HtlcInfo>> {
    let rows = sqlx::query(
        "SELECT txid,vout,block_height,htlc_type,value,recipient_addr,refund_addr,\
                secret_hash,timeout_height,state,spend_txid \
         FROM htlc_outputs \
         WHERE recipient_addr=$1 OR refund_addr=$1 \
         ORDER BY block_height DESC LIMIT $2"
    )
    .bind(address).bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows.iter().map(|r| HtlcInfo {
        txid: r.get("txid"),
        vout: r.get("vout"),
        block_height: r.get("block_height"),
        htlc_type: r.get("htlc_type"),
        value: r.get("value"),
        recipient_addr: r.get("recipient_addr"),
        refund_addr: r.get("refund_addr"),
        secret_hash: r.get("secret_hash"),
        timeout_height: r.get("timeout_height"),
        state: r.get("state"),
        spend_txid: r.get("spend_txid"),
    }).collect())
}

// ─── Agreements ──────────────────────────────────────────────────────────────

pub async fn get_agreement(pool: &PgPool, hash: &str) -> Result<Option<AgreementInfo>> {
    let row = sqlx::query(
        "SELECT agreement_hash,anchor_type,txid,block_height,milestone_id \
         FROM agreements WHERE agreement_hash=$1"
    )
    .bind(hash)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| AgreementInfo {
        agreement_hash: r.get("agreement_hash"),
        anchor_type: r.get("anchor_type"),
        txid: r.get("txid"),
        block_height: r.get("block_height"),
        milestone_id: r.get("milestone_id"),
    }))
}

// ─── Mining leaderboard ──────────────────────────────────────────────────────

pub async fn get_top_miners(pool: &PgPool, limit: i64) -> Result<Vec<MinerStats>> {
    let rows = sqlx::query(
        "SELECT address,blocks_mined,total_reward,last_block_height \
         FROM mining_leaderboard ORDER BY blocks_mined DESC LIMIT $1"
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows.iter().map(|r| MinerStats {
        address: r.get("address"),
        blocks_mined: r.get("blocks_mined"),
        total_reward: r.get("total_reward"),
        last_block_height: r.get("last_block_height"),
    }).collect())
}

// ─── Search ──────────────────────────────────────────────────────────────────

pub enum SearchResult {
    Block(i64),   // height
    Tx(String),   // txid
    Address(String),
}

pub async fn search(pool: &PgPool, query: &str) -> Result<Option<SearchResult>> {
    let q = query.trim();
    // Try integer → block height
    if let Ok(h) = q.parse::<i64>() {
        let exists: bool = sqlx::query("SELECT EXISTS(SELECT 1 FROM blocks WHERE height=$1)")
            .bind(h).fetch_one(pool).await?
            .get(0);
        if exists { return Ok(Some(SearchResult::Block(h))); }
    }
    // 64-char hex → block hash or txid
    if q.len() == 64 && q.chars().all(|c| c.is_ascii_hexdigit()) {
        let block_exists: bool = sqlx::query("SELECT EXISTS(SELECT 1 FROM blocks WHERE hash=$1)")
            .bind(q).fetch_one(pool).await?.get(0);
        if block_exists { return Ok(Some(SearchResult::Block(
            sqlx::query("SELECT height FROM blocks WHERE hash=$1")
                .bind(q).fetch_one(pool).await?.get("height")
        ))); }
        let tx_exists: bool = sqlx::query("SELECT EXISTS(SELECT 1 FROM txs WHERE txid=$1)")
            .bind(q).fetch_one(pool).await?.get(0);
        if tx_exists { return Ok(Some(SearchResult::Tx(q.to_string()))); }
    }
    // Irium address (Q/P prefix, 34 chars)
    if (q.starts_with('Q') || q.starts_with('P')) && q.len() == 34 {
        let addr_exists: bool = sqlx::query("SELECT EXISTS(SELECT 1 FROM address_stats WHERE address=$1)")
            .bind(q).fetch_one(pool).await?.get(0);
        if addr_exists { return Ok(Some(SearchResult::Address(q.to_string()))); }
    }
    Ok(None)
}
