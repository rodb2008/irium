# PoAW-X Phase 20 — Miner Onboarding at Scale (package + helper scripts)

**Status:** COMPLETE (docs + helper scripts). Onboarding remains **invite/operator-gated**;
no public endpoint, no public RPC, no `Anywhere` firewall. This consolidates the 19A–19D
flow into a repeatable onboarding runbook for additional trusted miners.

> `STRATUM_DEFAULT_DIFF=1` is the stratum **share** difficulty (not chain difficulty). Chain
> difficulty is automatic via LWMA-144. Delegation endpoint and RPC stay loopback-only.

> **Third-party pools (Phase 20 Step 4, opt-in).** The official Irium pool is **0% fee** and the
> default `poawx-register` flow is unchanged. A third-party operator may charge a capped fee (max
> **2% = 200 bps**) taken **only from the miner's PRIMARY allocation**. The miner opts in explicitly
> and signs the fee terms into the delegation:
> `poawx-register … --third-party-pool --fee-bps <1..200> --fee-pkh <base58-addr|40hex>`
> (works with `--emit-only` too). The wallet refuses `--fee-bps>0` without `--third-party-pool` +
> `--fee-pkh`, refuses fees over the cap, and (online) refuses unless the pool advertises the exact
> same terms. Fees are mainnet-hard-off.

> **Role precommit/reveal protocol (Phase 20 Step 6B, local/testnet, opt-in).** When the operator
> enables `IRIUM_POAWX_ROLE_PROTOCOL_ENABLED=1`, the pool collects real (non-synthetic) role data:
> the miner emits a **precommit** before the target height (hides secret/nonce) and a **reveal** at
> the target height, via the wallet helpers
> `irium-wallet poawx-role-precommit …` / `poawx-role-reveal … --prev-hash <h>`. Both are POSTed to
> the operator's **loopback-only** `/poawx/role-precommit` + `/poawx/role-reveal` endpoints (same
> loopback delegation server; no public bind). Production prefers collected role data over the
> testnet/devnet-only synthetic fallback (`IRIUM_POAWX_SYNTHETIC_ROLE_CLAIMS=1`). These endpoints are
> **not public gossip** and are mainnet-hard-off; chain difficulty stays automatic via LWMA-144.

> **Role gossip plumbing (Phase 20 Step 6C, testnet/devnet, opt-in).** Step 6C adds the *plumbing* for
> propagating role precommits/reveals between nodes: forward-compatible P2P wire message types and a
> conservative pool-side gossip engine (validate → dedupe → height-window → store → rebroadcast only
> valid), gated by `IRIUM_POAWX_ROLE_GOSSIP_ENABLED=1` (in addition to
> `IRIUM_POAWX_ROLE_PROTOCOL_ENABLED=1`), mainnet hard-off, default off. **This step is payload +
> validation + in-memory-relay only** — the live cross-process bridge between the node P2P bus and the
> pool store is a documented follow-up, so operators onboarding miners today still use the
> loopback-only Step 6B endpoints. **No public ports** are opened by Step 6C.

