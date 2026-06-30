# Finding Record Template

Copy this file to `docs/audit/phase26k-remediation-workflow/findings/F-NNN-<short-name>.md` (create the
`findings/` subfolder when the first real finding arrives) and fill it in. One file per finding.
**Template only — no findings exist yet.** No secrets, keys, wallet data, or raw private logs.

---

## F-NNN — `[title]`

| Field | Value |
|-------|-------|
| **Finding ID** | `F-NNN` |
| **Severity** | `[Critical / High / Medium / Low / Informational]` (auditor) / `[project view if disputed]` |
| **Status** | `[Open / Needs More Info / Accepted / Disputed / Fixed / Retested / Closed / Won't Fix / Accepted Risk]` |
| **Auditor** | `[name / company]` |
| **Date received** | `[YYYY-MM-DD]` |
| **Date closed** | `[YYYY-MM-DD or —]` |
| **Affected files / functions** | `[src/file.rs::fn, file:line]` |
| **Public-testnet impact** | `[pauses gate / no automatic pause / n/a]` |

### Summary
`[1–3 sentences: what the issue is.]`

### Reproduction steps
```
[exact branch/commit, env, commands, sequence]
```
- Observed: `[...]`
- Expected: `[...]`

### Impact
`[What an attacker/condition could achieve; blast radius; note mainnet is hard-off and out of scope.]`

### Classification rationale
`[Why this severity per FINDING_TRIAGE_POLICY.md. If disputed, record both positions.]`

### Project response
`[Accepted / Disputed (+evidence) / Needs More Info. Approver(s) for Critical/High.]`

### Fix plan (pre-change, per project change rule)
- Current behavior (exact lines): `[...]`
- Why it is wrong: `[...]`
- What will change: `[...]`
- What else in the file could be affected and why it will NOT break: `[...]`
- Confirmation: does **not** weaken any gate; no consensus/PoW/LWMA/difficulty/target/reward change.

### Remediation branch
- Branch: `audit/poawx-phase26-finding-NNN-<short-name>`
- One-finding-per-branch: `[yes / shared with F-MMM because <root cause>]`

### Fix commits
- `[local sha]` — `[subject]` `[(landed remote sha if VPS-1 fallback)]`
- `[test sha]` — `[negative + positive tests]`
- `[docs sha]` — `[record/tracker/dashboard update]`

### Tests
- Focused: `[command]` → `[N passed / 0 failed]`
- Negative (gate still rejects bad case): `[command]` → `[test name] ... ok`
- Full suite: `cargo test --lib -- --test-threads=1` → `[N passed / 0 failed]`
- Release build: `cargo build --release --bin iriumd --bin poawx-live-proof-harness` → `[exit 0]`

### Live devnet validation (if applicable; requires explicit approval)
- Approval ref: `[...]`
- What was validated: `[...]`; nodes at height `[H]`, tip `[hash]`
- Safety: loopback RPC, source-restricted P2P, isolated storage, exact pidfiles, mainnet + prod
  untouched, ports closed. (Logs summarized; no secrets.)

### Auditor retest
- Date / verdict: `[YYYY-MM-DD]` / `[Pass / Fail / Partial]`
- Notes: `[auditor's words / reference]`

### Closure status
`[Closed on auditor Pass at <commit> / Won't Fix (rationale) / Accepted Risk (rationale). Dashboard +
tracker updated. Public-testnet gate effect: <cleared this item / still blocked by ...>.]`
