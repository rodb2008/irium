# PoAW-X Phase 12-G -- Worker Identity Binding

**Branch:** 
**Status:** Local commit only -- NOT pushed
**Builds on:** Phase 12-F (5247615) -- connect_block consensus validation

---

## Problem Addressed

**R-3 (worker_pkh not authenticated):** The  field was accepted as-is with no cryptographic verification. Any value could be claimed without proof of key ownership.

---

## Design: Option A -- Signature Binding

Workers submit two additional fields with each receipt:

| Field | Type | Description |
|---|---|---|
|  | hex | Compressed SEC1 secp256k1 public key (33 bytes) |
|  | hex | Compact ECDSA signature over the worker challenge (64 bytes) |

Reuses existing k256 v0.13 crypto infrastructure already used in settlement proof verification.

---

## Worker Challenge



- **Height-specific:** sig at height N fails at N+1
- **Puzzle-specific:** sig for solution A fails for solution B
- **Non-replayable:** nonce derived from parent block hash changes each round

---

## PKH Binding



---

## Validation Order

### poawx_post_receipt:
1. O-2: active + non-mainnet check -> 503
2. Difficulty below minimum -> 503
3. Decode solution/nonce bytes
4. Nonce derivation check -> 400
5. **[NEW] verify_worker_identity** -> 400
6. PoW check -> 400
7. Store + persist

### submit_block_extended receipt loop:
1. Nonce check -> 400
2. Decode sol bytes
3. **[NEW] verify_worker_identity** -> 400 (defense in depth)
4. PoW check -> 400

Identity placed before PoW: fail-fast, cheaper check first.

---

## New Function: verify_worker_identity

Pure function -- no I/O, no env reads. Returns Err on:
- Empty pubkey or sig
- Non-hex encoding
- Invalid secp256k1 key
- PKH mismatch
- Signature verification failure

---

## Struct Changes

Both  and  gain:



 preserves backward compatibility with existing JSON receipt files.
~19 existing struct literal constructions in tests updated.

---

## Mainnet Safety

Mainnet gate (O-2, returns 503) fires before any receipt validation in both handlers. No new mainnet conditions added.

---

## Tests Added (12)

| Test | What it verifies |
|---|---|
| test_verify_worker_identity_valid | Valid keypair + sig accepted |
| test_verify_worker_identity_empty_fields_rejected | Empty pubkey/sig -> Err |
| test_verify_worker_identity_spoofed_pkh_rejected | Wrong pkh -> Err |
| test_verify_worker_identity_bad_pubkey_hex | Non-hex pubkey -> Err |
| test_verify_worker_identity_bad_sig_hex | Non-hex sig -> Err |
| test_verify_worker_identity_wrong_height | Sig at N fails at N+1 |
| test_verify_worker_identity_wrong_solution | Sig for A fails for B |
| test_verify_worker_identity_truncated_sig_rejected | 4-byte sig -> Err |
| test_poawx_12g_receipt_with_identity_fields_persists | Fields survive save+load |
| test_poawx_12g_mode_inactive_still_503 | O-2 gate: inactive mode |
| test_poawx_12g_trivial_difficulty_still_503 | O-2 gate: trivial difficulty |
| test_poawx_12g_mainnet_receipt_still_503 | O-2 gate: mainnet |

Total: 195 -> 207 tests.

---

## Files Changed

-  -- 2 structs updated; verify_worker_identity added; 2 call sites; 12 tests
-  -- this document

---

## Checks



Push status: **NOT pushed**.

---

## Gap Status

- **R-3 CLOSED** -- worker_pkh authenticated at receipt acceptance and block submission
- GAP-1 partial, R-2, R-4, T-1 remain deferred

## Next

1. T-1 -- End-to-end testnet integration test
2. P-1/P-2 -- RC security audit
3. R-2 -- Reorg-aware receipt pruning
