
-- 001_initial.sql — Irium Explorer DB schema
-- PostgreSQL 16
-- All heights are block heights (i64 stored as BIGINT).
-- All monetary values are in satoshis (i64 stored as BIGINT).
-- All hashes / addresses stored as TEXT (hex for hashes, base58 for addresses).

-- ─────────────────────────────────────────────────────────────────────────────
-- Indexer state (single-row, tracks sync progress and reorg detection)
-- ─────────────────────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS indexer_state (
    id                  INTEGER PRIMARY KEY DEFAULT 1,  -- always row 1
    synced_height       BIGINT NOT NULL DEFAULT -1,     -- -1 = not started
    synced_block_hash   TEXT   NOT NULL DEFAULT '',
    reorg_depth         INT    NOT NULL DEFAULT 0,
    last_updated_at     TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
INSERT INTO indexer_state (id) VALUES (1) ON CONFLICT DO NOTHING;

-- ─────────────────────────────────────────────────────────────────────────────
-- Blocks
-- ─────────────────────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS blocks (
    height          BIGINT      PRIMARY KEY,
    hash            TEXT        NOT NULL UNIQUE,
    prev_hash       TEXT        NOT NULL,
    merkle_root     TEXT        NOT NULL,
    timestamp       TIMESTAMPTZ NOT NULL,
    difficulty      TEXT        NOT NULL,   -- compact bits (e.g. "1d00ffff")
    nonce           TEXT        NOT NULL,   -- hex; large value, store as text
    tx_count        INT         NOT NULL,
    total_reward    BIGINT      NOT NULL,   -- satoshis (coinbase output sum)
    miner_address   TEXT,                  -- address of first coinbase output
    size_bytes      INT         NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_blocks_hash      ON blocks (hash);
CREATE INDEX IF NOT EXISTS idx_blocks_timestamp ON blocks (timestamp);
CREATE INDEX IF NOT EXISTS idx_blocks_miner     ON blocks (miner_address);

-- ─────────────────────────────────────────────────────────────────────────────
-- Transactions
-- ─────────────────────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS txs (
    txid            TEXT    PRIMARY KEY,
    block_height    BIGINT  NOT NULL REFERENCES blocks (height) ON DELETE CASCADE,
    block_hash      TEXT    NOT NULL,
    tx_index        INT     NOT NULL,   -- position in block (0=coinbase)
    version         INT     NOT NULL,
    locktime        INT     NOT NULL,
    is_coinbase     BOOLEAN NOT NULL,
    input_count     INT     NOT NULL,
    output_count    INT     NOT NULL,
    total_out       BIGINT  NOT NULL,   -- sum of all output values
    fee             BIGINT  NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_txs_block_height ON txs (block_height);
CREATE INDEX IF NOT EXISTS idx_txs_block_hash   ON txs (block_hash);

-- ─────────────────────────────────────────────────────────────────────────────
-- Transaction inputs
-- ─────────────────────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS tx_inputs (
    id              BIGSERIAL   PRIMARY KEY,
    txid            TEXT        NOT NULL REFERENCES txs (txid) ON DELETE CASCADE,
    vin_index       INT         NOT NULL,
    prev_txid       TEXT        NOT NULL,   -- all-zeros for coinbase
    prev_vout       BIGINT      NOT NULL,
    script_sig_hex  TEXT        NOT NULL,
    sequence        BIGINT      NOT NULL,
    is_coinbase     BOOLEAN     NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_tx_inputs_txid      ON tx_inputs (txid);
CREATE INDEX IF NOT EXISTS idx_tx_inputs_prev_txid ON tx_inputs (prev_txid);

-- ─────────────────────────────────────────────────────────────────────────────
-- Transaction outputs
-- ─────────────────────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS tx_outputs (
    id              BIGSERIAL   PRIMARY KEY,
    txid            TEXT        NOT NULL REFERENCES txs (txid) ON DELETE CASCADE,
    vout            INT         NOT NULL,
    value           BIGINT      NOT NULL,
    script_hex      TEXT        NOT NULL,
    script_type     TEXT        NOT NULL,   -- p2pkh | htlc | op_return | irium_data | unknown
    address         TEXT,                   -- NULL for non-P2PKH / non-HTLC
    spent_by_txid   TEXT,                   -- filled in when input referencing this output is indexed
    spent_by_vin    INT
);
CREATE INDEX IF NOT EXISTS idx_tx_outputs_txid        ON tx_outputs (txid);
CREATE INDEX IF NOT EXISTS idx_tx_outputs_address     ON tx_outputs (address);
CREATE INDEX IF NOT EXISTS idx_tx_outputs_spent_by    ON tx_outputs (spent_by_txid);
CREATE UNIQUE INDEX IF NOT EXISTS idx_tx_outputs_uniq ON tx_outputs (txid, vout);

-- ─────────────────────────────────────────────────────────────────────────────
-- Address balance + stats (updated incrementally as blocks are indexed)
-- ─────────────────────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS address_stats (
    address         TEXT    PRIMARY KEY,
    balance         BIGINT  NOT NULL DEFAULT 0,   -- current unspent satoshis
    total_received  BIGINT  NOT NULL DEFAULT 0,
    total_sent      BIGINT  NOT NULL DEFAULT 0,
    tx_count        INT     NOT NULL DEFAULT 0,
    first_seen_height BIGINT,
    last_seen_height  BIGINT
);

-- ─────────────────────────────────────────────────────────────────────────────
-- Agreement anchors (parsed from OP_RETURN outputs)
-- ─────────────────────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS agreements (
    agreement_hash  TEXT    PRIMARY KEY,    -- 64 hex chars
    anchor_type     TEXT    NOT NULL,       -- fund | release | refund | milestone_release | dispute_resolve
    txid            TEXT    NOT NULL,
    block_height    BIGINT  NOT NULL,
    milestone_id    TEXT,                   -- set for milestone_release
    discovered_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX IF NOT EXISTS idx_agreements_txid         ON agreements (txid);
CREATE INDEX IF NOT EXISTS idx_agreements_block_height ON agreements (block_height);
CREATE INDEX IF NOT EXISTS idx_agreements_type         ON agreements (anchor_type);

-- Party addresses associated with each agreement anchor tx
-- (extracted from adjacent HTLC or P2PKH outputs in the same tx)
CREATE TABLE IF NOT EXISTS agreement_parties (
    id              BIGSERIAL   PRIMARY KEY,
    agreement_hash  TEXT        NOT NULL REFERENCES agreements (agreement_hash) ON DELETE CASCADE,
    role            TEXT        NOT NULL,   -- recipient | refund | miner
    address         TEXT        NOT NULL,
    htlc_type       TEXT,                   -- irium_v1 | btc_swap_v1 | ltc_swap_v1 | NULL
    timeout_height  BIGINT
);
CREATE INDEX IF NOT EXISTS idx_agr_parties_hash ON agreement_parties (agreement_hash);
CREATE INDEX IF NOT EXISTS idx_agr_parties_addr ON agreement_parties (address);

-- ─────────────────────────────────────────────────────────────────────────────
-- HTLC outputs (settlement layer — every HTLC output gets a row)
-- ─────────────────────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS htlc_outputs (
    id              BIGSERIAL   PRIMARY KEY,
    txid            TEXT        NOT NULL,
    vout            INT         NOT NULL,
    block_height    BIGINT      NOT NULL,
    htlc_type       TEXT        NOT NULL,   -- irium_v1 | btc_swap_v1 | ltc_swap_v1
    value           BIGINT      NOT NULL,
    recipient_addr  TEXT        NOT NULL,
    refund_addr     TEXT        NOT NULL,
    secret_hash     TEXT        NOT NULL,   -- 64 hex chars (all-zeros for swap HTLCs)
    timeout_height  BIGINT      NOT NULL,
    -- State (updated as spend is indexed)
    state           TEXT        NOT NULL DEFAULT 'pending',  -- pending | claimed | refunded | expired
    spend_txid      TEXT,
    spend_block_height BIGINT
);
CREATE INDEX IF NOT EXISTS idx_htlc_txid           ON htlc_outputs (txid, vout);
CREATE INDEX IF NOT EXISTS idx_htlc_recipient      ON htlc_outputs (recipient_addr);
CREATE INDEX IF NOT EXISTS idx_htlc_refund         ON htlc_outputs (refund_addr);
CREATE INDEX IF NOT EXISTS idx_htlc_state          ON htlc_outputs (state);
CREATE INDEX IF NOT EXISTS idx_htlc_timeout        ON htlc_outputs (timeout_height);
CREATE UNIQUE INDEX IF NOT EXISTS idx_htlc_uniq    ON htlc_outputs (txid, vout);

-- ─────────────────────────────────────────────────────────────────────────────
-- Proof submissions (hashed proof data anchored via op_return or off-chain DB)
-- ─────────────────────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS proofs (
    id              BIGSERIAL   PRIMARY KEY,
    agreement_hash  TEXT        NOT NULL,
    proof_hash      TEXT        NOT NULL,   -- 64 hex chars
    submitted_by    TEXT        NOT NULL,   -- address
    txid            TEXT,                   -- NULL if submitted off-chain only
    block_height    BIGINT,
    submitted_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (agreement_hash, proof_hash)
);
CREATE INDEX IF NOT EXISTS idx_proofs_agreement ON proofs (agreement_hash);
CREATE INDEX IF NOT EXISTS idx_proofs_submitter ON proofs (submitted_by);

-- ─────────────────────────────────────────────────────────────────────────────
-- Mining leaderboard (refreshed per block)
-- ─────────────────────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS mining_leaderboard (
    address         TEXT    PRIMARY KEY,
    blocks_mined    INT     NOT NULL DEFAULT 0,
    total_reward    BIGINT  NOT NULL DEFAULT 0,
    last_block_height BIGINT,
    last_block_hash   TEXT
);
CREATE INDEX IF NOT EXISTS idx_mining_blocks ON mining_leaderboard (blocks_mined DESC);
