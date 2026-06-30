# PoAW-X Phase 13-B: Consensus-Level Receipt Verification in connect_block

## Summary

Phase 13-B implements full consensus-level validation of PoAW-X block receipts
in `connect_block`. After PoAW-X activation on active testnet/devnet, every
block is verified deterministically from its own data (block-contained receipts
added in Phase 13-A). No RPC state, no pending receipt memory, no local
persistence used.

This closes **P-2** (PoAW-X consensus-level PoW, identity, and reward
verification in `connect_block`).

## Validator Location

`src/chain.rs` — `fn validate_poawx_block_receipts(block, height, previous)`

Called from `connect_block` immediately after `validate_poawx_coinbase`.

## Activation Behavior

| Context                  | Behavior                                               |
|--------------------------|--------------------------------------------------------|
| No activation height set | Skip (legacy, always Ok)                               |
| Mode != `active`         | Skip (legacy, always Ok)                               |
| Network == mainnet       | Skip (mainnet unchanged)                               |
| height < activation_height | Skip (pre-activation, always Ok)                    |
| Active non-mainnet ≥ activation_height | All checks enforced             |

## Exact Consensus Rules Added

All 7 rules apply only in active non-mainnet mode after activation height:

### Rule 1 — Receipts Present
`block.poawx_receipts` must be `Some` and non-empty.

### Rule 2 — irx1 Root Matches Receipts
The 32-byte root in the coinbase OP_RETURN (`0x6a 0x24 irx1 <root>`) must
equal `irx1_root_from_block_receipts(receipts)`. Root must not be zero.

Root computation (`irx1_root_from_block_receipts` in `src/poawx.rs`):
```
receipts sorted by (height, lane, worker_pkh, commitment_nonce) ascending
inner_i = SHA256(receipt.height_le8 || receipt.lane_byte ||
                 receipt.worker_pkh || receipt.solution ||
                 receipt.commitment_nonce)
root = SHA256(inner_0 || inner_1 || ... || inner_N)
```
Matches `compute_poawx_receipts_root` in `iriumd.rs` (which uses hex-string
`PoawxPendingReceipt` fields — binary comparison is equivalent to hex string
comparison for sorting).

### Rule 3 — Commitment Nonce
For every receipt: `receipt.commitment_nonce` must equal the deterministic nonce
derived from the parent block:
```
parent_hash  = previous_block.header.hash_for_height(height - 1)
seed         = SHA256(parent_hash || (height-1)_le8 || b"poawx_assignment_seed_v1")
expected_nonce = SHA256(seed || b"commitment_nonce")
```

### Rule 4 — Worker PKH Derivation
For every receipt: `receipt.worker_pkh` must equal
`RIPEMD160(SHA256(receipt.worker_pubkey))`.

### Rule 5 — Worker Signature
For every receipt: `receipt.worker_sig` must be a valid secp256k1 ECDSA
signature (k256) over:
```
challenge = SHA256(receipt.solution || receipt.commitment_nonce || receipt.height_le8)
```
verified by `receipt.worker_pubkey`.

### Rule 6 — Puzzle PoW
For every receipt: `sha256d(seed || expected_nonce || receipt.solution)` must
have at least `IRIUM_POAWX_PUZZLE_DIFFICULTY_BITS` leading zero bits
(default 8, min 4, max 24). Reads same env var as `submit_block_extended`.

### Rule 7 — Reward Split
Each unique `worker_pkh` across all receipts must be paid at least:
```
worker_due = block_reward(height) × 100 / 1000   (10% per receipt)
required   = worker_due × count_for_this_pkh
```
via a P2PKH output (`OP_DUP OP_HASH160 <pkh> OP_EQUALVERIFY OP_CHECKSIG`)
in the coinbase.

## Files Changed

| File          | Change                                                                 |
|---------------|------------------------------------------------------------------------|
| `src/poawx.rs` | Added `count_leading_zero_bits(hash) -> u32` (pub) |
| `src/poawx.rs` | Added `irx1_root_from_block_receipts(receipts) -> [u8; 32]` (pub) |
| `src/chain.rs` | Added `poawx_block_difficulty_bits() -> u32` |
| `src/chain.rs` | Added `validate_poawx_reward_split_from_block(block, receipts, height)` |
| `src/chain.rs` | Added `validate_poawx_block_receipts(block, height, previous)` |
| `src/chain.rs` | `connect_block` now calls `validate_poawx_block_receipts` after `validate_poawx_coinbase` |

## Test Results

All 14 Phase 13-B tests pass:

| Test | Covers |
|------|--------|
| `phase13b_inactive_mode_always_ok` | Mode != active → skip |
| `phase13b_pre_activation_height_ok` | height < activation → skip |
| `phase13b_mainnet_unchanged` | Mainnet → skip |
| `phase13b_missing_receipts_rejected` | Rule 1: None receipts |
| `phase13b_empty_receipts_rejected` | Rule 1: empty receipts |
| `phase13b_zero_irx1_root_rejected` | Rule 2: zero root |
| `phase13b_valid_block_accepted` | All 7 rules — full round-trip |
| `phase13b_irx1_root_mismatch_rejected` | Rule 2: corrupted coinbase root |
| `phase13b_wrong_commitment_nonce_rejected` | Rule 3: wrong nonce |
| `phase13b_bad_worker_sig_rejected` | Rule 5: invalid signature |
| `phase13b_spoofed_pkh_rejected` | Rule 4: pkh/pubkey mismatch |
| `phase13b_insufficient_puzzle_difficulty_rejected` | Rule 6: low PoW |
| `phase13b_missing_worker_payout_rejected` | Rule 7: underpaid worker |
| `phase13b_legacy_block_wire_still_parses` | Pre-13-A wire format unchanged |

## P-2 Status

**P-2 is fully closed** for testnet/devnet validation.

`connect_block` now validates all 7 consensus properties from block-contained
data. Blocks that are accepted by `submit_block_extended` on the originating
node will also be accepted by `connect_block` on all receiving nodes (same
seed/nonce derivation, same difficulty check, same root algorithm).

## Remaining Blockers Before Real Miner Pilot

| Blocker | Status |
|---------|--------|
| P-1: block-contained receipts in wire format | **DONE** (Phase 13-A) |
| P-2: consensus verification in connect_block | **DONE** (Phase 13-B) |
| R-2: reorg receipt restore | Open — on reorg, receipts in orphaned blocks need recovery path |
| End-to-end devnet test with live blocks | Pending (no real miners invited) |
