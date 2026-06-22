# PoAW-X Phase 26G — Public Testnet Risk Register

Risks for a future, separately-approved public testnet. Each entry: description, impact, likelihood,
mitigation, residual risk. Likelihood/impact are qualitative (Low/Med/High) for a TESTNET context
(no real value at stake). **Production-ready: NO. Mainnet-ready: NO. Audited: NO.**

## 1. Technical / consensus

| ID | Risk | Impact | Likelihood | Mitigation | Residual |
|----|------|--------|-----------|------------|----------|
| T1 | A latent bug in epoch-seed alignment causes a validity divergence (node accepts a block another rejects). | High (testnet split) | Low | 26B tests (6-block chain + negatives); phase22a unchanged; independent audit before launch; abort on any divergence. | Low–Med until audited. |
| T2 | Deep chains (beyond a handful of blocks / one admission window per batch) expose untested edge cases. | Med | Med | Per-getblocks-batch admission serving; start small; ramp height gradually; collect rejection metrics. | Med (public-testnet target). |
| T3 | Gate env misconfiguration across operators causes spurious validation failures (looks like a consensus bug). | Med | Med | Single canonical activation env in the rollout checklist; verify env identical node↔harness; document failure signatures. | Low with checklist. |
| T4 | Accidental change to PoW/LWMA/difficulty/target/reward. | High | Low | Hard rule: no such changes; audit confirms none in `30bce64..c15c436`; CI/diff review. | Low. |

## 2. P2P

| ID | Risk | Impact | Likelihood | Mitigation | Residual |
|----|------|--------|-----------|------------|----------|
| P1 | Multi-block-from-scratch getblocks stalls before handshake-push (observed ~30–45 s on devnet). | Med (slow fresh sync) | Med | Handshake-push delivers blocks+admissions; aggressive-but-bounded P2P timing env; characterize at scale; treat prolonged stalls as an abort signal. | Med. |
| P2 | Eclipse / partition / peer churn under untrusted topology. | Med | Med | Source-restricted peers initially; known-operator bootstrap; monitor peer counts/convergence; document expected behavior. | Med (testnet target). |
| P3 | getblocks/headers flooding or unsolicited-message spam. | Med (DoS) | Med | Existing rate-limits/cooldowns/grace; no new request type; bounded admission send; monitor served/recv counts. | Med — see DoS section. |
| P4 | Same-host peer filtering / NAT prevents some links (observed in prior phases). | Low | Med | Dialer-only spokes behind NAT; hub reachable on source-restricted P2P; document topology constraints. | Low. |

## 3. Admission-cache

| ID | Risk | Impact | Likelihood | Mitigation | Residual |
|----|------|--------|-----------|------------|----------|
| A1 | Forged / tampered admission accepted. | High (if it bypassed phase21e) | Low | `ingest_bytes`/`reload_persisted_bytes` re-validate signature/digest/seed/true-VRF; tampered records rejected; phase21e equality unchanged. | Low (audit confirms). |
| A2 | Replay / cross-network reuse of an admission. | Med | Low | Digest binds `(network, height, seed, candidate[,V2])`; network-id checked; wrong height/seed won't match phase21e for another context. | Low. |
| A3 | Cache poisoning (extra/conflicting admissions). | Med | Low | Key `(height, role, solver)`; conflicting distinct digests rejected; one canonical per key; phase21e requires exact set equality. | Low. |
| A4 | Stale admissions linger / window mismatch rejects valid ones. | Low–Med | Med | `prune` drops far-below-tip entries; window 64 covers per-batch sync; phase21e binds height/seed. | Med (window tuning is future work). |
| A5 | Persisted-snapshot corruption crashes or mis-loads. | Med | Low | Atomic write (temp+rename); truncated-tail handling; per-record re-validation; never panics; bounded size. | Low. |

## 4. Fresh-sync

| ID | Risk | Impact | Likelihood | Mitigation | Residual |
|----|------|--------|-----------|------------|----------|
| F1 | A fresh node fails to obtain historical admissions and cannot sync. | Med | Low | 26E serves admissions before blocks at all four block-serve sites; receiver re-validates via ingest path; 26E live-validated. | Low (small chains); Med (deep chains, untested at scale). |
| F2 | Admission serving over-sends / amplifies (DoS). | Med | Low–Med | Bounded `≤16×served_block_count` per response; only on existing serve path; re-broadcast dedup. | Med — see DoS. |
| F3 | Fresh node trusts a peer beyond delivery. | High (if true) | Low | Delivery-only trust; every admission + block independently re-validated; no shortcut introduced. | Low. |

