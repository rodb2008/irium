# PoAW-X Phase 11-B: Canonical receipts_root + Full Solution Validation

**Branch:** `testnet/poawx-phase11b-canonical-receipts-validation`
**Commits:** 163b558 (implementation), 0bfc30f (initial docs), 442960d (plan update)
**Date:** 2026-06-11
**Status:** COMPLETE — regression soak PASS (PASS=17 FAIL=0 SKIP=0)

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
- A random `commitment_nonce` that does not match what was assigned
- A random `solution` that does not satisfy the puzzle

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

#### solution PoW validation

Validates the proof-of-work puzzle:
```
pow_hash = SHA256d(seed || commitment_nonce || solution)
leading_zero_bits(pow_hash) >= puzzle_difficulty (= 1)
```

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
| Old assignment (stale parent) rejected | Partially | YES (full nonce derivation) |

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

**Script:** `scripts/poawx-phase11b-canonical-receipts-validation.py`
**Wrapper:** `scripts/testnet-poawx-phase11b-canonical-receipts-validation.sh`

The Python script is self-contained: it spawns isolated testnet iriumd (port 39511) and
stratum (port 39512), runs all tests, and cleans up. No external services required.

### Final soak result (v6, 2026-06-11)

```
PASS: 17  FAIL: 0  SKIP: 0
RESULT: ALL PASS
```

| Check | Result | Detail |
|-------|--------|--------|
| T8-pre: mainnet processes | PASS | Detected on ports 38300/3333 |
| T8-pre: testnet ports free | PASS | 39511/39512 free before start |
| warmup | PASS | Chain advanced to tip_h=1 |
| T1: valid receipt | PASS | HTTP 200 |
| T2: wrong commitment_nonce | PASS | HTTP 400 |
| T3: insufficient PoW | PASS | HTTP 400 |
| T4: order-independent root | PASS | compute_root(A,B) == compute_root(B,A) |
| T5: iriumd root matches Python | PASS | Canonical roots identical |
| T6: fabricated receipt in SBE | PASS | HTTP 400 |
| T7-harness: 10-block stratum | PASS | 10/10 blocks, 0 FAIL |
| T7-irx1: receipt-bearing block | PASS | 1/10 blocks with irx1=True |
| T7-panics | PASS | 0 panics in iriumd log |
| T7-sbe | PASS | 1 block accepted via submit_block_extended |
| T7-receipts | PASS | 2 receipts stored |
| T7-stratum | PASS | 2 error lines (acceptable) |
| T8-post: mainnet PIDs | PASS | Unchanged throughout soak |

### Key v6 fix: stratum job refresh timing

The soak script restarts the stratum process after T1-T6 post receipts. This is required
because the stratum's `current` job cache is only updated when the `height:prevhash` key
changes — posting receipts to iriumd does not trigger a re-broadcast. After restart, the
stratum's first template poll fetches the h=2 template with pending receipts, setting
`current` to a job that includes them. T7's first harness block (h=2) then goes through
`submit_block_extended` and is accepted. Subsequent blocks have 0 pending receipts and
use the legacy `submit_block` path.

---

## Remaining Blockers Before Phase 11-C (Public Testnet)

| Item | Status | Notes |
|------|--------|-------|
| Cloud firewall ports 39510/39512 | PENDING | Required for external miners |
| VPS-2 direct P2P | PENDING | Remove SSH tunnel dependency |
| DNS seed node | PENDING | Register testnet DNS seed |
| Chain reset policy | PENDING | Document and implement |
| `getblock` RPC endpoint | PENDING | Missing endpoint needed for explorers |
| Disabled-mode ephemeral instability | CHECK | Verify resolved or document |
| External miner onboarding docs | PENDING | Miner guide for external testers |

---

## Next Steps

| Phase | Item |
|-------|------|
| 11-B | COMPLETE |
| 11-C | Operator runbook: systemd, log rotation, monitoring |
| 11-D | Limited external miner pilot (1-3 trusted testers) |
| 11-E | Public testnet launch candidate |
