# PoAW-X Phase 26 — External Auditor Handoff: Package Manifest

The exact set of materials to deliver to an independent reviewer. **NOT audited. NOT production-ready.
NOT mainnet-ready.** Mainnet PoAW-X is hard-off (`network_id == 0`). No public testnet has launched.

## Coordinates

- Repository: `https://github.com/iriumlabs/irium.git` (public)
- Branch: `testnet/poawx-phase20-blueprint-completion-local`
- Branch HEAD: **`22dfde8`** (docs)
- Last **source** change: **`0208368`**
- `origin/main` unchanged at `19c496dc5f2fa08981a109b10eeb257105c28c43`
- Full source audit range: **`30bce64..0208368`** (8 source files, +1006/−47; rest is tests + docs)

## Phase commit map (source)

| Change | Range | Code commit |
|--------|-------|-------------|
| 26B epoch-seed reconciliation | `30bce64..081a1bd` | `081a1bd` |
| 26D admission persistence | `bfe16fd..abb2fd3` | `de13a83` |
| 26E historical-admission serving | `abb2fd3..0208368` | `9de939f` |

## Send these first (in order)

1. `docs/audit/phase26j-external-handoff/SEND_READY_SUMMARY.md` — one-page orientation.
2. `docs/audit/phase26h-kickoff/README.md` → `AUDIT_SCOPE.md` → `AUDITOR_REVIEW_GUIDE.md` →
   `REPRO_COMMANDS.md`.
3. `docs/audit/phase26i-self-review/SELF_REVIEW_REPORT.md` + `AUDITOR_HANDOFF_NOTES.md` (internal
   pre-read — explicitly **not** an audit).

## Full file list to hand over

**Phase 26J external handoff (this folder):**
- `PACKAGE_MANIFEST.md` (this file)
- `AUDITOR_OUTREACH_MESSAGE.md`
- `AUDITOR_HANDOFF_CHECKLIST.md`
- `EXTERNAL_FINDINGS_TRACKER_COPY.md`
- `SEND_READY_SUMMARY.md`

**Phase 26H kickoff package:**
- `README.md`, `AUDIT_SCOPE.md`, `AUDITOR_REVIEW_GUIDE.md`, `FINDINGS_TRACKER.md`,
  `AUDIT_KICKOFF_EMAIL_DRAFT.md`, `AUDIT_DELIVERABLES.md`, `REPRO_COMMANDS.md`

**Phase 26I self-review package:**
- `SELF_REVIEW_REPORT.md`, `REPRO_EVIDENCE.md`, `INTERNAL_FINDINGS.md`, `AUDITOR_HANDOFF_NOTES.md`

**Background package:**
- `docs/audit/poawx-phase26-independent-audit-package.md`
- `docs/audit/poawx-phase26-technical-appendix.md`
- `docs/audit/poawx-phase26-auditor-checklist.md`

**Per-phase implementation + (summarized) live results:**
- `docs/poaw-x-phase26b-*.md`, `docs/poaw-x-phase26c-*.md`, `docs/poaw-x-phase26d-*.md`,
  `docs/poaw-x-phase26e-*.md`

**Public-testnet plan (context only; separately gated):**
- `docs/poaw-x-phase26g-public-testnet-readiness.md` (+ rollout-checklist / risk-register /
  operator-runbook)

## Recommended read order (for the auditor)

1. `SEND_READY_SUMMARY.md` → `phase26h-kickoff/README.md` → `AUDIT_SCOPE.md`.
2. `AUDITOR_REVIEW_GUIDE.md` (invariants, threat model, attacks).
3. `phase26i-self-review/AUDITOR_HANDOFF_NOTES.md` (priorities) + `INTERNAL_FINDINGS.md`.
4. Code via `REPRO_COMMANDS.md` (checkout → diff ranges → tests → build).
5. Record findings in the tracker (below).

## Non-live reproduction commands

- `docs/audit/phase26h-kickoff/REPRO_COMMANDS.md` (checkout, diff ranges, focused + full serialized
  tests, release build). All non-live; no VPS/firewall/sudo/secret commands.
- Self-review evidence of these same commands already run: `docs/audit/phase26i-self-review/REPRO_EVIDENCE.md`.

## Findings tracker

- External (auditor writes here): `docs/audit/phase26h-kickoff/FINDINGS_TRACKER.md`, with a clean
  send-copy at `docs/audit/phase26j-external-handoff/EXTERNAL_FINDINGS_TRACKER_COPY.md`.
- Internal pre-read (already populated, Informational only):
  `docs/audit/phase26i-self-review/INTERNAL_FINDINGS.md`.

## Engagement tracking

To go from "package prepared" to "auditor contacted / scheduled / findings tracked," see
`docs/audit/phase26l-engagement-tracker/` — `ENGAGEMENT_SUMMARY.md`, `NEXT_STEPS_TRACKER.md`,
`AUDITOR_SELECTION_CRITERIA.md`, `OWNER_ACTIONS_REQUIRED.md`, `AUDIT_ENGAGEMENT_STATUS.md`,
`SEND_CHECKLIST.md`. No auditor has been contacted and no message has been sent.

## Disclaimer

This package is an outreach/handoff bundle only. It is **not** an audit and asserts no sign-off.
"Audited" may only be claimed after an independent reviewer issues a scoped sign-off per
`docs/audit/phase26h-kickoff/AUDIT_DELIVERABLES.md`. No secrets, keys, wallet data, machine
credentials, or raw private logs are included.
