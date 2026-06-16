# PoAW-X Trusted Miner Pilot — Operator Runbook

**Version:** 2.0 (post Phase 15 — native_rewardable route proven)
**Validated code branch:** `origin/testnet/poawx-phase13-native-rewardable-cpuminer-e2e` @ `fad21c4`
**Node version:** v1.9.115 (PoAW-X + MTP, reconciled with official main `5d4604c`)
**Status:** PREPARATION ONLY — pilot NOT started. Launch requires explicit approval.
**Proven route (Phases 13-15):** `cpuminer/minerd -> native_rewardable -> submit_block_extended -> irx1_root -> BLOCK_ACCEPTED -> peer sync`. Full proof chain in `poaw-x-native-rewardable-miner-validation-summary.md`.

> **TESTNET ONLY.** Isolated PoAW-X devnet. **No real Irium coins, no real rewards, no mainnet compatibility.** Chain may reset at any time. No mainnet wallets/keys/addresses on this testnet.

> **ROUTE UPDATE (Phase 18 — delegated mode-1).** This v2.0 runbook documents the
> earlier `native_rewardable` + manually-seeded-receipt route. **Phase 18 replaced the
> seeded step with a non-custodial delegated (mode-1) flow:** the miner registers a
> one-time delegation, the pool produces the receipt, the miner wallet stays the sole
> payout identity, the delegate key is signer-only (never paid), and the official pool
> is 0% fee. **For a real external miner pilot, use the delegated route** — see
> `docs/poaw-x-phase19a-trusted-miner-pilot-readiness.md` and **Appendix A** below.
> The selected registration mechanism is the wallet **`--emit-only`** mode, **implemented
> locally in Phase 19B commit `1628843`** (local-only checkpoint, not pushed) — keep the
> delegation endpoint loopback-only; do not expose it and do not require miner SSH/tunnel
> access. A real external pilot still requires explicit operator approval before live testing.

---

## 0. Rewardable route — READ FIRST

PoAW-X **rewardable** CPU mining uses the gated **NATIVE_REWARDABLE** route, **not** rewardable cpuminer_compat.

- **Rewardable blocks come only from the deterministic native path.** A standard cpuminer/minerd is routed to native_rewardable when the stratum runs the explicit PoAW-X testnet/devnet config (IRIUM_STRATUM_ADAPTER_MODE=auto + IRIUM_STRATUM_NATIVE_REWARDABLE_ENABLED=1 + IRIUM_STRATUM_POAWX=1), proven byte-identical to a real cpuminer header (no byte-order bridge).
- **cpuminer_compat remains NON-rewardable on PoAW-X** — it may be used only for compatibility / share accounting, never for block promotion.
- **No variant sweep may promote a PoAW-X block.** Promotion is gated on a single deterministic canonical reconstruction; the compat byte-order sweep can never produce a rewardable candidate on the PoAW-X path.
- Rewardable production fires submit_block_extended with the pending receipt(s); the committed block carries the irx1_root matching the seeded receipt root.
- **Mainnet is byte-identical / unaffected** (poawx_enabled=false): legacy routing, the diff-1 floor, and the single-output coinbase are all unchanged on mainnet.

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
| Stratum endpoint | `PILOT_HOST:STRATUM_PORT` (operator-selected; 39512 is historical/optional, **not** mandatory) | **exposed to the invited/self-hosted miner IP only** |
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
6. Firewall: only the operator-selected `STRATUM_PORT` reachable, **source-restricted to the miner IP only** (never `Anywhere`); RPC/status stay private on `127.0.0.1`.
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
4. Confirm the pilot devnet ports clear (the operator-selected Node RPC/status/P2P + STRATUM_PORT for this pilot; the 39510/39511/39508/39512 set is only a historical example).
5. Optionally remove devnet data dirs under `$HOME`.
6. Confirm **both mainnets unchanged** (PID + binary hash) — VPS-1 and irium-eu.

## 11. Data / Privacy Rules

- Never commit or share: tokens, RPC auth, private keys, wallet secrets, env files, real miner IPs, personal emails/phones.
- Use placeholders (`PILOT_HOST`, `STRATUM_PORT`, etc.) in committed docs.
- Sanitize all shared log excerpts (mask IPs, strip auth headers).
- Keep 39511/39508 private at all times.

