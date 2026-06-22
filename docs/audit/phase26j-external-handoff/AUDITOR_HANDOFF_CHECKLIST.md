# External Auditor Handoff Checklist

Work through this before and during handoff. **NOT audited / production-ready / mainnet-ready.** No
item here requires sharing secrets, keys, wallet data, machine credentials, or mainnet access.

> Engagement tracking + owner inputs + the pre-send checklist live in
> `docs/audit/phase26l-engagement-tracker/` (`OWNER_ACTIONS_REQUIRED.md`, `SEND_CHECKLIST.md`,
> `NEXT_STEPS_TRACKER.md`). No outreach has been sent.

## Before outreach

- [ ] **Auditor contact confirmed** — name, company, and a verified email/contact channel
      (`AUDITOR_OUTREACH_MESSAGE.md` placeholders filled).
- [ ] **NDA decided** — determine whether an NDA is needed. The repo and these docs are public and
      contain no secrets, so an NDA may be unnecessary; confirm with the auditor and legal.
- [ ] **Send approval obtained** — explicit approval to send the outreach message to the named
      recipient (do not auto-send).

## Repo / scope confirmation

- [ ] **Repo access method** — public clone of `https://github.com/iriumlabs/irium.git` (no credentials
      needed). Confirm the auditor can clone.
- [ ] **Exact branch/commit** — `testnet/poawx-phase20-blueprint-completion-local`, HEAD `22dfde8`,
      source baseline `0208368`, full range `30bce64..0208368`. `origin/main` unchanged
      (`19c496dc5f2fa08981a109b10eeb257105c28c43`).
- [ ] **Scope confirmed** — epoch-seed alignment, admission persistence, historical-admission serving,
      phase21d/21e/22a invariants, P2P DoS/replay/cache-poisoning.
- [ ] **Non-goals confirmed** — mainnet enablement, real-value rewards, governance, live public testnet,
      and the hidden-precommit/ticket/delegation paths are out of scope (unchanged here).

## Deliverables / process

- [ ] **Deliverables agreed** — summary report, findings w/ severity + exploitability, recommended
      fixes, retest requirements, scoped sign-off / non-sign-off (see
      `docs/audit/phase26h-kickoff/AUDIT_DELIVERABLES.md`).
- [ ] **Retest process agreed** — how fixes are verified (prefer repo-local `connect_block`/unit tests;
      note any needing a separately-gated live run).
- [ ] **Findings tracking agreed** — auditor records in
      `docs/audit/phase26h-kickoff/FINDINGS_TRACKER.md` (clean copy:
      `EXTERNAL_FINDINGS_TRACKER_COPY.md`); internal pre-read is
      `docs/audit/phase26i-self-review/INTERNAL_FINDINGS.md`.
- [ ] **Severity scale aligned** — Critical / High / Medium / Low / Informational (definitions in the
      tracker and `docs/audit/phase26k-remediation-workflow/FINDING_TRIAGE_POLICY.md`).
- [ ] **Remediation workflow shared** — point the auditor to
      `docs/audit/phase26k-remediation-workflow/` (response lifecycle, branch policy, retest protocol,
      response templates, status dashboard, per-finding record template) so finding handling is
      understood up front.

## Safety confirmations

- [ ] **No mainnet access needed** — review is static + repo-local tests; mainnet is hard-off and out
      of scope.
- [ ] **No secrets shared** — no sudo passwords, private keys, wallet data, machine credentials, or raw
      private logs in any handed-over material. Logs in the package are summarized.
- [ ] **No live infra exposure** — no VPS access, firewall changes, or production endpoints are part of
      the handoff.
- [ ] **Claims policy honored** — no "audited / production-ready / mainnet-ready" claims pending the
      reviewer's scoped sign-off.

## At kickoff

- [ ] Share `SEND_READY_SUMMARY.md` and `PACKAGE_MANIFEST.md` first.
- [ ] Point to `REPRO_COMMANDS.md` for non-live reproduction.
- [ ] Confirm a point of contact and cadence for questions/updates.
