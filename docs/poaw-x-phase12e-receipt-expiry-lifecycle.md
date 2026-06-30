# PoAW-X Phase 12-E — Receipt Expiry & Lifecycle Cleanup

**Branch:** `testnet/poawx-phase12-completion-rc-hardening`
**Status:** Local commit only — NOT pushed
**Builds on:** Phase 12-D (dd7181a) — receipt persistence

---

## Problem Addressed

**R-5 (No receipt expiry):** Pending receipts persisted by Phase 12-D could survive indefinitely across restarts. The 500-receipt cap bounded memory but did not enforce correctness: a miner who solved a puzzle for block height 1 could have that receipt loaded 10,000 blocks later and presented to a miner building at height 10,001.

---

## Expiry Rule

A receipt is **expired** when:

```
tip_height > receipt.height + POAWX_RECEIPT_MAX_AGE_BLOCKS
```

Equivalently, using saturating arithmetic to avoid u64 overflow:

```rust
receipt.height.saturating_add(POAWX_RECEIPT_MAX_AGE_BLOCKS) < tip_height
```

**Keep** condition: `receipt.height.saturating_add(POAWX_RECEIPT_MAX_AGE_BLOCKS) >= tip_height`

**Future-height receipts** (`receipt.height > tip_height`) always satisfy the keep condition — they are never pruned.

---

## Constants

| Name | Value | Location |
|---|---|---|
| `POAWX_RECEIPT_MAX_AGE_BLOCKS` | `24` | `src/bin/iriumd.rs`, PoAW-X helpers section |

24 blocks was chosen as a conservative initial value for testnet: enough runway for a miner to complete a puzzle and submit a block before the receipt expires, while preventing unbounded accumulation.

---

## New Helper

```rust
fn prune_expired_poawx_receipts(receipts: &mut Vec<PoawxPendingReceipt>, tip_height: u64)
```

- **Pure function:** no I/O, no env reads, no mainnet check
- Removes receipts where `receipt.height.saturating_add(POAWX_RECEIPT_MAX_AGE_BLOCKS) < tip_height`
- Logs pruned count with `eprintln!` if any receipts removed
- Mainnet-safe by construction: callers receive empty Vec on mainnet from `load_poawx_pending_receipts`

---

## Pruning Call Sites

### 1. `poawx_post_receipt` (receipt acceptance)

Added **before** the receipts mutex lock: reads `tip_height` from `state.chain` (chain lock acquired and released). Added **inside** the receipts mutex lock block, after cap enforcement and before clone.

Lock ordering: chain lock released before receipts lock acquired — no deadlock risk.

### 2. `submit_block_extended` (block commit, cleanup)

Added **inside** the receipts mutex lock block, after `retain` removes committed-height receipts, before the `eprintln!` log. Uses `new_height` (the new chain tip after `connect_block`) as the prune threshold.

The `eprintln!` was updated to use a captured `cleared` variable so it reports only receipts removed by `retain`, not by prune.

### 3. AppState startup construction

After `load_poawx_pending_receipts()`, the startup sequence:
1. Load receipts from disk
2. Read `tip_height` from `shared_state` (chain already loaded before AppState construction)
3. Prune stale receipts
4. Save pruned list back to disk (no-op on mainnet)
5. Store in `Arc<Mutex<Vec<...>>>`

---

## Mainnet Safety

| Function | On mainnet |
|---|---|
| `load_poawx_pending_receipts()` | Returns empty Vec — prune receives no data |
| `save_poawx_pending_receipts()` | No-op — pruned list never written on mainnet |
| `prune_expired_poawx_receipts()` | Pure logic on empty Vec — no side effects |

All Phase 12-D mainnet no-ops remain unchanged. No new mainnet conditions added.

---

## Startup Behavior

On a fresh restart with a populated receipt file:

1. File is read and parsed
2. Stale receipts (older than 24 blocks from current tip) are pruned
3. File is updated with the pruned list
4. Node starts with only fresh receipts in memory

If the file is missing or corrupt: starts clean (Phase 12-D behavior unchanged).

---

## Reorg Handling

Not addressed in this phase. Receipts for orphaned block heights that were reverted will survive in memory / on disk until they age out naturally via the expiry window. Full reorg-aware pruning remains a future task (R-2).

---

## Tests Added (10)

All use `poawx_env_lock()` for serialization. Tests 1–8 are pure `#[test]`; tests 9–10 are `#[tokio::test]`.

| Test | What it verifies |
|---|---|
| `test_poawx_fresh_receipt_retained` | Receipt at tip height is retained |
| `test_poawx_stale_receipt_pruned` | Receipt older than max age is removed |
| `test_poawx_mixed_receipts_leaves_only_fresh` | Mixed list: stale removed, fresh kept |
| `test_poawx_future_height_receipt_retained` | Future-height receipt not pruned |
| `test_poawx_prune_empty_list_safe` | Empty Vec prune never panics |
| `test_poawx_startup_prune_stale_persisted` | Load + prune removes stale from file |
| `test_poawx_save_after_prune_updates_file` | Pruned-then-saved file contains only fresh receipts on reload |
| `test_poawx_mainnet_ignores_persistence_and_pruning` | No file written, no receipts loaded, prune on empty is safe |
| `test_poawx_12e_c3_gate_still_fires_with_receipt_file` | Phase 12-B C-3 gate rejects empty receipts even when receipt file exists |
| `test_poawx_12e_difficulty_check_still_fires_with_receipt_file` | Phase 12-C difficulty check rejects trivial bits even with file |

---

## Files Changed

| File | Change |
|---|---|
| `src/bin/iriumd.rs` | New constant + helper; 4 call sites updated; 10 tests added |
| `docs/poaw-x-phase12e-receipt-expiry-lifecycle.md` | This document |

---

## Checks Run

```
cargo fmt           — clean
cargo check         — 0 errors
cargo build --release — see commit
cargo test          — all tests pass (see commit)
```

## Commit

```
poawx: expire stale pending receipts
```

Push status: **NOT pushed** (per standing rules).

---

## Known Gaps (Deferred)

| ID | Description |
|---|---|
| **GAP-1** | `chain.rs/connect_block()` has no PoAW-X difficulty enforcement |
| **R-2** | No reorg handling — orphaned-height receipts age out naturally but are not immediately pruned on disconnect |
| **R-3** | `worker_pkh` not verified against coinbase at validation time |
| **R-4** | No reward split for puzzle work |
| **T-1** | End-to-end testnet integration test with real puzzle cycle |

---

## Next Recommended Phase

**Phase 12-F options:**

1. **T-1** — End-to-end testnet integration test (simulate full puzzle cycle without external miner)
2. **R-3** — Authenticate `worker_pkh` against coinbase at `poawx_post_receipt` time
3. **P-1/P-2** — RC security audit pass before any push consideration
