# PoAW-X Phase 24H — miner↔pool onboarding & coordination layer

**Status: coordination fix implemented + tested (code/test phase). NOT production-candidate;
NOT mainnet-ready.** Local-only; not pushed; remote branch absent; `main` untouched. No live
nodes/miners run. Mainnet hard-off. The pool holds **no** miner private keys, seed phrases, or
VRF secrets. Does not replace external audit.

## Exact Phase 24G blocker addressed

In `build_synthetic_phase20_ext`, the per-role solvers came from `synth_role_solver` → the pool
`primary_pkh` for all three roles. Meanwhile the candidate set was built from the node-validated
**admitted** candidates (distinct solver per role), and
`build_pool_true_vrf_section(&role_reward)` looks up the per-role AssignmentProofV2 **by
`role_reward.solver`**. So the lookup keys (primary_pkh) never matched the admitted miners'
solvers → no V2 found → the producer failed closed → no all-gates block.

## Coordination design (preferred model — node-validated data flow)

1. Miner/wallet creates proofs (ticket, AssignmentProofV2, candidate admission, finality vote)
   — proven live in Phase 24E/24G.
2. Node validates admissions/votes (all gates incl. true-VRF) and exposes them via loopback RPC.
3. **Pool selects role solvers from the node-validated admitted candidate set** (`best_for_role`
   per role) and sets `role_reward` to those solver pkhs.
4. Pool keys every ext section (role claims, candidate set, AVR2 V2 proofs) to the **same**
   admitted solver pkhs; fail-closed if any role lacks an admitted candidate.
5. Node remains the authoritative validator (`connect_block`); the pool cannot bypass it and
   custodies no secret.

**No pool-local onboarding endpoint was needed** — node RPC (admitted candidates + finality
votes) is sufficient (preferred design B). The trust boundary is unchanged: public/proof
material only into the pool; the node validates.

## What was implemented

- `pool: coordinate role solvers from admitted candidates` (`e298752`):
  - New `pool_role_reward_from_admitted(network_id, height, prev_hash)` → derives `RoleReward`
    from the admitted candidate set (best per role); `None` (fail closed) if any role lacks an
    admitted candidate.
  - `build_synthetic_phase20_ext`: under `pool_candidate_admission_enforced`, the per-role
    solvers (and thus `role_reward`, the role claims, and the AVR2 lookup) are derived from the
    admitted candidate set instead of `primary_pkh`. Off (non-all-gates) → unchanged
    (`synth_role_solver`).
  - `build_collected_phase20_ext` already derives `role_reward` from role-protocol reveals (the
    real production path); unchanged. Coordination there = the miner submits a matching reveal +
    admission + V2 + finality vote for the same solver.

## Behavior after the fix

- **Role solver selection:** role rewards are the admitted miners' solver pkhs (compute/verify/
  support), not the pool `primary_pkh`.
- **Official 0% fee:** `fee_bps = 0`, `fee_pkh = 0`; role rewards go to the selected solver
  pkhs.
- **Third-party fee:** `fee_bps`/`fee_pkh` set (cap 2.00%, fail-closed to 0% on invalid terms);
  role-reward solvers unchanged (PRIMARY-only fee; role rewards untaxed; no hidden/delegate
  output).
- **Finality coordination:** the SUPPORT solver must correspond to a committee member; the
  bundled finality proof needs ≥threshold member-signed Commit votes (node re-validates). The
  miner must supply a finality vote whose `member_pkh` == the SUPPORT solver — a coordination
  requirement (the pool only bundles).
- **Committed admission:** still requires admitted candidates for H+1 (pool bundles the
  committed root; node validates with activation grace).
- **Dominance/fairness:** weights deterministic; node validates against persisted parent state.
- **Secret safety:** the pool change adds no secret-handling; the only `secret` symbols are the
  pre-existing synthetic role-claim nonce and the test's VRF prove-input. The pool holds no
  miner private key / seed / VRF secret.

## Tests

- `delegation::phase24h_role_reward_derived_from_admitted_candidates` (new): admitted
  candidates with distinct solvers per role ⇒ `role_reward` == those solvers (≠ `primary_pkh`);
  AVR2 bundled; role claim + candidate set agree with `role_reward`; official fee-0; third-party
  fee (PRIMARY-only, role rewards untaxed); fail-closed when a role's admitted candidate is
  missing; helper fail-closed on empty cache.
- Existing coverage (unchanged) for the rest of section H: node accepts a block whose
  `role_reward.solver` matches the admitted candidates (`chain::phase22e_true_vrf_e2e_block`);
  rejects mismatched selected solver (`phase21d_candidate_set_enforcement`) and wrong score
  (`phase22e_wrong_candidate_score_rejects`); fee/reward paths (`phase20`, `reward`,
  `delegation`); fail-closed missing V2/ticket/puzzle/finality (`phase22d/22e`, per-section
  phase21 tests); wallet emits onboarding-compatible material with no secret leak
  (`phase22e_candidate_admission_v2_emit_and_submit`, `phase24f`).

## What remains for Phase 24I (live mined block)

The coordination layer now lets the pool key role rewards + V2 proofs to the admitted miners.
A real live all-gates block additionally needs, in one coordinated run:
1. the miner supplying matching material for **all** of: candidate admission (3 roles), V2
   proofs, ticket, the **SUPPORT** finality vote (member == support solver), role precommit/
   reveal (for the non-synthetic claim path), and H+1 admissions (committed-admission section);
2. dominance weights consistent with the node's genesis/parent state;
3. live PoW mining via cpuminer → `/rpc/submit_block_extended` → `connect_block`.

The **synthetic** producer path remains disallowed for the live success claim; Phase 24I must
use the collected/real path with the coordinated material above.

## Status

- **Production-candidate? NO.** **Mainnet-ready? NO.**
- Remaining blockers: real live mined all-gates block; cross-host P2P provider/firewall;
  independent audit; public testnet; governance/mainnet activation.
