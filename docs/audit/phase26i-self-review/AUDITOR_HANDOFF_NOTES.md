# PoAW-X Phase 26 — Auditor Handoff Notes (from Phase 26I self-review)

**Disclaimer: this is an INTERNAL self-review by the code authors, NOT an independent audit.** Nothing
here is a sign-off. The system is **NOT audited, NOT production-ready, NOT mainnet-ready**. These notes
exist to make the independent reviewer's job faster, not to pre-empt their conclusions.

## What the self-review did

Dry-ran the Phase 26H kickoff package end to end on branch
`testnet/poawx-phase20-blueprint-completion-local` (HEAD `1217c85`, source `0208368`): re-read the
scope, verified the diff ranges, proved phase22a unchanged, confirmed PoW/LWMA/reward/constants
untouched, ran the focused + full serialized test suites and the release build, and read the key
serving/reload/gate code paths.

## What the self-review found (summary)

- **No Critical/High issues.** No change that weakens a gate, alters consensus, or breaks sync.
- phase22a (`validate_block_committed_admission`) is **byte-identical** across `30bce64..0208368`.
- The phase21d/21e change is narrow: the *expected seed value* moved from `block.header.prev_hash` to
  `admission_epoch_seed(...)` (grandparent hash; genesis at the boundary). The **phase21e equality
  check is unchanged**.
- Persistence (`reload_persisted_bytes`) and serving (receiver `ingest_bytes`) **re-validate** every
  admission; serving is **bounded** (`16 × block_count`, early-return).
- Mainnet stays hard-off (`network_id == 0`).
- `cargo test --lib -- --test-threads=1` → **748 / 0**; both release binaries build.
- Six Informational observations recorded in `INTERNAL_FINDINGS.md`; four are flagged
  **Needs Auditor Review** (they are properties/limitations, not confirmed defects).

## What auditors should prioritize

1. **Independently re-derive the change surface** from history (don't trust the stated
   `30bce64..0208368` / "8 files, +1006/−47" — confirm it).
2. **phase22a-unchanged premise** — re-verify the function body diff yourself; the whole design rests
   on phase21d/21e being reconciled to phase22a, not phase22a being weakened.
3. **Epoch-seed correctness** (`admission_epoch_seed`) — walk H1 (activation/genesis boundary), H2,
   and H≥3 by hand; check whether a block producer can influence its own candidate-set seed
   (grandparent hash) in a useful way. (See `INTERNAL_FINDINGS.md` SR-001.)
4. **No-bypass property** — confirm persistence/serving cannot cause a block to connect without a
   matching, validated admitted set (phase21e). The negative tests (`phase26d`/`phase26e`) assert this;
   challenge them.
5. **Admission validation completeness** — forge/tamper/replay/cross-network admissions; confirm
   `validate` + network/seed/height binding reject them on both the live and reload paths.
6. **DoS surface** — the serving cap is `16 × block_count`; assess whether that constant is justified
   against the max legitimate admissions-per-height and under sustained getblocks pressure
   (`INTERNAL_FINDINGS.md` SR-002), plus window=64 / deep-sync behavior (SR-003) and the transient
   getblocks stall (SR-004).
7. **Crypto primitives** — the self-review treated signatures/digests/VRF outputs as opaque; an auditor
   with crypto depth should examine them.

## Where the evidence lives

- External delivery wrapper: `docs/audit/phase26j-external-handoff/` — `PACKAGE_MANIFEST.md` (full
  file list + read order), `SEND_READY_SUMMARY.md`, `AUDITOR_OUTREACH_MESSAGE.md`,
  `AUDITOR_HANDOFF_CHECKLIST.md`, `EXTERNAL_FINDINGS_TRACKER_COPY.md`.
- This folder (`docs/audit/phase26i-self-review/`): `SELF_REVIEW_REPORT.md`, `REPRO_EVIDENCE.md`,
  `INTERNAL_FINDINGS.md`, and these notes.
- Kickoff package: `docs/audit/phase26h-kickoff/` (`README.md`, `AUDIT_SCOPE.md`,
  `AUDITOR_REVIEW_GUIDE.md`, `REPRO_COMMANDS.md`, `FINDINGS_TRACKER.md`, `AUDIT_DELIVERABLES.md`,
  `AUDIT_KICKOFF_EMAIL_DRAFT.md`).
- Background package: `docs/audit/poawx-phase26-independent-audit-package.md`,
  `...-technical-appendix.md`, `...-auditor-checklist.md`.
- Per-phase implementation + (summarized) live results: `docs/poaw-x-phase26{b,c,d,e}-*.md`.
- The auditor should record findings in `docs/audit/phase26h-kickoff/FINDINGS_TRACKER.md` (the external
  tracker), keeping this folder's `INTERNAL_FINDINGS.md` as the internal pre-read.

## Known limitations (carried in, not resolved here)

- Self-review is **not independent** (single reviewer = author).
- No live, adversarial, multi-operator, or scale testing in this phase.
- phase21e propagation-sensitivity is a documented, pre-existing property.
- Admission window = 64; deep-chain/scale sync unproven beyond small devnet runs.
- Live results referenced (26C/26D/26E) are summarized from prior phases, not re-run here.

## Disclaimer (restated)

This handoff and the accompanying self-review are internal. They do **not** assert that the code is
audited, production-ready, or mainnet-ready. An independent audit with a scoped sign-off (per
`docs/audit/phase26h-kickoff/AUDIT_DELIVERABLES.md`) is the prerequisite for any such claim and for any
public-testnet launch.
