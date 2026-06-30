# Finding Triage Policy

How PoAW-X Phase 26 audit findings are classified and what each severity obliges. **No findings exist
yet; this is policy only.** **NOT audited / production-ready / mainnet-ready.** Response times below
are working-time targets, measured from the project acknowledging receipt; they are commitments to
*begin and prioritize*, not guarantees of a fix by that time.

Standing rules regardless of severity:
- **Mainnet discussion stays blocked** for the entire Phase 26 program — no severity "unblocks"
  mainnet. PoAW-X is hard-off for `network_id == 0`.
- **No fix weakens a gate or changes consensus/PoW/LWMA/difficulty/target/reward** to make a finding
  "go away." If the only remediation would weaken a gate, that is a stop-and-escalate decision, not a
  routine fix.
- **No finding is "closed"** without auditor retest or explicit, documented project acceptance.

## Critical

- **Definition:** A confirmed consensus break, a validation-gate bypass, or any path to a mainnet
  impact. The network can be made to accept invalid state, or honest nodes diverge on identical input.
- **PoAW-X examples:** a block connects without a matching, validated admitted candidate set (phase21e
  bypass); phase22a committed-admission check is satisfiable by a forged/replayed commitment; a forged
  or cross-network admission is accepted as valid; the epoch-seed change lets a producer freely choose
  its own candidate-set seed to control admissions; any way to flip PoAW-X on for `network_id == 0`.
- **Response time:** begin within **24h**; remediation plan within **3 business days**.
- **Approves remediation:** project lead **and** the consensus/security owner; auditor consulted on the
  fix approach before implementation.
- **Public testnet:** **must pause** (and must not start if not yet launched) until fixed + auditor-retested.
- **Mainnet:** remains blocked (unchanged).

## High

- **Definition:** A viable path to a Critical-class outcome under realistic conditions, a DoS that can
  stall honest nodes, or a validity divergence that requires specific but plausible conditions.
- **PoAW-X examples:** historical-admission serving can be driven to exhaust a node (bound ineffective
  in practice); cache poisoning causes a node to reject valid blocks or accept a wrong admitted set
  under narrow timing; persisted-admission reload accepts a tampered record under some encoding;
  fresh-sync can be wedged by a malicious server.
- **Response time:** begin within **3 business days**; remediation plan within **5 business days**.
- **Approves remediation:** project lead + consensus/security owner.
- **Public testnet:** **must pause** until fixed + retested (or the auditor agrees a documented
  mitigation reduces it below High).
- **Mainnet:** remains blocked.

## Medium

- **Definition:** Exploitable only under narrow conditions, resource-exhaustion with existing
  mitigations, or a weakened-but-not-broken invariant / robustness gap with limited impact.
- **PoAW-X examples:** the `16 × block_count` serving multiplier is larger than necessary; window=64
  edge behavior is suboptimal but not unsafe; non-fatal handling of a malformed persisted file could
  be tightened; transient getblocks stall affects time-to-sync but not validity.
- **Response time:** begin within **10 business days**; scheduled into the remediation backlog.
- **Approves remediation:** project lead.
- **Public testnet:** does **not** automatically pause; must be listed as a known item and decided
  explicitly before/along with launch readiness.
- **Mainnet:** remains blocked.

## Low

- **Definition:** Limited-impact issue, hardening opportunity, or robustness concern with no realistic
  exploit.
- **PoAW-X examples:** defensive logging gaps, minor input-validation hardening, clearer error strings,
  redundant checks worth adding for defense-in-depth.
- **Response time:** begin within **20 business days** or batch into a hardening pass.
- **Approves remediation:** project lead (may delegate).
- **Public testnet:** does not pause; tracked as known.
- **Mainnet:** remains blocked.

## Informational

- **Definition:** Style, clarity, documentation, or defense-in-depth suggestions with no security
  impact.
- **PoAW-X examples:** cosmetic compiler warnings (e.g. unused `committee`, self-assign in test code),
  doc clarifications, naming.
- **Response time:** best-effort / backlog; no SLA.
- **Approves remediation:** anyone on the team; optional.
- **Public testnet:** no impact.
- **Mainnet:** remains blocked.

## Severity disputes

If the project and auditor disagree on severity, default to the **higher** severity until resolved, and
record both positions in the finding record (`FINDING_RECORD_TEMPLATE.md`). The auditor's severity
stands for sign-off purposes unless they agree to revise it.
