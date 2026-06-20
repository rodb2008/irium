# PoAW-X Phase 24D — two-VPS all-gates retry (PARTIAL — paused at operator firewall gate)

**Status: PARTIAL. Phase 24D did NOT complete the full all-gates rehearsal.** It paused, by
operator decision, at the cross-host firewall gate. Local-only; not pushed; remote branch
absent; `main` untouched. **Mainnet hard-off; not mainnet-ready.** This is **not** a public
testnet and does **not** replace an external audit.

This retry followed the Phase 24B incident + the Phase 24C fail-closed storage fix
(`docs/poaw-x-phase24c-storage-isolation-hardening.md`). All storage used explicit
`$HOME`-rooted isolated dirs; the `~/.irium` fallback hazard did not recur.

## What this phase did NOT validate (no claim)

- ❌ No firewall rule was opened.
- ❌ No cross-host P2P link was established (VPS-2 ↔ VPS-1).
- ❌ No all-gates block was produced.
- ❌ No official fee-0 block validation claim.
- ❌ No third-party fee block validation claim.
- ❌ No observer (node B) byte-identical validation claim.
- ❌ No restart/reload validation claim.

## What WAS achieved (verified)

- **VPS-2 code transfer without push:** a `git bundle` of the local branch
  (`testnet/poawx-phase20-blueprint-completion-local`) was created on VPS-1, transferred to
  VPS-2 (via local relay, no push), and cloned into a dedicated working copy
  `/home/irium/irium-p24d-src`.
  - bundle sha256 = `3ecb94ffe80b5929bbcacf64696f41bc3168b4c31e6401478318f4fcaa382b1a`
  - cloned HEAD on VPS-2 = `57a8ddf` (matches VPS-1).
- **VPS-2 clone/build success:** `cargo build --release --bin iriumd --bin irium-wallet`
  (nice'd) finished in 13m26s.
- **Node A** (VPS-1, producer) launched with isolated `$HOME`-rooted storage — banner verified:
  - `Using blocks dir: /home/irium/irium-p24d-nodeA/blocks`
  - `Using state dir: /home/irium/irium-p24d-nodeA/state`
  - devnet, height 0, **all PoAW-X gates enabled**, P2P loopback-only.
- **Node B** (VPS-2, observer) launched with isolated `$HOME`-rooted storage — banner verified:
  - `Using blocks dir: /home/irium/irium-p24d-nodeB/blocks`
  - `Using state dir: /home/irium/irium-p24d-nodeB/state`
  - devnet, height 0, all gates enabled, P2P loopback-only.
- **Phase 24C fail-closed storage fix validated live on BOTH hosts:** each node resolved
  storage to its isolated p24d dirs (each blocks dir held only its own genesis `block_0.json`);
  neither node fell back to `~/.irium`.
- **Both nodes stayed loopback-only** (status/RPC/P2P on `127.0.0.1`); nothing was exposed.
- **Both mainnet stores untouched:** VPS-1 `~/.irium` (2 pre-existing orphan dirs) and VPS-2
  `~/.irium` (4 pre-existing orphan dirs) — **no new orphan dirs** on either host.
- **Both mainnets alive throughout:** VPS-1 PID 219530, VPS-2 PID 1851441. VPS-1 prod pool
  workers alive (4).

## Gate list (all enabled on devnet; mainnet hard-off)

Phase20 production, tickets required, penalty required, anti-domination required, candidate
set required, candidate admission required, committed admission required, true VRF required,
puzzle work required, finality committee required, finality gossip required, role gossip
enabled, hidden precommit (`IRIUM_POAWX_*` env, all gated; `network_id == 0` hard-off).

## VPS roles

- VPS-1 `207.244.247.86`: node A (producer) + (intended) pool/stratum.
- VPS-2 `157.173.116.134`: node B (observer) + (intended) wallet/miner identity, candidate
  admission origin, finality vote origin.

## Firewall

No firewall rule was opened. The operator chose to stop at the handoff. No UFW change was made
by anyone; no cross-host port was ever exposed.

## Where Phase 24D paused

At the cross-host firewall gate (section F). Establishing VPS-2 → VPS-1 P2P (port 40510)
requires an operator-only UFW allow rule (source-restricted to VPS-2) plus rebinding node A's
P2P to `0.0.0.0:40510`. The operator directed a stop before any of that.

## Honest limitations

- **Cross-host P2P requires an operator firewall handoff** (UFW), which is intentionally not
  performed by the agent.
- **Full all-gates block production** additionally needs the pool/stratum + cpuminer harness
  with `/poawx/assignment` seeding; `iriumd` has no internal miner, and this path hit a harness
  wall in the Phase 14-F rehearsal. It was not attempted here.
- **Phase 24D is a partial rehearsal only.** The all-gates consensus behavior remains exercised
  in-process by the test suite (e.g. `chain::phase22e_true_vrf_e2e_block`), which is separate
  from this live rehearsal.

## Cleanup

- Node A stopped by exact pidfile PID (VPS-1); node B stopped by exact pidfile PID (VPS-2). No
  `pkill`/`killall`; mainnet PIDs 219530 / 1851441 untouched.
- Removed: `/home/irium/irium-p24d-nodeA` (VPS-1); `/home/irium/irium-p24d-nodeB` +
  `/home/irium/irium-p24d-src` (VPS-2). No pool/wallet dirs were ever created.
- Preserved: `/home/irium/phase24d-all-gates-artifacts/` on both hosts (node logs, bundle
  checksum, `evidence-vps1.txt` / `evidence-vps2.txt`).
- No Phase24D ports bound on either host (40508/40510/40511/40512/40513/40514/40518/40520/
  40521 all clear).
- No new orphan dirs in either `~/.irium`; both mainnets + VPS-1 prod pool workers alive.

## Status

Phase 24B incident root cause stays fixed (24C). Phase 24D proved the **isolated-storage launch
path is safe on both hosts** but is a **partial rehearsal**: a future completion needs the
operator firewall handoff (cross-host P2P) and pool/cpuminer harness work (block production),
ideally on a host without mainnet. No mainnet-ready / audited / public-testnet claim.
