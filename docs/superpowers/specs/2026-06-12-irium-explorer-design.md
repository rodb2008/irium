# Irium Explorer Design

**Date:** 2026-06-12
**Status:** Implemented — pending end-to-end validation

## Overview

A self-hostable blockchain explorer for the Irium network, running as a set of Docker services.
Reads from `iriumd` via HTTP RPC and presents indexed data through a REST API and React frontend.

## Architecture

```
iriumd (port 38300)
    |
    v
[Indexer]  ─── PostgreSQL 16 ─── [API] (port 3400)
                                     |
                                  [Frontend / nginx] (port 3401)
```

### Components

| Service | Language | Image | Purpose |
|---------|----------|-------|---------|
| `db` | — | postgres:16-alpine | Persistent indexed data |
| `indexer` | Rust | custom | Syncs iriumd → PostgreSQL from block 0 |
| `api` | Rust (axum) | custom | REST API reading PostgreSQL |
| `frontend` | React/nginx | custom | Browser UI served by nginx |

## Database Schema (10 tables)

- `indexer_state` — single-row cursor (best_height, best_hash)
- `blocks` — height, hash, prev_hash, merkle_root, timestamp, nonce, bits, version, miner_address, total_reward, tx_count, difficulty
- `txs` — txid, block_height, tx_index, is_coinbase, fee, input_count, output_count, total_out
- `tx_inputs` — txid, vin, prev_txid, prev_vout, is_coinbase, script_sig_hex
- `tx_outputs` — txid, vout, value, script_type (p2pkh/htlc/op_return/irium_data/unknown), address, script_hex, spent_by_txid, is_htlc, htlc_type, htlc_hash, timeout_height, htlc_state
- `address_stats` — address, balance, total_received, total_sent, tx_count
- `agreements` — anchor_hash, anchor_type, block_height, txid
- `agreement_parties` — anchor_hash, address
- `htlc_outputs` — txid, vout, htlc_type, value, hash_lock, timeout_height, sender_pkh, receiver_pkh, state, claim_txid
- `mining_leaderboard` — address, blocks_found, total_reward (updated per-block)

## Irium-Specific Format Handling

### Transaction format
Irium uses a 1-byte length prefix (always 0x20 = 32) before each `prev_txid` in inputs.
Standard Bitcoin has fixed 32-byte `prev_txid`. The indexer handles this in `decoder/tx.rs`.

### Address encoding
Version byte `0x39` → base58check → produces Q- or P-prefix 34-char addresses.
Standard Bitcoin uses version byte `0x00` (1-prefix).

### HTLC variants
Three tags identify HTLC outputs in scriptPubKey:
- `0xc0` (83 bytes) — HTLCv1 (IRM↔IRM)
- `0xc3` (87 bytes) — BTC Swap v1
- `0xc7` (87 bytes) — LTC Swap v1

### Agreement anchors
OP_RETURN payload format: `agr1:<type>:<64-hex-hash>[:<milestone_id>]`
Types: `f`=fund, `l`=release, `r`=refund, `m`=milestone_release, `x`=dispute_resolve

### Reorg handling
Indexer maintains `INDEXER_REORG_DEPTH=6` checkpoints. On detecting a fork, rolls back
`tx_inputs`, `tx_outputs`, `txs`, `blocks`, and `indexer_state` above the divergence height.

## API Endpoints (port 3400)

| Method | Path | Description |
|--------|------|-------------|
| GET | `/status` | Chain tip, indexer height, sync status |
| GET | `/blocks?limit=N&before=H` | Block list, most-recent-first |
| GET | `/block/height/:height` | Block by height with txid list |
| GET | `/block/hash/:hash` | Block by hash with txid list |
| GET | `/tx/:txid` | Transaction with inputs and outputs |
| GET | `/address/:address` | Address stats (balance, received, sent, tx_count) |
| GET | `/address/:address/txs` | Address transaction history |
| GET | `/address/:address/htlcs` | Address HTLC outputs |
| GET | `/agreement/:hash` | Agreement anchor and parties |
| GET | `/miners?limit=N` | Mining leaderboard |
| GET | `/search?q=…` | Dispatch search by type (height/hash/txid/address) |

