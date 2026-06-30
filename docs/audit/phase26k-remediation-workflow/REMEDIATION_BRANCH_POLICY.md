# Remediation Branch Policy

Branch/commit discipline for fixing audit findings while preserving full traceability. **Process only —
no findings exist yet.** **NOT audited / production-ready / mainnet-ready.**

## Base and target

- All remediation branches are cut from the **test branch**
  `testnet/poawx-phase20-blueprint-completion-local` (current HEAD `0e196ba`).
- Remediation lands back on the **test branch only**. **Never `main`.** No PR, no merge to `main`, no
  tag, no release.

## Branch naming

```
audit/poawx-phase26-finding-<ID>-<short-name>
```
- `<ID>` = the finding's stable ID, e.g. `F-001`.
- `<short-name>` = a few kebab-case words, e.g. `epoch-seed-bound`, `serving-cap`.
- Examples: `audit/poawx-phase26-finding-F-001-phase21e-bypass`,
  `audit/poawx-phase26-finding-F-004-serving-cap`.

## One finding per branch (when practical)

- Prefer **one finding per remediation branch / commit group** so each fix is independently reviewable
  and retestable.
- If two findings are genuinely entangled (same root cause), they may share a branch — document the
  linkage in both finding records and the commit message.
- Keep unrelated changes out of a remediation branch (no opportunistic refactors).

## Commit style

- Imperative subject referencing the finding ID, e.g.:
  - `fix(poawx): F-001 reject block lacking matching admitted set`
  - `test(poawx): F-001 negative test for phase21e bypass`
  - `docs(audit): F-001 record fix + retest evidence`
- Body: what changed, why, and an explicit "does NOT weaken gate / no consensus param change" note.
- Co-authorship/footers per repo convention.
- Group code + its tests + its doc updates together (or as a tight sequence on the branch).

## Required artifacts per remediation

1. **Code** — the minimal fix.
2. **Tests** — at least one **negative test** proving the gate still rejects the bad case, plus updated
   positive coverage. Full serialized lib suite must pass (`RETEST_PROTOCOL.md`).
3. **Docs** — update the finding record (`FINDING_RECORD_TEMPLATE.md`), the findings tracker, and the
   dashboard.
4. **Evidence** — captured test/build output (summarized; no secrets) in the finding record.

## No force push

- **No force push** unless explicitly approved in writing for a specific, named reason (e.g. removing
  an accidentally committed secret). Default is fast-forward / additive history only.
- Never rewrite shared test-branch history to "tidy" remediation.

## Audit traceability

- Every remediation branch maps 1:1 (or documented N:1) to a finding ID.
- The finding record lists the exact **fix commit hash(es)**; the dashboard reflects status.
- When the push fallback is used (VPS-1 `git am` re-creates SHAs), record **both** the local and the
  landed remote SHA in the finding record so the chain of custody is unambiguous.
- Do not delete remediation branches until the finding is Closed and the auditor has the final SHAs.

## Guardrails

- Test branch only; mainnet/prod untouched; PoAW-X hard-off for `network_id == 0` preserved.
- No fix may weaken phase21d/21e/22a or change PoW/LWMA/difficulty/target/reward.
- No secrets/keys/wallet data/raw logs in branches or commit messages.