## 5. Operator / configuration

| ID | Risk | Impact | Likelihood | Mitigation | Residual |
|----|------|--------|-----------|------------|----------|
| O1 | Operator exposes RPC/stratum publicly or uses broad firewall rules. | High (exposure) | Med | Hard rule loopback-only RPC + source-restricted P2P; checklist gate; reachability verification. | Med (human factor). |
| O2 | Operator uses default storage (`.irium`/`/tmp`) and corrupts real data. | High | Med | Isolated `IRIUM_*_DIR` mandatory; storage banner check; runbook "what not to do". | Low–Med. |
| O3 | Operator runs stock cpuminer/minerd (produces invalid Irium PoW). | Low (rejected) | Med | Hard rule native-only; harness instructions; node rejects invalid PoW anyway. | Low. |
| O4 | Inconsistent baseline build across operators. | Med | Med | Pinned commit hash; reproducible release build; verify HEAD on every node. | Low. |
| O5 | Accidental mainnet/prod interaction during ops. | High | Low | Exact-PID-only stop; protect mainnet PIDs; runbook forbids touching mainnet/pool. | Low. |

## 6. Dynamic Windows IP / firewall

| ID | Risk | Impact | Likelihood | Mitigation | Residual |
|----|------|--------|-----------|------------|----------|
| D1 | A home-ISP participant's egress IP changes, breaking source-restricted P2P. | Med (connectivity) | High | Recheck egress IP before each session; re-scope the single source rule (operator-approved); never broaden. | Med (operational, not security). |
| D2 | Firewall change requires sudo and is mishandled. | Med | Low | Operator-approved interactive/`-S` sudo only for the exact rule; never print/store/commit the password; minimal scope. | Low. |

## 7. Audit gap

| ID | Risk | Impact | Likelihood | Mitigation | Residual |
|----|------|--------|-----------|------------|----------|
| AU1 | Launching before independent audit; an unreviewed flaw surfaces in public. | High (reputation) | Med | Prerequisite: audit completed or formally in progress; testnet only (no value); abort criteria; audit package ready (26F). | Med until audit done. |

## 8. Performance / DoS

| ID | Risk | Impact | Likelihood | Mitigation | Residual |
|----|------|--------|-----------|------------|----------|
| PD1 | Admission serving + gossip fan-out + sync bursts exhaust CPU/mem/bandwidth. | Med | Med | Bounded admission send; existing serve rate-limits; collect resource metrics; cap participants in early rounds. | Med (characterize at scale). |
| PD2 | Disk growth from the admission snapshot under churn. | Low | Low | Snapshot bounded by pruned cache size; atomic rewrite; monitor disk. | Low. |
| PD3 | Sync-stall recovery loops waste resources on a large gap. | Low–Med | Med | Handshake-push + gap-healer recovery; monitor stall events; ramp chain depth. | Med. |

## 9. False-claim / reputation

| ID | Risk | Impact | Likelihood | Mitigation | Residual |
|----|------|--------|-----------|------------|----------|
| R1 | Someone claims production-ready / mainnet-ready / audited based on testnet results. | High (reputation) | Med | Explicit claims policy in readiness + communication plan; "testnet only, no value, may reset" notice; every doc states NOT production/mainnet-ready/audited. | Med (governance/comms). |
| R2 | Testnet results over-generalized to mainnet safety. | Med | Med | Document scope/limitations (phase21e propagation sensitivity, window, untrusted-peer assumptions); separate mainnet path via governance. | Med. |

## Overall residual

A public testnet is a reasonable next step **as a testnet** (no value at stake, mainnet hard-off) to
exercise untrusted-peer, scale, and adversarial assumptions and to feed the audit — provided the
prerequisites (esp. audit-in-progress, pinned build, loopback-RPC/source-restricted-P2P discipline,
and the claims policy) are met. It is NOT a step toward mainnet without a completed audit and
governance.
