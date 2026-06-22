# PoAW-X Phase 26 — Internal Self-Review Report (Phase 26I)

**This is an INTERNAL self-review, NOT an independent audit.** It does not constitute a sign-off and
must not be read as one. **NOT audited. NOT production-ready. NOT mainnet-ready.** Mainnet PoAW-X
remains hard-off (`network_id == 0`).

The author of this review is the same party that wrote the code; it cannot substitute for independent
review. Its purpose is to dry-run the Phase 26H kickoff package as an auditor would, surface anything
obvious before external engagement, and hand off cleanly.

## Baseline / scope

- Repo: `https://github.com/iriumlabs/irium.git`
- Branch: `testnet/poawx-phase20-blueprint-completion-local`
- Branch HEAD at review: `1217c85` (docs). Last **source** change: `0208368`.
- `origin/main` unchanged at `19c496dc5f2fa08981a109b10eeb257105c28c43`.
- Source under review: `30bce64..0208368` — 8 source files, +1006/−47.
- Scope: the Phase 26 changes only (26B epoch-seed reconciliation, 26D admission persistence, 26E
  historical-admission serving). Out of scope: mainnet, governance, real-value rewards, a live public
  testnet, and the hidden-precommit/ticket/delegation paths (unchanged here).

## Commands run (all non-live; see `REPRO_EVIDENCE.md` for outputs)

- Repo state: `git fetch` / checkout / `pull --ff-only` / `rev-parse HEAD` / `ls-remote origin main` /
  `status --short`.
- Diff ranges: `git diff --stat 30bce64..0208368 -- 'src/*.rs'`.
- phase22a unchanged proof: extracted `validate_block_committed_admission` from `30bce64` and `0208368`
  and diffed (identical).
- Consensus-params proof: `git diff --name-only 30bce64..0208368 | grep -iE 'pow|lwma|difficulty|target|reward|constants'` (no matches).
- Focused tests: `phase26b_multiblock_epoch_seed_soak`, all `phase26b`, `phase26d`, `phase26e`.
- Full suite: `cargo test --lib -- --test-threads=1`.
- Release build: `cargo build --release --bin iriumd --bin poawx-live-proof-harness`.
- Targeted source reads of `send_historical_admissions`, the receiver handler, `reload_persisted_bytes`,
  and `candidate_admission_gate`.

## Claims verified (within this internal, non-independent review)

| # | Claim | Result | Evidence |
|---|-------|--------|----------|
| 1 | phase22a (`validate_block_committed_admission`) unchanged in range | **Confirmed** | 90-line function body byte-identical across `30bce64..0208368` |
| 2 | No PoW/LWMA/difficulty/target/reward/constants change | **Confirmed** | no such files in `git diff --name-only` of the range |
| 3 | phase21d still enforced | **Confirmed** | seed/height/canonical/dominance checks present; only the *expected seed value* changed to `epoch_seed` |
| 4 | phase21e equality still required | **Confirmed** | `cs.serialize() != admitted.serialize()` → reject; unchanged except the lookup is keyed on `epoch_seed` |
| 5 | Admissions re-validated on reload | **Confirmed** | `reload_persisted_bytes`: network check + `adm.validate(...)` + conflicting-digest reject |
| 6 | Admissions re-validated on fresh sync | **Confirmed** | receiver `PoawxCandidateAdmission` handler ingests via `ingest_bytes` (full validation) |
| 7 | Historical-admission serving bounded | **Confirmed** | `cap = block_count * 16` (saturating); early-return on `sent >= cap`; only on block-serve responses |
| 8 | Mainnet PoAW-X hard-off | **Confirmed** | `candidate_admission_gate`/committed gate return false for `network_id == 0` |
| 9 | No block acceptance without matching admissions | **Confirmed** | phase21e equality preserved; `phase26d`/`phase26e` negative tests reject an empty/absent admission set |
| 10 | Builders are not validators (mainnet-hard-off) | **Confirmed (by inspection)** | builder gates hard-off; node re-validates every block in `connect_block` |
| 11 | Epoch-seed helper purely additive | **Confirmed** | `poawx_committed_admission.rs` diff = only `admission_epoch_seed` added; `AdmissionCommitmentV1` unchanged |

**Test result:** `cargo test --lib -- --test-threads=1` → **748 passed / 0 failed**. Release build of
both binaries succeeded (exit 0).

## Claims NOT independently verified (require the external auditor)

- **Cryptographic soundness** of admission signatures/digests and VRF outputs — this review treats
  them as opaque validated values; it does not analyze the primitives.
- **Adversarial / multi-operator / scale behavior** — no live or adversarial run was performed (this
  phase is non-live). The 26C/26D/26E live results are summarized from prior phases, not re-run here.
- **The pre-existing phase21e propagation-sensitivity property** ("best among candidates admitted to
  THIS node in the window") — unchanged by Phase 26, but its security implications are an auditor call.
- **DoS surface under real network load** — the send is bounded in code, but real-world rate/throughput
  bounds were not load-tested here.
- **Completeness of "8 files / +1006/−47" as the entire attack surface** — verified against the stated
  range, but an auditor should independently re-derive the range from history.

## Issues found

No Critical or High issues found in this internal review. No issue was found that weakens a validation
gate, alters consensus, or breaks sync; therefore the Phase 26I stop-and-report condition was **not**
triggered and no source change is proposed. Lower-severity observations and items that warrant
independent eyes are recorded in `INTERNAL_FINDINGS.md` (all Informational / Needs-Auditor-Review).

## Limitations of this review

- Single-reviewer, same author as the code — **not independent**; confirmation bias is unmitigated.
- Static + test-based only; no live, adversarial, or scale testing.
- VRF/crypto primitives treated as opaque.
- Reproduces the stated diff range rather than independently re-deriving the full change surface.

## Final self-review verdict

Within the bounds of an **internal, non-independent** review, the Phase 26 changes appear consistent
with their stated premise: they change admission **availability** (persistence + serving) and the
**expected seed value** for phase21d/21e, while leaving the phase21e equality gate and phase22a
unchanged, and leaving PoW/LWMA/difficulty/target/reward and mainnet behavior untouched. All 748 lib
tests pass and both release binaries build. **This is not an audit and is not a sign-off.** The code is
ready to hand to an independent auditor; see `AUDITOR_HANDOFF_NOTES.md`.
