# PoAW-X Phase 20 — Design Gap: Third-Party Pool Fee (BLOCKED)

**Status:** BLOCKED on consensus parameters. Not implemented. The safe default
(**official pool = 0% fee**, `fee_bps > 0` rejected) is unchanged.

## Current state (repo ground truth)
- The 226-byte `Delegation` carries a `fee_bps` field, but it **must be 0** everywhere:
  the wallet (`poawx-register` / `--emit-only`), the pool (`verify_and_store`,
  `build_mode1_*`), and consensus (`connect_block`) all **fail closed** on `fee_bps > 0`.
- There is **no fee-output consensus path**: nothing defines a fee payout address, a fee cap,
  or how a fee output is verified in the coinbase.

## What is undefined (must be specified before code)
1. **Explicit third-party-pool mode** — how a block/delegation is marked as third-party
   (vs the official 0% pool), and how nodes know to expect a fee output.
2. **Fee cap** — the maximum allowed `fee_bps` (consensus-enforced), to prevent abusive fees.
3. **Fee pkh binding** — the fee recipient address, **signed into the delegation** by the
   miner so it cannot be changed after the miner agrees (transparency-before-delegation).
4. **Fee coinbase output format** — canonical encoding/ordering of the fee output and the
   miner's net output, deterministically verifiable on sync.
5. **connect_block + sync verification** — exact checks: fee ≤ cap, fee output pays the bound
   fee pkh exactly, miner net output correct, mismatch/over-cap/hidden-fee all reject.
6. **Relationship to the multi-role reward split** (that design gap) — whether the fee is a
   role in the split or a separate output.

## Constraints any implementation MUST preserve
- Official Irium pool default remains **0%**.
- `fee_bps > 0` allowed **only** in an explicit third-party mode, never by accident.
- Fee transparent to the miner **before** delegation; fee terms **signed/bound** into the
  delegation; cannot change after.
- Fee payout consensus-verifiable; miner still receives correct net payout.
- The pool **delegate key is not automatically the fee recipient** — the fee pkh must be a
  configured/bound pool-fee address.
- Mainnet stays disabled; invalid/excessive/mismatched fee rejects.

## Required tests once defined (from the task)
official pool fee 0 · third-party fee only in explicit mode · fee mismatch rejects · fee
over cap rejects · fee output correct · miner net output correct · delegate key not auto fee
recipient unless configured.

## Decision needed
This is the most consensus-sensitive of the three gaps (it moves real value to a non-miner
party). Provide items 1–6 (mode flag, cap, fee-pkh binding, output format, verification
rules, split relationship). Until then: **BLOCKED — official 0% fee only; `fee_bps > 0`
rejected.** This is the single most important unresolved consensus decision before any
fee-bearing pool can exist.
