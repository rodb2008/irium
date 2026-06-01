# Block-time V2 activation runbook

This document describes the coordinated hard-fork upgrade that switches the
Irium protocol block-time target from `T = 600s` to `T = 120s` while
rescaling the halving interval from `210_000` to `1_050_000` blocks to
preserve a roughly four-year halving calendar at the new `T`.

It mirrors the structure of `docs/htlcv1_activation_commit_workflow.md`.

---

## Rationale

The live mainnet chain has been producing blocks at ~60–120 seconds for
the entire post-LWMA period (per the project memory entry
`project_blocks_per_hour.md`), well below the protocol target of 600
seconds. LWMA cannot fully compensate because per-block step clamps cap
the difficulty hardening rate at 2× per block; observed block time stays
~5–10× faster than `T` indefinitely whenever hashrate keeps growing.

This drift produces three concrete problems:

1. **Reality–protocol mismatch.** Frontend tooling (Explorer, Miner
   panel, Block Reward estimates) hardcodes `BLOCKS_PER_HOUR = 60`
   against observed pace; the protocol number says 6. Every developer
   touching the codebase has to know which one is correct in which
   context.
2. **Compressed halving cadence.** At observed ~90s blocks, the first
   halving lands at ~219 days from genesis rather than the designed ~4
   years.
3. **No equilibrium.** LWMA hardening is permanently active and
   cannot converge, which slightly degrades the difficulty signal's
   responsiveness to genuine hashrate drops.

Switching to `T = 120s` aligns the protocol target with the observed
rate (within 2× rather than 5–10×). Coupling that switch with a 5×
expansion of `HALVING_INTERVAL` preserves the original economic curve:
roughly four calendar years between halvings, just expressed as 1.05M
blocks instead of 210k.

---

## Confirmed parameters

| Parameter | Pre-fork (V1) | Post-fork (V2) |
| --- | --- | --- |
| `BLOCK_TARGET_INTERVAL` | 600 s | 120 s |
| `HALVING_INTERVAL` | 210_000 | 1_050_000 |
| LWMA v1 max solvetime ceiling | 6 × 600 = 3600 s | 6 × 120 = 720 s |
| LWMA v2 max solvetime ceiling | 10 × 600 = 6000 s | 10 × 120 = 1200 s |
| Initial subsidy | 50 IRM | 50 IRM (unchanged) |
| Max future block time | 7200 s | 7200 s (unchanged — Bitcoin convention) |
| Coinbase maturity | 100 blocks | 100 blocks (unchanged — ~3.3h at V2) |
| `LWMA_WINDOW` / `LWMA_V2_WINDOW` | 60 / 30 blocks | 60 / 30 blocks (unchanged) |
| Per-block step clamp factors | 2× / 2× | 2× / 2× (unchanged) |

`COINBASE_MATURITY`, `MAX_FUTURE_BLOCK_TIME`, and the LWMA window sizes
are deliberately left at their V1 values: they are well-understood
constants whose semantics at `T=120s` remain reasonable.

---

## Code surface

Modified upstream files:
- `src/activation.rs` — adds `MAINNET_BLOCK_TIME_V2_ACTIVATION_HEIGHT`,
  `runtime_block_time_v2_env_override()`, and
  `resolved_block_time_v2_activation_height(network)`. Three unit
  tests pin the mainnet `None`-pending-governance default and the
  devnet env-override pattern.
- `src/constants.rs` — adds `BLOCK_TARGET_INTERVAL_V1`,
  `BLOCK_TARGET_INTERVAL_V2`, `HALVING_INTERVAL_V1`,
  `HALVING_INTERVAL_V2`, plus the height-aware accessors
  `block_target_interval(height)` and `halving_count(height)`.
  `block_reward(height)` is rewritten on top of `halving_count` and
  keeps its existing signature, so all seven existing callers
  (chain.rs, iriumd.rs, irium-miner.rs ×2, irium-miner-gpu.rs,
  irium-explorer.rs ×2) pick up the rescaled halving automatically.
- `src/chain.rs` — `LwmaParams.max_solvetime` is replaced with
  `LwmaParams.solvetime_clamp_factor`; the ceiling is now derived at
  use time via `LwmaParams.max_solvetime_at(target_height)`. The LWMA
  expected-time computation reads `block_target_interval(target_height)`.
  Legacy retarget hardcodes `BLOCK_TARGET_INTERVAL_V1` (the legacy
  codepath is unreachable at any height past the LWMA activation, all
  far below any future V2 fork). The dead `expected_time` helper is
  removed.

