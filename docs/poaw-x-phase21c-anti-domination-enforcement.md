# PoAW-X Phase 21C — Persistent, reorg-safe anti-domination enforcement

**Status:** Implemented (gated, testnet/devnet only, **mainnet hard-off**, default
off; old behavior byte-identical when off). Local-only; not pushed. Builds on the
Phase 21A anti-domination primitive (`src/poawx_dominance.rs`) and the Phase 21B
ticket/penalty enforcement pattern. PoAW-X is **consensus/network-level**; the pool
is only one miner interface, not the owner — the node re-validates everything.

## What is persisted (and how)

`ChainState` gains a `dominance: PersistentDominance` field. It holds explicit
per-`(miner_pkh, window_id)` reward buckets (`DominanceBucket`: primary/compute/
verify/support/total + valid_role_count + last_reward_height), so `apply_event` and
`revert_event` are **EXACT inverses** — the property reorg-safety requires.
Integer/fixed-point only, saturating arithmetic, no floats.

- `window_id = height / window_len`; "recent" = the last `lookback` windows
  (inclusive). `window_len` (default **2016**) and `lookback` (default **2**) are
  configurable only behind the testnet/devnet gate (`IRIUM_POAWX_ANTI_DOMINATION_WINDOW`
  / `…_LOOKBACK`, clamped ≥ 1). Buckets older than the recent range plus a margin are
  pruned, so the map stays bounded; the live disconnect path only touches recent
  tips, and restart/reorg rebuild-from-chain reconstructs everything regardless.
- **No separate on-disk store is invented.** Persistence is achieved by the existing
  pattern: the node already **replays `connect_block`** on restart (iriumd.rs) and in
  `rebuild_to_tip` (reorg), so dominance is deterministically rebuilt from the
  accepted chain. The only added live mutation is the exact `revert` in
  `disconnect_tip_block`, mirroring the existing `undo_logs` revert pattern.

## Connect-block update (reward events)

After a block is accepted, `connect_block` calls `apply_block_dominance`. Reward
events are derived from each receipt's `phase20_ext`:

- **PRIMARY → receipt `worker_pkh`** (the payout identity); COMPUTE/VERIFY/SUPPORT →
  `ext.role_reward.{compute,verify,support}_contributor_pkh`.
- Amounts come from the block subsidy via the canonical **55/22/13/10** split
  (`multi_role_amounts(block_reward(height))`), so **official fee-0 and third-party
  fee blocks produce IDENTICAL role amounts**. The fee output and the delegate are
  **never credited** as worker rewards (they are not role allocations).
- Deterministic across nodes (no env, no ordering effects beyond receipt order).

## Disconnect / reorg / restart

- `disconnect_tip_block` calls `revert_block_dominance` — the exact inverse of the
  apply for the removed tip.
- Both reorg paths (`reorg_to_tip` disconnect→connect, and `rebuild_to_tip` fresh
  replay) therefore converge to the new tip's state. Reorg A→B yields exactly B's
  state; an independent rebuild of B yields the same.
- Restart replays `connect_block`, rebuilding identical state.
- Proven by `chain::tests::phase21c_dominance_connect_disconnect_reorg`
  (connect/disconnect exact inverse, reorg A→B == rebuilt B, rebuild == restart,
  gate-off no-op) using a deterministic state-commitment **digest** over all buckets.

## State commitment (digest)

`PersistentDominance::digest()` is a canonical SHA-256 over the window/lookback
header + every non-empty bucket sorted by `(pkh, window_id)` (role totals +
valid_role_count). Order-independent; two nodes with identical accepted chains +
identical gate config produce the identical digest.

## Gated enforcement (ext binding + node validation)

- `Phase20ReceiptExt` gains an **OPTIONAL trailing `DOM1`** section carrying 4 role
  fairness weights `[PRIMARY, COMPUTE, VERIFY, SUPPORT]` — **byte-identical to
  pre-21C when absent**. Deserialize uses a strict magic-dispatch loop over the
  trailing `TPK1` (ticket) + `DOM1` (dominance) sections in any order, rejecting
  unknown/truncated trailing data.
