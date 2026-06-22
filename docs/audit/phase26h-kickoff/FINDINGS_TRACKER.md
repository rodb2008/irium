# PoAW-X Phase 26 — Findings Tracker

Living record of audit findings against baseline source `0208368` (HEAD `972bb9c`). The auditor adds
rows; the project fills "Project response", "Fix commit", and "Retest evidence". **No findings have
been confirmed yet — this is an empty tracker for the review.** NOT audited / production-ready /
mainnet-ready.

## Legend

- **Severity:** Critical · High · Medium · Low · Informational
- **Status:** Open · Accepted · Fixed · Won't Fix · Needs More Info · Retested

## Findings

| ID | Severity | Status | Title | Affected file / function | Description | Auditor recommendation | Project response | Fix commit | Retest evidence |
|----|----------|--------|-------|--------------------------|-------------|------------------------|------------------|-----------|-----------------|
| F-001 | _TBD_ | Open | _(auditor to fill)_ | | | | | | |
| F-002 | _TBD_ | Open | | | | | | | |
| F-003 | _TBD_ | Open | | | | | | | |

> Add rows as needed. Use stable IDs (`F-NNN`). Keep descriptions reproducible (file:line, inputs,
> observed vs expected). Do not include secrets, keys, wallet data, or raw machine-private logs in any
> entry.

## Severity guidance (project's working definitions; auditor may refine)

- **Critical** — consensus break or a confirmed phase21e/phase22a bypass (block accepted without a
  matching validated admission), forged/replayed admission accepted as valid, or any mainnet impact.
- **High** — a viable path to the above under realistic conditions, or a DoS that can stall honest
  nodes, or a divergence in validity between honest nodes on identical inputs.
- **Medium** — exploitable only under narrow conditions, resource-exhaustion with mitigations, or a
  weakened-but-not-broken invariant.
- **Low** — limited-impact issue, hardening gap, or robustness concern.
- **Informational** — style, clarity, defense-in-depth suggestions, or documentation gaps.

## Triage workflow

1. Auditor files a finding (Open) with severity + reproduction.
2. Project triages: Accepted / Won't Fix / Needs More Info (with rationale).
3. If Accepted and fixed, project records "Fix commit" (docs/code on the test branch; no main/PR/tag/
   release per the current change rules) and moves to Fixed.
4. Auditor retests, records "Retest evidence", and moves to Retested.
5. Final sign-off (or non-sign-off) summarizes the closed/residual findings (see `AUDIT_DELIVERABLES.md`).

## Summary counters (update as findings land)

| Severity | Open | Accepted | Fixed | Retested | Won't Fix |
|----------|------|----------|-------|----------|-----------|
| Critical | 0 | 0 | 0 | 0 | 0 |
| High | 0 | 0 | 0 | 0 | 0 |
| Medium | 0 | 0 | 0 | 0 | 0 |
| Low | 0 | 0 | 0 | 0 | 0 |
| Informational | 0 | 0 | 0 | 0 | 0 |
