# PoAW-X Phase 11-B: Canonical receipts_root + Full Solution Validation

**Branch:** `testnet/poawx-phase11b-canonical-receipts-validation`  
**Commit:** 163b558  
**Date:** 2026-06-11  
**Status:** IMPLEMENTED — pending regression soak

---

## Summary

Phase 11-B fixes the two HIGH RISK items identified in the Phase 11-A readiness audit:

| # | Issue | Risk | Status |
|---|-------|------|--------|
| 1 | receipts_root was insertion-order-dependent | Multi-miner splits: iriumd and stratum compute different roots | FIXED |
| 2 | commitment_nonce and solution accepted as opaque hex | Fake receipts accepted; no work actually verified | FIXED |

---

## Change 1: Canonical receipts_root Sort Order

### What was wrong

Both `compute_poawx_receipts_root` (iriumd) and `compute_receipts_root_from_pending`
(stratum) computed the root by iterating receipts in the order they were inserted.

With two miners:
- iriumd receives receipt A then B → root = SHA256(H(A) || H(B))
- stratum reads pending list in a different order → root = SHA256(H(B) || H(A))
- These roots differ → irx1 mismatch → block rejected every time

### Fix

Both functions now sort receipts by a canonical key before computing the root:

```
sort key: (height asc, lane bytes, worker_pkh hex bytes, commitment_nonce hex bytes)
```

The sort key is identical in iriumd and stratum. A stable tiebreaker on `commitment_nonce`
ensures full determinism even when two workers submit for the same height and lane.

The hash inputs (height_le64, lane bytes, worker_pkh decoded, solution decoded,
commitment_nonce decoded) are unchanged. Only the iteration order is fixed.

### Files changed

- `pool/irium-stratum/src/block.rs`: `compute_receipts_root_from_pending`
- `src/bin/iriumd.rs`: `compute_poawx_receipts_root`

### Tests added

4 new unit tests in `pool/irium-stratum/src/block.rs`:

| Test | What it verifies |
|------|-----------------|
| `single_receipt_stable` | Single receipt always produces the same root |
| `two_receipts_order_independent` | AB root == BA root |
| `many_receipts_shuffled_same_root` | 5-receipt forward == reversed |
| `different_heights_different_root` | Height is part of the root input |

---

## Change 2: Full Receipt Solution Validation

### What was wrong

`poawx_post_receipt` accepted any hex string as `commitment_nonce` and `solution`
without verifying them against the assignment. An attacker could submit a receipt with:
- A random `commitment_nonce` that doesn't match what was assigned
- A random `solution` that doesn't satisfy the puzzle

The receipt would be accepted and included in the irx1 commitment, meaning blocks could
be accepted with zero actual assigned work verified.

### Fix

`poawx_post_receipt` now fully validates both fields before storing the receipt.

#### commitment_nonce validation

Re-derives the expected nonce from chain state:
```
parent_h    = receipt.height - 1
parent_hash = chain[parent_h].header.hash_for_height(parent_h)
seed        = SHA256(parent_hash || parent_h_le64 || "poawx_assignment_seed_v1")
expected_nonce = SHA256(seed || "commitment_nonce")
```

Rejects the receipt if `req.commitment_nonce != expected_nonce`.

This is the same derivation used in `poawx_get_assignment`. The seed is unique per
parent block: if the chain advances, old commitment_nonces become invalid and new
assignments must be fetched.

#### solution PoW validation

Validates the proof-of-work puzzle:
```
pow_hash = SHA256d(seed || commitment_nonce || solution)
leading_zero_bits(pow_hash) >= puzzle_difficulty (= 1)
```

The difficulty=1 requirement means any solution that produces a hash with at least
one leading zero bit is valid. This is intentionally low for the testnet phase.

#### Upper-bound check

Rejects receipts for blocks whose parent has not yet been mined:
```rust
if parent_h as usize >= guard.chain.len() {
    return Err(StatusCode::BAD_REQUEST);
}
```

This prevents receipt submissions for blocks far in the future, where the seed
cannot yet be derived (parent block doesn't exist).

#### Same validation in submit_block_extended

`submit_block_extended` now applies the same nonce + PoW checks to all receipts in
the request body before calling `connect_block`. This prevents an attacker from
bypassing `poawx_post_receipt` and submitting a block with fabricated receipts directly.

### Files changed

- `src/bin/iriumd.rs`: `poawx_post_receipt` — full validation before receipt storage
- `src/bin/iriumd.rs`: `submit_block_extended` — same validation before connect_block

---

## Protocol Invariants After Phase 11-B

| Invariant | Before 11-B | After 11-B |
|-----------|-------------|------------|
| receipts_root deterministic for any receipt order | NO | YES |
| iriumd and stratum compute identical root | NO (insertion-order race) | YES |
| commitment_nonce validated against assignment | NO | YES |
| solution PoW validated against puzzle | NO | YES |
| Fake receipt rejected at POST time | NO | YES |
| Fake receipt in submit_block_extended rejected | NO | YES |
| Old assignment (stale parent) rejected | Partially (staleness by height) | YES (full nonce derivation) |

---

## What Phase 11-B Does NOT Change

- The root hash format (38-byte irx1 OP_RETURN: `0x6a 0x24 "irx1" <32-byte root>`)
- The inner hash algorithm (`SHA256(height_le64 || lane_bytes || pkh_dec || sol_dec || nonce_dec)`)
- The outer hash algorithm (`SHA256(concat(inner hashes))`)
- The puzzle difficulty (still hardcoded at 1)
- Receipt deduplication logic (by height + lane + worker_pkh)
- Staleness check (reject if `receipt.height + 2 < chain.height`)
- Mainnet hard-disable (unchanged, tested in Phase 10-F)
- All existing Stratum irx1 injection logic
- All existing submit_block_extended block-level validation

---

## Regression Soak

See `scripts/testnet-poawx-phase11b-canonical-receipts-validation.sh` for the
regression soak script. It verifies:

1. Multiple receipts submitted in different orders produce the same root
2. A receipt with a wrong commitment_nonce is rejected (HTTP 400)
3. A receipt with an insufficient solution is rejected (HTTP 400)
4. The full stratum path (10 blocks) still produces irx1=True for all blocks

---

## Next Steps

| Phase | Item |
|-------|------|
| 11-B complete | Regression soak with two receipts in different orders |
| 11-C | Open cloud firewall for 39510/39512; fix VPS-2 direct P2P |
| 11-D | Limited external miner pilot (1-3 trusted testers) |