### 11.1 Repository policy (local-only until completion)

- **PoAW-X work stays local on the VPS.** Do **not** `git push` PoAW-X/testnet branches,
  recreate deleted remote branches, merge, open a PR, or tag — until the owner explicitly
  approves a final push/release. Commit locally only.
- Never push or touch `main`. The delegation endpoint and RPC are **never** exposed publicly.
- The miner→operator exchange is the **signed delegation payload only** — never a seed or
  private key. The operator POSTs it over loopback (see Appendix A / `--emit-only`).

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

## 13. Cleanup safety (MANDATORY — see incident doc)

> Ref: `docs/poaw-x-mainnet-cleanup-incident.md` (2026-06-15 mainnet outage caused by
> a broad process-name kill). These rules are non-negotiable.

**Never** stop processes by broad/shared name:
- ❌ `pkill -f "iriumd"`, `pkill -f irium`, `killall iriumd*`, `pgrep -f iriumd | xargs kill`
- The production binary is `iriumd-current` / `iriumd-5d4604c` — any `iriumd`/`irium`
  substring match reaches **production**. Binary-path isolation does NOT protect the
  runtime process from a name-based kill.

**Allowed teardown only:**
1. Record pilot PIDs at startup to pidfiles (`/tmp/pilot-node.pid`, `/tmp/pilot-stratum.pid`).
2. Teardown by **exact pidfile** (`kill "$(cat /tmp/pilot-node.pid)"`) or **exact devnet
   port** (`fuser -k 39512/tcp 39511/tcp 39510/tcp 39508/tcp`) — never a mainnet port.
3. If a path match is unavoidable, use the **full unique pilot binary path**, never the bare name.

**Verify production before AND after every pilot test:**
```bash
systemctl show -p MainPID --value iriumd        # unchanged across the test
sha256sum /proc/$(systemctl show -p MainPID --value iriumd)/exe   # stays 7c07ae2c…
systemctl is-active iriumd                       # active before and after
```
If MainPID changes or the service is not active after teardown → incident:
restore with `sudo systemctl start iriumd` first, before any other work.


---

## 14. Two-VPS pilot layout (proven, Phases 14-15)

| Host | Role | Notes |
|---|---|---|
| **VPS-1** | Node A + stratum | Mining target. Node A P2P exposed to VPS-2 only; stratum exposed to the miner IP only; RPC/status private on `127.0.0.1`. |
| **VPS-2** | Node B (peer/sync validator) + miner | Node B dials the VPS-1 public IP to peer; the miner (external or self-hosted on VPS-2) connects to the VPS-1 stratum over the public internet. |

- **Internal/devnet ports (localhost or VPS-restricted, examples):** Node A RPC `127.0.0.1:39811`, status `127.0.0.1:39808`, P2P `39810` (VPS-2-restricted); Node B RPC/status `127.0.0.1:39821/39818`, P2P `39820`.
- **External pilot port (operator-selected):** the stratum port (e.g. `39812`), exposed **only** to the miner IP. Pick a free port per pilot; never reuse `39512` blindly and never `39512` if a prior rule exists.
- The P2P peer filter rejects loopback/private peers, so two nodes peer via **routable** IPs (cross-VPS), not `127.0.0.1` — Node B dials the VPS-1 public IP through a source-restricted firewall rule.

## 15. Miner connection instructions

```
Algorithm:  sha256d
URL:        stratum+tcp://<VPS-1-public-ip>:<operator-selected-stratum-port>
Username:   <testnet-address>.<worker>      (worker default: w1)
Password:   x
```

Example (pooler/cpuminer 2.5.1 or cpuminer-multi):
```bash
minerd -a sha256d -o stratum+tcp://<host>:<port> -u <testnet-address>.w1 -p x -t <threads>
```
The <testnet-address> must be the address the pending receipt is seeded for (miner == worker), so the reward-split passes and the block commits.

> **Superseded for external pilots (Phase 18).** The seeded-receipt requirement above
> applies only to the legacy `native_rewardable` route. On the delegated mode-1 route
> there is **no manual seed**: the miner registers a one-time delegation (see Appendix A),
> the pool produces the receipt automatically, and `<wallet-address>.<worker>` must match
> the registered delegation. The miner command itself is unchanged.

