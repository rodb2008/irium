# PoAW-X Phase 12-C: Puzzle Difficulty Hardening

**Branch:** `testnet/poawx-phase12-completion-rc-hardening`
**Date:** 2026-06-12
**Status:** COMPLETE -- blocker D-1 closed, all tests pass

---

## Summary

Phase 12-C replaced the hardcoded `PUZZLE_DIFFICULTY = 1` constant across three
call sites with a configurable, bounds-enforced system.  Active testnet nodes now
require a minimum of 4 leading zero bits by default; the default when no env var
is set is 8 bits.  Mainnet remains entirely unaffected.

---

## Blocker Closed

### D-1 -- PUZZLE_DIFFICULTY hardcoded to 1 bit

**Before:** Three independent call sites in `src/bin/iriumd.rs` used hardcoded
difficulty values that could not be changed without recompiling:

| Location | Hardcoded value |
|---|---|
| `poawx_get_assignment` -- JSON response field | `"puzzle_difficulty": 1u64` |
| `poawx_post_receipt` -- PoW check | `const PUZZLE_DIFFICULTY: u32 = 1` |
| `submit_block_extended` -- receipt loop | `if leading < 1` |

A 1-bit difficulty means ANY sha256d output satisfies it with ~50% probability on
the first attempt.  This is trivially solvable and provides no meaningful work
commitment.

**Fix:** All three call sites now read from `poawx_puzzle_difficulty_bits()`, a
new module-level helper function that enforces safe bounds.

---

## Difficulty Design

### Environment Variable

```
IRIUM_POAWX_PUZZLE_DIFFICULTY_BITS=<N>
```

### Constants

| Constant | Value | Meaning |
|---|---|---|
| `POAWX_DEFAULT_DIFFICULTY_BITS` | 8 | Leading zero bits when env var is not set |
| `POAWX_MIN_ACTIVE_DIFFICULTY_BITS` | 4 | Minimum allowed for active-mode testnet |
| `POAWX_MAX_DIFFICULTY_BITS` | 24 | Upper cap; values above this are clamped |

### Behavior Table

| Env var state | Result | Caller behavior |
|---|---|---|
| Not set | 8 (default) | Accepted (>= MIN_ACTIVE) |
| Set to valid number within [MIN_ACTIVE, MAX] | Parsed value | Accepted |
| Set to valid number < MIN_ACTIVE | Raw value (e.g. 2) | **Rejected: 503** |
| Set to valid number > MAX | Capped at 24 | Accepted |
| Set to non-numeric string | 0 (fail-closed) | **Rejected: 503** |

### Helper Function

```rust
fn poawx_puzzle_difficulty_bits() -> u32 {
    match std::env::var("IRIUM_POAWX_PUZZLE_DIFFICULTY_BITS")
        .ok()
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        None => POAWX_DEFAULT_DIFFICULTY_BITS,
        Some(v) => match v.parse::<u32>() {
            Ok(n) => n.min(POAWX_MAX_DIFFICULTY_BITS),
            Err(_) => { eprintln!(...); 0 }
        },
    }
}
```

### Puzzle work expectation by difficulty

| Bits | Expected SHA256d attempts | Approx CPU time |
|---|---|---|
| 4 | 16 | < 0.1 ms |
| 8 (default) | 256 | < 1 ms |
| 12 | 4 096 | ~1 ms |
| 16 | 65 536 | ~10 ms |
| 20 | 1 048 576 | ~100 ms |
| 24 (max) | 16 777 216 | ~1 s |

---

## Caller Changes

### `poawx_get_assignment`

Replaced `"puzzle_difficulty": 1u64` with `"puzzle_difficulty": poawx_puzzle_difficulty_bits() as u64`.
Miners now receive the correct configured difficulty in the assignment response.

### `poawx_post_receipt`

Added a difficulty config check immediately after the `is_active && is_non_mainnet` guard:

```rust
let difficulty = poawx_puzzle_difficulty_bits();
if difficulty < POAWX_MIN_ACTIVE_DIFFICULTY_BITS {
    // fail closed -> HTTP 503
}
```

Replaced the inline manual bit-count loop + `const PUZZLE_DIFFICULTY = 1` block with:
```rust
let leading = count_leading_zero_bits(&pow_hash);
if leading < difficulty { ... }
```

The difficulty check fires **before** the chain-height lookup, so misconfigured nodes
reject receipt submissions immediately without holding the chain lock.

### `submit_block_extended`

Added a difficulty config check at the **top** of the `if !req.poawx_receipts.is_empty()`
block, before the chain lock:

