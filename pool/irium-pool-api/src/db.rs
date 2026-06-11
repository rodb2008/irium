use rusqlite::{Connection, Result, params};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct BlockRow {
    pub height: u64,
    pub miner_address: String,
    pub block_time: u64,
    pub difficulty: f64,
    pub reward_sats: u64,
    pub hash: String,
    pub found_at_unix: u64,
}

pub fn init(path: &str) -> Result<Connection> {
    let conn = Connection::open(path)?;
    conn.execute_batch("
        PRAGMA journal_mode=WAL;
        CREATE TABLE IF NOT EXISTS blocks (
            height        INTEGER PRIMARY KEY,
            miner_address TEXT    NOT NULL DEFAULT '',
            block_time    INTEGER NOT NULL DEFAULT 0,
            difficulty    REAL    NOT NULL DEFAULT 0.0,
            reward_sats   INTEGER NOT NULL DEFAULT 5000000000,
            hash          TEXT    NOT NULL DEFAULT '',
            found_at_unix INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS idx_blocks_miner  ON blocks(miner_address);
        CREATE INDEX IF NOT EXISTS idx_blocks_time   ON blocks(found_at_unix);
        CREATE TABLE IF NOT EXISTS pool_snapshots (
            unix_time          INTEGER PRIMARY KEY,
            total_hashrate_hps REAL    NOT NULL DEFAULT 0.0,
            active_miners      INTEGER NOT NULL DEFAULT 0,
            blocks_found_today INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS idx_snap_time ON pool_snapshots(unix_time);
    ")?;
    Ok(conn)
}

pub fn upsert_block(conn: &Connection, b: &BlockRow) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO blocks
         (height, miner_address, block_time, difficulty, reward_sats, hash, found_at_unix)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![b.height, b.miner_address, b.block_time,
                b.difficulty, b.reward_sats, b.hash, b.found_at_unix],
    )?;
    Ok(())
}

pub fn tip_height(conn: &Connection) -> Option<u64> {
    conn.query_row(
        "SELECT MAX(height) FROM blocks",
        [],
        |r| r.get::<_, Option<u64>>(0),
    )
    .ok()
    .flatten()
}

pub fn get_blocks(conn: &Connection, limit: u64, offset: u64) -> Result<Vec<BlockRow>> {
    let mut stmt = conn.prepare(
        "SELECT height, miner_address, block_time, difficulty, reward_sats, hash, found_at_unix
         FROM blocks ORDER BY height DESC LIMIT ?1 OFFSET ?2",
    )?;
    let rows = stmt.query_map(params![limit, offset], row_to_block)?;
    rows.collect()
}

pub fn get_block(conn: &Connection, height: u64) -> Result<Option<BlockRow>> {
    let mut stmt = conn.prepare(
        "SELECT height, miner_address, block_time, difficulty, reward_sats, hash, found_at_unix
         FROM blocks WHERE height = ?1",
    )?;
    let mut rows = stmt.query_map(params![height], row_to_block)?;
    Ok(rows.next().transpose()?)
}

pub fn count_blocks(conn: &Connection) -> u64 {
    conn.query_row("SELECT COUNT(*) FROM blocks", [], |r| r.get::<_, u64>(0))
        .unwrap_or(0)
}

pub fn blocks_for_miner(conn: &Connection, address: &str, limit: u64) -> Result<Vec<BlockRow>> {
    let mut stmt = conn.prepare(
        "SELECT height, miner_address, block_time, difficulty, reward_sats, hash, found_at_unix
         FROM blocks WHERE miner_address = ?1 ORDER BY height DESC LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![address, limit], row_to_block)?;
    rows.collect()
}

pub fn count_blocks_for_miner(conn: &Connection, address: &str) -> u64 {
    conn.query_row(
        "SELECT COUNT(*) FROM blocks WHERE miner_address = ?1",
        params![address],
        |r| r.get::<_, u64>(0),
    )
    .unwrap_or(0)
}

pub fn insert_snapshot(
    conn: &Connection,
    unix_time: u64,
    hashrate: f64,
    active_miners: u64,
    blocks_today: u64,
) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO pool_snapshots
         (unix_time, total_hashrate_hps, active_miners, blocks_found_today)
         VALUES (?1, ?2, ?3, ?4)",
        params![unix_time, hashrate, active_miners, blocks_today],
    )?;
    Ok(())
}

pub fn get_snapshots(conn: &Connection, since_unix: u64) -> Result<Vec<(u64, f64)>> {
    let mut stmt = conn.prepare(
        "SELECT unix_time, total_hashrate_hps FROM pool_snapshots
         WHERE unix_time >= ?1 ORDER BY unix_time ASC",
    )?;
    let rows = stmt.query_map(params![since_unix], |r| {
        Ok((r.get::<_, u64>(0)?, r.get::<_, f64>(1)?))
    })?;
    rows.collect()
}

pub fn blocks_found_since(conn: &Connection, since_unix: u64) -> u64 {
    conn.query_row(
        "SELECT COUNT(*) FROM blocks WHERE found_at_unix >= ?1",
        params![since_unix],
        |r| r.get::<_, u64>(0),
    )
    .unwrap_or(0)
}

fn row_to_block(r: &rusqlite::Row<'_>) -> rusqlite::Result<BlockRow> {
    Ok(BlockRow {
        height:        r.get(0)?,
        miner_address: r.get(1)?,
        block_time:    r.get(2)?,
        difficulty:    r.get(3)?,
        reward_sats:   r.get(4)?,
        hash:          r.get(5)?,
        found_at_unix: r.get(6)?,
    })
}
