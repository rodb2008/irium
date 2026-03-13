use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};

use crate::model::{Swap, SwapEvent, SwapState};

#[derive(Clone)]
pub struct Storage {
    conn: Arc<Mutex<Connection>>,
}

impl Storage {
    pub fn open(path: &str) -> Result<Self> {
        let conn = Connection::open(path).with_context(|| format!("open sqlite at {path}"))?;
        let s = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        s.init()?;
        Ok(s)
    }

    fn init(&self) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| anyhow!("sqlite lock poisoned"))?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS swaps (
                id TEXT PRIMARY KEY,
                tester_handle TEXT NOT NULL,
                session_token TEXT NOT NULL,
                btc_receive_address TEXT NOT NULL,
                btc_htlc_address TEXT,
                btc_funding_txid TEXT,
                btc_spent_txid TEXT,
                irium_htlc_txid TEXT,
                irium_htlc_vout INTEGER,
                irium_spend_txid TEXT,
                secret_hash_hex TEXT NOT NULL,
                state TEXT NOT NULL,
                next_action TEXT NOT NULL,
                expected_amount_sats INTEGER NOT NULL,
                btc_confirmations INTEGER NOT NULL,
                timeout_height_hint INTEGER,
                manual_review INTEGER NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE UNIQUE INDEX IF NOT EXISTS idx_swaps_session_token ON swaps(session_token);
            CREATE TABLE IF NOT EXISTS swap_events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                swap_id TEXT NOT NULL,
                event_type TEXT NOT NULL,
                payload TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_swap_events_swap_id ON swap_events(swap_id, id);
            CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );",
        )?;
        let _ = conn.execute("ALTER TABLE swaps ADD COLUMN irium_htlc_vout INTEGER", []);
        Ok(())
    }

    pub fn set_intake_paused(&self, paused: bool) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| anyhow!("sqlite lock poisoned"))?;
        conn.execute(
            "INSERT INTO settings(key, value) VALUES('intake_paused', ?1)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![if paused { "1" } else { "0" }],
        )?;
        Ok(())
    }

    pub fn intake_paused(&self) -> Result<bool> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| anyhow!("sqlite lock poisoned"))?;
        let v: Option<String> = conn
            .query_row(
                "SELECT value FROM settings WHERE key='intake_paused'",
                [],
                |r| r.get(0),
            )
            .optional()?;
        Ok(matches!(v.as_deref(), Some("1") | Some("true")))
    }

    pub fn insert_swap(&self, swap: &Swap) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| anyhow!("sqlite lock poisoned"))?;
        conn.execute(
            "INSERT INTO swaps(
                id,tester_handle,session_token,btc_receive_address,btc_htlc_address,btc_funding_txid,btc_spent_txid,
                irium_htlc_txid,irium_htlc_vout,irium_spend_txid,secret_hash_hex,state,next_action,expected_amount_sats,
                btc_confirmations,timeout_height_hint,manual_review,created_at,updated_at
            ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19)",
            params![
                swap.id,
                swap.tester_handle,
                swap.session_token,
                swap.btc_receive_address,
                swap.btc_htlc_address,
                swap.btc_funding_txid,
                swap.btc_spent_txid,
                swap.irium_htlc_txid,
                swap.irium_htlc_vout.map(|v| v as i64),
                swap.irium_spend_txid,
                swap.secret_hash_hex,
                state_to_str(swap.state),
                swap.next_action,
                swap.expected_amount_sats as i64,
                swap.btc_confirmations as i64,
                swap.timeout_height_hint.map(|v| v as i64),
                if swap.manual_review { 1 } else { 0 },
                swap.created_at.to_rfc3339(),
                swap.updated_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn get_swap_public(&self, id: &str) -> Result<Option<Swap>> {
        self.get_swap_internal("id", id)
    }

    pub fn list_live_swaps(&self) -> Result<Vec<Swap>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| anyhow!("sqlite lock poisoned"))?;
        let mut stmt = conn.prepare(
            "SELECT id,tester_handle,session_token,btc_receive_address,btc_htlc_address,btc_funding_txid,btc_spent_txid,
                    irium_htlc_txid,irium_htlc_vout,irium_spend_txid,secret_hash_hex,state,next_action,expected_amount_sats,btc_confirmations,
                    timeout_height_hint,manual_review,created_at,updated_at
             FROM swaps WHERE state NOT IN ('claimed','refunded','failed','expired') ORDER BY updated_at DESC",
        )?;
        let rows = stmt.query_map([], row_to_swap)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn update_swap(&self, swap: &Swap) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| anyhow!("sqlite lock poisoned"))?;
        conn.execute(
            "UPDATE swaps SET
                btc_htlc_address=?2,btc_funding_txid=?3,btc_spent_txid=?4,irium_htlc_txid=?5,irium_htlc_vout=?6,irium_spend_txid=?7,
                state=?8,next_action=?9,btc_confirmations=?10,timeout_height_hint=?11,manual_review=?12,updated_at=?13
             WHERE id=?1",
            params![
                swap.id,
                swap.btc_htlc_address,
                swap.btc_funding_txid,
                swap.btc_spent_txid,
                swap.irium_htlc_txid,
                swap.irium_htlc_vout.map(|v| v as i64),
                swap.irium_spend_txid,
                state_to_str(swap.state),
                swap.next_action,
                swap.btc_confirmations as i64,
                swap.timeout_height_hint.map(|v| v as i64),
                if swap.manual_review { 1 } else { 0 },
                Utc::now().to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn append_event(
        &self,
        swap_id: &str,
        event_type: &str,
        payload: serde_json::Value,
    ) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| anyhow!("sqlite lock poisoned"))?;
        conn.execute(
            "INSERT INTO swap_events(swap_id,event_type,payload,created_at) VALUES(?1,?2,?3,?4)",
            params![
                swap_id,
                event_type,
                payload.to_string(),
                Utc::now().to_rfc3339()
            ],
        )?;
        Ok(())
    }

    pub fn list_events(&self, swap_id: &str) -> Result<Vec<SwapEvent>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| anyhow!("sqlite lock poisoned"))?;
        let mut stmt = conn.prepare(
            "SELECT id,swap_id,event_type,payload,created_at FROM swap_events WHERE swap_id=?1 ORDER BY id ASC",
        )?;
        let rows = stmt.query_map(params![swap_id], |row| {
            let payload_s: String = row.get(3)?;
            let created_s: String = row.get(4)?;
            Ok(SwapEvent {
                id: row.get(0)?,
                swap_id: row.get(1)?,
                event_type: row.get(2)?,
                payload: serde_json::from_str(&payload_s)
                    .unwrap_or(serde_json::json!({"raw": payload_s})),
                created_at: DateTime::parse_from_rfc3339(&created_s)
                    .map(|d| d.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now()),
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    fn get_swap_internal(&self, field: &str, value: &str) -> Result<Option<Swap>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| anyhow!("sqlite lock poisoned"))?;
        let sql = format!(
            "SELECT id,tester_handle,session_token,btc_receive_address,btc_htlc_address,btc_funding_txid,btc_spent_txid,
                    irium_htlc_txid,irium_htlc_vout,irium_spend_txid,secret_hash_hex,state,next_action,expected_amount_sats,btc_confirmations,
                    timeout_height_hint,manual_review,created_at,updated_at
             FROM swaps WHERE {field}=?1"
        );
        conn.query_row(&sql, params![value], row_to_swap)
            .optional()
            .map_err(Into::into)
    }
}

fn row_to_swap(row: &rusqlite::Row) -> rusqlite::Result<Swap> {
    let state_s: String = row.get(11)?;
    let created_s: String = row.get(17)?;
    let updated_s: String = row.get(18)?;
    Ok(Swap {
        id: row.get(0)?,
        tester_handle: row.get(1)?,
        session_token: row.get(2)?,
        btc_receive_address: row.get(3)?,
        btc_htlc_address: row.get(4)?,
        btc_funding_txid: row.get(5)?,
        btc_spent_txid: row.get(6)?,
        irium_htlc_txid: row.get(7)?,
        irium_htlc_vout: row.get::<_, Option<i64>>(8)?.map(|v| v as u32),
        irium_spend_txid: row.get(9)?,
        secret_hash_hex: row.get(10)?,
        state: str_to_state(&state_s),
        next_action: row.get(12)?,
        expected_amount_sats: row.get::<_, i64>(13)? as u64,
        btc_confirmations: row.get::<_, i64>(14)? as u32,
        timeout_height_hint: row.get::<_, Option<i64>>(15)?.map(|v| v as u64),
        manual_review: row.get::<_, i64>(16)? != 0,
        created_at: DateTime::parse_from_rfc3339(&created_s)
            .map(|d| d.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now()),
        updated_at: DateTime::parse_from_rfc3339(&updated_s)
            .map(|d| d.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now()),
    })
}

pub fn state_to_str(state: SwapState) -> &'static str {
    match state {
        SwapState::Created => "created",
        SwapState::Quoted => "quoted",
        SwapState::Accepted => "accepted",
        SwapState::SecretCommitted => "secret_committed",
        SwapState::IriumHtlcCreated => "irium_htlc_created",
        SwapState::IriumHtlcConfirmed => "irium_htlc_confirmed",
        SwapState::BtcHtlcCreated => "btc_htlc_created",
        SwapState::BtcHtlcConfirmed => "btc_htlc_confirmed",
        SwapState::ClaimInitiated => "claim_initiated",
        SwapState::Claimed => "claimed",
        SwapState::RefundPending => "refund_pending",
        SwapState::Refunded => "refunded",
        SwapState::Failed => "failed",
        SwapState::Expired => "expired",
        SwapState::ManualReview => "manual_review",
    }
}

pub fn str_to_state(s: &str) -> SwapState {
    match s {
        "created" => SwapState::Created,
        "quoted" => SwapState::Quoted,
        "accepted" => SwapState::Accepted,
        "secret_committed" => SwapState::SecretCommitted,
        "irium_htlc_created" => SwapState::IriumHtlcCreated,
        "irium_htlc_confirmed" => SwapState::IriumHtlcConfirmed,
        "btc_htlc_created" => SwapState::BtcHtlcCreated,
        "btc_htlc_confirmed" => SwapState::BtcHtlcConfirmed,
        "claim_initiated" => SwapState::ClaimInitiated,
        "claimed" => SwapState::Claimed,
        "refund_pending" => SwapState::RefundPending,
        "refunded" => SwapState::Refunded,
        "failed" => SwapState::Failed,
        "expired" => SwapState::Expired,
        "manual_review" => SwapState::ManualReview,
        _ => SwapState::Failed,
    }
}
