# PoAW-X — Blueprint Completion: Phases 27-30 (Gaps 7, 3, 10, 12)

**Status: LOCAL TECHNICAL IMPLEMENTATION of the four remaining blueprint gaps —
gated, mainnet hard-off, NOT activated, NOT audited.** Branch
`testnet/poawx-phase20-blueprint-completion-local`. Every mechanism below is
disabled by default and forced off on mainnet (`network_id == 0`). This document
is not a claim of audit, production-readiness, or mainnet-readiness. External
security review, a public testnet, and a community/governance activation decision
remain pending (see "What comes next").

## A. Scope

The 2026-06-24 blueprint audit identified four mechanisms that were partial or
missing. This phase set closes them. Phase numbering maps to gaps in the order
implemented:

| Phase | Gap | Mechanism | Commit |
|---|---|---|---|
| 27 | 7 | Finality threshold defaults to 2/3 supermajority | `e906d8e` |
| 28 | 3 | Multi-source assignment seed | `8cfa015` |
| 29 | 10 | Adaptive security-posture engine wired | `d634a5c` |
| 30 | 12 | Solo PoAW-X mining (miner key + `--poawx`) | `694fb47` |

Prerequisite context (committed earlier in the same series): fraud-proof v1 —
finality-equivocation slashing (`97f2c3f`), which provides the double-signing
penalty that Phase 27 pairs with; and a build fix syncing `main.rs` with the
`lib.rs` module tree so `cargo test --all` compiles (`8191ba3`).

All consensus-affecting changes are additive and gated; with every new gate unset
the node produces and validates byte-identical blocks to before this work (proven
by the unchanged pre-existing test suite).

## B. Phase 27 / Gap 7 — Finality threshold 2/3 supermajority

**What it implements.** The blueprint requires a 2/3 finality threshold. The node
previously defaulted the finality-committee Commit-vote threshold to 1/1
(unanimous of the present committee). `finality_threshold()` now defaults to
**2/3**. The math is factored into a pure, param-driven helper
`finality_threshold_values(num, den)` so the default is unit-tested race-free; the
required Commit votes for a committee of size N are `ceil(N * num / den)`, clamped
to `[1, N]`.

**Gate variables.**
- `IRIUM_POAWX_FINALITY_THRESHOLD_NUM` / `IRIUM_POAWX_FINALITY_THRESHOLD_DEN` —
  override the 2/3 default (1/1 unanimous is still selectable).
- Finality enforcement itself is gated by
  `IRIUM_POAWX_FINALITY_COMMITTEE_ACTIVATION_HEIGHT` +
  `IRIUM_POAWX_FINALITY_COMMITTEE_REQUIRED`; the threshold only takes effect once
  the finality committee is active.

**Mainnet hard-off.** The finality committee (and therefore any threshold) is
forced off when `network_id == 0`. The 2/3 value only governs behavior on a
non-zero network with finality activated.

**Pool requirement on testnet.** `irium-stratum` carries its own
`pool_finality_threshold()` which still defaults to 1/1. When finality is enabled
on a pool-mined testnet, set `IRIUM_POAWX_FINALITY_THRESHOLD_NUM=2` and
`IRIUM_POAWX_FINALITY_THRESHOLD_DEN=3` in BOTH the node and the pool environment so
the pool-built finality proofs match the node-expected threshold.

## C. Phase 28 / Gap 3 — Multi-source assignment seed

**What it implements.** The randomized-assigned-work seed (the candidate-set /
true-VRF seed, `admission_epoch_seed`) was a single value: the grandparent block
hash. A party mining consecutive blocks could grind it. When the gate is active,
the seed for height T mixes four sources, all sealed by/at block T-1 so the
committed-admission freeze one block ahead and the validator at T agree, and the
proposer of T cannot set them:

1. the grandparent hash (the prev-block source),
2. the parent block finality-proof digest (committee-controlled — the anti-grind core),
3. the parent precommit_root (hidden-precommit miner commitments),
4. epoch-index keying (epoch entropy / domain separation).

