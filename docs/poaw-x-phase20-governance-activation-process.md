# PoAW-X Phase 20 — Governance / Community Activation Process

**Status:** Process doc COMPLETE. This defines the **process** to reach a mainnet activation
decision; it does **not** activate anything. Mainnet PoAW-X mode-1 remains **hard-disabled
until an explicit future activation height** (see the mainnet activation safety framework).

## 1. Prerequisites before any activation vote
- Consensus design gaps resolved + implemented + tested where in scope: fairness matrix,
  multi-role reward split, third-party fee (or an explicit decision to ship without them).
- Internal E2E green (Phases 18–19: emit-only, mode-1, two-VPS, observer validation).
- Trusted external miner pilot completed with no consensus/identity/reward failures.
- Public testnet run completed for the required duration, incident-free.
- Reproducible binary + verified hash; security review of the consensus boundary.

## 2. Testnet success criteria
- Mode-1 blocks accepted + independently re-validated by peers over real network paths.
- Miner-paid / delegate-unpaid / fee-0% invariants hold across the run.
- No accepted-share-without-valid-receipt; no reward-split or identity mismatch.
- No crash/restart loop; reorg + persistence behave correctly.

## 3. External miner pilot success criteria
- ≥1 trusted external miner onboarded via `--emit-only` (non-custodial), mined an accepted
  mode-1 block, observer re-validated it; firewall handoff opened + removed cleanly; mainnet/
  prod untouched. (Proven in 19D with our own VPS-2; repeat with a real third party.)

## 4. Public testnet duration & incident-free requirement
- A defined minimum window (operator-set) of continuous operation with **zero** mainnet
  impact and **zero** unresolved consensus incidents.

## 5. Community review
- Publish the consensus spec (lanes, reward split, fee, activation height) and the validation
  record; open a review window; address findings before the vote.

## 6. Activation-height policy
- Activation is by an **explicit, far-future block height**, announced well in advance,
  defaulting to OFF; never enabled by env/config accident.

## 7. Rollback policy
- If a critical issue appears post-activation-decision but pre-height, the height can be
  cancelled/postponed by a new announcement; nodes default-off until the height.

## 8. Mainnet safety review
- Final consensus-boundary audit; confirm mainnet mode-1 hard-reject paths and default-off
  gating (with regression tests) before any height is set.

## 9. Binary reproducibility / hash verification
- The activation binary must build reproducibly to a published hash; operators verify the
  running exe hash matches before/after (as done for the official mainnet binary today).

## 10. Operator communications & announcement
- Operators coordinate the activation height, monitoring, and rollback comms.
- **Public announcement draft (template):**
  > "Irium PoAW-X mainnet activation is proposed for block height <FAR_FUTURE_HEIGHT>.
  > Until that height, PoAW-X mode-1 remains disabled on mainnet. Node operators must run the
  > reproducible binary <HASH> by <DATE>. The full consensus spec and testnet validation
  > record are published at <LINK>. This is a proposal subject to community review."

## 11. Standing wording
**Mainnet mode-1 remains hard-disabled until activation.** No activation occurs as part of
Phase 20. This document is process scaffolding only.
