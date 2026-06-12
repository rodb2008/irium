
use anyhow::Result;
use sqlx::{PgPool, Postgres, Transaction};
use chrono::{DateTime, Utc};
use crate::decoder::{
    script::{classify_script, ScriptClass},
    tx::{decode_tx, TxInput, TxOutput},
};
use crate::rpc::RpcBlock;

// ─── Top-level entry point ─────────────────────────────────────────────────

pub async fn index_block(pool: &PgPool, block: &RpcBlock) -> Result<()> {
    let mut dbtx = pool.begin().await?;
    let timestamp: DateTime<Utc> = DateTime::from_timestamp(block.header.time, 0)
        .unwrap_or_default();

    let parsed_txs: Vec<_> = block.tx_hex.iter().enumerate()
        .map(|(i, hex)| {
            let tx = decode_tx(hex)?;
            Ok((i, tx))
        })
        .collect::<Result<Vec<_>>>()?;

    let total_reward: i64 = parsed_txs.get(0)
        .map(|(_, t)| t.outputs.iter().map(|o| o.value).sum())
        .unwrap_or(0);

    // Upsert block
    sqlx::query(
        "INSERT INTO blocks \
         (height,hash,prev_hash,merkle_root,timestamp,difficulty,nonce,tx_count,total_reward,miner_address,size_bytes) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11) \
         ON CONFLICT (height) DO NOTHING"
    )
    .bind(block.height)
    .bind(&block.header.hash)
    .bind(&block.header.prev_hash)
    .bind(&block.header.merkle_root)
    .bind(timestamp)
    .bind(&block.header.bits)
    .bind(block.header.nonce.to_string())
    .bind(parsed_txs.len() as i32)
    .bind(total_reward)
    .bind(block.miner_address.as_deref())
    .bind(0i32)
    .execute(&mut *dbtx)
    .await?;

    for (tx_index, parsed) in &parsed_txs {
        let is_coinbase = parsed.inputs.first().map(|i| i.is_coinbase()).unwrap_or(false);
        let total_out: i64 = parsed.outputs.iter().map(|o| o.value).sum();

        sqlx::query(
            "INSERT INTO txs \
             (txid,block_height,block_hash,tx_index,version,locktime,is_coinbase,input_count,output_count,total_out,fee) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11) \
             ON CONFLICT (txid) DO NOTHING"
        )
        .bind(&parsed.txid)
        .bind(block.height)
        .bind(&block.header.hash)
        .bind(*tx_index as i32)
        .bind(parsed.version)
        .bind(parsed.locktime as i32)
        .bind(is_coinbase)
        .bind(parsed.inputs.len() as i32)
        .bind(parsed.outputs.len() as i32)
        .bind(total_out)
        .bind(0i64)
        .execute(&mut *dbtx)
        .await?;

        for (vin_idx, inp) in parsed.inputs.iter().enumerate() {
            insert_input(&mut dbtx, &parsed.txid, vin_idx, inp, is_coinbase).await?;
        }
        for (vout_idx, out) in parsed.outputs.iter().enumerate() {
            insert_output(&mut dbtx, &parsed.txid, vout_idx, out, block.height).await?;
        }
        if !is_coinbase {
            for inp in &parsed.inputs {
                mark_output_spent(&mut dbtx, &inp.prev_txid, inp.prev_vout, &parsed.txid).await?;
            }
        }
    }

    if let Some(miner) = &block.miner_address {
        upsert_miner(&mut dbtx, miner, total_reward, block.height, &block.header.hash).await?;
    }

    sqlx::query(
        "UPDATE indexer_state SET synced_height=$1, synced_block_hash=$2, last_updated_at=NOW() WHERE id=1"
    )
    .bind(block.height)
    .bind(&block.header.hash)
    .execute(&mut *dbtx)
    .await?;

    dbtx.commit().await?;
    Ok(())
}

// ─── Helpers ──────────────────────────────────────────────────────────────

