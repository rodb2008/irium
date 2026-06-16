# PoAW-X Phase 20 — Multi-Role Reward Split (DESIGN GAP RESOLVED — consensus primitives + validation implemented; pool production follow-up)

**Status:** **RESOLVED for consensus primitives + validation + tests** (testnet/devnet-gated,
mainnet hard-off). **Pool block-production wiring + live `connect_block` enforcement + node
receipt-wire/persist threading remain a documented follow-up** (per the owner spec item 20).
Local-only; not pushed.

## Owner-supplied spec (now recorded)
Weights (basis points of the block subsidy, total 10000):
- PRIMARY_MINER = **5500** (55%) — the miner/block-producing identity = receipt `worker_pkh`; never the pool delegate key.
- COMPUTE_CONTRIBUTOR = **2200** (22%) — consensus-bound payout role only (assignment is the separate fairness-matrix task).
- VERIFY_CONTRIBUTOR = **1300** (13%) — consensus-bound payout role only (no hidden-assignment/reveal here).
- SUPPORT_CONTRIBUTOR = **1000** (10%) — consensus-bound payout role only; **not a treasury**; never a pool/delegate key unless that exact pkh is the support pkh in the role claim.

## What is implemented (this commit set)
Consensus primitives in `src/poawx.rs`:
- `MULTI_ROLE_{PRIMARY,COMPUTE,VERIFY,SUPPORT}_BPS` = 5500/2200/1300/1000, `MULTI_ROLE_TOTAL_BPS=10000` (constant-sum test).
- `RoleReward { compute_contributor_pkh, verify_contributor_pkh, support_contributor_pkh }` — 60-byte canonical wire (compute‖verify‖support); `serialize`/`deserialize`/`digest`; the **primary pkh is NOT stored here** (it is the receipt `worker_pkh`, so it can never be replaced by a pool key).
- `multi_role_amounts(total) -> [primary, compute, verify, support]`: each non-primary = `floor(total*bps/10000)` (u128 intermediates); **remainder → PRIMARY**; sum is **exactly** `total` (no over/underpay).

Gating in `src/activation.rs` + `src/chain.rs`:
- `poawx_multi_role_reward_activation_height()` reads `IRIUM_POAWX_MULTI_ROLE_REWARD_ACTIVATION_HEIGHT`.
- `multi_role_reward_active(height)` — **mainnet always false** (hard-off until explicit future governance activation); testnet/devnet gate on the height.

Canonical-coinbase validator in `src/chain.rs` (`validate_multi_role_coinbase_outputs`, pure):
- Output order is fixed: **PRIMARY, COMPUTE, VERIFY, SUPPORT** (after any zero-value `irx1` OP_RETURN, which is allowed and ignored).
- Amounts must equal `multi_role_amounts(total)` exactly; exactly four P2PKH role outputs.
- Rejects: wrong amount, wrong order, missing role (≠4), value-bearing non-P2PKH output (hidden fee), extra P2PKH (delegate/5th payout), primary pkh ≠ supplied worker pkh.
- **Duplicate pkh rule:** duplicate role pkhs are kept as **separate** outputs in fixed order (no aggregation) — aggregating them is rejected.

## Rounding rule
`floor(total*bps/10000)` per non-primary role; the integer-division remainder is added to
PRIMARY_MINER so outputs sum to the full reward exactly.

## Activation gate
`IRIUM_POAWX_MULTI_ROLE_REWARD_ACTIVATION_HEIGHT` (testnet/devnet only). Before the height,
behavior is unchanged (existing 10%/receipt path); mainnet is hard-off regardless.

## Tests (all passing)
poawx: amounts exact-split + remainder + zero; 60-byte wire round-trip + digest sensitivity +
truncation; JSON round-trip; bps total = 10000; v1/v2 receipt encoding unchanged (pre-activation
byte-identical). chain: valid coinbase accepted (with/without irx1); rejections (amount, order,
missing role, hidden fee, extra p2pkh, primary mismatch); duplicate-pkh kept separate +
aggregation rejected; gate mainnet-off + testnet height + no-height-off.

## What remains (follow-up, explicitly out of scope here)
1. **Pool production wiring** — the stratum building the canonical four-output coinbase after
   activation (depends on the role-claim source for COMPUTE/VERIFY/SUPPORT pkhs).
2. **Node receipt-wire + persistence threading** — carrying `RoleReward` in the block receipt
   section (v3) + JSON persist/reload + reorg mapping + P2P, and calling
   `validate_multi_role_coinbase_outputs` from `connect_block`/`submit_block_extended` when
   `multi_role_reward_active(height)` and a role section is present.
3. Still **separate tasks** (not resolved here): the **CPU/GPU/ASIC fairness matrix**,
   **hidden assignment / commit-reveal**, and **third-party pool fee** (each has its own
   design-gap doc).

The hard consensus core (split math, canonical output format, validation rules, activation
gating, mainnet-off) is implemented and tested; the remaining work is integration/production
wiring following the proven Phase 18B mode-1 threading pattern.
