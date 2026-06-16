# PoAW-X Phase 20 — Miner Onboarding at Scale (package + helper scripts)

**Status:** COMPLETE (docs + helper scripts). Onboarding remains **invite/operator-gated**;
no public endpoint, no public RPC, no `Anywhere` firewall. This consolidates the 19A–19D
flow into a repeatable onboarding runbook for additional trusted miners.

> `STRATUM_DEFAULT_DIFF=1` is the stratum **share** difficulty (not chain difficulty). Chain
> difficulty is automatic via LWMA-144. Delegation endpoint and RPC stay loopback-only.

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
