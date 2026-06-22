# PoAW-X Phase 26 — Expected Audit Deliverables

What the independent review should produce. Baseline source `0208368` (HEAD `972bb9c`). **NOT audited
/ production-ready / mainnet-ready.**

## 1. Summary report
- Scope reviewed (commit range, files), methodology, and time spent.
- An overall assessment of whether the Phase 26 changes preserve the validation gates (especially: no
  phase21e/phase22a bypass; phase22a unchanged; no consensus/PoW/reward change; mainnet hard-off).
- A clear, plain-language conclusion an operator can act on.

## 2. Findings (with severity)
- Each finding: ID, severity (Critical/High/Medium/Low/Informational), affected file/function,
  description, and reproduction (inputs, observed vs expected). Recorded in `FINDINGS_TRACKER.md`.
- A severity-tally table.

## 3. Exploitability notes
- For each non-informational finding: preconditions, attacker capability required, realistic
  likelihood, and blast radius (testnet vs any mainnet implication — noting mainnet is hard-off).
- Explicit PoC steps where applicable (no secrets; non-live where possible).

## 4. Recommended fixes
- Concrete, minimal remediation per finding, with a note on whether the fix risks weakening a gate
  (must not) or touching consensus/PoW/reward (must not without separate governance).

## 5. Retest requirements
- For each Accepted+Fixed finding: the exact test/observation that constitutes a pass, so retest is
  unambiguous. Prefer repo-local `connect_block`/unit tests; note any that need a (separately gated)
  live run.

## 6. Final sign-off / non-sign-off statement
- An explicit statement of one of:
  - **Sign-off (scoped):** the Phase 26 changes, within the reviewed scope, do not weaken the
    validation gates and contain no Critical/High open findings — with stated assumptions.
  - **Conditional:** sign-off pending specific fixes/retests (list).
  - **Non-sign-off:** unresolved Critical/High findings (list) preclude sign-off.
- The statement must scope itself to the reviewed commit range and to devnet/testnet (not mainnet).

## 7. Limitations and assumptions
- What was and was NOT examined (e.g. VRF internals treated as opaque validated digests; hidden-
  precommit/ticket/delegation paths out of scope; no live multi-operator/scale/adversarial testing;
  no mainnet review).
- The pre-existing phase21e propagation-sensitivity assumption ("admitted to THIS node").
- Any tooling/version assumptions (Rust toolchain, OS) used for the review.

## Format / handling
- Markdown or PDF; reference file:line at the reviewed commit.
- No secrets, private keys, wallet data, or raw machine-private logs.
- Deliverables should NOT assert production-ready, mainnet-ready, or (beyond the reviewer's own
  scoped sign-off) "audited" in a broader sense than the engagement covers.