```rust
if !req.poawx_receipts.is_empty() {
    let difficulty = poawx_puzzle_difficulty_bits();
    if difficulty < POAWX_MIN_ACTIVE_DIFFICULTY_BITS {
        // fail closed -> HTTP 503
    }
    let (sbe_seed, sbe_nonce) = { ... };  // chain lock happens after
    for r in &req.poawx_receipts {
        let leading = count_leading_zero_bits(&pow_hash);
        if leading < difficulty { ... }
    }
}
```

Placing the config check before the chain lock makes it testable without a
populated chain in unit tests.

---

## New Functions

### `fn poawx_puzzle_difficulty_bits() -> u32`

Module-level.  Reads `IRIUM_POAWX_PUZZLE_DIFFICULTY_BITS`, applies cap, fails closed
on invalid input.  No env-var writes; callers enforce `MIN_ACTIVE` separately.

### `fn count_leading_zero_bits(hash: &[u8; 32]) -> u32`

Module-level.  Extracted from the three identical inline loops.  Counts leading zero
bits in a 32-byte big-endian hash by scanning byte-by-byte until a non-zero byte or
the end.

---

## Files Changed

| File | Change |
|---|---|
| `src/bin/iriumd.rs` | Added `POAWX_DEFAULT_DIFFICULTY_BITS`, `POAWX_MIN_ACTIVE_DIFFICULTY_BITS`, `POAWX_MAX_DIFFICULTY_BITS` constants |
| `src/bin/iriumd.rs` | Added `poawx_puzzle_difficulty_bits()` and `count_leading_zero_bits()` |
| `src/bin/iriumd.rs` | Updated `poawx_get_assignment` JSON response |
| `src/bin/iriumd.rs` | Updated `poawx_post_receipt` -- difficulty config check + refactored PoW check |
| `src/bin/iriumd.rs` | Updated `submit_block_extended` -- difficulty config check before chain lock + refactored PoW check |
| `src/bin/iriumd.rs` | Added 8 new tests to `mod tests` |

---

## Automated Tests Added

### Pure function tests (no async, no chain state)

| Test | Assertion |
|---|---|
| `test_poawx_difficulty_default_is_sane` | Default (no env var) returns value >= MIN_ACTIVE and > 1 |
| `test_poawx_difficulty_env_parsed` | `BITS=10` returns 10 |
| `test_poawx_difficulty_invalid_fails_closed` | Non-numeric string returns 0 |
| `test_poawx_difficulty_too_high_is_capped` | `BITS=9999` returns POAWX_MAX_DIFFICULTY_BITS |
| `test_poawx_difficulty_below_min_returned_raw` | `BITS=2` returns 2 (< MIN_ACTIVE); caller will reject |

### Endpoint integration tests

| Test | Setup | Expected |
|---|---|---|
| `test_sbe_rejects_trivial_difficulty_when_active` | mode=active, testnet, BITS=2 (<MIN), non-empty receipts | HTTP 503 |
| `test_poawx_receipt_rejects_trivial_difficulty` | mode=active, testnet, BITS=2 (<MIN) | HTTP 503 |
| `test_poawx_mainnet_ignores_difficulty_setting` | mainnet (default), BITS=4 | HTTP 503 (O-2 guard fires first) |

---

## Checks Run

```
cargo fmt           -- OK
cargo build --release -- OK (3 pre-existing warnings; 0 new warnings; 0 errors)
cargo test -- --test-threads=1 -- ALL PASS
```

---

## What Was NOT Changed

- `src/p2p.rs` `stateless_block_precheck` -- C-1 irx1 check is NOT weakened
- `src/poawx.rs` -- helper functions unchanged
- All consensus files (`chain.rs`, `pow.rs`, etc.) -- untouched
- Mainnet guard (O-2) -- still fires before any difficulty check for mainnet
- C-2 / C-3 guards -- unchanged

---

## Note: connect_block Validation Gap (Follow-Up)

Phase 12-C does not add difficulty enforcement to the `connect_block()` path in
`chain.rs`.  The P2P precheck (C-1) guards the P2P path; the SBE endpoint guards
the direct-submit path.  However, there is no difficulty check inside
`connect_block()` itself.  A node that bypasses both paths (e.g., directly calls
the chain API) would not have difficulty enforced at the block-acceptance layer.

This is documented as a follow-up, not silently assumed to be solved.

---

## Remaining Blockers (deferred to Phase 12-D+)

| ID | Severity | Description |
|---|---|---|
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
| GAP-1 | MED | connect_block() has no difficulty enforcement (see note above) |

---

## Commit

```
poawx: harden puzzle difficulty configuration
```

Closes D-1 from Phase 12-A audit.
No push -- local commit on `testnet/poawx-phase12-completion-rc-hardening`.
