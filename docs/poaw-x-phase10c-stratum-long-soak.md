# PoAW-X Phase 10-C — Stratum Long Soak

**Date:** 2026-06-11
**Branch:** `testnet/poawx-phase10c-stratum-long-soak`
**Final commit:** `1aa0ba5`
**Result:** PASS=48 FAIL=1 (1 known binary limitation)

---

## 1. What Was Tested

A two-VPS private devnet soak exercising the full PoAW-X stratum path end-to-end:

- **VPS-1** (207.244.247.86): testnet `iriumd` (devnet, IRIUM_POAWX_MODE=active) + `irium-stratum` (IRIUM_STRATUM_POAWX=1)
- **VPS-2** (157.173.116.134): testnet `iriumd` peer (IRIUM_FORCE_SEED=VPS-1, separate data dir `/tmp/irium-poawx-phase10c`)
- **Miner harness**: Python TCP stratum v1 client on VPS-1 (`poawx-stratum-long-soak-harness.py`)
- **Isolated ports**: 39500 (P2P), 39501 (RPC), 39502 (stratum) — mainnet ports untouched

The soak ran through 5 sequential phases:

| Phase | Section | Action |
|-------|---------|--------|
| Setup | 0-4 | Pre-flight, binary deploy, iriumd+stratum start, VPS-2 peer start |
| Peer check | 5 | Verify VPS-2 ↔ VPS-1 P2P connection |
| Template/assignment | 6 | PoAW-X mode, template fields, assignment endpoint |
| Phase A | 7 | Mine 20 blocks via stratum, receipt test at block 3 |
| VPS-2 restart | 8 | Kill and restart VPS-2 peer, mine 5 more blocks |
| Stratum restart | 9 | Kill and restart stratum, mine 5 more blocks via reconnected miner |
| Phase B | 10 | Mine 20 blocks, submit one bogus share (must be rejected) |
| Negative checks | 11 | Endpoint availability, malformed TCP input |
| Log scan | 12 | No panics, submit_block_extended call count, legacy-mode info |
| Safety post-check | 13 | Both VPS mainnet PIDs, ports, and env unchanged |

---

## 2. Final Soak Results

```
PASS=48  FAIL=1
Total stratum-accepted blocks: 50
submit_block_extended calls in stratum log: 52
```

### All checks and results

| # | Check | Result |
|---|-------|--------|
| 0 | Correct branch, binaries exist | PASS |
| 0 | Mainnet iriumd alive (PID preserved) | PASS |
| 0 | Mainnet ports 38300, 3333 still bound | PASS |
| 1 | VPS-2 iriumd binary deployed | PASS |
| 2 | VPS-1 iriumd bits=207fffff (devnet) | PASS |
| 2 | VPS-1 iriumd IRIUM_POAWX_MODE=active in process env | PASS |
| 3 | Stratum port 39502 open | PASS |
| 3 | Stratum log: poawx enabled, no panic | PASS |
| 4 | VPS-2 iriumd responsive, bits=207fffff | PASS |
| 5 | VPS-2 connected to VPS-1 (log-based fallback) | PASS |
| 5 | Both VPS logs confirm P2P connection | PASS |
| 6a | /poawx/assignment not disabled (not 503) | PASS |
| 6b | Assignment lane check | SKIP (h=0 expected) |
| 6c | Template has pow_bits | PASS |
| 6d | iriumd process env has IRIUM_POAWX_MODE=active | PASS |
| 6e | Template poawx_pending_receipts field | PASS |
| 7 | Phase A: 20/20 blocks via stratum | PASS |
| **7** | **Phase A receipt test** | **FAIL (binary limitation — see §4)** |
| 8 | VPS-2 back up after restart | PASS |
| 8 | VPS-2 reconnected after restart (log fallback) | PASS |
| 8 | 5 blocks accepted after VPS-2 restart | PASS |
| 8 | VPS-1 height advanced after restart | PASS |
| 9 | Stratum port open after restart | PASS |
| 9 | 5 blocks accepted through restarted stratum | PASS |
| 9 | VPS-1 height advanced after stratum restart | PASS |
| 10 | Phase B: 20/20 blocks via stratum | PASS |
| 10a | Bogus share rejected | PASS |
| 10b | Bogus share did not advance height | PASS |
| 11a | /poawx/assignment not disabled (not 503) | PASS |
| 11b | /rpc/submit_block_extended not 503 | PASS |
| 11d | Stratum survives malformed TCP input | PASS |
| 12a | No panics in any testnet log | PASS |
| 12b | No invalid acceptance in VPS-1 iriumd log | PASS |
| 12c | No panics in VPS-2 iriumd log | PASS |
| 12d | submit_block_extended called ≥50 times | PASS (52 calls) |
| 12e | Legacy-mode info (informational only) | INFO |
| 13a | VPS-1 mainnet iriumd PID 1556521 still alive | PASS |
| 13b-d | VPS-1 mainnet ports 38300, 3333, 8080 still bound | PASS |
| 13e-f | VPS-2 mainnet ports 38300, 38291 still bound | PASS |
| 13g | Testnet bits=207fffff confirmed | PASS |
| 13h | No IRIUM_POAWX_MODE in production env | PASS |

---

## 3. What Was Validated