## 16. Operator firewall rules (source-restricted, temporary)

- **Require the miner public IP before opening the stratum port.** Never open the stratum to `Anywhere`.
- Open exactly two source-restricted rules on VPS-1 (operator runs sudo; agent only prints commands; add a descriptive comment):
```
sudo ufw allow from <VPS-2-IP> to any port <NODE_A_P2P>  proto tcp
sudo ufw allow from <MINER_IP> to any port <STRATUM_PORT> proto tcp
```
- **Remove both immediately after the pilot** and verify absent:
```
sudo ufw delete allow from <VPS-2-IP> to any port <NODE_A_P2P>  proto tcp
sudo ufw delete allow from <MINER_IP> to any port <STRATUM_PORT> proto tcp
sudo ufw status verbose | grep -E "<NODE_A_P2P>|<STRATUM_PORT>" || echo rules-absent
```
- Do **not** use port `39512` as mandatory; if referenced anywhere it is optional/operator-selected and source-restricted only.

## 17. Validation checklist (native_rewardable pilot)

- [ ] Miner connects from the **expected public IP** (stratum is source-restricted to it).
- [ ] Stratum logs `adapter_kind=native_rewardable_reserved`.
- [ ] `REWARDABLE_SHARE_ACCEPTED` (>=1 accepted share).
- [ ] `REWARDABLE_CANDIDATE`.
- [ ] `submit_block_extended` (receipts present).
- [ ] `BLOCK_ACCEPTED` on Node A; node logs `block_extended accepted ... cleared_receipts=N`.
- [ ] Committed block has a non-zero `irx1_root`.
- [ ] `irx1_root` **matches the seeded receipt root**.
- [ ] Node A and Node B end at the **same height and same tip hash**.
- [ ] Payout/worker address recorded in stratum logs **matches the supplied miner address**.

If shares are accepted but no block is found in the window: report the accepted-share proof + miner hashrate/difficulty; do **not** fake block success; recommend lowering the devnet difficulty further **only** under the explicit non-mainnet gate (`STRATUM_DEFAULT_DIFF` < 1, devnet floor), or extending the window.

## 18. Proof status

Internally proven end-to-end (no external party yet):
- **Phase 13** — real cpuminer (pooler 2.5.1) mined through `native_rewardable` and committed a local height-1 block with the correct `irx1_root`.
- **Phase 14** — same path synced across two VPS nodes over real cross-VPS P2P (matching height/hash/`irx1_root`).
- **Phase 15** — self-hosted remote miner on VPS-2 connected to the VPS-1 stratum over the public internet and produced a block that synced to Node B.

Phases 14 and 15 were **validation-only and added no code commits** (the proven code is `fad21c4`). See `poaw-x-native-rewardable-miner-validation-summary.md`.

The **delegated mode-1** route (the route selected for real external pilots) was proven
in **Phase 18** at `491a4de`: 18C single-node E2E and 18D two-node cross-VPS sync. See
`poaw-x-phase18-delegated-receipts-validation-summary.md`.

---

## Appendix A — Delegated mode-1 route (Phase 18; selected for external pilots)

This is the route to use for a real trusted external miner. It removes the manual
seeded receipt: the miner registers a one-time, non-custodial delegation; the pool
produces the mode-1 receipt automatically; the miner wallet remains the sole payout
identity; the delegate key is signer-only and never paid; the official fee is 0%.

**Validated:** branch base `491a4de`; Phase 18C (single-node E2E) + Phase 18D
(two-node cross-VPS sync). Reference: `docs/poaw-x-phase19a-trusted-miner-pilot-readiness.md`.

### A.1 Registration model — `--emit-only` (LOCKED; keeps the endpoint loopback-only)

1. Operator runs the pool with the loopback-only delegation endpoint
   (`IRIUM_POAWX_DELEGATION_BIND=127.0.0.1:<delegation-port>`).
2. Operator reads the pool identity locally and sends it to the miner out-of-band:
   ```
   curl -sS http://127.0.0.1:<delegation-port>/poawx/pool-identity   # pool_pubkey, network_id, fee_bps=0, domain
   ```
