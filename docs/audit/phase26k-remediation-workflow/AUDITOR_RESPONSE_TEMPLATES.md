# Auditor Response Templates

Reusable message templates for each step of handling a finding. Fill `[...]` placeholders. **Process
only — no findings exist yet.** Keep messages free of secrets, keys, wallet data, and raw private
logs. Do not claim audited / production-ready / mainnet-ready.

---

## 1. Acknowledge a finding

> Hi `[Auditor Name]`,
> Thanks — we've received finding **`[F-NNN]`** ("`[title]`") and logged it as **Open**. We're
> reproducing it now against `testnet/poawx-phase20-blueprint-completion-local` (`[commit]`). We'll
> follow up with our classification and proposed response by `[date per triage SLA]`.
> — `[Your Name / Irium Labs]`

---

## 2. Ask for clarification

> Hi `[Auditor Name]`,
> On **`[F-NNN]`**, we couldn't fully reproduce / want to make sure we understand the conditions. Could
> you clarify:
> - `[exact branch/commit you tested]`
> - `[inputs / sequence / env]`
> - `[observed vs expected]`
> Status is **Needs More Info** until we can reproduce. Thanks for the detail.
> — `[Your Name]`

---

## 3. Accept a finding

> Hi `[Auditor Name]`,
> We agree with **`[F-NNN]`** and classify it **`[Severity]`** (`[one-line rationale]`). Status →
> **Accepted**. `[For Critical/High: we have paused / will not start the public testnet until this is
> fixed and you've retested.]` Our remediation plan: `[summary]`. We'll implement on a dedicated
> remediation branch and share the fix + evidence for your retest.
> — `[Your Name]`

---

## 4. Dispute a finding (with evidence)

> Hi `[Auditor Name]`,
> Thanks for **`[F-NNN]`**. After review we believe `[it is not exploitable as described / the severity
> should be lower]`, for these concrete reasons:
> - `[code reference file:line and what it enforces]`
> - `[test or invariant that prevents the described outcome]`
> - `[reasoning]`
> We've recorded this as **Disputed** and kept your original severity pending resolution (we default to
> the higher severity while open). We'd value your view on the evidence above — happy to be wrong if we
> missed something.
> — `[Your Name]`

---

## 5. Report a fix

> Hi `[Auditor Name]`,
> **`[F-NNN]`** is fixed on branch `audit/poawx-phase26-finding-[ID]-[short-name]`.
> - Fix commit(s): `[local sha]` `[(landed remote: sha)]`
> - What changed: `[summary]`; this does **not** weaken any gate or change consensus/PoW/reward.
> - Tests: `[focused + negative results]`; full suite `cargo test --lib -- --test-threads=1` →
>   `[N passed / 0 failed]`; release build exit 0. `[Live devnet: <summary> if run.]`
> Evidence is in the finding record. Status → **Fixed (pending your retest)**.
> — `[Your Name]`

---

## 6. Ask for retest

> Hi `[Auditor Name]`,
> Could you retest **`[F-NNN]`** against `[commit/branch]`? Suggested checks:
> `[exact commands / scenario]`. Please record Pass / Fail / Partial in the finding record / tracker.
> Let us know if you need anything else to reproduce.
> — `[Your Name]`

---

## 7. Close a finding

> Hi `[Auditor Name]`,
> Recording **`[F-NNN]`** as **Closed** based on `[your retest pass on <commit> / our documented
> decision: Won't Fix | Accepted Risk with rationale]`. The dashboard and tracker are updated.
> `[If it gated the testnet: the public-testnet gate for this item is now clear / still blocked by
> <other findings>.]` Thanks for the thorough review.
> — `[Your Name]`

---

> Reminder: closure requires auditor retest **or** explicit documented project acceptance. No template
> here implies the overall system is audited; that depends on the auditor's final scoped sign-off.
