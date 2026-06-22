# PoAW-X Phase 26B — epoch-seed alignment implementation RESULT

**Status: IMPLEMENTED + PROVEN by repo-local tests. No live run, no live nodes.**
Implements the Phase 26A Option C recommendation (minimal admission-epoch-seed alignment). A
multi-block all-gates PoAW-X chain is now satisfiable: **6 sequential all-gates blocks are accepted
by the full `connect_block` pipeline** with both phase21d and phase22a enforced, and phase22a was
NOT modified. NOT production-ready / mainnet-ready / audited.

## Implemented invariant

The candidate-admission EPOCH seed for height `H` is the seed the parent froze in its outgoing
committed admission = the parent block's `prev_hash` (the grandparent hash `hash(H-2)`); at the
activation boundary (genesis parent, whose `prev_hash` is all-zero) it falls back to this block's
`prev_hash` (the genesis hash). The candidate set / admissions / AVR2 (true-VRF) are seeded by this
epoch seed; the puzzle, finality, and role-claim sections and the *outgoing* committed admission
continue to use the block's own `prev_hash` (which is exactly `H+1`'s epoch seed).

```
epoch_seed(H) = parent.prev_hash            (H >= 2, parent non-genesis)
              = block.prev_hash (genesis)   (activation boundary, H1)
```

Pure helper `poawx_committed_admission::admission_epoch_seed(parent_prev_hash, block_prev_hash)`.
No wire-format change.

## Files changed

- `src/poawx_committed_admission.rs` — new pure `admission_epoch_seed(...)` helper (+ doc).
- `src/chain.rs` — `validate_block_candidate_sets` (phase21d/21e) now expects `cs.seed == epoch_seed`
  (derived from the current tip = parent) and keys the phase21e admitted-set lookup on the epoch
  seed; **phase22a (`validate_block_committed_admission`) is unchanged.** Plus 3 new tests + 2 test
  env helpers.
- `src/poawx_mining_harness.rs` — devnet/test builder seeds the candidate set / admissions / AVR2
  with the epoch seed; keeps puzzle/finality/claims on `prev_hash`; builds the *outgoing* commitment
  as the exact candidate set the next block will present (height `H+1`, seed = this block's
  `prev_hash`, dominance weights at `H+1`); includes the multi-block dominance replay (replays prior
  heights' reward events via `block_reward(h)` / `multi_role_amounts` / `PersistentDominance::
  apply_event` / `RoleRewardKind`). New `parent_prev_hash` parameter.
- `src/bin/poawx-live-proof-harness.rs` — fetches the parent (tip) block's `prev_hash` from the node
  to supply the epoch seed for `H >= 2`; mainnet rejection, loopback-only RPC, and isolated/default
  storage protections unchanged. (Not exercised live in this phase.)

NOT changed: phase22a logic; the puzzle/finality/dominance gate validators; `src/pow.rs`, LWMA,
difficulty, target, block reward (`src/constants.rs`), or any mainnet behavior (PoAW-X stays
hard-off for `network_id == 0`).

## Why both gates are preserved

- **phase21d** keeps every check — exact node-recomputed candidate-set seed (now the epoch seed),
  canonical ordering, best-for-role selection, dominance-weight match, AVR2 binding, and admitted-set
  equality. Only the *expected seed value* was corrected.
- **phase22a** is byte-for-byte unchanged: outgoing commitment self-consistency + incoming match of
  `(net, target_height, seed, root, count)` against the parent's commitment. It is exercised on every
  block `>= 2` in the soak (the parent's commitment must match the child's candidate set), so a
  passing 6-block soak proves phase22a holds together with phase21d.

## Why H2+ is resolved

For every `H >= 2`, `epoch_seed(H) == parent.prev_hash`, which is exactly the seed the parent froze
in its commitment. So `cs.seed == parent_commitment.seed` and the roots match, while phase21d now
expects that same epoch seed — the prior contradiction is gone. H1 is unchanged (graced; epoch seed
== genesis).

## Test results (repo-local; `cargo test --lib`)

- `phase26b_multiblock_epoch_seed_soak` — **PASS**: builds + `connect_block`-accepts **6 sequential
  all-gates blocks** (genesis → 6); asserts `cs.seed == epoch_seed` (grandparent) at every height;
  asserts dominance weights are the genesis baseline (1000) at H1 and strictly below at H2+.
- `phase26b_stale_immediate_parent_seed_rejected` — **PASS**: a height-2 block seeded by the
  immediate parent (pre-26B seeding) is rejected with `phase21d ... seed` (gate preserved).
- `phase26b_committed_admission_root_and_replay_rejected` — **PASS**: a tampered root, a commitment
  frozen for a different seed/epoch (replay/stale), and a wrong target-height commitment all fail
  `matches_candidate_set` (phase22a binding preserved, replay-safe).
- Regression: `phase24l_lib_builder_connect_block` (H1 single-block) and the full phase21/phase22/
  phase24 suites — **PASS**.
- **Full lib suite: 744 passed / 0 failed** (run with `--test-threads=1`; this suite's env-mutating
  tests must be serialized — one pre-existing test, `phase24k_native_pow_all_gates_validators`, does
  not take the shared env lock and is flaky only under parallel execution; it passes in isolation and
  serialized. No gate was disabled or delayed to pass tests.)
- Release build: `cargo build --release --bin iriumd --bin poawx-live-proof-harness` — success.

## Claim

Multi-block all-gates PoAW-X is now satisfiable in-process: 6 sequential blocks accepted by the real
`connect_block` pipeline with phase21d and phase22a both enforced and phase22a unchanged. NOT
claimed: production-ready, mainnet-ready, audited, or validated on live nodes.

## Next recommended phase

Phase 26C — live three-system multi-block soak rerun (Windows + VPS-1 + VPS-2), using this build, to
confirm the in-process result over real nodes + cross-host propagation + restart/resync.