A single shared resolver (`expected_epoch_seed` / `resolve_epoch_seed_parts`) is
used by the candidate-set gate AND both committed-admission seed checks (outgoing
freeze + incoming verification), so they never diverge; the true-VRF binding
follows via the candidate-set seed.

**Gate variable.** `IRIUM_POAWX_MULTISOURCE_SEED_ACTIVATION_HEIGHT`. Unset =>
legacy single grandparent-hash seed, byte-identical to pre-Phase-28 blocks.

**Mainnet hard-off.** `multisource_seed_gate` returns false for `network_id == 0`.

**Pool requirement on testnet.** `irium-stratum` builds the candidate sets and
committed-admission commitments and currently freezes the legacy grandparent-hash
seed. Do NOT enable `IRIUM_POAWX_MULTISOURCE_SEED_ACTIVATION_HEIGHT` on a
pool-mined testnet until the pool implements the identical v2 seed formula and is
configured with the same activation height; otherwise the node will reject
pool-built blocks (candidate-set / committed-admission seed mismatch). Self-hosted
solo mining (Phase 30) already produces the v2 seed.

**Honest limitation.** The strong anti-grinding guarantee requires the finality
committee to be active (it supplies the committee-controlled source). With finality
off, the v2 seed still mixes the precommit and epoch keying but the committee
source is empty.

## D. Phase 29 / Gap 10 — Adaptive security-posture engine

**What it implements.** The Normal / Caution / Defense / Recovery state machine
existed but nothing computed signals or held the mode. It is now wired as a
node-local security posture on the chain state: after each accepted block the node
recomputes `NetworkSignals` from chain-derived data — reward concentration
(persistent anti-domination state), participation (distinct miners and role
solvers over a 32-block window), recent slashes (persistent penalty state, windowed),
finality availability — plus a node-local reorg-pressure counter (raised on
disconnect, decayed on connect). The mode transitions with hysteresis (Defense ->
Recovery -> Normal). The current mode is exposed in the iriumd `/status` response
as `poawx_adaptive_mode` so miners and operators can observe the posture.

**Gate variable.** `IRIUM_POAWX_ADAPTIVE_MODE_ACTIVATION_HEIGHT`. Unset => the
posture stays Normal and the update is a no-op.

**Mainnet hard-off.** `adaptive_mode_gate` returns false for `network_id == 0`.

**Consensus boundary (important).** The adaptive posture is ADVISORY and
node-local. It never gates block validity, because some inputs (reorg
observations) are not deterministic across nodes. It is a security/mining-policy
signal (confirmation multiplier, stricter verification, role fallback), not a
consensus rule.

**Pool requirement on testnet.** None. The posture is node-local and does not
affect block construction or validity, so the pool needs no change. Operators may
read `poawx_adaptive_mode` from each node independently.

**Follow-on.** Consuming the policy concretely (e.g. a wallet using
`confirmation_multiplier` for confirmation depth) is a downstream task.

## E. Phase 30 / Gap 12 — Solo PoAW-X mining

**What it implements.** Previously a miner could only participate in PoAW-X via the
pool; `irium-miner` built plain coinbase blocks. Two parts:

1. **Key-parameterized builder.** The all-gates block builder is generalized by
   identity. `AllGatesIdentities::dev()` reproduces the fixed devnet keys
   byte-identically (the existing harness path is unchanged). A new
   `AllGatesIdentities::solo(miner_secret)` derives every role from one miner
   secret — worker + finality member + compute/verify/support all equal the miner
   pkh, with domain-separated per-role assignment secrets. `build_solo_poawx_block`
   returns a node-acceptable all-gates block. This is proven by a `connect_block`
   test (a real miner-key block is accepted; the miner is verified to be every
   role identity).

2. **`irium-miner --poawx` mode.** Loop: fetch the block template, fetch the parent
   prev_hash, build the all-gates block with the miner key, POST the candidate
   admissions to `/poawx/candidate-admission`, and submit via
   `/rpc/submit_block_extended`.

**Gate / configuration variables.**
- `IRIUM_POAWX_MINER_SECRET_HEX` — 32-byte miner secret (64 hex chars) the solo
  miner plays all roles with.
