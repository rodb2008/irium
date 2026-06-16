# PoAW-X Phase 20 — Metrics / Monitoring (PARTIAL: doc + counter plan)

**Status:** Monitoring doc + safe counter plan COMPLETE. New PoAW-X counters are a
**non-consensus, additive** change deferred to a focused commit so the proven stratum/node
binaries are not perturbed in this audit pass. Metrics bind **loopback by default**; public
metrics requires future explicit operator approval.

## What exists today
- The stratum exposes **`/metrics`** (`metrics_loop`, bound from `STRATUM_METRICS_BIND`,
  forced to `127.0.0.1`/`localhost`/`[::1]` by `main.rs`). It already surfaces aggregate
  share counters, per-miner observability, connection-gate counters, and round-eligible
  counters.
- The node exposes `/status` (loopback) with height/tip/peers and a heartbeat log.
- Gated `IRIUM_POAWX_PRODUCER_TRACE=1` emits PoAW-X producer traces (no secrets).

## Safe monitoring today (no code change)
Operators can already monitor a pilot from loopback:
- height/tip/peers: `curl -s 127.0.0.1:<status>/status`
- aggregate shares + connection gates: `curl -s 127.0.0.1:<metrics>/metrics`
- producer/mode-1 activity: grep the stratum log for `submit_block_extended`,
  `BLOCK_ACCEPTED`, `[poawx-trace] build_mode1 OK`, `reject ... reason=`.
- `scripts/poawx-log-passfail.sh <stratum.log> <node.log>` summarizes PASS/FAIL signals.

## Planned PoAW-X counters (additive to `/metrics`, non-consensus)
Each is a process counter incremented at an existing log/decision point; **redacted** (no
secrets/keys/seeds/full delegation hex):
- `poawx_active_delegations` (gauge, from `all_active`)
- `poawx_delegations_rejected_total{reason=...}` (wrong_worker / wrong_pkh / expired /
  network / nonzero_fee / pool_pubkey_mismatch / bad_json)
- `poawx_mode1_receipts_built_total`, `poawx_receipt_build_failures_total`
- `poawx_submit_block_extended_attempts_total`, `poawx_blocks_accepted_total`
- `poawx_shares_rejected_total{reason=...}` incl. `low_difficulty`
- `poawx_irx1_present_total` / `poawx_irx1_missing_total`
- `poawx_native_rewardable_active` (gauge 0/1), `poawx_stratum_share_diff` (gauge)
- `poawx_fee_mismatch_total`, `poawx_delegate_paid_violation_total` (always 0 in 0%-fee mode)
- peer sync height/hash and observer-validation status from the node `/status`.

> `chain difficulty` is **not** a stratum metric — it is LWMA-144-derived on the node and
> exposed via block headers; the stratum only reports its **share** diff.

## Privacy rules (enforced in the plan)
- No private keys, no wallet seeds, no RPC tokens in metrics.
- No full delegation hex (only counts / miner_pkh prefixes if ever surfaced).
- Metrics endpoint loopback-only by default; public exposure = future operator decision with
  source-restriction (never `Anywhere`).

## Why code is deferred
Adding counters touches `pool/irium-stratum/src/stratum.rs` (the proven block path). It is
non-consensus and low-risk, but to keep this audit pass from perturbing the validated binary,
the counters land in a separate, clearly-scoped commit with their own tests. The monitoring
**capability today** (status + /metrics + logs + log-scanner script) is sufficient for the
controlled trusted-miner pilot.
