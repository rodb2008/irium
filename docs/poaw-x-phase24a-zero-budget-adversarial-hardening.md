# PoAW-X Phase 24A — zero-budget adversarial hardening & all-gates validation

**Internal hardening pass by Claude Code. This does NOT replace an independent external audit
(see `docs/audit/poaw-x/` + `docs/poaw-x-phase23a-true-vrf-internal-security-review.md`).** It
is a deep local adversarial review + test-hardening step before any public testnet. Local-only
(branch `testnet/poawx-phase20-blueprint-completion-local`); not pushed; remote branch absent;
`main` untouched. **Mainnet hard-off; not mainnet-ready.**

## Scope

The full PoAW-X local implementation (Phase 20 → 23B): multi-role rewards + 55/22/13/10 split,
fees, delegation, hidden precommit, role precommit/reveal + gossip, tickets/Sybil, penalty,
anti-domination, candidate set, candidate admission + chain-committed admission, true-VRF
`AssignmentProofV2`, puzzle work modes, finality committee + vote gossip, wallet emit helpers,
pool mirrors, node-authoritative validation, and all mainnet-hard-off gates.

## Why this is not an external audit

It is performed by the same agent that wrote the code, without independent adversarial
incentive or formal verification, and does not audit the upstream `vrf_fun`/`secp256kfun`
crates. An external audit remains REQUIRED before any public/non-test/mainnet use.

## Gate matrix (all default-off; `network_id == 0` hard-off; each tested)

| Mechanism | Activation env | Required env | File |
|---|---|---|---|
| Multi-role reward | `…MULTI_ROLE_REWARD_ACTIVATION_HEIGHT` | (activation-only) | `poawx.rs`/`chain.rs` |
| Fairness matrix | `…FAIRNESS_MATRIX_ACTIVATION_HEIGHT` | (activation-only) | `poawx.rs` |
| Delegation | `…DELEGATION_ACTIVATION_HEIGHT` | — | `poawx.rs` |
| Third-party fee | `…THIRD_PARTY_FEE_ACTIVATION_HEIGHT` | `…THIRD_PARTY_POOL_MODE` | `poawx.rs`/`chain.rs` |
| Hidden precommit | `…HIDDEN_PRECOMMIT_ACTIVATION_HEIGHT` | — | `poawx.rs`/`chain.rs` |
| Role gossip | `…ROLE_GOSSIP_ENABLED` (+role protocol) | — | `pool/…/delegation.rs` |
| Tickets / Sybil | `…TICKETS_ACTIVATION_HEIGHT` | `…TICKETS_REQUIRED` | `poawx_ticket.rs` |
| Penalty | `…PENALTY_STATE_ACTIVATION_HEIGHT` | `…PENALTY_STATE_REQUIRED` | `poawx_penalty.rs` |
| Anti-domination | `…ANTI_DOMINATION_ACTIVATION_HEIGHT` | `…ANTI_DOMINATION_REQUIRED` | `poawx_dominance.rs` |
| Candidate set | `…CANDIDATE_SET_ACTIVATION_HEIGHT` | `…CANDIDATE_SET_REQUIRED` | `poawx_candidate.rs` |
| Assignment proof (V1) | `…ASSIGNMENT_PROOF_ACTIVATION_HEIGHT` | `…ASSIGNMENT_PROOF_REQUIRED` | `poawx_candidate.rs` |
| Candidate admission | `…CANDIDATE_ADMISSION_ACTIVATION_HEIGHT` | `…CANDIDATE_ADMISSION_REQUIRED` | `poawx_admission.rs` |
| Committed admission | `…COMMITTED_ADMISSION_ACTIVATION_HEIGHT` | `…COMMITTED_ADMISSION_REQUIRED` | `poawx_committed_admission.rs` |
| Puzzle work | `…PUZZLE_WORK_ACTIVATION_HEIGHT` | `…PUZZLE_WORK_REQUIRED` | `poawx_puzzle.rs` |
| Finality committee | `…FINALITY_COMMITTEE_ACTIVATION_HEIGHT` | `…FINALITY_COMMITTEE_REQUIRED` | `poawx_finality.rs` |
| Finality gossip | `…FINALITY_GOSSIP_ACTIVATION_HEIGHT` | `…FINALITY_GOSSIP_REQUIRED` | `poawx_finality.rs` |
| **True VRF** | `…TRUE_VRF_ACTIVATION_HEIGHT` | `…TRUE_VRF_REQUIRED` | `poawx_candidate.rs`/`chain.rs` |

(All env vars are prefixed `IRIUM_POAWX_`.) Confirmed: each `*_ACTIVATION_HEIGHT` defaults to
None (off); each `*_REQUIRED` defaults to false; **44** `network_id == 0` hard-off guards across
node + pool; each gate has a `*_gate(0,…) == false` / mainnet-hard-off test. Activation requires
three independent conditions (network ≠ 0 + activation height + required flag).

