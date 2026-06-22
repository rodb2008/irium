# PoAW-X Phase 26G — Public Testnet Operator Runbook

A high-level operator runbook for a future, separately-approved public testnet **dry run**. This is
a plan, not a launch. **Do not launch without explicit approval.** Commands are intentionally
high-level (no secrets, no machine-private values). **Production-ready: NO. Mainnet-ready: NO.
Audited: NO.**

Pair with: readiness doc, rollout checklist, risk register. Baseline commit: `c15c436`
(`origin/main` unchanged at `19c496dc5f2fa08981a109b10eeb257105c28c43`).

## Required machines (dry-run reference topology)

- **Hub** (well-connected host): accepts inbound P2P on a source-restricted port; bootstrap point.
- **≥ 2 spokes** (incl. at least one behind NAT, e.g. a workstation): dial the hub; mine and/or
  observe.
- One spoke designated for the **restart** test, one for the **fresh-wipe** test.
- Each machine may already run an unrelated mainnet node/pool — those are **off-limits** (see "what
  not to do").

## Ports (reference; agree exact numbers per deployment)

- **P2P** (cross-host): one TCP port per node, e.g. hub `P2P_HUB`; firewall allows inbound ONLY from
  the agreed peer source IPs. No `0.0.0.0/0`, no all-ports, no UDP.
- **RPC**: `127.0.0.1:<rpc>` — **loopback-only on every node**.
- **Status**: `127.0.0.1:<status>` — **loopback-only on every node**.
- Spokes behind NAT bind P2P loopback (dialer-only) and reach the hub outbound.

### Must remain loopback-only
- RPC (`IRIUM_NODE_HOST=127.0.0.1`) on every node.
- Status server (`IRIUM_STATUS_HOST=127.0.0.1`) on every node.
- No public RPC, no stratum exposure, ever.

## High-level sequence (dry run)

1. **Prep (all nodes):** checkout the baseline commit; verify HEAD == `c15c436` and `origin/main`
   unchanged; release-build `iriumd` + `poawx-live-proof-harness`. Confirm the rollout checklist §1–6.
2. **Connectivity:** confirm each spoke can reach the hub's source-restricted P2P port using a
   temporary, auto-exiting listener; confirm it auto-exits (no lingering open port). If a home-ISP
   spoke's egress IP changed, re-scope the single source firewall rule (operator-approved) — never
   broaden.
3. **Launch hub first**, then spokes (each dials the hub via `IRIUM_ADDNODE`). Confirm storage
   banners show isolated dirs, `/poawx/assignment` is served at genesis, RPC/status are loopback-only,
   and peer counts match the topology. Confirm all nodes share the same genesis hash.
4. **Mine a short chain** with the Irium-native harness (no stock cpuminer): originate blocks from
   ≥ 2 operators. After EACH block, verify convergence (below) before mining the next.
5. **Restart test:** stop one spoke by its exact pidfile PID; restart it with the SAME isolated
   storage. Verify it reloads the persisted admissions and rebuilds the active chain to the tip from
   disk; verify tip/irx1 match peers; mine one more block and verify it receives it.
6. **Fresh-wipe test:** stop another spoke by exact PID; FULLY wipe its isolated storage (data +
   blocks + state, incl. `candidate_admissions.dat`); restart it as a brand-new node dialing the hub.
   Verify it receives served historical admissions, syncs the chain from scratch, and converges
   (tip/irx1 match peers); mine one more block and verify it receives it.
7. **Hold / observe** for the agreed window; collect metrics (no secrets).
8. **Stop safely** (below) and write the post-test report.

## How to capture evidence

- Per node, periodically record: height, tip hash, irx1 root (via loopback `/status` +
  `/rpc/block?height=<h>`), peer count, sync-stall events, getblocks served/received, admissions
  ingested/rejected (by reason), re-broadcast counts.
- Record per-block propagation latency (mine → all-converged) and per-restart/fresh-sync convergence
  time.
- Summarize logs for any shared report; do NOT share raw logs containing machine-private data; never
  include sudo passwords, private keys, wallet data, or credentials.

## How to verify same height/tip/irx1 across nodes

- For each node (over its loopback RPC): `height` and `best_header_tip.hash` from `/status`, and
  `irx1_root` from `/rpc/block?height=<height>`.
- Converged ⇔ all nodes report the SAME `height`, the SAME tip hash, and the SAME `irx1_root`.
- After mining block H, poll until all nodes report height H with identical tip/irx1 before mining
  H+1. A node lagging by one block while it pulls is expected briefly; a persistent divergence in
  validity (one node rejects what another accepts) is an ABORT condition.

## How to verify restart and fresh-wipe sync

- **Restart (keep-storage):** after restart, the node log should show it reloaded N persisted
  candidate admissions and a startup "source-of-truth tip=<H>"; height reaches the tip from disk;
  tip/irx1 match peers; a subsequently-mined block is received.
- **Fresh-wipe:** the node starts with no blocks (`contiguous_from_zero=0`, local height 0); it
  receives historical admissions served alongside blocks (the serving peer may show the fresh node
  re-broadcasting them); height climbs to the tip; tip/irx1 match peers; a subsequently-mined block
  is received.

## How to stop safely (cleanup)

- Stop ONLY testnet nodes, by EXACT pidfile PIDs (no pkill, no killall). Refuse if a PID matches a
  protected mainnet PID.
- Verify all testnet P2P/RPC/status ports are closed.
- Verify mainnet/prod processes and the production pool are still alive and untouched.
- Verify default storage (`.irium`) mtime predates the run (untouched).
- Leave firewall rules as agreed; remove temporary rules only if explicitly decided.
- Preserve artifacts (summarized, no secrets).

## What NOT to do

- Do NOT touch `main`; no PR/merge/tag/release; no force push.
- Do NOT enable PoAW-X on mainnet; do NOT change PoW/LWMA/difficulty/target/reward; do NOT weaken or
  disable phase21d/21e/22a or any gate; do NOT stage activation to bypass a gate.
- Do NOT stop, modify, or share storage with any mainnet node or the production pool; never reuse a
  mainnet PID; protected mainnet PIDs must stay alive.
- Do NOT expose RPC or stratum publicly; do NOT open `0.0.0.0/0`, all-ports, UDP, or RPC/stratum
  ports; do NOT add broad firewall rules.
- Do NOT use `/tmp`, `~/.irium`, or `%USERPROFILE%\.irium` for test storage; do NOT touch real
  wallets/keys.
- Do NOT use stock cpuminer/minerd.
- Do NOT print, store, echo, or commit sudo passwords, private keys, wallet data, or credentials.
- Do NOT claim production-ready / mainnet-ready / audited based on testnet results.
- Do NOT launch the public testnet without explicit approval.
