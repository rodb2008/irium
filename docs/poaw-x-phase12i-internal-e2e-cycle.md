# Phase 12-I: Internal End-to-End PoAW-X Cycle Test

**Status:** Complete (local commit c4119b9, not pushed)
**Branch:** testnet/poawx-phase12-completion-rc-hardening
**Depends on:** Phase 12-H (reward split enforcement, commit e2f8bc4)

---

## Goal

Prove the full PoAW-X-aware flow works end-to-end at the handler level without a
live node, real miners, or public network exposure. Covers every guard layer from
receipt submission through coinbase reward-split enforcement.

---

## Test Helpers (7 added)

| Helper | Purpose |
|--------|---------|
| `sbe_seed_nonce_for_height(state, height)` | Derives seed/nonce from genesis parent for any target height |
| `brute_force_solution(seed, nonce, bits)` | Finds a solution meeting `bits` leading zero bits by incrementing a counter |
| `make_e2e_receipt(state, height, sk, bits)` | Builds a fully valid `PoawxPendingReceipt` (brute-forced solution + correct sig) |
| `header_for_height(height)` | Builds a `SubmitBlockHeader` whose `hash` field equals `hash_for_height(height)` |
| `coinbase_no_irx1_hex()` | Serialized coinbase tx with no irx1 OP_RETURN |
| `coinbase_irx1_no_payout_hex(root)` | Coinbase with irx1 OP_RETURN, no P2PKH worker output |
| `coinbase_irx1_wrong_pkh_hex(root, wrong_pkh, amount)` | Coinbase with irx1 + P2PKH to wrong worker |

### Seed/Nonce Derivation

Both `sbe_seed_nonce_for_height` and the production `poawx_post_receipt` /
`submit_block_extended` handlers use the same algorithm:

```
seed  = SHA256(parent_hash || parent_h.to_le_bytes() || b"poawx_assignment_seed_v1")
nonce = SHA256(seed || b"commitment_nonce")
```

where `parent_h = height - 1` and `parent_hash = chain[parent_h].header.hash_for_height(parent_h)`.

### Puzzle Difficulty

Tests that brute-force solutions use `IRIUM_POAWX_PUZZLE_DIFFICULTY_BITS=4`
(the minimum: `POAWX_MIN_ACTIVE_DIFFICULTY_BITS`). This averages ~16 iterations.
Default difficulty (8 bits, ~256 iterations) is used for tests that expect PoW
rejection (test 7 uses the 0xff×32 solution which does not meet either threshold).

---

## Tests (12 added — 231 total, 0 failures)

### Group A: Receipt Posting

| # | Test | Coverage |
|---|------|---------|
| 1 | `test_poawx_12i_receipt_post_full_cycle` | Full cycle: brute-force puzzle → post → 200 → pending state updated |
| 2 | `test_poawx_12i_receipt_fields_preserved_in_state` | All fields (lane, solution, nonce, pubkey, sig) preserved exactly |
| 3 | `test_poawx_12i_stale_receipt_rejected` | Height 1 with chain at 100 → `req.height + 2 < chain.height` → 400 |

### Group B: SBE Receipt-Loop Rejections

| # | Test | Fails at | Why |
|---|------|----------|-----|
| 4 | `test_poawx_12i_sbe_missing_worker_sig_rejected` | Identity check | Empty `worker_sig` |
| 5 | `test_poawx_12i_sbe_spoofed_pkh_rejected` | Identity check | `worker_pkh` doesn't match `hash160(pubkey)` |
| 6 | `test_poawx_12i_sbe_wrong_nonce_rejected` | Nonce check | `commitment_nonce = "00"×32` ≠ derived nonce |
| 7 | `test_poawx_12i_sbe_insufficient_pow_rejected` | PoW check | `0xff×32` solution doesn't meet 8-bit difficulty |

### Group C: Late-Path Rejections (after valid receipt passes)

These tests use `make_e2e_receipt` (4-bit brute-force) and `header_for_height(1)`
to pass the receipt loop and header hash check, then fail at irx1/payout.

| # | Test | Fails at | Why |
|---|------|----------|-----|
| 8 | `test_poawx_12i_sbe_missing_irx1_rejected` | irx1 check | Coinbase has no irx1 OP_RETURN |
| 9 | `test_poawx_12i_sbe_missing_worker_payout_rejected` | Reward split | irx1 present, no P2PKH to worker |
| 10 | `test_poawx_12i_sbe_wrong_payout_pkh_rejected` | Reward split | P2PKH present but to `[0xde; 20]`, not worker |

### Group D: Regression Guards

| # | Test | Coverage |
|---|------|---------|
| 11 | `test_poawx_12i_mainnet_sbe_unaffected` | O-2 guard: mainnet SBE still returns 503 |
| 12 | `test_poawx_12i_legacy_submit_rejected_when_active` | C-3 guard: legacy `submit_block` still returns 405 |

---

## Scope and Known Gaps

### What this covers

- The full in-process handler path from receipt posting through reward-split enforcement
- Every guard layer (O-2 network, C-3 mode, nonce, identity, PoW, irx1, payout)
- Seed/nonce derivation consistency with production code
- Correct `hash_for_height` computation in test headers

### What this does NOT cover (GAP-1, T-1)

| Gap | Description | Phase |
|-----|-------------|-------|
| **GAP-1** | Receipt PoW difficulty not enforced at chain-consensus level (`connect_block`) | Future |
| **T-1** | Full live-node end-to-end test (requires mining a real PoW-valid block) | Future |
| **connect_block path** | Tests 8–10 fail before `connect_block` is reached | T-1 |
| **R-2** | Reorg-aware receipt pruning | Future |

The `header_for_height` helper uses `prev_hash: [0;32]` and `merkle_root: [0;32]`.
This is intentional — `connect_block` (which validates these fields) is never reached
by tests in Group C because they fail at irx1/payout first.

---

## Blockers Remaining

| ID | Description |
|----|-------------|
| GAP-1 | Receipt-level PoW difficulty enforcement in chain consensus |
| R-2 | Reorg-aware receipt pruning |
| T-1 | Live-node end-to-end testnet integration |
| P-1/P-2 | RC security audit |
