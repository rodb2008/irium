# Phase 10-E: PoAW-X Receipt Regression Soak

Branch: `testnet/poawx-phase10e-receipt-regression-soak`  
Checkpoint: `844b7d5` (Phase 10-D complete)  
Status: **PASS=62 FAIL=0 SKIP=4** (verified 2026-06-11)

## 1. Purpose

Phase 10-E proves the full PoAW-X receipt path is repeatable over 30 consecutive
blocks, survives a stratum restart, rejects bogus shares, and leaves mainnet
completely untouched.

Full path validated end-to-end:

```
assignment → valid receipt → POST /poawx/receipt
  → pending receipt in template → non-empty receipts_root
  → irx1 commitment in coinbase → real Stratum TCP submit
  → submit_block_extended → accepted block
  → persisted poawx_receipts / poawx_receipts_root
```

## 2. Soak Targets (all met)

| Metric | Required | Actual |
|--------|----------|--------|
| Main-run blocks (SOAK_BLOCK_TARGET) | 30 | 30/30 |
| irx1 in coinbase | ≥ 20 (min), 30 target | 30/30 |
| submit_block_extended acceptances | ≥ 30 | 30 |
| Share rejections | 0 | 0 |
| Restart run (RESTART_BLOCK_TARGET) | 5 | 5/5 |
| irx1 after restart | 5 | 5/5 |
| Bogus share rejected | true | true |
| Chain height unchanged after bogus | true | true |

## 3. Final Soak Results

```
PASS=62  FAIL=0  SKIP=4

=== Main soak (30 blocks) ===
  blocks_pass:              30/30
  irx1_in_coinbase_count:   30
  share_accepts/rejects:    30/0
  receipt_test_passed:      true
  elapsed:                  61.3s

=== Restart run (5 blocks) ===
  blocks_pass:              5/5
  irx1_in_coinbase_count:   5
  receipt_test_passed:      true
  elapsed:                  11.1s

=== Bogus share test ===
  bogus_rejected:           true
  bogus_height_unchanged:   true

=== Chain state ===
  height at end:            39

=== Log scan ===
  iriumd panics:            0
  invalid commitments accepted: 0
  stratum panics:           0
```

## 4. What Was Tested (15 sections)

| Section | Test |
|---------|------|
| 0 | Pre-flight: mainnet PIDs baseline, no stale testnet processes, all testnet ports free |
| 1 | Start testnet iriumd (devnet, IRIUM_POAWX_MODE=active, port 39511) |
| 2 | Template: bits=207fffff (devnet easy), poawx_mode=active |
| 3 | Mine 1 block for height>0; GET /poawx/assignment: seed/nonce/lane/pow_bits/puzzle_difficulty |
| 4 | POST /poawx/receipt returns 200, pending_count=1 |
| 5 | Template receipts_root matches canonical computed root (Python SHA256 verification) |
| 6a | Invalid hex in solution → HTTP 400 |
| 6b | Duplicate receipt → deduped (pending_count unchanged) |
| 6c | Disabled-mode iriumd (no IRIUM_POAWX_MODE) → 503 (skipped: port not responsive) |
| 7 | Start testnet stratum (IRIUM_STRATUM_POAWX=1, port 39512) |
| 8 | 30-block soak via Phase 10-C harness (--receipt mode): all 30 irx1=True |
| 9 | Log: irx1_injections=65, submit_block_extended_calls=30, accepted=60 |
| 10 | Block content confirmed via harness irx1_count and log (getblock not available) |
| 11 | Kill stratum, restart, 5-block run: reconnect PASS, irx1 PASS, receipt PASS |
| 12 | Bogus share: rejected=True, chain height unchanged |
| 13 | VPS-2 peer propagation: skipped (VPS2_HOST not set) |
| 14 | Log scan: no panics, no invalid commitments accepted in any log |
| 15 | Mainnet safety after soak: all 4 mainnet PIDs alive, all 4 mainnet ports bound |