## Adversarial test categories & results

All categories below are covered (pre-existing + Phase 23A/24A additions). **All green.**

1. **True VRF / AssignmentProofV2** — malformed/short/long proof, wrong output/proof/key/role/
   height/network/ticket/seed/solver, V1-only-when-V2-required, committed-root changes with the
   VRF output, wallet secret never printed. ✅
2. **Candidate / committed admission** — missing/extra candidate, wrong committed root, reorg
   admission-root differences, activation grace, malformed trailing wire, malformed CAC1. ✅
3. **Tickets / Sybil / penalty** — expiry/role/pkh binding, Sybil threshold, penalty status,
   malformed/short ticket wire (no panic). ✅
4. **Dominance / anti-domination** — apply/disconnect/reorg rollback, third-party fee not
   credited as worker reward, deterministic fairness weight, no floats in consensus. ✅
5. **Puzzle work modes** — wrong mode/solution/challenge, malformed PZL1, bounded memory, fast
   verify, finality placeholder not accepted when the finality committee is required. ✅
6. **Finality committee / vote gossip** — duplicate/forged/wrong-member/wrong-block votes,
   threshold, malformed vote + proof (truncated header/body, trailing junk, count overflow),
   cache dedupe/prune, pool bundles only member-signed votes (cannot sign). ✅
7. **Pool/wallet/node trust boundaries** — pool holds no VRF secret + cannot prove, node
   re-validates all bundled proofs, wallet emit-only, loopback-only RPC, mainnet disabled. ✅

### New in Phase 24A (test-only)
Deterministic malformed-wire corpus for CAC1 (committed admission), finality vote + proof,
PZL1 (puzzle solution), and tickets — confirming bounded deserialization rejects
truncated/oversized/empty/count-overflow input without panics. Complements the Phase 23A corpus
(AVR2 ext section, candidate-admission trailing wire, V2 proof, pool mirror).

## Static safety scan

- **No `unsafe`** in any `poawx_*` module.
- **No wall-clock** (`SystemTime`/`Instant`/`UNIX_EPOCH`/`now()`) in the PoAW-X modules.
- **No `HashMap`/`HashSet`** in PoAW-X modules — deterministic `BTreeMap`/`BTreeSet` + explicit
  sorts only.
- **No floats in consensus** — the only `f64` is a `#[cfg(test)]` distribution test.
- **`unwrap`/`expect`:** in non-test PoAW-X paths only two `expect`s remain (the finality vote
  signer `sign_prehash` and `PuzzleMode::from_id(i % 5)`), both provably non-failing. The
  ticket deserialize `rd(..).try_into().unwrap()` calls are guarded by upfront length checks
  (`b.len() < 145` with per-branch bond/tail checks; `TicketProof` exact-length), so the slices
  and conversions are always in-bounds — no DoS.
- **Bounded deserialization:** every wire type enforces an exact `*_WIRE` length or a
  header + bounded-count length (finality proof caps `count > FINALITY_MAX_VOTES`), with
  `*_MAX_BYTES`/`*_CAP` anti-oversize caps on gossip ingest.

## Dependency safety scan

`cargo tree | grep -Ei 'openssl|secp256k1-sys|bindgen|native-tls'` → **NONE**. `ring` is the
pre-existing rustls TLS backend (not pulled by the VRF crates). The pool crate has **no VRF
dependency** (`vrf_fun`/`secp256kfun` absent from `pool/irium-stratum/Cargo.toml`). No new
native crypto dependency.

## Verification results

fmt clean (lib + pool). `cargo test --lib poawx` → 132 (single-thread AND parallel); phase20
33; reward 9; wallet poawx 6; wallet 428; iriumd bin 256; pool 96 (phase20 21, delegation 42,
native_rewardable 6).

## Bugs found / fixed

**None.** No Critical/High/Medium issues surfaced; no consensus code changed (test-only +
docs). The malformed-wire corpus confirmed the existing bounded-deserialization is correct.

## Remaining risks

- **No paid independent audit** has been performed.
- **Public testnet** has not been run (admission/finality public-network propagation untested
  at scale).
- **Governance / mainnet activation** still pending.
- `vrf_fun`/`secp256kfun` remain pre-1.0 and unaudited upstream.

## Ready for Phase 24B (two-VPS all-gates rehearsal)?

**Yes — for a LOCAL/controlled two-VPS rehearsal only** (all gates enabled on a non-mainnet
network, no public exposure, mainnet/prod untouched). This is the right next step to exercise
real P2P admission/finality propagation before any external audit or public testnet. It is NOT
a substitute for the external audit, and must not touch mainnet/prod or bind public ports.
