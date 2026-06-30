# PoAW-X Phase 12-F — Deep Consensus Validation in connect_block()

**Branch:** `testnet/poawx-phase12-completion-rc-hardening`
**Status:** Local commit only — NOT pushed
**Builds on:** Phase 12-E (ca38206) — receipt expiry

---

## Problem Addressed

**GAP-1 (Partial closure):** `chain.rs/connect_block()` had no PoAW-X enforcement. Blocks missing the `irx1` coinbase commitment could be accepted into the chain even when PoAW-X was active. The P2P precheck (Phase 12-B) blocked such blocks at the network boundary, but connect_block itself imposed no check — a node could accept invalid blocks via direct RPC or a future internal code path.

This phase closes GAP-1 at the **commitment level**: the presence of a non-zero `irx1` OP_RETURN in the coinbase is now enforced in `connect_block()` in addition to the P2P layer. Individual receipt PoW difficulty validation is not performed here (that would require coupling consensus to the RPC receipt pool state, which is architecturally unsound).

---

## New Function

```rust
fn validate_poawx_coinbase(block: &Block, height: u64) -> Result<(), String>
```

**Location:** `src/chain.rs`, module-level free function, between the closing `}` of `impl ChainState` and `fn is_coinbase`.

**Logic** (mirrors `stateless_block_precheck` C-1 gate in `p2p.rs`):
1. Parse `IRIUM_POAWX_ACTIVATION_HEIGHT` env var — if absent, return `Ok(())`
2. Check `IRIUM_POAWX_MODE == active` — if not, return `Ok(())`
3. Check `network_kind_from_env() != Mainnet` — if mainnet, return `Ok(())`
4. If `height < act_h`, return `Ok(())`
5. Call `crate::poawx::block_has_irx1_commitment(block)` — if false, return `Err(...)`

Returns `Ok(())` for all non-active/pre-activation/mainnet cases.

---

## Call Site

In `connect_block`, the check is inserted **between** `validate_block_header` and `validate_and_apply_transactions`:

```rust
self.validate_block_header(&block, expected_height, previous)?;
validate_poawx_coinbase(&block, expected_height)?;   // NEW — before any mutations

let reward = block_reward(expected_height);
let (_fees, _coinbase_total, subsidy_created, undo) = self
    .validate_and_apply_transactions(...)?;
```

**Safety rationale:** `validate_and_apply_transactions` mutates UTXO state and BTC relay headers. Placing the PoAW-X check before it ensures a rejected block leaves chain state unmodified.

---

## Mainnet Safety

| Code path | On mainnet |
|---|---|
| `validate_poawx_coinbase` | Returns `Ok(())` immediately (NetworkKind::Mainnet check, third gate) |
| `connect_block` behaviour | Unchanged — mainnet blocks accepted normally |

Mainnet nodes with no `IRIUM_POAWX_ACTIVATION_HEIGHT` or `IRIUM_POAWX_MODE` set are doubly protected: first gate (no activation height) returns early before any further evaluation.

---

## main.rs Fix

`src/main.rs` is a thin secondary binary that manually re-declares library modules. `mod poawx;` was added so that `validate_poawx_coinbase`'s `crate::poawx::` reference resolves correctly when cargo builds this target.

---

## Tests Added (10)

### chain.rs — 7 unit tests (pure `#[test]`)

All use `chain_poawx_env_lock()` for env-var serialization. Test blocks constructed with a minimal coinbase transaction; no valid PoW required (testing the pure validation function directly).

| Test | What it verifies |
|---|---|
| `test_validate_poawx_coinbase_no_activation_env_always_ok` | No env var set → Ok() always |
| `test_validate_poawx_coinbase_mode_inactive_always_ok` | POAWX_MODE not active → Ok() |
| `test_validate_poawx_coinbase_pre_activation_height_ok` | height < act_h → Ok() even without irx1 |
| `test_validate_poawx_coinbase_rejects_missing_commitment` | Post-activation, no irx1 → Err containing irx1 |
| `test_validate_poawx_coinbase_rejects_zero_root` | Post-activation, zero irx1 root → Err |
| `test_validate_poawx_coinbase_accepts_valid_irx1` | Post-activation, non-zero irx1 root → Ok() |
| `test_validate_poawx_coinbase_mainnet_gate_skips_check` | IRIUM_NETWORK=mainnet → Ok() regardless of irx1 |

### iriumd.rs — 3 regression tests (pure `#[test]`)

| Test | What it verifies |
|---|---|
| `test_poawx_12f_irx1_commitment_false_for_no_irx1_script` | `block_has_irx1_commitment` (same fn used by chain.rs) returns false for no-irx1 block |
| `test_poawx_12f_irx1_commitment_true_for_valid_script` | Returns true for block with valid irx1 OP_RETURN |
| `test_poawx_12f_receipt_persistence_regression` | Phase 12-D persistence (save/load) unaffected by chain.rs change |

---

## GAP-1 Status

**Partially closed.** `connect_block` now enforces irx1 commitment presence after activation height on non-mainnet networks. What remains:

| Remaining gap | Reason deferred |
|---|---|
| Individual receipt PoW difficulty enforcement | Would require coupling consensus to RPC receipt pool — architecturally unsound |
| Worker\_pkh authentication against coinbase | Phase 12-G scope (R-3) |
| Reward split validation | Phase 12-G scope (R-4) |

---

## Files Changed

| File | Change |
|---|---|
| `src/chain.rs` | New `validate_poawx_coinbase` free function; call in `connect_block`; 7 tests + 2 helpers |
| `src/bin/iriumd.rs` | 3 regression tests |
| `src/main.rs` | Added `#[allow(dead_code)] mod poawx;` |
| `docs/poaw-x-phase12f-connect-block-consensus-validation.md` | This document |

---

## Checks Run

```
cargo fmt           — clean
cargo check         — 0 errors, 3 pre-existing unused-variable warnings
cargo build --release — clean (2m 07s)
cargo test          — 195 passed, 0 failed (up from 192 in Phase 12-E)
```

## Push Status

**NOT pushed** (per standing rules — explicit approval required).

---

## Known Remaining Gaps

| ID | Description |
|---|---|
| **GAP-1 (partial)** | connect_block now checks irx1 commitment; individual receipt PoW difficulty still not enforced in consensus |
| **R-2** | No reorg handling — receipts for orphaned heights age out naturally |
| **R-3** | worker_pkh not verified against coinbase |
| **R-4** | No reward split for puzzle work |
| **T-1** | End-to-end testnet integration test |

---

## Next Recommended Phase

- **Phase 12-G / R-3**: Authenticate worker_pkh against coinbase at poawx_post_receipt time
- **Phase 12-G / P-1/P-2**: RC security audit pass before any push consideration
- **Phase 12-G / T-1**: End-to-end testnet integration test
