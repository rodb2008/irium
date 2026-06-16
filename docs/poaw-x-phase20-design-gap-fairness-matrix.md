# PoAW-X Phase 20 — Design Gap: CPU/GPU/ASIC Fairness Matrix (BLOCKED)

**Status:** BLOCKED on consensus parameters. Not implemented — implementing it now would
require inventing consensus-critical rules the repo does not define.

## Current state (repo ground truth)
- The PoAW-X assignment carries a `lane`. Only **`lane="cpu"`** is implemented end-to-end
  (`EXPECTED_LANE_FIRST=b'c'` in `pool/irium-stratum/src/delegation.rs`; `gpu` is rejected
  in tests). There is **no GPU lane, no ASIC lane**.
- There is **no commit-reveal / hidden-assignment scheme** in code: the assignment
  (`/poawx/assignment`) exposes `seed`, `commitment_nonce`, `puzzle_difficulty`, `lane`
  deterministically per height. It is deterministic and verifiable, but not *hidden before
  reveal*.
- Block difficulty is automatic via **LWMA-144** (untouched, not part of this matrix).

## What the blueprint requires (from the task) vs what is undefined
Required properties: no hardware class mandatory; no permanent dominance; assignments
unpredictable before reveal and verifiable after; cheap node verification; stock-CPU path
stays valid; current mode-1 path must not regress.

**Undefined consensus parameters (must be specified by the owner before code):**
1. **Lane set & identity** — exact lanes (cpu/gpu/asic?), their on-wire encoding, and how a
   miner/delegation is bound to a lane.
2. **Per-lane eligibility rule** — which lane(s) are eligible at a given height, and the
   deterministic function `(height, seed) → eligible lane(s)`.
3. **Hidden-assignment / reveal scheme** — if assignments must be *hidden before reveal*, the
   commitment scheme (what is committed, when revealed, how peers verify), since none exists.
4. **Distribution / anti-dominance target** — the intended long-run share per class and the
   mechanism that enforces "no permanent dominance" (rotation? quota? difficulty per lane?).
5. **Interaction with reward split** — whether lane/class affects payout (ties into the
   multi-role split design gap).

## Constraints that any implementation MUST preserve
- Mainnet mode-1 stays hard-disabled until explicit activation (§K).
- Stock CPU mining path remains valid; the proven mode-1 single-CPU path must not regress.
- Node verification stays O(1)-ish per block (cheap).
- Testnet-gated; fail-closed on malformed lane/assignment.

## What CAN be done now without inventing consensus
- A **simulation harness** (non-consensus, off-chain) that models lane eligibility and
  distribution over many `(height, seed)` values **once the rules in items 1–4 are defined**.
  Building it now would mean simulating an undefined spec, so it is deferred.
- The existing assignment **determinism/verifiability** for the CPU lane is already covered by
  lib `poawx` tests (irx1 root, message hash, assignment height/lane mismatch fail-closed).

## Decision needed
Provide items 1–5 above (exact lane set, eligibility function, reveal scheme if any,
distribution target, and reward interaction). With those, this becomes a normal
testnet-gated implementation + the simulation harness + consensus tests, following the proven
mode-1 pattern. Until then: **BLOCKED, CPU-lane-only remains the supported path.**