async fn insert_input(
    dbtx: &mut Transaction<'_, Postgres>,
    txid: &str,
    vin_idx: usize,
    inp: &TxInput,
    is_coinbase: bool,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO tx_inputs (txid,vin_index,prev_txid,prev_vout,script_sig_hex,sequence,is_coinbase) \
         VALUES ($1,$2,$3,$4,$5,$6,$7) ON CONFLICT DO NOTHING"
    )
    .bind(txid)
    .bind(vin_idx as i32)
    .bind(&inp.prev_txid)
    .bind(inp.prev_vout as i64)
    .bind(hex::encode(&inp.script_sig))
    .bind(inp.sequence as i64)
    .bind(is_coinbase)
    .execute(&mut **dbtx)
    .await?;
    Ok(())
}

async fn insert_output(
    dbtx: &mut Transaction<'_, Postgres>,
    txid: &str,
    vout_idx: usize,
    out: &TxOutput,
    block_height: i64,
) -> Result<()> {
    let class = classify_script(&out.script_pubkey, out.value);

    let script_type: &str;
    let address: Option<String>;
    let is_htlc: bool;
    let htlc_variant: Option<String>;
    let timeout_height: Option<i64>;
    let secret_hash: Option<String>;
    let recipient_addr: Option<String>;
    let refund_addr: Option<String>;

    match &class {
        ScriptClass::P2Pkh { address: addr, .. } => {
            script_type = "p2pkh";
            address = Some(addr.clone());
            is_htlc = false;
            htlc_variant = None; timeout_height = None;
            secret_hash = None; recipient_addr = None; refund_addr = None;
        }
        ScriptClass::Htlc(p) => {
            script_type = "htlc";
            address = Some(p.recipient_addr.clone());
            is_htlc = true;
            htlc_variant = Some(p.variant.as_str().to_string());
            timeout_height = Some(p.timeout_height as i64);
            secret_hash = Some(hex::encode(p.secret_hash));
            recipient_addr = Some(p.recipient_addr.clone());
            refund_addr = Some(p.refund_addr.clone());
        }
        ScriptClass::OpReturn { .. } => {
            script_type = "op_return";
            address = None; is_htlc = false;
            htlc_variant = None; timeout_height = None;
            secret_hash = None; recipient_addr = None; refund_addr = None;
        }
        ScriptClass::IriumData => {
            script_type = "irium_data";
            address = None; is_htlc = false;
            htlc_variant = None; timeout_height = None;
            secret_hash = None; recipient_addr = None; refund_addr = None;
        }
        ScriptClass::Unknown => {
            script_type = "unknown";
            address = None; is_htlc = false;
            htlc_variant = None; timeout_height = None;
            secret_hash = None; recipient_addr = None; refund_addr = None;
        }
    }

    sqlx::query(
        "INSERT INTO tx_outputs (txid,vout,value,script_hex,script_type,address) \
         VALUES ($1,$2,$3,$4,$5,$6) ON CONFLICT (txid,vout) DO NOTHING"
    )
    .bind(txid)
    .bind(vout_idx as i32)
    .bind(out.value)
    .bind(hex::encode(&out.script_pubkey))
    .bind(script_type)
    .bind(address.as_deref())
    .execute(&mut **dbtx)
    .await?;

    if let Some(addr) = &address {
        sqlx::query(
            "INSERT INTO address_stats (address,balance,total_received,tx_count,first_seen_height,last_seen_height) \
             VALUES ($1,$2,$2,1,$3,$3) \
             ON CONFLICT (address) DO UPDATE SET \
               balance          = address_stats.balance + EXCLUDED.balance, \
               total_received   = address_stats.total_received + EXCLUDED.total_received, \
               tx_count         = address_stats.tx_count + 1, \
               last_seen_height = GREATEST(address_stats.last_seen_height, EXCLUDED.last_seen_height)"
        )
        .bind(addr)
        .bind(out.value)
        .bind(block_height)
        .execute(&mut **dbtx)
        .await?;
    }

    if is_htlc {
        sqlx::query(
            "INSERT INTO htlc_outputs \
             (txid,vout,block_height,htlc_type,value,recipient_addr,refund_addr,secret_hash,timeout_height) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9) ON CONFLICT (txid,vout) DO NOTHING"
        )
        .bind(txid)
        .bind(vout_idx as i32)
        .bind(block_height)
        .bind(htlc_variant.as_deref().unwrap_or(""))
        .bind(out.value)
        .bind(recipient_addr.as_deref().unwrap_or(""))
        .bind(refund_addr.as_deref().unwrap_or(""))
        .bind(secret_hash.as_deref().unwrap_or(""))
        .bind(timeout_height.unwrap_or(0))
        .execute(&mut **dbtx)
        .await?;
    }

    if let ScriptClass::OpReturn { anchor: Some(anch), .. } = &class {
        sqlx::query(
            "INSERT INTO agreements (agreement_hash,anchor_type,txid,block_height,milestone_id) \
             VALUES ($1,$2,$3,$4,$5) \
             ON CONFLICT (agreement_hash) DO UPDATE SET \
               anchor_type=$2, txid=$3, block_height=$4, milestone_id=$5"
        )
        .bind(&anch.agreement_hash)
        .bind(anch.anchor_type.as_str())
        .bind(txid)
        .bind(block_height)
        .bind(anch.milestone_id.as_deref())
        .execute(&mut **dbtx)
        .await?;
    }

    Ok(())
}

