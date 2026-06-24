# PoAW-X Phase 2 — Simulation Findings

**Status: a headless, devnet-only simulation of the 10 blueprint scenarios against
the REAL consensus machinery.** Not a mainnet/production component; mainnet
hard-off throughout (`IRIUM_NETWORK=devnet`, `network_id=2`). The simulation is a
verification tool, not a claim of audit or production-readiness.

## What the simulation proves

`src/bin/poawx-simulation.rs` runs all 10 scenarios in-process with no live node.
It builds a real `ChainState` exactly like the node (`load_locked_genesis` ->
`block_from_locked` -> `ChainParams` -> `ChainState::new`) and drives the real
consensus code:

- **Scenarios 1, 2, 6, 7, 8** run end-to-end through `ChainState::connect_block` /
  `process_block`, with blocks built by the proven
  `poawx_mining_harness::build_solo_poawx_block`.
- **Scenarios 3, 4, 5, 9, 10** exercise the exact consensus primitives
  `connect_block` itself calls — `PersistentDominance` + `effective_score`,
  `TicketProof::validate` + `meets_sybil_target`, and `resolve_epoch_seed_parts`.
  The block builder produces single-identity, single-candidate-per-role blocks, so
  competitive selection, heterogeneous-miner dominance, bad-ticket rejection and
  multi-height multisource cases cannot be expressed as a connectable block; the
  primitives are the real consensus functions, not reimplementations.

## How to run

```
cargo run --bin poawx-simulation
```

Each scenario prints a PASS / FAIL / INFO verdict with quantitative evidence,
followed by a structured report. The process exits `0` iff every required scenario
passes; INFO findings (scenario 6) do not fail the run. Latest run: **9 pass, 0
fail, 1 info, exit 0.**

## Passing scenarios (quantitative results)

| # | Scenario | Mechanism exercised | Result |
|---|---|---|---|
| 1 | Normal mining | `connect_block`, 100 blocks, 4 miners | 100 blocks connected; every block split exactly 5500/2200/1300/1000 bps (55/22/13/10); 0 bad splits; adaptive mode = normal |
| 2 | Low participation | `connect_block`, 1 miner, 40 blocks | `ChainState::adaptive_mode()` == Caution (active_miner_count = 1 < 3) |
| 3 | One dominant miner | `PersistentDominance` + `effective_score`, 200 blocks | dominant builds 90% -> recent reward share = 900 permille; anti-domination weight 526 vs fresh 1000 (reduced 47%); real `effective_score` 526 < 1000 |
| 4 | One dominant pool | same engine, single pool identity | identical: share 900 permille, weight reduced 47%, effective_score reduced |
| 5 | Sybil attacker | `TicketProof::validate` + `meets_sybil_target`, 50 ids @ 18 required bits | 50/50 minimal-PoW tickets rejected ("insufficient sybil work"); a properly-mined control ticket accepted |
| 7 | No ASIC participation | `connect_block`, CPU/GPU-labeled miners | chain advanced to height 20 (consensus has no hardware-class gating; neutrality = nothing blocks any class) |
| 8 | No GPU participation | `connect_block`, CPU/ASIC-labeled miners | chain advanced to height 20 |
| 9 | Randomness manipulation | `resolve_epoch_seed_parts` (multisource gate on) | multi-source seed differs from the grindable grandparent-only seed; changing ONLY the committee finality digest changes the seed -> a consecutive-block proposer cannot bias it alone |
| 10 | Reward fairness | `PersistentDominance`, 1000 blocks, 5 hashrates | hashrates 30/25/20/15/10%; measured reward shares 300/250/200/150/100 permille; worst deviation from hashrate = 0 permille; max share 300 permille (< 700 Defense line) |

## Scenario 6 finding (INFORMATIONAL): no finality-checkpoint reorg protection

Scenario 6 builds a real main chain (h1, h2, h3 — where h3 carries a finality proof
finalizing h2) and a competing fork diverging at h1, then feeds the fork through the
real `process_block` fork-choice path.

Observed:

- **Work-monotonic reorg IS enforced (PASS sub-check):** an equal-work competing
  fork (height 3 == main height 3) did NOT reorg the chain.
- **FINDING:** a heavier fork (height 4 > main height 3) **reorganized past the
  finalized block.** The real `reorg_to_tip` ran and disconnected 2 blocks
  (including the finalized h2), replacing them with the fork.

Root cause: fork choice in `ChainState::process_block` / `reorg_to_tip` is purely
**cumulative proof-of-work** (`cumulative > total_work`). The PoAW-X finality
committee proof finalizes a block for the finality *gate*, but nothing makes the
chain refuse a heavier reorg that undoes a finalized block. The only reorg
immutability that exists today is the separate **anchor checkpoint** mechanism
(`verify_block_against_anchors`), which is hardcoded operator checkpoints, not
PoAW-X finality.

This is exactly the kind of gap a simulation is meant to surface: it is recorded
as an INFORMATIONAL finding (it does not fail the run) because the blueprint
requirement ("reorg past a finality checkpoint fails") is not yet met by the code.

## Required before Phase 3 devnet

**Finality-checkpoint reorg protection.** Before a Phase 3 devnet that relies on
finality, `process_block` / `reorg_to_tip` must refuse any reorg whose fork point
is at or below the deepest finalized height. Sketch:

1. Track the deepest finalized height as accepted blocks carry valid finality
   proofs (a finalized-height watermark on `ChainState`, advanced when a block's
   finality proof meets the committee threshold for its parent).
2. In `reorg_to_tip` (and the `process_block` reorg branch), reject a reorg when
   the common-ancestor height is below the finalized watermark, regardless of the
   fork's cumulative work — fail closed.
3. Make the watermark reorg-safe (revert on disconnect) and replay-deterministic,
   mirroring the persistent dominance/penalty pattern.
4. Add a connect_block/process_block test that reproduces scenario 6 and asserts
   the heavier fork is rejected once the finalized watermark covers the fork point;
   then flip scenario 6 in this harness from INFO to a PASS.

Until that lands, finality on PoAW-X is advisory with respect to reorgs: the
heaviest-work chain still wins.