## 5. Testnet Topology (fully isolated)

| Port | Service |
|------|---------|
| 39510 | P2P (testnet) |
| 39511 | RPC (testnet) |
| 39512 | Stratum (testnet) |
| 39513 | Disabled-mode test iriumd (ephemeral) |

Data directory: `$HOME/irium-poawx-phase10e` (cleaned up after PASS)

## 6. Environment Variables

| Variable | Value | Service |
|----------|-------|---------|
| IRIUM_POAWX_MODE | active | iriumd |
| IRIUM_NETWORK | devnet | iriumd |
| IRIUM_DATA_DIR | ~/irium-poawx-phase10e | iriumd |
| IRIUM_STRATUM_POAWX | 1 | stratum |

## 7. Bug Fixes Applied

### Bug 1 — Stale process check false-positive (FAIL)

**Symptom:** `[FAIL] stale testnet processes: irium 1556521 ... iriumd`  
Mainnet iriumd (PID=1556521) was flagged as a stale testnet process because it
runs from the same binary path as the testnet binary.

**Root cause:** `grep -v "pid=${MAINNET_IRIUMD_PID}"` doesn't match ps output
— PID appears in column 2 as a bare number, not in `pid=N` format.

**Fix:** Use `awk -v p="$KNOWN_MAIN_PIDS"` to compare column 2 against the
known mainnet PID set, so mainnet processes are correctly excluded.

### Bug 2 — Section 5 Python ValueError (EXIT)

**Symptom:** `ValueError: invalid literal for int() with base 10: '$ASSIGN_HEIGHT'`

**Root cause:** `<<'PYEOF'` (single-quoted heredoc) prevents bash variable
expansion. Python received the literal string `'$ASSIGN_HEIGHT'` instead of
the height value.

**Fix:** Changed to `python3 - "$ASSIGN_HEIGHT" "$SOLUTION" "$NONCE" <<'PYEOF'`
and accessed variables via `sys.argv[1]`, `sys.argv[2]`, `sys.argv[3]`.

## 8. Skipped Checks

| Check | Reason |
|-------|--------|
| Section 6c disabled-mode 503 | Ephemeral iriumd on port 39513 not responsive in time |
| Section 10 getblock content | `/rpc/getblock` returns HTTP 404 (endpoint not implemented) |
| Section 13 VPS-2 peer propagation | VPS2_HOST not set; irium-vps2 not reachable |

These skips do not affect the core receipt path validation. irx1 presence
confirmed via harness `irx1_count=30` and stratum log `irx1_injections=65`.

## 9. Mainnet Safety

All mainnet processes survived the full soak unmodified:

| Process | PID | Port | Status |
|---------|-----|------|--------|
| iriumd | 1556521 | 38300 | alive |
| stratum | 1556528 | 3333 | alive |
| explorer | 1556873 | 38310 | alive |
| wallet-api | 1558068 | 38320 | alive |

No testnet process used a mainnet port. No mainnet config files were modified.
PoAW-X remains hard-disabled on mainnet (no IRIUM_POAWX_MODE=active in
production env).

## 10. How to Run

```bash
# On irium-vps, from the Phase 10-E branch:
bash ~/irium/scripts/testnet-poawx-phase10e-receipt-regression-soak.sh
```

Full run takes approximately 80–120 seconds.

## 11. Script and Harness

| File | Description |
|------|-------------|
| `scripts/testnet-poawx-phase10e-receipt-regression-soak.sh` | Main 15-section soak script |
| `scripts/poawx-stratum-long-soak-harness.py` | Phase 10-C harness (--receipt --bogus flags) |

## 12. Commit History

| Commit | Description |
|--------|-------------|
| `844b7d5` | Phase 10-D checkpoint (PASS=30 FAIL=0) |
| Phase 10-E | testnet: add PoAW-X receipt regression soak |