3. Miner builds and signs the delegation **locally** with the wallet `--emit-only`
   mode (implemented in Phase 19B commit `1628843`) and returns only the signed
   payload (no private key — the miner private key never leaves the wallet):
   ```
   irium-wallet poawx-register --emit-only \
     --pool-pubkey <66hex> --network-id <1|2> \
     --addr <miner-address> --worker <worker> \
     --expiry-height <N> --fee-bps 0 > poawx-delegation.json
   ```
4. Operator POSTs the payload to the loopback-only endpoint:
   ```
   curl -sS -X POST http://127.0.0.1:<delegation-port>/poawx/delegation \
     -H 'content-type: application/json' --data @poawx-delegation.json
   ```
5. Miner runs stock cpuminer unchanged (see §15).

> **Tip (optional, safe):** before step 4, validate the received payload with the
> read-only helper — it checks the JSON shape (exactly `delegation,worker,miner_pkh`),
> the 226-byte delegation length, and refuses anything containing a secret token, and
> prints the loopback curl for you to run:
> `scripts/poawx-delegation-validate.sh poawx-delegation.json --port <delegation-port>`.
> The helper makes no network call and posts nothing.

> **`--emit-only` is implemented locally in Phase 19B commit `1628843`** (local-only
> checkpoint, not pushed). It lets an external miner register without exposing the
> loopback endpoint or being granted SSH/tunnel access. A real external pilot still
> requires explicit operator approval before live testing. An SSH tunnel may be used
> ONLY as an emergency/dev fallback, never as the pilot path.

### A.2 Stratum environment (delegated mode-1)

```
IRIUM_NETWORK=testnet IRIUM_STRATUM_POAWX=1 IRIUM_STRATUM_NATIVE_REWARDABLE_ENABLED=1
IRIUM_STRATUM_ADAPTER_MODE=native_rewardable IRIUM_STRATUM_VARDIFF_ENABLED=0 STRATUM_DEFAULT_DIFF=1
# STRATUM_DEFAULT_DIFF=1 = share diff at the block target; avoids the cpuminer flood/stall a
# sub-1 value causes on native_rewardable. This is a stratum SHARE knob, NOT chain difficulty.
IRIUM_POAWX_DELEGATION_BIND=127.0.0.1:<delegation-port>   # non-loopback is refused
IRIUM_POAWX_STATE_DIR=<state-dir>
IRIUM_POAWX_DELEGATE_KEY_PATH=<state-dir>/poawx_delegate_key.hex   # signer-only, 0600
IRIUM_POAWX_DELEGATIONS_PATH=<state-dir>/poawx_delegations.json    # no private keys
```

### A.3 Validation checklist (delegated mode-1)

- [ ] Delegation stored (registry entry present; **no private key** in the file).
- [ ] Pool identity reports `fee_bps=0`.
- [ ] Miner connects from the expected IP; `adapter_kind=native_rewardable`.
- [ ] `submit_block_extended … receipts=N` (mode-1) → `BLOCK_ACCEPTED`.
- [ ] Block has a non-zero `irx1_root`; the block receipt carries the **embedded 226-byte delegation**.
- [ ] Coinbase pays the **miner pkh only** (single p2pkh output); **delegate pkh NOT paid**.
- [ ] Observer node (if used) reaches the same height + tip hash.
- [ ] **No compat / variant-sweep promotion** (single deterministic canonical reconstruction only).

### A.4 Operator caveats (Phase 18C/18D)

- Activation height is a testnet/devnet config value, **never** a mainnet value.
- The two-phase bootstrap (poawx-inactive → block 1 → active) is an isolated-E2E
  artifact, not part of a pilot against an already-live chain.
- A standalone observer/sync node (copied binary, no repo) must run from a CWD
  containing `bootstrap/anchors.json` and `bootstrap/trust/allowed_anchor_signers`
  (genesis is embedded; anchors are not).
- On testnet/devnet feed peers via node-config `p2p_seeds` or env `IRIUM_MANUAL_PEERS`;
  `IRIUM_STATIC_PEERS` is mainnet-only for the dialer.
- Never `pkill -f` the node binary path; kill by exact PID after `/proc/<pid>/cmdline` check.