- When `anti_domination_enforced(height)` (`IRIUM_POAWX_ANTI_DOMINATION_ACTIVATION_HEIGHT`
  + `IRIUM_POAWX_ANTI_DOMINATION_REQUIRED=1`, mainnet hard-off), `connect_block`
  calls `validate_block_dominance_weights`: each production receipt **must** carry
  `role_dominance_weights` equal to the **node-recomputed** weight
  (`fairness_weight(BASE=1000, recent_reward_share)`) for each role pkh, computed from
  the **persisted state (= parent state, since this block is validated before it is
  applied)**. Fails closed on missing/mismatched weights.
- The weight is a **per-claim** quantity (a deterministic baseline scaled by the
  miner's recent reward share). It proves each included weight is correctly computed
  from state.

## Pool side (one interface, not the owner)

`pool/irium-stratum` mirrors the `DOM1` wire byte-for-byte and the fairness math.
It applies fairness weights to **select among collected candidates**
(`select_candidate_by_fairness_weight`: highest weight wins, deterministic lower-pkh
tie-break, **no hardware-class or pool-ownership assumptions**) and attaches the 4
role weights to the produced ext when `pool_anti_domination_enforced(height)`. If it
cannot produce a required weight it attaches none and the node fails closed. The
pool view is populated operationally from authoritative node state; the node
re-validates every weight, so a diverging pool is simply rejected.

## Honest limitation — global candidate optimality is Phase 21D (pending)

The current Phase 20 ext does not carry the full candidate set, so the node can
verify that **each included weight is correctly computed from persisted state**, but
it cannot yet verify the producer selected the **globally best-weighted worker among
all (possibly unseen) candidates**. That requires a candidate-set commitment / VRF
private-assignment phase and is **explicitly deferred to Phase 21D** (not faked).
What IS implemented and enforced here: persistent + reorg-safe state, the
connect/disconnect/reorg/restart lifecycle, the deterministic per-claim weight,
node validation of included weights, and pool selection weighting.

## Penalty interaction

Penalty status already gates high-trust roles via the Phase 21B path
(`penalty_state_enforced`): suspended/slashed identities are rejected from
VERIFY/SUPPORT. Dominance weighting (this phase) reduces — but does not ban —
heavily-rewarded miners; the two are independent gates and compose.

## Gates (all default off, mainnet hard-off)

- `IRIUM_POAWX_ANTI_DOMINATION_ACTIVATION_HEIGHT` — tracking/active height.
- `IRIUM_POAWX_ANTI_DOMINATION_REQUIRED=1` — turns on consensus enforcement.
- `IRIUM_POAWX_ANTI_DOMINATION_WINDOW`, `IRIUM_POAWX_ANTI_DOMINATION_LOOKBACK` —
  window tuning (testnet/devnet only).

Each gate returns false on mainnet (`network_id == 0`) regardless of env. Chain
difficulty remains **LWMA-144 automatic** — dominance never touches PoW.

## Tests

- `poawx_dominance`: apply/revert exact inverse, recent-window share + weight,
  window roll-off, bounded pruning, order-independent digest, enforced-gate logic.
- `chain`: `phase21c_dominance_connect_disconnect_reorg` (event derivation incl. fee
  exclusion; connect/disconnect/reorg/restart; gate-off no-op);
  `phase21c_dominance_weight_enforcement` (accept correct, reject wrong/missing,
  mainnet hard-off).
- `poawx`: `phase21c_ext_dominance_section_roundtrip_backward_compatible`
  (absent byte-identical, present, precommit+dom, ticket+dom both, unknown-magic
  reject).
- pool: `phase21c_pool_dominance_weights_and_selection` (node reads pool DOM1, absent
  byte-identical, fairness selection + tie-break, mainnet hard-off).

**Excluded (not in this track):** public testnet with outside miners, independent
security audit, community vote, mainnet activation, VRF/private-assignment, puzzle
work-mode primitives, finality-committee integration.
