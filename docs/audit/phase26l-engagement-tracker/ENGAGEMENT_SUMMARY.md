# PoAW-X Phase 26 — Audit Engagement Summary

One-page status to move from "package prepared" to "auditor contacted / audit scheduled / findings
tracked." **No auditor has been contacted, no audit has happened, and no email has been sent.**
**NOT audited. NOT production-ready. NOT mainnet-ready.** Mainnet PoAW-X is hard-off
(`network_id == 0`); public testnet remains gated.

## Coordinates

- Repo: `https://github.com/iriumlabs/irium.git` (public)
- Branch: `testnet/poawx-phase20-blueprint-completion-local`
- Branch HEAD: **`6c7681a`** (docs); last **source** change `0208368`.
- `origin/main` unchanged at `19c496dc5f2fa08981a109b10eeb257105c28c43`.
- Full source audit range: `30bce64..0208368` (8 source files, +1006/−47).

## What is ready

- **Kickoff package** (`docs/audit/phase26h-kickoff/`) — scope, review guide, repro commands,
  deliverables, findings tracker, outreach email draft.
- **Internal self-review** (`docs/audit/phase26i-self-review/`) — 748/0 tests, phase22a byte-unchanged
  proof, 6 Informational items (not audit findings).
- **External handoff package** (`docs/audit/phase26j-external-handoff/`) — manifest, one-page summary,
  outreach message template, handoff checklist, findings-tracker send-copy.
- **Remediation workflow** (`docs/audit/phase26k-remediation-workflow/`) — triage policy, response
  lifecycle, branch policy, retest protocol, response templates, status dashboard, finding record
  template.
- **This engagement tracker** (`docs/audit/phase26l-engagement-tracker/`) — next-steps tracker,
  selection criteria, owner-actions, status page, send checklist.

## What is NOT ready

- **Auditor not chosen / not contacted** — name, company, and contact are unknown (placeholders).
- **No NDA decision, timeline, or budget** recorded.
- **No send approval** — nothing has been or will be sent without explicit approval + recipient.
- **No external findings, no retest, no sign-off** — the audit has not started.

## Audit scope

Testnet/devnet PoAW-X Phase 26 changes only: epoch-seed alignment, candidate-admission persistence,
historical-admission serving, and the phase21d/21e/22a invariants, plus the P2P DoS/replay/cache-
poisoning surface. Out of scope: mainnet, real-value rewards, governance, a live public testnet, and
the hidden-precommit/ticket/delegation paths.

## Docs to send first

1. `docs/audit/phase26j-external-handoff/SEND_READY_SUMMARY.md`
2. `docs/audit/phase26j-external-handoff/PACKAGE_MANIFEST.md`
3. `docs/audit/phase26h-kickoff/README.md` → `AUDIT_SCOPE.md` → `AUDITOR_REVIEW_GUIDE.md` →
   `REPRO_COMMANDS.md`
4. `docs/audit/phase26i-self-review/SELF_REVIEW_REPORT.md` + `AUDITOR_HANDOFF_NOTES.md` (pre-read; not
   an audit)

## The audit ask

Independently verify that the Phase 26 changes do not weaken any validation gate — phase22a unchanged,
phase21e equality still required, no block accepted without a matching validated admission, admissions
not forgeable/replayable/cross-network-reusable, persistence/serving corruption-safe and DoS-bounded,
mainnet unaffected.

## Expected deliverables

Summary report, findings with severity + exploitability, recommended fixes, retest requirements, and a
scoped sign-off / non-sign-off — per `docs/audit/phase26h-kickoff/AUDIT_DELIVERABLES.md`.

## Current claims status

- Production-ready: **no**
- Mainnet-ready: **no**
- Audited: **no**

## Explicit next action

**Choose an auditor** (see `AUDITOR_SELECTION_CRITERIA.md`), **provide their details**
(`OWNER_ACTIONS_REQUIRED.md`), then **send the kickoff package** after completing `SEND_CHECKLIST.md`
and obtaining explicit send approval. Track progress in `NEXT_STEPS_TRACKER.md`.
