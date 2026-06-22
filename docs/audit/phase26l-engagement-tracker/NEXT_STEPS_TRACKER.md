# Audit Engagement — Next-Steps Tracker

Living checklist to take PoAW-X Phase 26 from "package prepared" to "audit complete." Update Status and
Due as you go. **No auditor contacted yet; no audit has happened; no email sent.** **NOT audited /
production-ready / mainnet-ready.**

Status values: `Not started` · `In progress` · `Blocked (needs owner input)` · `Done` · `N/A`
Owner key: `Owner` = project owner/user · `Project` = engineering · `Auditor` = external reviewer

| # | Action | Owner | Status | Due | Notes |
|---|--------|-------|--------|-----|-------|
| 1 | Choose auditor / reviewer | Owner | Blocked (needs owner input) | `[date]` | See `AUDITOR_SELECTION_CRITERIA.md` |
| 2 | Fill auditor name / company / contact | Owner | Blocked (needs owner input) | `[date]` | Into `AUDITOR_OUTREACH_MESSAGE.md` placeholders |
| 3 | Decide NDA (needed or not) | Owner | Not started | `[date]` | Repo+docs are public, no secrets; NDA may be unnecessary |
| 4 | Fill timeline / budget notes | Owner | Not started | `[date]` | `[Timeline]`, `[Budget/Scope Notes]` |
| 5 | Send kickoff message | Owner | Not started | `[date]` | Only after `SEND_CHECKLIST.md` + explicit send approval |
| 6 | Confirm audit scope with auditor | Project + Auditor | Not started | `[date]` | `AUDIT_SCOPE.md` |
| 7 | Confirm deliverables | Project + Auditor | Not started | `[date]` | `AUDIT_DELIVERABLES.md` |
| 8 | Confirm repo access method | Project + Auditor | Not started | `[date]` | Public clone; no credentials |
| 9 | Schedule kickoff call (if needed) | Owner | Not started | `[date]` | Optional |
| 10 | Receive findings | Auditor | Not started | `[date]` | Log in findings tracker + record per finding |
| 11 | Triage findings | Project | Not started | `[date]` | `phase26k.../FINDING_TRIAGE_POLICY.md` |
| 12 | Remediate findings | Project | Not started | `[date]` | Test branch only; `REMEDIATION_BRANCH_POLICY.md` |
| 13 | Retest with auditor | Project + Auditor | Not started | `[date]` | `RETEST_PROTOCOL.md` |
| 14 | Receive final report / sign-off or non-sign-off | Auditor | Not started | `[date]` | Scoped statement only |
| 15 | Decide public testnet gate | Owner | Blocked (gated) | `[date]` | Gate stays BLOCKED until decided post-audit |
| 16 | Archive audit artifacts | Project | Not started | `[date]` | Final report, findings, retest evidence into `docs/audit/` |

## How to update

- Move a row to `In progress` when work starts; `Done` when complete.
- Steps 1–5 are **owner-gated** — the project cannot proceed to outreach without owner input (see
  `OWNER_ACTIONS_REQUIRED.md`).
- Steps 10–14 run per finding via the Phase 26K workflow; reflect aggregate state in
  `AUDIT_ENGAGEMENT_STATUS.md` and `phase26k.../AUDIT_STATUS_DASHBOARD.md`.
- Public-testnet (15) and mainnet remain gated/blocked throughout; do not mark 15 `Done` as "launch"
  here — it records the **decision**, which is separately approval-gated.