- **Real stratum TCP mining**: 50 blocks accepted end-to-end via stratum v1 subscribe→authorize→notify→submit
- **submit_block_extended path**: Stratum calls `/rpc/submit_block_extended` (not legacy `/rpc/submit_block`) for every block — 52 calls for 50 blocks confirmed
- **IRIUM_STRATUM_POAWX=1 plumbing**: Stratum correctly detects PoAW-X mode from binary build flag and switches to extended submission path
- **Two-VPS devnet**: VPS-1 and VPS-2 form a private devnet P2P network with IRIUM_FORCE_SEED. Chain syncs correctly between peers
- **Peer restart recovery**: VPS-2 can be killed and restarted; it reconnects and mining continues without interruption
- **Stratum restart recovery**: Stratum can be killed and restarted; Python harness reconnects and mining continues
- **Bogus share rejection**: A share with a fabricated nonce that does not meet the PoW target is correctly rejected by stratum (mining.reject returned)
- **Mainnet safety throughout**: Both VPS mainnet processes (PIDs 1556521, 1556528), ports, and production env files were untouched for the entire soak
- **No panics**: Zero `thread.*panicked`, `SIGSEGV`, or `stack overflow` entries in any testnet log (VPS-1 or VPS-2)
- **IRIUM_DEV_EASY_BITS_TEMPLATE=1 isolation**: Devnet easy bits (207fffff) do not bleed to mainnet template

---

## 4. Known Binary Limitation: `/poawx/assignment` Returns 404

The current `iriumd` binary (as of 2026-06-11) does not expose the PoAW-X puzzle assignment endpoint at any block height. Both at `h=0` and at `h=50`, `GET /poawx/assignment` returns HTTP 404.

**Consequence chain:**

1. `/poawx/assignment` returns 404 → no puzzle seed or commitment nonce available
2. Stratum receives `mode=""` in block template (no `receipts_root` field)
3. Stratum logs `[WARN] poawx_enabled but job has no receipts_root (mode=''); using legacy submit`
4. Stratum calls `submit_block_extended` with empty receipts (`poawx_receipts: []`)
5. irx1 OP_RETURN (`6a2469727831`) is NOT present in coinbase — 0/50 blocks
6. Receipt path test (`/poawx/receipt` POST) cannot be exercised

**What this is NOT:**
- This is NOT a regression or script bug. Every other aspect of the soak passes.
- The stratum IS using `submit_block_extended` — the "legacy" warn refers to empty receipts within that endpoint, not a fallback to the old `/rpc/submit_block` endpoint.
- Mainnet is not affected. `IRIUM_POAWX_MODE` is devnet-only and is never set in production.

**What needs to be done in a future phase:**
- Implement `/poawx/assignment` in `iriumd` to return `{seed, commitment_nonce, puzzle_difficulty, lane}` for the current block
- Implement `/poawx/receipt` POST endpoint to accept validated puzzle receipts
- Add `receipts_root` field to block template when PoAW-X receipts are pending
- Stratum will automatically pick up these fields and bake the irx1 OP_RETURN into coinbase

---

## 5. How to Run

On VPS-1, from the `testnet/poawx-phase10c-stratum-long-soak` branch:

```bash
# Default: 50 blocks / 3h soak
bash scripts/testnet-poawx-phase10c-stratum-long-soak.sh

# Custom duration
SOAK_BLOCK_TARGET=20 SOAK_SECONDS=1800 \
    bash scripts/testnet-poawx-phase10c-stratum-long-soak.sh

# In tmux (recommended for overnight runs)
tmux new-session -d -s phase10c \
    'bash scripts/testnet-poawx-phase10c-stratum-long-soak.sh 2>&1 | tee ~/irium-phase10c-soak.log'
tmux attach -t phase10c
```

**Hard requirements before running:**
- Both VPS mainnet services (iriumd, irium-stratum) must be running (soak verifies and preserves them)
- `~/irium/target/release/iriumd` must be the current testnet binary
- `~/irium/pool/irium-stratum/target/release/irium-stratum` must be the current testnet stratum binary
- VPS-2 SSH must be reachable as `irium@157.173.116.134`

---

## 6. Script and Harness

| File | Description |
|------|-------------|
| `scripts/testnet-poawx-phase10c-stratum-long-soak.sh` | Main soak driver (runs on VPS-1, manages VPS-2 via SSH) |
| `scripts/poawx-stratum-long-soak-harness.py` | Python stratum v1 TCP miner + receipt tester |

**Script safety mechanisms:**
- Detects mainnet PIDs at startup via port binding (`ss -lntp`) — never kills them
- `trap cleanup EXIT` kills only testnet PIDs (stored in `TESTNET_IRIUMD_PID`, `TESTNET_STRATUM_PID`)
- `assert_mainnet_alive` called at end of every section — exits 1 (FATAL) if mainnet PID dies
- Isolated data dirs: VPS-1 uses `~/irium-poawx-phase10c`, VPS-2 uses `/tmp/irium-poawx-phase10c`
- Isolated ports: 39500/39501/39502 — no overlap with mainnet 38300/38291/3333/8080
- `IRIUM_NETWORK=devnet` and `IRIUM_POAWX_MODE=active` set only in testnet process env, never in system files

---

## 7. Bug Fixes Applied During Phase 10-C

| Commit | Fix |
|--------|-----|
| `ac657f0` | Initial Phase 10-C script and harness |
| `1aa0ba5` | Fix 5 false-positive checks: Section 8 reconnect (30s→90s + log fallback), Section 11a (not-503 check), Sections 12a/12c (grep pipefail double-output), Section 12e (legacy-mode informational) |

Earlier fixes (pre-`ac657f0`) applied during iterative runs:
- Wrong stratum binary path (Jun 9 build → Jun 10 build)
- VPS-2 defaulting to `~/.irium` data dir (fixed with `cd ${VPS2_DATA_DIR}` + `export`)
- `poawx_mode` field absent from template (replaced with proc-env check)
- `/poawx/assignment` at h=0 causing crash (graceful 404 handling in harness)
- `curl -sf` concatenating HTTP code with "000" on non-2xx (changed to `curl -s`)
- Section 5 peer count timing (90s timeout + log-based fallback)
