# PoAW-X Trusted Miner Pilot — Operator Runbook

**Version:** 1.0 (post Phase 14-F)
**Validated branch:** `origin/testnet/poawx-phase12-completion-rc-hardening` @ `a0aedc6`
**Node version:** v1.9.115 (PoAW-X + MTP, reconciled with official main `5d4604c`)
**Status:** PREPARATION ONLY — pilot NOT started. Launch requires explicit approval.

> **TESTNET ONLY.** Isolated PoAW-X devnet. **No real Irium coins, no real rewards, no mainnet compatibility.** Chain may reset at any time. No mainnet wallets/keys/addresses on this testnet.

---

## 1. Purpose

Validate that a small number (1–3) of **trusted, invited** external miners can connect to the isolated PoAW-X testnet stratum, submit valid shares, and that the resulting blocks carry valid PoAW-X `irx1` receipts verifiable by the operator over the private RPC — all without any impact to mainnet.

This is **not** a public testnet and **not** a reward program.

## 2. Scope

- 1–3 trusted external miners, invited individually.
- Stratum v1 over TCP, CPU mining (sha256d), difficulty 1.
- Time-boxed session (see `DURATION` placeholder in the invite).
- Single operator-run testnet node + testnet stratum.

## 3. Environment Plan

| Component | Value | Exposure |
|---|---|---|
| Operator testnet node | devnet binary from repo target / isolated devnet path (NOT the mainnet service binary) | — |
| Stratum endpoint | `PILOT_HOST:STRATUM_PORT` (established pilot port: 39512) | **exposed to invited miner only** |
| P2P (seed, optional) | `PILOT_HOST:39510` | optional, node-operator testers only |
| RPC | `127.0.0.1:39511` | **PRIVATE — localhost only, never exposed** |
| Status | `127.0.0.1:39508` | **PRIVATE — localhost only** |
| Data dirs | under `$HOME` (e.g. `/home/irium/irium-devnet-pilot-*`) | — |
| Devnet env | `IRIUM_NETWORK=devnet`, `IRIUM_POAWX_MODE=active`, `IRIUM_POAWX_ACTIVATION_HEIGHT=1`, `IRIUM_POAWX_PUZZLE_DIFFICULTY_BITS=4` | — |

**No mainnet collision:** the testnet node must use devnet ports (39510/39511/39508/39512) and `$HOME` devnet data dirs only. Mainnet uses 38300/8080/38291 and `/home/irium/.irium`. Mainnet `iriumd.service` now runs the official binary from `/home/irium/mainnet/bin/iriumd-current` — building/running the testnet node cannot affect it (binary isolation verified, Phase 14-F).

## 4. Operator Preflight (must pass before invite)

See `poaw-x-trusted-miner-operator-checklist.md`. Summary:
1. Branch/hash = `a0aedc6`; clean tree.
2. Both mainnets healthy on official v1.9.115 binary; PIDs unchanged; PoAW-X OFF on mainnet.
3. No old devnet processes; devnet ports clear.
4. Testnet node boots from clean devnet genesis, PoAW-X active.
5. RPC 39511 confirmed NOT publicly reachable.
6. Firewall: only `STRATUM_PORT` (39512) reachable from the miner; 39511/39508 blocked publicly.
7. Testnet stratum up and pointed at the testnet node.

## 5. Miner Preflight (communicated via invite)

- Stratum v1 CPU miner (e.g. cpuminer-multi), sha256d.
- Outbound TCP to `PILOT_HOST:STRATUM_PORT`.
- A throwaway testnet address/worker name (`TESTNET_WALLET_OR_WORKER_NAME`) — **never** a mainnet wallet.

## 6. Connection Flow

1. Operator sends connection details privately (host/port/worker/start time).
2. Miner connects: `stratum+tcp://PILOT_HOST:STRATUM_PORT`, worker `TESTNET_WALLET_OR_WORKER_NAME`, password `x`.
3. Miner receives `mining.set_difficulty` (1) then `mining.notify` (PoAW-X job).
4. Miner submits shares; accepted shares return `{"result": true}`.
5. When a receipt is pending, the stratum injects the `irx1` commitment into the coinbase; an accepted share at target produces a block with a non-zero `irx1_root`.

## 7. Monitoring Flow

Run the sanitized monitoring commands (Section in operator checklist) throughout:
- process/ports/peers, stratum connections, accepted shares, receipt persistence, `irx1_root`, height/tip, error scan.
Capture logs continuously (sanitized — no tokens/IPs in shared excerpts).

## 8. Success Criteria

See `poaw-x-trusted-miner-acceptance-criteria.md`. In brief: external miner connects, ≥1 valid accepted share, valid PoAW-X receipt/irx1 path where applicable, operator verifies `irx1_root` privately, no mainnet impact, no public RPC, no crash/panic/restart loop, no consensus/reward-split failures.

## 9. Failure / Stop Criteria

See `poaw-x-trusted-miner-stop-conditions.md`. **Immediately stop** the pilot on any mainnet impact, public RPC exposure, accepted-share-but-invalid-receipt, consensus rejection, identity/reward mismatch, resource pressure, secret leakage in logs, or crash/restart loop.

## 10. Rollback / Shutdown

1. Signal miner(s) to stop (Ctrl+C their miner).
2. Stop the testnet stratum process (devnet stratum only).
3. Stop the testnet node (devnet PID / `fuser -k` the devnet ports — devnet only, never mainnet).
4. Confirm devnet ports clear (39510/39511/39508/39512).
5. Optionally remove devnet data dirs under `$HOME`.
6. Confirm **both mainnets unchanged** (PID + binary hash) — VPS-1 and irium-eu.

## 11. Data / Privacy Rules

- Never commit or share: tokens, RPC auth, private keys, wallet secrets, env files, real miner IPs, personal emails/phones.
- Use placeholders (`PILOT_HOST`, `STRATUM_PORT`, etc.) in committed docs.
- Sanitize all shared log excerpts (mask IPs, strip auth headers).
- Keep 39511/39508 private at all times.

## 12. Final Go / No-Go Checklist

- [ ] Operator preflight all PASS
- [ ] Both mainnets healthy + untouched (official binary)
- [ ] RPC 39511 confirmed private
- [ ] Firewall: only STRATUM_PORT exposed
- [ ] Testnet node + stratum up, PoAW-X active, clean genesis
- [ ] Trusted miner identified, invite sent with placeholders filled privately
- [ ] Stop-conditions + acceptance criteria reviewed
- [ ] **Explicit launch approval obtained**

Only when every box is checked AND explicit approval is given may the pilot start.
