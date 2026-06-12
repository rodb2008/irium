# Phase 12-H: PoAW-X Reward Split Enforcement

**Status:** Complete (local commit, not pushed)
**Branch:** testnet/poawx-phase12-completion-rc-hardening
**Closes:** R-4 — no reward split enforcement for PoAW-X puzzle work
**Depends on:** Phase 12-G (worker identity binding, commit 0ee1fc5)

---

## Problem (R-4)

A block could include valid PoAW-X participation receipts (each with a verified
worker_pkh from Phase 12-G) but have a coinbase that does not pay the worker
anything at all. There was no on-submit enforcement that the coinbase outputs
contain the required worker share.

## Solution

### Constant

```rust
const POAWX_WORKER_REWARD_PERMILLE: u32 = 100;
// 10% of block subsidy per receipt
```

### Pure Helper

```rust
fn poawx_worker_due(base_reward: u64) -> u64 {
    base_reward * POAWX_WORKER_REWARD_PERMILLE as u64 / 1000
}
```

At the genesis subsidy (50 IRM = 5,000,000,000 sat), each receipt requires a
worker payout of 500,000,000 sat (5 IRM). This halves with the subsidy.

### Validation Function

```rust
fn poawx_validate_reward_split(
    coinbase: &Transaction,
    receipts: &[PoawxPendingReceipt],
    height: u64,
) -> Result<(), String>
```

For each distinct `worker_pkh` in the receipts:
1. Counts how many receipts belong to that worker (`count`)
2. Computes `required = poawx_worker_due(block_reward(height)) * count`
3. Sums all P2PKH outputs in the coinbase that pay to that exact `worker_pkh`
4. Rejects (`Err`) if `total_paid < required`

### Enforcement Point

Called inside `submit_block_extended`, within the existing
`if !req.poawx_receipts.is_empty()` block, **after** the irx1 commitment check:

```rust
poawx_validate_reward_split(coinbase, &req.poawx_receipts, req.height)
    .map_err(|e| {
        eprintln!("[submit_block_extended] reject: reward split: {}", e);
        StatusCode::BAD_REQUEST
    })?;
```

The `coinbase` variable is already bound by the irx1 check scope — no extra
decoding needed.

---

## Scope and Limitations

- **Testnet only** via O-2 guard (`IRIUM_NETWORK != mainnet`). Enforcement never
  reaches the validation logic on mainnet.
- **Submit-path enforcement** — not chain consensus. `worker_pkh` is stored in
  local receipt state (`PoawxPendingReceipt`), not written into the block
  header or transaction data. A node that did not run `submit_block_extended`
  (e.g., a peer that relayed the block) cannot re-validate this rule on
  `connect_block`. This is a known limitation; full consensus enforcement
  requires embedding worker claims in the block itself (future work).
- **Per-receipt payout** — the rule scales: 2 receipts for the same worker
  require 2x the individual payout.
- **Exact P2PKH match** — the coinbase output must use the `p2pkh_script`
  encoding for the worker's PKH. Payments via non-standard scripts are NOT
  credited.
- Does **not** solve reorg handling (R-2) or end-to-end testnet integration (T-1).

---

## Tests (12 added, 219 total, 0 failures)

| Test | Coverage |
|------|----------|
| `test_poawx_worker_due_calculation` | Pure arithmetic: 50 IRM -> 5 IRM; halved; zero |
| `test_poawx_validate_reward_split_empty_receipts_ok` | Empty receipts -> no payment required |
| `test_poawx_validate_reward_split_valid_payout` | Exact required amount -> accepted |
| `test_poawx_validate_reward_split_underpaid` | 1 sat short -> rejected, error names payout |
| `test_poawx_validate_reward_split_missing_output` | Worker PKH not in coinbase outputs -> rejected |
| `test_poawx_validate_reward_split_wrong_script_type` | OP_RETURN-only coinbase -> rejected |
| `test_poawx_validate_reward_split_multiple_workers_both_paid` | Two workers, both paid -> accepted |
| `test_poawx_validate_reward_split_multiple_workers_one_missing` | Two workers, one missing -> rejected |
| `test_poawx_validate_reward_split_multi_receipts_same_worker_paid` | 2 receipts -> 2x payout required, provided |
| `test_poawx_validate_reward_split_multi_receipts_same_worker_underpaid` | 2 receipts -> 2x required, 1x paid -> rejected |
| `test_poawx_12h_mainnet_still_503` | O-2 guard: mainnet still 503 |
| `test_poawx_12h_inactive_mode_still_503` | Inactive PoAW-X mode still 503 |

---

## Blockers Remaining

| ID | Description |
|----|-------------|
| GAP-1 | Receipt-level PoW difficulty not enforced in chain consensus |
| R-2 | Reorg-aware receipt pruning |
| T-1 | End-to-end testnet integration test |
| P-1/P-2 | RC security audit |