- `IRIUM_POAWX_PUZZLE_DIFFICULTY_BITS` — receipt PoW difficulty (must match the node).
- The full PoAW-X all-gates env (activation heights + `*_REQUIRED`) must match the
  target node so the built sections validate.

**Mainnet hard-off.** Both `build_solo_poawx_block` (via `guard_network`) and the
`--poawx` runtime reject `network_id == 0`; solo PoAW-X mining is devnet/testnet
only.

**Pool requirement on testnet.** None — solo mining is an alternative to
pool-mediated mining. The solo miner simply runs with the same gate env as the
node it submits to.

**Test coverage boundary (honest).** The builder is fully unit-tested through
`connect_block`. The `--poawx` live node round-trip (template / admission POST /
submit_block_extended over RPC) cannot be covered by `cargo test --all` and is the
documented live-integration path; its request shape mirrors the proven
`poawx-live-proof-harness` submit path.

## F. Mainnet hard-off summary

Every gate in this phase set returns false when `network_id == 0`. Activation on
any non-zero network additionally requires BOTH a configured `*_ACTIVATION_HEIGHT`
AND (for enforced mechanisms) `*_REQUIRED=1` — three independent conditions, none
defaulted on. No mechanism here is, or can be, active on mainnet without an
explicit operator decision behind the pending review/testnet/audit steps below.

New gate variables introduced by this phase set:
- `IRIUM_POAWX_MULTISOURCE_SEED_ACTIVATION_HEIGHT`
- `IRIUM_POAWX_ADAPTIVE_MODE_ACTIVATION_HEIGHT`
- `IRIUM_POAWX_MINER_SECRET_HEX` (solo miner config, not a consensus gate)
- finality threshold default changed (2/3) for `IRIUM_POAWX_FINALITY_THRESHOLD_NUM/DEN`
- prerequisite fraud proof: `IRIUM_POAWX_FRAUD_PROOF_{ACTIVATION_HEIGHT,REQUIRED}`

## G. Pool coordination summary (irium-stratum)

`irium-stratum` is a separate crate (own `Cargo.lock`, depends on the node only as
a dev-dependency) and was not modified.

| Gate | Pool action needed before enabling on a pool-mined testnet |
|---|---|
| Finality 2/3 (Phase 27) | Set `FINALITY_THRESHOLD_NUM=2`/`DEN=3` in the pool env to match the node default. |
| Multi-source seed (Phase 28) | Pool must implement the identical v2 seed formula and use the same activation height, or the node rejects pool blocks. Do not enable until then. |
| Adaptive engine (Phase 29) | None (node-local, advisory). |
| Solo mining (Phase 30) | None (solo is an alternative to pool mining). |

## H. Verification

`cargo test --all -- --test-threads=1` was run after every gap on irium-vps and was
green each time (final: 15 test binaries, 0 failed, 2308 tests passed). Env-sensitive
tests require `--test-threads=1` (the project convention). With every new gate unset,
the full pre-existing suite is byte-identical, demonstrating no regression.

## I. What comes next (blueprint activation strategy)

This phase set is local technical implementation only. Per the blueprint activation
strategy, the path to any non-test network and eventually mainnet remains:

1. **External security audit** of the new consensus code — fraud-proof verification
   and slashing, the multi-source seed derivation and its committed-admission
   coupling, and the finality threshold/economics — alongside the still-pending
   audit items from earlier phases (the `vrf_fun`/`secp256kfun` true-VRF dependency,
   the finality committee logic, and candidate-admission / finality-vote gossip
   behavior on a public network).
2. **Public testnet** enabling these gates with the pool updated/configured to match
   (Phase 28 and Phase 27 require pool coordination), exercising real multi-party
   participation, cross-host P2P, and the solo `--poawx` miner.
3. **Community / governance review** of the activation parameters (activation heights,
   `*_REQUIRED` flags, finality threshold, seed epoch length).
4. **Mainnet activation decision** — a separate, explicit step that sets activation
   heights and `*_REQUIRED=1` on the mainnet network id. Until then every mechanism
   here is hard-off. No mainnet activation is claimed, scheduled, or possible without
   that decision.
