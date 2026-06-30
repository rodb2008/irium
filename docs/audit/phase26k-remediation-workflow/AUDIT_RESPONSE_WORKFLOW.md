# Audit Response Workflow

The lifecycle for a single PoAW-X Phase 26 audit finding, from receipt to closure. **Process only — no
findings exist yet.** **NOT audited / production-ready / mainnet-ready.** No source changes happen
outside an approved remediation step on the **test branch only**.

## Lifecycle

### 1. Finding received
- Auditor submits a finding (ideally on `FINDING_RECORD_TEMPLATE.md` or the findings tracker).
- Create a per-finding record from `FINDING_RECORD_TEMPLATE.md` with a stable ID `F-NNN`.
- Acknowledge using the ack template (`AUDITOR_RESPONSE_TEMPLATES.md`). Status → **Open**.

### 2. Reproduce / understand
- Reproduce locally using `docs/audit/phase26h-kickoff/REPRO_COMMANDS.md` and the finding's steps.
- Read the affected code end-to-end before forming an opinion (per the project's change rule: read the
  whole file, identify everything affected, before any change).
- If not reproducible or unclear, request clarification (template). Status → **Needs More Info**.

### 3. Classify
- Apply `FINDING_TRIAGE_POLICY.md` to assign severity.
- Record severity + rationale. On dispute, default to the higher severity (see triage policy).

### 4. Accept / dispute / needs more info
- **Accept:** project agrees it is a valid finding to remediate. Status → **Accepted**.
- **Dispute:** project believes it is not an issue or severity is wrong; respond with concrete evidence
  (code refs, tests, reasoning) using the dispute template. Status → **Disputed** (record both sides).
- **Needs More Info:** await auditor input.
- Critical/High acceptance requires the approvers named in the triage policy and may **pause public
  testnet** immediately.

### 5. Create remediation plan
- Before coding, write the plan in the finding record: current behavior (exact lines), why it is wrong,
  what changes, what else in the file could be affected and why it will **not** break. This mirrors the
  project's mandatory pre-change plan.
- For Critical/High, share the plan with the auditor before implementing.
- Confirm the fix does **not** weaken any gate or touch consensus/PoW/LWMA/reward; if it would,
  **stop and escalate** rather than proceed.

### 6. Implement on test branch only
- Create a remediation branch per `REMEDIATION_BRANCH_POLICY.md`
  (`audit/poawx-phase26-finding-<ID>-<short-name>`), branched from the test branch.
- Implement the minimal change. Update docs and tests alongside code. **No `main`, no PR, no merge, no
  tag, no release, no force push.**

### 7. Run tests
- Run the required suite per `RETEST_PROTOCOL.md`: focused tests, full serialized lib suite, release
  build, plus **negative tests** proving the gate still rejects the bad case.
- Capture evidence (commands + summarized results) into the finding record and `RETEST_PROTOCOL.md`
  format.

### 8. Live devnet validation (if needed)
- Required when the finding touches P2P/sync/persistence or consensus-adjacent logic
  (`RETEST_PROTOCOL.md`). Devnet only: loopback RPC, source-restricted P2P, isolated storage, exact
  pidfiles, no firewall/sudo, mainnet + prod untouched.
- This step is **stop-and-ask**: a live run requires explicit approval before execution.

### 9. Auditor retest
- Provide the fix commit(s) and evidence; request retest (template).
- Auditor independently retests and records the result in the finding record + tracker.

### 10. Close finding
- Closes only on **auditor retest pass** or **explicit, documented project acceptance** (e.g. Won't
  Fix with rationale, or Accepted Risk). Update status and the dashboard. Use the close template.

### 11. Update final audit package
- Reflect closure in `AUDIT_STATUS_DASHBOARD.md`, the findings tracker, and the deliverables/sign-off
  record (`docs/audit/phase26h-kickoff/AUDIT_DELIVERABLES.md`).
- The audit is "complete" only when the auditor issues a scoped sign-off / non-sign-off — not when the
  project believes findings are addressed.

## Status values

`Open → (Needs More Info) → Accepted | Disputed → Fixed (on test branch) → Retested → Closed`
(or `Won't Fix` / `Accepted Risk`, documented). Mirror these in the findings tracker.

## Guardrails (every step)

- Test branch only; no `main`/PR/merge/tag/release/force-push.
- No fix weakens a gate or changes consensus/PoW/LWMA/difficulty/target/reward.
- Mainnet stays blocked; public testnet stays gated.
- No secrets, keys, wallet data, or raw private logs in records.
- No claim of audited/production-ready/mainnet-ready.
