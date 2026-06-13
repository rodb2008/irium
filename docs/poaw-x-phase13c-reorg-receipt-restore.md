# PoAW-X Phase 13-C: Reorg Receipt Restore (R-2)

## Summary

Phase 13-C implements the R-2 requirement: when a PoAW-X block is
disconnected during a chain reorg, its receipts are safely restored to
`poawx_pending_receipts` so they can be included in a future block.

This closes **R-2** (reorg receipt restore).

## Problem

Phase 13-A added PoAW-X receipts to the block wire format. Phase 13-B
enforced that `connect_block` validates every receipt. But neither phase
handled the case where a PoAW-X block is disconnected during a p2p-
triggered reorg: receipts from orphaned blocks were silently dropped, even
if the worker's PoW was still valid and the receipts had not expired.

## Architecture

`ChainState` (chain.rs) handles all consensus operations including reorgs
via `reorg_to_tip`. `AppState.poawx_pending_receipts` (iriumd.rs) holds the
pending receipt state. These two structures are architecturally separate and
cannot call each other directly.

### Integration Point

The chosen approach (simplest, no cross-module dependencies):

1. **ChainState** gains a `reorg_orphaned_blocks: Vec<Block>` field (plain
   `Vec` — no inner Mutex; always accessed under the outer ChainState lock).
2. **`reorg_to_tip`** pushes disconnected blocks that have receipts into
   `reorg_orphaned_blocks` after a successful reorg.
3. **`submit_block_extended`** (iriumd.rs) drains `reorg_orphaned_blocks`
   at its entry point and calls `restore_orphaned_poawx_receipts` to merge
   recovered receipts into `poawx_pending_receipts`.

This is a passive drain: restoration happens on the next block submission
after any reorg, not immediately. This is acceptable because:
- Reorgs on a small testnet are rare.
- Workers submit blocks frequently during active operation.
- There is no risk of double-counting: restoration is idempotent and
  deduplicates by (height, lane, worker_pkh).

## Data Flow

```
p2p.rs receives block B' (more work than current tip)
  └─ ChainState.process_block(B')
       └─ reorg_to_tip(B'.hash)
            ├─ disconnect_tip_block() × N  → disconnected: Vec<Block>
            ├─ connect_block(each new_branch block)
            └─ reorg_orphaned_blocks.extend(disconnected with receipts)

iriumd.rs: submit_block_extended (next miner submission)
  ├─ drain chain.reorg_orphaned_blocks → orphaned: Vec<Block>
  ├─ restore_orphaned_poawx_receipts(&mut pending, &orphaned, tip_height)
  │    ├─ skip expired (receipt.height + 24 < tip_height)
  │    ├─ skip duplicates (same height+lane+worker_pkh already in pending)
  │    └─ push remaining as PoawxPendingReceipt (hex-string format)
  └─ save_poawx_pending_receipts(snapshot)
```

## Expiry Rule

`POAWX_RECEIPT_MAX_AGE_BLOCKS = 24`. A receipt at `height H` is expired
when `H + 24 < tip_height`. At the exact boundary (`H + 24 == tip_height`)
the receipt is NOT expired and IS restored. This matches the existing
expiry behavior in `prune_expired_poawx_receipts`.

## Files Changed

| File | Change |
|------|--------|
| `src/chain.rs` | Added `pub reorg_orphaned_blocks: Vec<Block>` to `ChainState` struct |
| `src/chain.rs` | Initialized field in `ChainState::new()` and `rebuild_to_tip()` |
| `src/chain.rs` | `reorg_to_tip` pushes orphaned blocks after successful reorg |
| `src/bin/iriumd.rs` | Added `block_receipt_to_pending()` (reverse of `pending_receipt_to_block_receipt`) |
| `src/bin/iriumd.rs` | Added `restore_orphaned_poawx_receipts()` with expiry + dedup |
| `src/bin/iriumd.rs` | `submit_block_extended` drains and restores at entry |

## Invariants Preserved

- No receipts from the canonical (winning) chain are affected.
- Restoration is idempotent: calling it twice for the same blocks adds no
  duplicates.
- Expired receipts are never restored, preventing stale receipts from
  entering future blocks.
- The chain lock and pending_receipts lock are never held simultaneously:
  the drain completes before the restore lock is acquired.
- Mainnet is fully gated: `save_poawx_pending_receipts` returns early on
  mainnet, and the restore path only matters when PoAW-X mode is active.

## Test Results

All 10 Phase 13-C tests pass:

| Test | Covers |
|------|--------|
| `phase13c_block_receipt_to_pending_fields_correct` | `block_receipt_to_pending` hex conversion |
| `phase13c_restore_empty_orphaned_noop` | No orphaned blocks → pending unchanged |
| `phase13c_restore_adds_receipts_to_pending` | Valid receipt restored to pending |
| `phase13c_restore_idempotent_no_duplicates` | Same block twice → receipt added once |
| `phase13c_expired_receipt_not_restored` | height+24 < tip → skipped |
| `phase13c_non_expired_at_boundary_restored` | height+24 == tip → included |
| `phase13c_receipt_expired_by_one_skipped` | height+24 == tip-1 → skipped |
| `phase13c_dedup_across_two_orphaned_blocks` | Same receipt in two blocks → once |
| `phase13c_restore_does_not_remove_existing_pending` | Existing pending preserved |
| `phase13c_block_without_receipts_ignored` | Blocks with None/empty receipts no-op |

## R-2 Status

**R-2 is fully closed.**

Receipts from reorg-orphaned PoAW-X blocks are recovered on the next
`submit_block_extended` call, subject to the 24-block expiry window and
deduplication. No duplicates, no reward abuse, no stale receipts.

## Remaining Before Real Miner Pilot

| Blocker | Status |
|---------|--------|
| P-1: block-contained receipts in wire format | **DONE** (Phase 13-A) |
| P-2: consensus verification in connect_block | **DONE** (Phase 13-B) |
| R-2: reorg receipt restore | **DONE** (Phase 13-C) |
| End-to-end devnet test with live blocks | Pending (no real miners invited) |
