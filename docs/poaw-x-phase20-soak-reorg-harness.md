# PoAW-X Phase 20 — Long Soak / Restart / Reorg Harness (loopback-only)

**Status:** Harness + documented full-run commands COMPLETE. All runs are **loopback-only,
isolated `$TROOT`, exact pidfiles, no sudo/firewall/systemd, no public ports, no
`pkill`/`killall`**. Chain difficulty stays automatic via LWMA-144; `STRATUM_DEFAULT_DIFF=1`
is the stratum share difficulty only.

The reusable script is `scripts/poawx-soak-harness.sh` (safe `--help`/dry-run by default; it
prints exact commands and only acts when explicitly told). It does **not** start anything on
import; running real services requires explicit operator approval.

## What each scenario covers (and how it was already proven)
| Scenario | Method | Already proven |
|---|---|---|
| single-node long soak | loopback node+stratum+cpuminer on fresh `$TROOT`, mine N blocks | 19C single-node E2E |
| two-node sync soak | observer node B dials node A over the path; both advance | 18D, 19D observer sync |
| restart node/pool during active mining | stop by exact pidfile, restart, confirm tip continuity | two-phase bootstrap (18C/19C) |
| pending receipt persistence/reload | node persists `poawx_pending_receipts`; restart reloads | 18B-3 (`4da6dd1`) |
| reorg with mode-1 delegated receipts | competing tip; delegation preserved through pending↔block restore | 18B-3 reorg mapper |
| invalid receipt rejection | submit malformed/forged receipt → rejected | 18B consensus tests |
| stale assignment rejection | assignment height/lane mismatch → fail closed | delegation `assignment_context_from_dto` tests |
| expired delegation rejection | tip > expiry → rejected | `verify_and_store_rejections`, `all_active` |
| observer node validation | node B re-validates embedded delegation on sync | 18D, 19D |
| cleanup verification | exact pidfile kill, `$TROOT` removed, ports clear | 19C/19D cleanup |
| mainnet/prod PID safety | check PIDs alive before+after | every phase |

## Full soak command (run later, operator-approved)
```
# loopback ports: status 39808 / rpc 39811 / stratum 39812 / delegation 39813 / metrics 39814
# TROOT=/home/irium/phase20-soak  (fresh, under $HOME)
scripts/poawx-soak-harness.sh plan        # prints the exact loopback bring-up + scenarios
scripts/poawx-soak-harness.sh smoke       # short single-node bring-up + emit-only + 1 mode-1 block, then cleanup
# (a long soak = the same loop over many blocks/restarts; documented in `plan`)
```

## Safety rules baked into the harness
- All binds `127.0.0.1` (stratum may bind a host IP only in an operator-approved two-VPS run,
  never here).
- Services started with `nohup`, real PID captured, **killed by exact pidfile** after a
  `/proc/<pid>/cmdline` + prod-pid-allowlist check.
- `$TROOT` removed at the end; never a mainnet/prod path.
- Never `pkill`/`killall`; never sudo/ufw/systemd.
- `STRATUM_DEFAULT_DIFF=1` (share diff) — chain difficulty is automatic (LWMA-144).

## Smoke vs full
The committed harness runs a **short smoke** (bring-up → emit-only → one mode-1 block →
cleanup) on demand; a **long soak** is the same loop extended over many blocks and
node/pool restarts. The long soak is documented but **not auto-run** (time + operator
approval). Two-VPS soak reuses the 19D procedure and is operator-approved only.
