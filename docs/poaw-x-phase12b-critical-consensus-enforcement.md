# PoAW-X Phase 12-B: Critical Consensus Enforcement

**Branch:** `testnet/poawx-phase12-completion-rc-hardening`
**Date:** 2026-06-12
**Status:** COMPLETE — all blockers closed, all tests pass

---

## Summary

Phase 12-B closed three CRITICAL consensus blockers and one OPTIONAL gap that were
identified in the Phase 12-A remaining-work audit.  Every P2P-synced block,
pool-submitted block, and extended-submission path now enforces PoAW-X rules when
the operator has opted in via environment variables.

---

## Blockers Closed

### C-1 — P2P block validation: irx1 commitment absent from chain layer

**Before:** A peer-synced block could be accepted without any irx1 coinbase
commitment.  The consensus enforcement in `submit_block_extended` only applied to
directly submitted blocks; the P2P path went straight through `process_block()` ->
`connect_block()` in `chain.rs` with no PoAW-X checks.

**Fix:** Added a PoAW-X guard to `stateless_block_precheck()` in `src/p2p.rs`.
This function is called on every P2P-received block before it enters the chain
machinery.  The guard fires when:

```
IRIUM_POAWX_MODE=active
AND IRIUM_POAWX_ACTIVATION_HEIGHT=<N>   (block height >= N)
AND IRIUM_NETWORK != mainnet            (mainnet always exempt)
```

Any block at or above the activation height that lacks a well-formed irx1
OP_RETURN commitment is rejected with an explanatory error string.

A new environment variable `IRIUM_POAWX_ACTIVATION_HEIGHT` was introduced as a
height gate.  This prevents IBD breakage for old chains that predate PoAW-X: nodes
syncing from genesis can replay pre-PoAW-X blocks, and enforcement activates only
from the configured height onward.

**File:** `src/p2p.rs` -- `stateless_block_precheck()`

---

### C-2 -- `/rpc/submit_block` legacy pool endpoint: zero PoAW-X integration

**Before:** The legacy `submit_block` RPC handler accepted any block when
`IRIUM_POAWX_MODE=active`, silently bypassing all PoAW-X requirements.  A miner
using the standard pool endpoint could submit PoAW-X-free blocks and have them
accepted.

**Fix:** Added an early-exit guard immediately after `require_rpc_auth` in
`async fn submit_block`.  When `IRIUM_POAWX_MODE=active` and the network is not
mainnet, the request is rejected with `HTTP 405 Method Not Allowed` and a log
message directing miners to `/rpc/submit_block_extended`.

Mainnet is explicitly exempt: the guard reads `network_kind_from_env()` and only
fires on non-mainnet deployments.

**File:** `src/bin/iriumd.rs` -- `async fn submit_block`

---

### C-3 -- `submit_block_extended`: accepts empty receipt list, skips irx1 check

**Before:** When `req.poawx_receipts` was empty the function skipped the entire
receipts-root construction and irx1 commitment check.  A miner could post an
empty `poawx_receipts` array and have the block accepted without any puzzle work.

**Fix:** Added a guard immediately after `require_rpc_auth` in
`async fn submit_block_extended`.  When `IRIUM_POAWX_MODE=active` and the network
is not mainnet, an empty `poawx_receipts` field causes the request to be rejected
with `HTTP 400 Bad Request`.

**File:** `src/bin/iriumd.rs` -- `async fn submit_block_extended`

---

### O-2 -- `submit_block_extended`: missing mainnet guard for PoAW-X receipts

**Before:** The extended submission path did not explicitly reject receipt
submissions on mainnet.  Although mainnet has no PoAW-X activation today, a
misconfigured operator could accidentally leak PoAW-X metadata through the mainnet
RPC, or a future mainnet upgrade path could receive unexpected data.

**Fix:** Added a mainnet guard in the same block as the C-3 fix:

```
if !is_non_mainnet && !req.poawx_receipts.is_empty() {
    -> HTTP 503 Service Unavailable
}
```

This runs before the C-3 check so mainnet always returns 503 for non-empty
receipts, regardless of `IRIUM_POAWX_MODE`.

**File:** `src/bin/iriumd.rs` -- `async fn submit_block_extended`

---

## New Files

### `src/poawx.rs` (new)

Shared, pure consensus helpers with no environment-variable reads.  All
activation logic stays with the caller.

| Function | Description |
|---|---|
| `block_has_irx1_commitment(block)` | Returns `true` iff the coinbase has a well-formed 38-byte irx1 OP_RETURN with non-zero root. |
| `irx1_root_from_block_bytes(block)` | Extracts the 32-byte irx1 root from the coinbase, or `None`. |

**irx1 commitment format enforced:**
```
script_pubkey[0]    = 0x6a        (OP_RETURN)
script_pubkey[1]    = 0x24        (push 36 bytes)
script_pubkey[2..6] = b"irx1"    (tag)
script_pubkey[6..38] != [0; 32]  (non-zero root)
total length = 38 bytes
```

**Unit tests (8):** cover valid commitment, zero root rejection, wrong tag,
wrong length, missing coinbase, empty transactions, root extraction, and
extraction failure on invalid script.

---

## Files Modified