> **Role gossip live bridge (Phase 20 Step 6D, testnet/devnet, opt-in).** Step 6D wires the live
> cross-process bridge: iriumd ingests `PoawxRolePrecommit`/`PoawxRoleReveal` P2P gossip into a
> node-side cache and rebroadcasts; four **loopback-only** node RPC endpoints
> (`/poawx/role-gossip/{precommit,reveal,precommits,reveals}`) let the pool forward local
> submissions (→ P2P broadcast) and fetch node-collected gossip into its store before producing a
> block. Enable with `IRIUM_POAWX_ROLE_GOSSIP_ENABLED=1` on **both** iriumd and the pool (mainnet
> hard-off, default off); optional `IRIUM_POAWX_ROLE_GOSSIP_WINDOW` (default 64) and
> `IRIUM_POAWX_ROLE_GOSSIP_NODE_RPC` (defaults to the pool's node RPC base). **All bridge endpoints
> are loopback-only** — bind iriumd's RPC to `127.0.0.1` (as in the Step 5A recipe); **no public
> ports** are opened by Step 6D. The next steps are a local loopback live E2E, then a two-VPS live
> role-gossip E2E (only with the operator firewall handoff). Production still prefers real
> (bridged) role data over the synthetic fallback (`IRIUM_POAWX_SYNTHETIC_ROLE_CLAIMS=1`).

> **Live validation done (Steps 6E/6F, 2026-06-18).** The role-gossip → Phase 20 production path
> is validated live: **6E** (single-VPS loopback) and **6F** (two-VPS, role gossip over real
> cross-VPS P2P with an observer node validating byte-identical) each produced an official fee-0
> block and a third-party-fee block from **collected role-gossip data, synthetic OFF**, with
> hidden-precommit enforcement and restart/reload preservation. 6F used an **operator-only,
> source-restricted** firewall (only stratum + P2P public to the one miner VPS; removed after);
> delegation/RPC/status/metrics stayed loopback-only. See
> `poaw-x-phase20-step6e-loopback-role-gossip-e2e.md` and
> `poaw-x-phase20-step6f-two-vps-role-gossip-e2e.md`. **Still open before any public rollout:** a
> public/external (non-self-operated) miner test, and the remote slow-cpuminer low-devnet PoW
> caveat (remote CPU connects/authorizes/receives work but may not land a diff-1 share — an
> environment limitation, not Phase 20 logic). Mainnet remains disabled; LWMA-144 untouched.

## 1. Miner requirements
- A stock SHA-256d CPU miner (`cpuminer`/`minerd`) — unchanged, no version-rolling needed.
- The Irium wallet binary with `poawx-register --emit-only` (Phase 19B+).
- Outbound TCP to the operator's stratum host:port (operator-provided).
- A throwaway testnet address; **never** a mainnet wallet/seed.

## 2. Onboarding flow (per miner)
1. **Operator → miner (out-of-band):** the public pool identity package — `pool_pubkey`,
   `network_id`, `fee_bps=0`, `domain`, expiry-height guidance, stratum host:port, worker
   name. Generate with `scripts/poawx-pool-identity-package.sh` (read-only; loopback GET).
2. **Miner (offline):** sign the delegation locally; the private key never leaves the wallet:
   ```
   irium-wallet poawx-register --emit-only --pool-pubkey <66hex> --network-id <1|2> \
     --addr <miner-address> --worker <worker> --expiry-height <N> --fee-bps 0 > poawx-delegation.json
   ```
3. **Miner → operator:** send **only** `poawx-delegation.json` (no seed/private key).
4. **Operator:** validate then POST over loopback:
   ```
   scripts/poawx-delegation-validate.sh poawx-delegation.json --port <delegation-port>
   curl -sS -X POST http://127.0.0.1:<delegation-port>/poawx/delegation \
     -H 'content-type: application/json' --data @poawx-delegation.json
   ```
5. **Operator firewall handoff (operator-run only):** source-restrict the stratum port to the
   miner IP (runbook Appendix B / checklist Section Q). Never `Anywhere`.
6. **Miner mines:**
   ```
   minerd -a sha256d -o stratum+tcp://<pool-host>:<stratum-port> -u <miner-address>.<worker> -p x -t <threads>
   ```

## 3. Expected logs (operator side, success)
`[conn] accepted from <miner-ip>` → `assigned_diff=1` → (on a found block)
`submit_block_extended … receipts=1` → `BLOCK_ACCEPTED accepted_height=N`; block has a
non-zero `irx1_root`, coinbase pays the miner pkh (single output), delegate not paid, fee 0%.

## 4. Common failures
| Symptom | Cause | Fix |
|---|---|---|
| all shares `low_difficulty`, miner floods/stalls | sub-1 share diff | use `STRATUM_DEFAULT_DIFF=1` |
| miner stalls after minutes | cpuminer flakiness | supervised restart (harness) |
| delegation `rejected` | wrong worker/pkh/expired/network/fee>0 | re-issue emit-only with correct args |
| no block in window | diff-1 variance at the automatic chain target | extend window; do NOT lower chain difficulty |
| miner can't connect | firewall rule missing/closed | operator opens source-restricted rule |

## 5. Privacy & safety (non-negotiable)
- Miner sends only the signed payload — never seed/private key.
- Operator never receives a private key; registry stores none.
- No public delegation endpoint; no public RPC; no `Anywhere` firewall.
- Exact-pidfile teardown only; never `pkill`/`killall`.

## 6. Helper scripts (all read-only / no sudo / no firewall changes)
- `scripts/poawx-pool-identity-package.sh` — print the public identity package for a miner.
- `scripts/poawx-delegation-validate.sh` — validate a miner's payload before POST.
- `scripts/poawx-pilot-readiness-check.sh` — pre-flight readiness (ports/PIDs/loopback binds).
- `scripts/poawx-firewall-template.sh` — print the exact operator UFW open/verify/close
  commands (prints only; never executes).
- `scripts/poawx-log-passfail.sh` — scan node+stratum logs for PASS/FAIL signals.
