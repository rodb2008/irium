# PoAW-X Phase 20 — Step 7A: external (non-self-operated) trusted-miner test plan

**Status: PREP ONLY (docs/preflight) — the test has NOT been run.** This document is the test
pack + exact preflight checklist for the one remaining Phase 20 blocker: a **public/external,
non-self-operated** trusted-miner test. All prior live E2Es (5A/5B/6E/6F) were self-operated
(operator-controlled VPS-1 + VPS-2). Step 7A prepares; a later step runs it only after the
operator collects the miner inputs and performs the firewall handoff.

## Scope guardrails (Step 7A)
- **No endpoints exposed, no ports bound, no services/miner started, no firewall changes, no
  invites sent** in Step 7A — docs only.
- Phase 20 local readiness is **COMPLETE** (Steps 1–6F, full test suites green, see
  `poaw-x-phase20-production-wiring-status.md`).
- **Mainnet activation is NOT claimed** and remains hard-off on every gate.
- Official pool fee remains **0%**; third-party fee remains **explicit opt-in only** (capped 2%,
  miner-signed into the delegation).
- Chain difficulty remains **LWMA-144 automatic**. `STRATUM_DEFAULT_DIFF=1` (if used in an
  isolated test) is the stratum **share** difficulty only — not chain difficulty.
- **The external miner test is NOT yet complete.**

## Required external miner inputs (operator collects before the run)
The operator must collect and record (the miner provides **public** information only):
1. external miner **public IP**
2. miner **wallet address** (testnet; throwaway — never a mainnet wallet)
3. **worker name** (e.g. `rig1`)
4. machine **OS / CPU / GPU** details (for hash-rate expectations)
5. what the miner can run:
   - [ ] stock cpuminer only
   - [ ] wallet helper commands (emit-only delegation; role precommit/reveal emit)
   - [ ] optional node/gossip helper (Tier 2, only if later approved)
6. preferred **test window** (date/time + duration)
7. **confirmation** they understand **no private key / seed / wallet file is ever sent**
8. **confirmation** they will share **only**: signed payloads, public wallet address, worker
   name, public IP

## Two-tier external test design

### Tier 1 — external stock-cpuminer connectivity + PoW attempt
- The external miner runs **stock cpuminer** against the **source-restricted** stratum
  (restricted by UFW to the miner's IP only).
- The operator posts the miner's **signed delegation** to the **loopback-only** delegation
  endpoint (the miner emits it offline with `--emit-only` and sends only the signed JSON).
- Role gossip may remain **operator-assisted** (operator injects via loopback / node bridge).
- **Objective / success criteria:**
  - external miner **connects** to the stratum over the real internet
  - **authorizes** (worker accepted; for cpuminer family, unsolicited `set_version_mask` is
    suppressed so auth succeeds)
  - **receives Phase 20 work** (multi-role coinbase job)
  - ideally **lands a PoW share/block**
  - if no share lands in the bounded window, **record**: hash rate, elapsed time, work received,
    job/difficulty seen (this is the documented slow-CPU / low-devnet-height caveat — an
    environment limitation, not a Phase 20 logic defect, as long as connect/auth/work/validation
    all succeed)

### Tier 2 — external role-precommit/reveal participation (no private-key transfer)
- The miner uses the wallet helper to **emit JSON only**:
  - `irium-wallet poawx-role-precommit …` → role precommit JSON (hides secret/nonce)
  - `irium-wallet poawx-role-reveal … --prev-hash <h>` → role reveal JSON
- The miner sends the JSON to the operator **out-of-band** (or through a future safe endpoint
  **only if explicitly approved and implemented later** — **not** in Step 7A).
- The operator injects it via the **loopback** role endpoint / node bridge.
- **Objective:** prove a **non-self-operated** miner can participate in the role protocol
  **without any private-key transfer** (the wallet signs locally; only signed payloads leave).
- **Not yet live:** there is **no public role/delegation endpoint**. Tier 2 today is
  out-of-band JSON only. A public role-submission endpoint is **NOT claimed ready** and would
  require its own design + approval step.

> **Do not** open public role/delegation/RPC endpoints in Step 7A (or in the Tier-1 run). Only
> the stratum (and optionally P2P, if a remote gossip node is used) may be public, and only
> source-restricted to the external miner's IP.

## Recommended Step 7 live port plan (placeholders — DO NOT bind now)
Bind these only at run time, in a later step, after the operator firewall handoff:
- status: **loopback** `127.0.0.1:<STATUS_PORT>`
- RPC: **loopback** `127.0.0.1:<RPC_PORT>`
- delegation/role: **loopback** `127.0.0.1:<DELEG_PORT>`
- metrics: **loopback** `127.0.0.1:<METRICS_PORT>`
- stratum: **source-restricted to the external miner IP** `0.0.0.0:<STRATUM_PORT>` + UFW allow
  from `<MINER_IP>` only
- P2P: **optional**, only if a remote gossip node participates; **source-restricted** to the
  miner IP `0.0.0.0:<P2P_PORT>` + UFW allow from `<MINER_IP>` only
- Choose a **fresh Step 7 port block** (e.g. `403xx`) distinct from 5A/5B/6E/6F. **Delegation,
  RPC, status, metrics, and role endpoints stay loopback-only.**

## Operator preflight checklist (run before the live test, in the later step)
1. verify repo clean at `0b9414b` (or the then-current audited HEAD) — `git status -sb`,
   `git rev-parse HEAD`
2. verify **no upstream / not pushed**
3. verify mainnet/prod alive: VPS-1 mainnet `219530`, VPS-1 prod pool **4** workers, VPS-2
   mainnet `1851441`
4. verify **no Step5/6/7 test ports** listening
5. **select fresh Step 7 ports** (status/RPC/deleg/metrics loopback; stratum [+optional P2P]
   source-restricted)
6. **collect external miner IP** (+ the other required inputs above)
7. **prepare** source-restricted UFW **open** commands (do NOT run them in Step 7A):
   `sudo ufw allow from <MINER_IP> to any port <STRATUM_PORT> proto tcp comment 'poawx phase20 step7 ext miner stratum temp'`
   (+ `<P2P_PORT>` only if a remote gossip node is used)
8. **prepare** UFW **close** commands:
   `sudo ufw delete allow from <MINER_IP> to any port <STRATUM_PORT> proto tcp` (+ P2P)
9. prepare exact **pidfile paths** (e.g. `<TROOT>/pids/{node,stratum}.pid`) — exact-pid stop only
10. prepare **artifacts path** (e.g. `/home/irium/phase20-step7-artifacts`)
11. prepare **stop/failure plan**: stop exact test pids only (no `pkill`/`killall`), leave test
    root on failure for diagnosis, do not broad-cleanup, **stop before UFW close** for operator
    handoff, do not push
12. verify public exposure is limited to **stratum** (and **P2P** only if needed)
13. confirm **delegation / RPC / status / metrics / role endpoints loopback-only** (verified
    blocked from the miner IP after firewall open)

## Stop / bounding rules for the run
- Bounded PoW window (e.g. 10–20 min); if no share lands, record the caveat and proceed.
- Stop all test processes by **exact pidfile** only; **never** `pkill`/`killall`.
- Two firewall handoffs (operator-only): one before UFW **open**, one before UFW **close**.
- Mainnet, prod pool, and difficulty/LWMA-144 must remain untouched throughout.

## What is NOT claimed
External miner test complete · remote cpuminer PoW solved · public role-gossip rollout complete ·
public activation complete · mainnet-ready · production mainnet deployment complete.
