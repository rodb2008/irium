# PoAW-X Founder Two-VPS Pilot Rehearsal

**Date:** 2026-06-15
**Type:** Founder-controlled rehearsal — VPS-2 acting as external miner/client to VPS-1. NOT a community test, NOT a public testnet, NOT mainnet.

**Verdict: PARTIAL PASS** — the external stratum transport path was proven **live** (VPS-2 → VPS-1 over the real external network); the receipt→irx1→block consensus path was **not re-proven live in this rehearsal** (harness genesis limitation) but is already proven in Phase 14-F (74/74 ×2, lane="cpu" live).

---

## 1. Build / Identity

- Branch/commit tested: `testnet/poawx-phase12-completion-rc-hardening` @ `5751e5a` (code identical to validated `d01e469`; later commits docs-only).
- Testnet node binary: validated branch build at the repo target (isolated from the mainnet service binary), sha256 `cc7f79f0…`.
- Testnet stratum: `pool/irium-stratum` v0.1.1, sha256 `4856c31b…` (built in the pool's own isolated target).
- Miner client: python stratum harness (`poawx-stratum-long-soak-harness.py`) — **no `cpuminer` on VPS-2**, so this is a **simulated external-miner path** over the real external network (clearly not full real-cpuminer proof).

## 2. Roles & Ports

| Host | Role | Detail |
|---|---|---|
| VPS-1 (`VPS1_PUBLIC_IP`) | Operator: testnet node + stratum | node RPC `127.0.0.1:39511` (PRIVATE), status `127.0.0.1:39508` (PRIVATE), P2P `0.0.0.0:39510` (ufw-blocked), stratum `0.0.0.0:39512` |
| VPS-2 (`VPS2_IP`) | External miner/client | ran the stratum harness against `VPS1_PUBLIC_IP:39512`; RPC reached only via SSH reverse-tunnel |

- Devnet env: `IRIUM_NETWORK=devnet`, `IRIUM_POAWX_MODE=active`, activation height 1, difficulty 4 bits; data/state dirs under `$HOME` (removed at cleanup).
- RPC kept private: the harness's RPC calls went over an SSH `-R` reverse tunnel (`VPS-2 localhost:39511 → VPS-1 localhost:39511`); RPC was **never** bound publicly.

## 3. Firewall

- ufw is **active** on VPS-1 (default-deny), so `0.0.0.0:39512` was not publicly reachable until a rule was added.
- Temporary rule (operator-applied via sudo): allow **only** `VPS2_IP` → `39512/tcp` (comment `poawx-pilot-vps2-temporary`).
- External reachability confirmed from VPS-2 before mining.
- **Removal command** (operator, after rehearsal): `sudo ufw delete allow from VPS2_IP to any port 39512 proto tcp`.

## 4. Connection / Share Result (live, external)

Operator-side stratum log evidence (VPS-2 connecting over the external network):
- `[conn] accepted id=… from VPS2_IP:…` — external TCP connection accepted (3 connection events).
- `[subscribe]` + `[authorize] worker=<testnet-throwaway-addr>.soak adapter_kind=cpuminer_compat` — subscribe & authorize OK.
- `[sharecheck-cpuminer]` + `[SHARE_ACCEPTED]` + `[COMPAT_SOLVED_SHARE]` — share validated and accepted.

Miner-side harness summary (from VPS-2): subscribe OK, `set_difficulty=1`, authorize OK, **1 share accepted / 0 rejected**, **bogus share rejected** (`stale share`, height unchanged).

| Proof item | Result |
|---|---|
| VPS-2 connects from external network | ✅ live |
| VPS-1 stratum sees the miner | ✅ live |
| ≥1 testnet share accepted | ✅ live |
| Bogus share rejected | ✅ live |
| VPS-1 node stable / VPS-2 client stable | ✅ |
| No mainnet service changes | ✅ |
| RPC 39511 private (SSH tunnel only) | ✅ |
| Clean shutdown | ✅ |

## 5. Receipt / irx1 / Block Result

- **Not produced live in this rehearsal.** At genesis (h=0) the node's `/poawx/assignment` returns 404 (by design, available after height ≥ 1), so the long-soak harness could not seed a pending receipt; with no pending receipt the stratum falls back to legacy submit, which the PoAW-X-active node correctly rejects (405). Net: shares accepted, but no block/irx1 via this harness at genesis.
- This is a **harness genesis-flow limitation, not a node defect**. The full receipt→irx1→block path (assignment, receipt, worker signature, puzzle PoW, irx1 root match, reward split, P2P propagation) is proven in **Phase 14-F** (two-VPS E2E 74/74 PASS ×2, including a live `lane="cpu"` block accepted and synced).

## 6. Negative / Safety Checks

| Check | Result |
|---|---|
| RPC 39511 reachable publicly | ✅ NO (localhost bind; tunnel only) |
| Mainnet ports/services unchanged | ✅ both hosts |
| Mainnet wallet used | ✅ NO (throwaway testnet worker name) |
| Public exposure beyond intended stratum path | ✅ NONE (ufw source-restricted to VPS2_IP) |
| Unknown external IPs connected | ✅ NONE (only VPS2_IP) |
| Stop conditions triggered | ✅ NONE |
| Secrets/personal info in logs/docs | ✅ NONE (sanitized; IPs placeholdered) |
| Resource pressure endangering mainnet/pool | ✅ NONE |
| Production pool services | ✅ untouched (all active) |

## 7. Mainnet Untouched (both hosts)

| Host | PID (before==after) | Hash |
|---|---|---|
| VPS-1 | 4042499 | `7c07ae2c…` |
| irium-eu | 1851441 | `7c07ae2c…` |

Production pool services on VPS-1 (irium-stratum / -443 / -legacy / -solo / pool-api) remained active throughout.

## 8. Cleanup

- Testnet node + stratum stopped; devnet ports (39512/39510/39511/39508) clear.
- Devnet data/state dirs removed; VPS-2 temp harness removed.
- Temporary ufw rule: removal command handed to operator (see §3).

## 9. Limitations

- Simulated external-miner path (python harness, not real `cpuminer`); transport/stratum proven, not a real-miner-binary proof.
- No live irx1 block produced in this rehearsal (genesis harness limitation); consensus path relies on Phase 14-F evidence.
- Single external client (VPS-2); no concurrency/soak.

## 10. Readiness to Proceed to One Trusted Community Miner

**Conditionally yes.** The external transport/stratum path is proven live and the consensus path is proven in 14-F. Before a community miner:
1. Demonstrate the stratum producing a real irx1 block from a **seeded pending receipt** (operator-side, post-genesis) so a miner's accepted share yields a PoAW-X block end-to-end via stratum (closes the one gap this rehearsal left).
2. Use a real `cpuminer`/`cpuminer-multi` client for at least one session to confirm real-miner compatibility (not just the harness).
3. Apply the trusted-miner runbook/checklist (firewall source-restricted, RPC private, monitoring, stop conditions) and obtain explicit launch approval.