The 14 `ChainParams { ... }` construction sites across `main.rs`,
`iriumd.rs`, `p2p.rs`, `irium-miner.rs`, `irium-p2p.rs`, and the
`chain.rs` test fixtures need ZERO updates: `LwmaParams::new` /
`::new_v2` keep their `(activation_height, pow_limit)` signatures.

The seven `block_reward(height)` callers need ZERO updates: the
function signature is unchanged.

---

## Phased rollout

### Phase 1 — Land the code dark (this commit)

Ship every consensus change with
`MAINNET_BLOCK_TIME_V2_ACTIVATION_HEIGHT = None`. Mainnet behaviour is
bit-for-bit unchanged: `block_target_interval(h)` returns 600 for every
height, `halving_count(h)` reduces to the classic
`(h - 1) / HALVING_INTERVAL_V1` formula, and the LWMA windows continue
to compute against `T = 600s`. The new accessors and tests merely make
the code height-aware so a later activation commit can flip a single
constant.

Verification:
1. `cargo build --release` clean (zero new warnings, zero errors).
2. `cargo test --all --release` green on local Windows + both VPS.
3. Existing LWMA / activation-boundary / reward-curve tests all pass
   unchanged, confirming no semantic regression at the V1 plateau.

### Phase 2 — Devnet soak (separate commit, post-merge)

In a follow-up commit:
- Pick a small devnet activation height (e.g. 50).
- Set `IRIUM_BLOCK_TIME_V2_ACTIVATION_HEIGHT=50` plus
  `IRIUM_NETWORK=devnet` on the devnet node.
- Mine ≥ 200 blocks across the boundary.
- Monitor:
    - Difficulty curve crosses smoothly (no more than 4× single-block jump).
    - Observed block time converges to ~120s post-fork.
    - `halving_count` snapshot at heights `fork`, `fork+1`,
      `fork + HALVING_INTERVAL_V2`, `fork + HALVING_INTERVAL_V2 + 1`.
    - No orphan/reorg spikes around the boundary.

### Phase 3 — Mainnet activation commit (governance-gated)

A dedicated activation commit per the workflow in
`docs/htlcv1_activation_commit_workflow.md`:
- Flip `MAINNET_BLOCK_TIME_V2_ACTIVATION_HEIGHT` from `None` to
  `Some(<height>)`.
- Picked height SHOULD provide ≥ 6 weeks of lead time at the
  observed pace, computed from the release-candidate build date.
- Release notes call out the coupled change (T → 120s, halving →
  1.05M blocks) and the calendar implications.

The activation commit is intentionally separate from this code-landing
commit so the activation decision can be reverted without unwinding the
plumbing.

---

## Monitoring checklist (post-activation)

At and immediately after the activation height:

1. **Difficulty curve** — sample target bits at `fork - 5`,
   `fork - 1`, `fork`, `fork + 1`, ... `fork + 30`. The LWMA ratio
   step from `T=600` to `T=120` plus observed ~90s solvetimes implies
   the new target should ease modestly (~25% in a single window step).
   Any single-block jump beyond 2× is a red flag — the per-block step
   clamp should prevent it.
2. **Observed block time** — rolling 30-block average should drift
   toward ~120s within ~2× the LWMA window of the activation height.
3. **Halving counter** — `halving_count(fork)` and
   `halving_count(fork + 1)` must be equal. Spot-check via the
   `/blockreward?height=<h>` debug path or via mining a coinbase and
   reading the subsidy.
4. **No subsidy discontinuity** — coinbase value at height `fork + 1`
   must equal coinbase value at height `fork` (modulo fees).
5. **Mempool / orphan rate** — should remain steady. A spike at the
   boundary indicates a node-version disagreement and demands
   immediate triage.

---

## Rollback signal

A block-time hard fork is not safely auto-revertable; once miners
produce post-fork blocks, those blocks are valid only under the new
rules. Rollback would require:
1. A coordinated downgrade by every miner controlling > 50% of
   hashrate, abandoning post-fork blocks.
2. A subsequent activation-revert commit raising
   `MAINNET_BLOCK_TIME_V2_ACTIVATION_HEIGHT` back to `None` or to a
   future height.

Trigger conditions that justify a rollback attempt:
- Sustained orphan rate > 5% across the first 100 post-fork blocks
  (indicates node-version disagreement, not a consensus bug, but
  unsafe to operate through).
- Difficulty trough deeper than 10× the pre-fork target sustained for
  > 100 blocks (signals that observed pace has not converged and the
  network is operating under an exploitable difficulty regime).
- Reorg depth > 6 blocks observed at or near the boundary.

None of these conditions are expected; this section exists so the
response criteria are written down before they are needed, not after.