Rate limiting: 60 req/min per IP (configurable). Localhost exempt.

## Frontend Pages

| Route | Page |
|-------|------|
| `/` | Home: chain status + recent blocks table |
| `/block/height/:id` | Block detail + tx list |
| `/block/hash/:id` | Block by hash |
| `/tx/:txid` | Transaction: inputs + outputs |
| `/address/:address` | Address: stats + tx history + HTLCs |
| `/miners` | Mining leaderboard |

Stack: React 18 + TypeScript + Vite + Tailwind CSS v4 + TanStack Query + React Router v6.

## Deployment

```bash
cp .env.example .env
# edit .env: set DB_PASSWORD, IRIUMD_RPC_URL
docker-compose up -d
```

Services bind to `127.0.0.1` (localhost only). Put nginx/Caddy in front for public access.

### nginx reverse proxy (explorer.iriumlabs.org)

```nginx
location / { proxy_pass http://127.0.0.1:3401; }
```

### Systemd auto-start (optional)

Create `/etc/systemd/system/irium-explorer.service`:
```ini
[Unit]
Description=Irium Explorer
After=docker.service
Requires=docker.service

[Service]
WorkingDirectory=/home/irium/irium-explorer
ExecStart=/usr/bin/docker-compose up
ExecStop=/usr/bin/docker-compose down
Restart=always
User=irium

[Install]
WantedBy=multi-user.target
```

## Implementation Files

```
/home/irium/irium-explorer/
  docker-compose.yml           — 4 services (db, indexer, api, frontend)
  .env.example                 — config template
  docker/
    Dockerfile.indexer         — rust:latest → cargo build -p irium-explorer-indexer
    Dockerfile.api             — rust:latest → cargo build -p irium-explorer-api
    Dockerfile.frontend        — node:22-alpine build → nginx:alpine serve
  explorer/
    Cargo.toml                 — workspace [indexer, api]
    indexer/
      src/
        main.rs                — entry: dotenv, tracing, pool, migrate, indexer::run
        config.rs              — Config from env
        rpc.rs                 — RpcClient → /rpc/blocks, /status
        indexer.rs             — sync loop, reorg detection
        db/read.rs             — get_indexer_state
        db/write.rs            — index_block, mark_output_spent, rollback_above
        decoder/
          address.rs           — P2PKH (0x39) → Irium address (33 unit tests)
          script.rs            — classify_script → ScriptClass (17 unit tests)
          tx.rs                — decode_tx with 1-byte length prefix (33 unit tests)
      migrations/001_initial.sql
    api/
      src/
        main.rs
        config.rs
        rate_limit.rs          — governor-based middleware
        models.rs
        error.rs
        db.rs
        routes/               — status, blocks, txs, address, agreements, miners, search
  frontend/
    src/
      App.tsx                  — router
      api.ts                   — typed API client
      lib/fmt.ts               — satToIrm, shortHash, timeAgo, fmtTime
      components/              — Card, HashLink, Layout, SearchBar, StatRow
      pages/                   — Home, BlockPage, TxPage, AddressPage, MinersPage
    public/
      irium-logo.png           — copied from /home/irium/irium/assets/
```

## Test Coverage

- `decoder/address.rs` — 4 unit tests (known address vectors)
- `decoder/script.rs` — 17 unit tests (P2PKH, HTLCv1, BTC/LTC swap, OP_RETURN, anchor)
- `decoder/tx.rs` — 33 unit tests (real coinbase from block 30220, reorg detection, truncated hex)

## Key Decisions

1. **In-process tx_hex decoding** over a separate parser service — simpler, no IPC overhead
2. **runtime sqlx::query()** over compile-time `sqlx::query!()` — avoids needing DATABASE_URL at build time
3. **docker-compose** over systemd service files — self-contained, portable, matches spec requirement
4. **governor** crate for rate limiting — zero-allocation token bucket, no external Redis dependency
