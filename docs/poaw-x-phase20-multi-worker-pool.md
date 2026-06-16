# PoAW-X Phase 20 — Many-Miner / Multi-Worker Pool Behavior (PARTIAL)

**Status:** Registry layer **COMPLETE + tested**; multi-miner **block production** is a
documented gap that depends on the multi-role reward-split decision.

## What works today (registry / delegation layer)
`JsonDelegationStore` (pool/irium-stratum/src/delegation.rs) keys delegations by
`(miner_pkh, worker)` and supports **many** concurrent entries:
- multiple workers for one miner, and multiple miners, coexist without collision;
- `get(pkh, worker)` resolves exactly — **no worker can resolve to another's record**;
- `all_active(tip)` returns only active, unexpired delegations;
- the registry stores **no private keys**;
- reload preserves every entry.

`verify_and_store` already fails closed on: wrong worker, wrong miner pkh, expired delegation,
wrong network, non-zero fee, and pool-pubkey mismatch. Consensus (`connect_block`) validates
the per-receipt worker payout (10%/receipt) for **multiple workers/receipts** (Phase 12-H).

**Phase 20 test added:** `multi_worker_registry_isolation_and_reload` — two miners + a
same-miner second worker + an expired entry; verifies exact resolution, no cross-pay key,
`all_active` honors expiry, reload preserves all, no private key in the file.

Mapping to the task's required cases:
| Case | Covered by |
|---|---|
| two workers, same miner | new multi-worker test (`aa/rig1`, `aa/rig3`) |
| two miners, different workers | new multi-worker test (`aa/rig1`, `bb/rig2`) |
| wrong worker rejects | `verify_and_store_rejections` + `get` isolation |
| wrong pkh rejects | `verify_and_store_rejections` |
| expired delegation rejects | `verify_and_store_rejections` + `all_active` expiry |
| registry reload preserves all workers | new multi-worker test (reload) |
| no worker can steal another's payout | `get` isolation (exact `(pkh,worker)` key) |
| reorg/pending receipt restore mapping | node-side (iriumd) preserves delegation through pending↔block + reorg (Phase 18B-3 `4da6dd1`) |

## The gap (multi-miner block production)
The **stratum** still produces a **single-miner** coinbase (it pays one connected miner's
address). Committing a block that pays **several different miners** in one coinbase requires
**per-worker coinbase outputs**, whose canonical format and reward weighting are part of the
**multi-role reward-split design gap** (`poaw-x-phase20-design-gap-multi-role-reward-split.md`).

Until that consensus decision is made, the supported production model is **one trusted miner
per block** (each miner's own delegation → its own block paying itself), which is exactly the
proven 18C/18D/19C/19D path. Multiple miners may be **registered** concurrently; they just
cannot yet be **paid in a single block**.

## Recommended sequence
1. Resolve the multi-role reward-split spec (weights, role pkh binding, coinbase format).
2. Implement per-worker coinbase outputs in the stratum native_rewardable path (gated).
3. Extend `connect_block` validation for multi-miner coinbases (likely already close, given
   the per-worker 10%/receipt rule) and add the multi-miner block E2E test.