async fn mark_output_spent(
    dbtx: &mut Transaction<'_, Postgres>,
    prev_txid: &str,
    prev_vout: u32,
    spending_txid: &str,
) -> Result<()> {
    sqlx::query(
        "UPDATE address_stats SET \
           balance    = address_stats.balance - COALESCE(o.value, 0), \
           total_sent = address_stats.total_sent + COALESCE(o.value, 0) \
         FROM tx_outputs o \
         WHERE o.txid=$1 AND o.vout=$2 AND address_stats.address=o.address"
    )
    .bind(prev_txid)
    .bind(prev_vout as i32)
    .execute(&mut **dbtx)
    .await?;

    sqlx::query("UPDATE tx_outputs SET spent_by_txid=$3 WHERE txid=$1 AND vout=$2")
        .bind(prev_txid).bind(prev_vout as i32).bind(spending_txid)
        .execute(&mut **dbtx).await?;

    sqlx::query("UPDATE htlc_outputs SET state='claimed', spend_txid=$3 WHERE txid=$1 AND vout=$2")
        .bind(prev_txid).bind(prev_vout as i32).bind(spending_txid)
        .execute(&mut **dbtx).await?;

    Ok(())
}

async fn upsert_miner(
    dbtx: &mut Transaction<'_, Postgres>,
    address: &str,
    reward: i64,
    block_height: i64,
    block_hash: &str,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO mining_leaderboard (address,blocks_mined,total_reward,last_block_height,last_block_hash) \
         VALUES ($1,1,$2,$3,$4) \
         ON CONFLICT (address) DO UPDATE SET \
           blocks_mined      = mining_leaderboard.blocks_mined + 1, \
           total_reward      = mining_leaderboard.total_reward + EXCLUDED.total_reward, \
           last_block_height = GREATEST(mining_leaderboard.last_block_height, EXCLUDED.last_block_height), \
           last_block_hash   = CASE \
             WHEN mining_leaderboard.last_block_height < EXCLUDED.last_block_height \
             THEN EXCLUDED.last_block_hash \
             ELSE mining_leaderboard.last_block_hash \
           END"
    )
    .bind(address).bind(reward).bind(block_height).bind(block_hash)
    .execute(&mut **dbtx).await?;
    Ok(())
}

pub async fn rollback_above(pool: &PgPool, reorg_height: i64) -> Result<()> {
    let mut dbtx = pool.begin().await?;
    sqlx::query("DELETE FROM blocks WHERE height > $1")
        .bind(reorg_height).execute(&mut *dbtx).await?;
    sqlx::query("UPDATE indexer_state SET synced_height=$1, synced_block_hash='' WHERE id=1")
        .bind(reorg_height).execute(&mut *dbtx).await?;
    dbtx.commit().await?;
    Ok(())
}
