# HTLCv1 Mainnet Activation Abort Criteria

## A. NO-GO (Before Height Is Finalized)
Do not finalize activation height if any condition is true:
- Less than required operator/miner upgrade coverage.
- Unresolved consensus-critical bug reports.
- Persistent chain instability (peer collapse, fork symptoms, stalled blocks).
- Unresolved test failures on activation release commit.

## B. ABORT (Before Activation, After Height Announcement)
Abort and reschedule if any condition is true within final 24h:
- Major node operators report inability to upgrade in time.
- Critical infrastructure outage (RPC, monitoring, alerting, seed/discovery path).
- High-severity regression found in mempool/template/block validation.

## C. EMERGENCY RESPONSE (After Activation)
Trigger emergency response if any condition is true:
- Chain split indicators persist > 10 minutes.
- Block connection failures spike and remain non-transient.
- Widespread tx rejection beyond expected policy variance.
- Evidence of consensus divergence between upgraded nodes.

Immediate actions:
1. Freeze non-essential changes.
2. Open incident bridge.
3. Capture node logs and height/tip snapshots from all major operators.
4. Communicate status publicly (issue acknowledged, triage underway).

## D. Rollback Principles
- If rollback is required, use coordinated operator instruction.
- Rollback decision must include:
  - exact target commit/version,
  - exact env rollback instructions,
  - post-rollback validation checks.

## E. Resume Criteria
Resume rollout only when:
- root cause identified,
- fix validated on testnet/devnet,
- operator readiness reconfirmed,
- new notice window issued.
