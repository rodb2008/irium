# PoAW-X Phase 20 — Broad Public Testnet Readiness (package; do NOT launch)

**Status:** Readiness package COMPLETE. **Public testnet is NOT launched** by this phase.
Launching requires explicit operator approval and resolution of the consensus design gaps
(fairness matrix, multi-role split, fee) if those are in scope for the public testnet.

> Chain difficulty automatic via LWMA-144. Mainnet PoAW-X mode-1 hard-disabled. Nothing here
> binds public ports or runs a network.

## 1. Public testnet checklist (pre-launch)
- [ ] Consensus scope frozen: which of mode-0/mode-1, lanes, reward model are live (today:
      CPU lane, 10%/receipt, official 0% fee; GPU/ASIC + multi-role + fee are design-gapped).
- [ ] Seed node(s) provisioned, **isolated from mainnet/prod** (separate binaries, paths, ports).
- [ ] Genesis + `bootstrap/anchors.json` + `bootstrap/trust/allowed_anchor_signers` shipped
      and reproducible (hash-verified).
- [ ] Endpoint exposure decided per §4 (source-restricted vs public).
- [ ] Abuse protection in place (§5).
- [ ] Metrics/monitoring up (loopback or operator-restricted) (§6).
- [ ] Rollback plan rehearsed (§8).
- [ ] Activation-height policy set (§10) — far-future, explicit.
- [ ] Support/incident channel staffed (§9).

## 2. Capacity assumptions (initial)
- Small seed set (1–3 nodes); tens of miners max in the first public window.
- CPU-lane mode-1 throughput is intentionally modest (diff-1 share = block target; LWMA-144
  retargets chain difficulty automatically as hashrate grows).
- No faucet / no value; chain may reset.

## 3. Bootstrap / anchors / seed config
- Each node runs from a CWD containing `bootstrap/anchors.json` +
  `bootstrap/trust/allowed_anchor_signers` (genesis embedded; anchors are not).
- Peers via node-config `p2p_seeds` (testnet/devnet); **not** `IRIUM_STATIC_PEERS`
  (mainnet-only dialer).
- Seed nodes advertise a public P2P endpoint; RPC/status/delegation/metrics stay loopback.

## 4. Source-restricted vs public endpoints
| Endpoint | Public testnet stance |
|---|---|
| P2P (seed) | public (it must accept peers) — with peer rate-limits / anti-eclipse |
| Stratum | start **source-restricted to invited miners**; broaden only with abuse controls |
| RPC / status / delegation / metrics | **loopback/private**; never public; delegation endpoint refuses non-loopback by code |

## 5. Abuse protection plan
- Stratum connection-gate (existing): max sessions, per-IP cap, ban threshold/duration.
- Share validation fail-closed; `low_difficulty` flood handled by `STRATUM_DEFAULT_DIFF=1`.
- Delegation endpoint loopback-only → no remote delegation spam; registration is via
  `--emit-only` payloads the operator submits.
- Per-peer P2P scoring / anti-eclipse (existing).

## 6. Metrics plan
Per `poaw-x-phase20-metrics-monitoring.md`: loopback `/metrics` + `/status` + log scanning;
public metrics only with operator-approved source-restriction.

## 7. Known limitations (state plainly to testers)
- CPU lane only (no GPU/ASIC lane yet); single-miner block production; official 0% fee only;
  no value; chain may reset; no mainnet compatibility.

## 8. Rollback plan
- Stop services by exact pidfile; remove the testnet `$TROOT`; verify ports clear and
  mainnet/prod untouched. Chain reset is acceptable testnet behavior; announce resets.

## 9. Support / incident response
- Single operator contact; private issue intake; sanitized logs only (mask IPs, strip
  tokens). Escalate anything touching mainnet/prod immediately (stop first).

## 10. Activation-height selection policy
- Any consensus activation (e.g. enabling mode-1 broadly) uses an **explicit, far-future
  height** announced in advance; never by env/config accident (see mainnet safety framework).

## 11. Block explorer / API
- Optional; if added, read-only and rate-limited; must not expose RPC tokens or private data.