| File | Change |
|---|---|
| `src/lib.rs` | Added `pub mod poawx;` |
| `src/poawx.rs` | New file: consensus helpers + 8 unit tests |
| `src/p2p.rs` | `stateless_block_precheck`: C-1 irx1 height-gated guard |
| `src/bin/iriumd.rs` | `submit_block`: C-2 legacy-path rejection guard |
| `src/bin/iriumd.rs` | `submit_block_extended`: C-3 + O-2 receipts guards |
| `src/bin/iriumd.rs` | Test module: 5 new integration tests + `poawx_env_lock()` helper |

---

## Automated Tests Added

### `src/poawx.rs` -- 8 unit tests

All gating is pure-function, no env vars:

| Test | Assertion |
|---|---|
| `test_has_irx1_valid` | Returns `true` for a well-formed irx1 output |
| `test_has_irx1_zero_root` | Returns `false` when root bytes are all zero |
| `test_has_irx1_wrong_tag` | Returns `false` for wrong 4-byte tag |
| `test_has_irx1_wrong_length` | Returns `false` for 37-byte script |
| `test_has_irx1_no_coinbase` | Returns `false` for empty transaction list |
| `test_irx1_root_extraction` | Extracts correct 32-byte root |
| `test_irx1_root_returns_none` | Returns `None` when no valid commitment |
| `test_has_irx1_no_matching_output` | Returns `false` when coinbase has no matching output |

### `src/bin/iriumd.rs` -- 5 integration tests

Uses `poawx_env_lock()` (serialised via `OnceLock<Mutex<()>>`) to prevent
env-var races across parallel `#[tokio::test]` runs:

| Test | Setup | Expected |
|---|---|---|
| `test_submit_block_rejected_when_poawx_active` | `IRIUM_POAWX_MODE=active`, `IRIUM_NETWORK=testnet` | HTTP 405 |
| `test_submit_block_proceeds_past_gate_when_inactive` | No `IRIUM_POAWX_MODE` | Not HTTP 405 |
| `test_sbe_rejects_empty_receipts_when_poawx_active` | `IRIUM_POAWX_MODE=active`, `IRIUM_NETWORK=testnet`, empty receipts | HTTP 400 |
| `test_sbe_allows_empty_receipts_when_poawx_inactive` | No `IRIUM_POAWX_MODE`, empty receipts | Not HTTP 400 |
| `test_sbe_rejects_poawx_receipts_on_mainnet` | `IRIUM_NETWORK` unset (mainnet default), non-empty receipts | HTTP 503 |

---

## Checks Run

```
cargo fmt          -- OK (no formatting changes)
cargo build --release -- OK (3 pre-existing warnings; 0 errors)
cargo test -- --test-threads=1 -- ALL PASS
```

**Test result summary:**

```
test result: ok. 541 passed; 0 failed
test result: ok.  26 passed; 0 failed
test result: ok.  16 passed; 0 failed
test result: ok. 269 passed; 0 failed
test result: ok. 415 passed; 0 failed
test result: ok.   5 passed; 0 failed
(total: 1 272 tests, 0 failures)
```

---

## Build Warnings (pre-existing, not introduced by Phase 12-B)

```
warning: unused variable `client`       src/bin/iriumd.rs:15364
warning: unused variable `relay_tip`    src/bin/iriumd.rs:15367
warning: unused variable `relay_tip`    src/bin/iriumd.rs:15498
```

All three existed before Phase 12-B.  No new warnings introduced.

---

## Environment Variables Introduced

| Variable | Required | Description |
|---|---|---|
| `IRIUM_POAWX_ACTIVATION_HEIGHT` | Only for C-1 P2P enforcement | Block height from which P2P blocks must carry irx1.  Without this, C-1 guard is skipped (safe IBD). |
| `IRIUM_POAWX_MODE` | For C-1, C-2, C-3 | Set to `active` to enable PoAW-X enforcement on this node. |
| `IRIUM_NETWORK` | For all PoAW-X guards | Any value other than `mainnet` is treated as non-mainnet. |

---

## Remaining Blockers (deferred to Phase 12-C+)

The following items from the Phase 12-A audit remain open:

| ID | Severity | Description |
|---|---|---|
| D-1 | HIGH | `PUZZLE_DIFFICULTY` hardcoded to 1-bit -- no adaptive difficulty |
| R-1 | HIGH | Puzzle receipts not persisted to disk; lost on restart |
| R-2 | HIGH | No reorg handling for puzzle receipts |
| R-3 | MED | `worker_pkh` not authenticated against block coinbase |
| R-4 | MED | Reward split for puzzle work not implemented |
| R-5 | MED | Receipt expiry / stale-receipt cleanup not implemented |
| T-1 | MED | No end-to-end testnet integration test for full PoAW-X flow |
| M-1 | MED | Mining software not updated for SBE submission format |
| M-2 | MED | No documented deployment runbook for PoAW-X testnet RC |
| L-1 through L-5 | LOW | Logging, metrics, and alerting gaps |
| P-1, P-2 | MED | RC hardening and security audit not started |

---

## Commit

```
consensus: enforce poawx critical activation paths
```

Closes C-1, C-2, C-3, O-2 from Phase 12-A audit.
No push -- local commit on `testnet/poawx-phase12-completion-rc-hardening`.
