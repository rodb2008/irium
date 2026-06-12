# PoAW-X Phase 12-D — Receipt Persistence & Restart Recovery

**Branch:** `testnet/poawx-phase12-completion-rc-hardening`
**Status:** Local commit only — NOT pushed
**Builds on:** Phase 12-C (bf9951a) — puzzle difficulty hardening

---

## Problem Addressed

**R-1 (Receipt Loss on Restart):** `poawx_pending_receipts` lived entirely in `AppState` (an in-memory `Arc<Mutex<Vec<PoawxPendingReceipt>>>`). Every node restart discarded all accepted receipts. Miners who solved puzzles before a restart had no path to include their work in the next committed block.

---

## Design

### Persistence target

```
storage::state_dir() / "poawx_pending_receipts.json"
```

A single JSON array file, matching the existing `proofs.json` / `policies.json` patterns. Overridden in tests via `IRIUM_POAWX_RECEIPTS_FILE` env var.

### Mainnet safety

All three new helpers check `network_kind_from_env() == NetworkKind::Mainnet` first:

| Function | On mainnet |
|---|---|
| `load_poawx_pending_receipts()` | Returns `Vec::new()` immediately |
| `save_poawx_pending_receipts()` | Returns without touching disk |

PoAW-X is disabled on mainnet (O-2 gate). Persistence on mainnet would be meaningless and risky.

### Startup recovery

`load_poawx_pending_receipts()` follows the `load_all_disputes_at_startup()` pattern:

- Missing file → returns empty `Vec` (clean start, never panics)
- Corrupt / wrong-shape JSON → logs `eprintln!` warning, returns empty `Vec` (never panics)
- Valid array → logs count, returns receipts

### Size cap

```rust
const POAWX_MAX_PENDING_RECEIPTS: usize = 500;
```

After each `poawx_post_receipt`, if `pending.len() > 500`, the oldest receipts (front of Vec) are drained. This prevents unbounded disk/memory growth in a long-running testnet node.

### Atomicity

`std::fs::write` is used directly (same pattern as `proofs.json`, `policies.json`). A crash mid-write yields a corrupt file; `load_poawx_pending_receipts` detects this on next startup and starts clean. Full atomic rename-on-write is deferred to Phase 12-E if needed.

---

## New Helpers

All inserted in `src/bin/iriumd.rs`, in the Phase 10-D PoAW-X helpers section, after `count_leading_zero_bits`:

| Item | Purpose |
|---|---|
| `const POAWX_MAX_PENDING_RECEIPTS: usize = 500` | Max receipts kept in memory and on disk |
| `fn poawx_receipts_file() -> PathBuf` | Returns file path; respects `IRIUM_POAWX_RECEIPTS_FILE` override |
| `fn load_poawx_pending_receipts() -> Vec<PoawxPendingReceipt>` | Safe startup load; mainnet-safe |
| `fn save_poawx_pending_receipts(receipts: &[PoawxPendingReceipt])` | Atomic-ish write; mainnet-safe |

---

## Call Site Changes

### `AppState` construction (main startup — line ~16787)

```rust
// Before
poawx_pending_receipts: Arc::new(Mutex::new(Vec::new())),

// After
poawx_pending_receipts: Arc::new(Mutex::new(load_poawx_pending_receipts())),
```

Test AppState (`create_test_state`) unchanged — keeps `Vec::new()` to avoid filesystem side-effects in tests.

### `poawx_post_receipt` (after dedup + push)

Added cap enforcement (drops oldest when `len > 500`) and `save_poawx_pending_receipts` call. Save happens **outside** the mutex lock on a cloned snapshot.

### `submit_block_extended` (after `retain` cleanup)

Added `save_poawx_pending_receipts` call after consumed receipts are removed. Save happens **outside** the mutex lock on a cloned snapshot.

---

## Tests Added (8)

All use `poawx_env_lock()` for env-var serialization. Pure `#[test]` (no async needed).

| Test | What it verifies |
|---|---|
| `test_poawx_receipts_saved_and_reloaded` | Save then load; all fields survive roundtrip |
| `test_poawx_receipts_clean_start_no_file` | Missing file → empty Vec, no panic |
| `test_poawx_corrupt_receipt_file_starts_clean` | Corrupt JSON → empty Vec, no panic |
| `test_poawx_wrong_json_shape_starts_clean` | Valid JSON but wrong type → empty Vec, no panic |
| `test_poawx_mainnet_save_is_noop` | Save on mainnet writes no file |
| `test_poawx_mainnet_load_returns_empty` | Load on mainnet ignores populated file |
| `test_poawx_receipt_cap_drops_oldest` | 502 receipts → cap drains oldest 2; height 2 is first |
| `test_poawx_consumed_receipts_not_reloaded` | Retain removes height-100; reload returns only height-200 |

---

## Phase 12-B / 12-C Guards Preserved

Phase 12-D touches only:
- The three PoAW-X helper functions (new, not modifying existing logic)
- One `AppState` field initializer (startup load instead of `Vec::new()`)
- Two existing call sites (added save after existing logic)

No changes to:
- O-2 mainnet gate (still in `poawx_post_receipt`, `submit_block_extended`)
- C-3 empty-receipt rejection (`submit_block_extended`)
- Difficulty minimum check (`poawx_puzzle_difficulty_bits`, `POAWX_MIN_ACTIVE_DIFFICULTY_BITS`)
- Any consensus files (`chain.rs`, `pow.rs`, `consensus.rs`, `settlement.rs`, `activation.rs`)

---

## Known Gaps (Deferred)

| ID | Description |
|---|---|
| **GAP-1** | `chain.rs/connect_block()` has no PoAW-X difficulty enforcement (documented since Phase 12-C) |
| **R-2** | No reorg handling — receipts for orphaned heights are not automatically purged |
| **R-3** | `worker_pkh` not authenticated against coinbase at validation time |
| **R-4** | No reward split for puzzle work |
| **R-5** | No receipt expiry — stale receipts from old heights accumulate until consumed or capped |

---

## Checks Run

```
cargo fmt           — clean, no changes
cargo check         — 0 errors, 3 pre-existing unused-variable warnings
cargo build --release — see commit artifacts
cargo test          — all tests pass (see commit)
```

## Commit

```
poawx: persist pending receipts for restart recovery
```

Push status: **NOT pushed** (awaiting approval per standing rules).
