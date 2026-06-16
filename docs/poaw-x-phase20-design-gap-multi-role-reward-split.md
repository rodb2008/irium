# PoAW-X Phase 20 — Design Gap: Multi-Role Reward Split (BLOCKED)

**Status:** BLOCKED on consensus parameters. Not implemented.

## Current state (repo ground truth)
- The only consensus reward rule is `validate_poawx_reward_split_from_block` (chain.rs):
  `worker_due = block_reward(height) × POAWX_WORKER_REWARD_PERMILLE / 1000` = **10% per
  receipt**, paid to each receipt's `worker_pkh`. In the proven mode-1 single-miner path the
  coinbase pays the miner pkh as a single p2pkh output.
- There is **no multi-role split** (e.g. 55/22/13/10) defined anywhere in the repo. That
  weighting exists only in external "blueprint memory," **not** in versioned consensus.

## What is undefined (must be specified before code)
1. **Role set** — exactly which roles share the reward (miner / pool / treasury /
   relay / …?). The four-way "55/22/13/10" implies four roles, but the repo names none.
2. **Per-role weights** — the exact integer permille/percent for each role, and whether they
   are fixed or governance-adjustable.
3. **Role → pkh binding** — how each role's payout address is determined and bound
   (in the delegation? in a consensus config? in the coinbase with a proof?).
4. **Coinbase output format** — the canonical ordering/encoding of the multiple p2pkh outputs
   so blocks are deterministically verifiable on sync.
5. **connect_block + sync validation rules** — the new exact-sum / per-role checks
   (no overpay, no missing role, correct scripts), and how they replace/extend the current
   10%/receipt rule.
6. **Non-custodial + 0%-fee invariants** — confirm the miner/direct payout stays
   non-custodial and the official pool fee stays 0% (delegate key NOT a reward recipient
   unless a future fee design says so — see the third-party fee design gap).

## Constraints any implementation MUST preserve
- Reward split consensus-verifiable; outputs sum exactly; no overpayment.
- Mode-1 delegated receipt still pays the **miner pkh** correctly (non-custodial).
- Official pool fee 0%; delegate key not paid (unless explicit fee design).
- Mainnet mode-1 hard-disabled until activation; legacy PoW path unchanged.
- Testnet-gated; invalid split rejects; mainnet remains disabled.

## Required tests once defined (from the task)
reward outputs sum correctly · role outputs match configured split · no overpayment · no
delegate payout unless designed · invalid split rejects · mainnet remains disabled · legacy
PoW path unchanged · block + sync validation updated if coinbase format changes.

## Decision needed
Provide items 1–5 (role set, exact weights, pkh binding, coinbase format, validation rules).
Inventing the weights or roles now would create binding consensus from a guess — disallowed.
**BLOCKED** until the owner supplies the canonical multi-role specification. Until then the
10%-per-receipt worker rule remains the consensus reward model.
