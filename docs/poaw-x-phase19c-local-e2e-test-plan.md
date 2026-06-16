# PoAW-X Phase 19C — Isolated Local E2E Test Plan for `poawx-register --emit-only`

**Version:** 1.0 (Phase 19C — test plan, NOT yet executed)
**Branch:** `testnet/poawx-phase19b-wallet-emit-only-delegation` (**local-only, not pushed**)
**Validated code under test:** wallet `--emit-only` = commit `1628843` (+ `7767ac1` test/refactor polish).
**Status:** This is the **exact next live-test plan**. It has **not** been executed. Running
it requires explicit operator approval. It is **isolated, loopback-only, single-VPS**, and
touches no mainnet/prod services.

> **TESTNET/DEVNET ONLY.** No real coins, no rewards, no mainnet compatibility.
> The chain may reset at any time. Never use mainnet wallets/keys/addresses.

---

## 0. Purpose

Prove the Phase 19B non-custodial `--emit-only` registration path works end-to-end on an
isolated, loopback-only testnet: a stock cpuminer mines a mode-1 delegated block whose
delegation was registered **without** exposing the delegation endpoint and **without** the
online wallet POST path — the miner signs locally and the operator POSTs the signed payload
over loopback.

## 1. Ports (all `127.0.0.1` only)

| Service | Bind |
|---|---|
| Node RPC | `127.0.0.1:39711` |
| Node status | `127.0.0.1:39708` |
| Stratum | `127.0.0.1:39712` |
| Delegation server | `127.0.0.1:39713` |
| Stratum metrics | `127.0.0.1:39714` |
| P2P | none (single node; no peers) |

No public ports, no firewall, no `0.0.0.0`, no `sudo`/systemd. These loopback ports do not
collide with mainnet (`38300/8080/38291`) or the production pool.

## 2. `$TROOT` layout

`TROOT=/home/irium/phase19c-emitonly-e2e`
```
$TROOT/node/            # IRIUM_DATA_DIR
$TROOT/stratum-state/   # poawx_delegate_key.hex (0600), poawx_delegations.json (no privkeys)
$TROOT/wallet/wallet.json   # IRIUM_WALLET_FILE (throwaway)
$TROOT/logs/  $TROOT/pids/  $TROOT/artifact/
```

## 3. Preflight (read-only; abort on any failure)

1. `git branch --show-current` = `…phase19b…`; working tree clean; branch local-only (no push).
2. Build at HEAD: `cargo build --release --bin iriumd --bin irium-wallet`; in `pool/irium-stratum`: `cargo build --release`.
3. `irium-wallet poawx-register --emit-only` → fails with a *missing-arg* error (not "unknown flag").
4. Mainnet/prod alive: PIDs `219530` + `4042500-4042503` (record for after).
5. Ports `39708/39711/39712/39713/39714` free; no leftover `$TROOT`.
6. `cpuminer` present (`/home/irium/phase13-devnet/cpuminer-src/minerd`); `bootstrap/anchors.json` present in repo cwd.

## 4. Procedure (exact commands available in the runbook Appendix A; summary here)

Two-phase bootstrap (isolated-E2E artifact only; see §6):
1. **Phase 1** — node poawx-INACTIVE, stratum up, stock cpuminer → mine **block 1** (legacy); create the throwaway wallet (`IRIUM_WALLET_FILE`), capture address.
2. Stop miner/stratum/node by exact pidfile.
3. **Phase 2** — restart node **active** (`IRIUM_POAWX_MODE=active`, `IRIUM_POAWX_ACTIVATION_HEIGHT=2`, `IRIUM_POAWX_DELEGATION_ACTIVATION_HEIGHT=2`), restart stratum (delegation server on `127.0.0.1:39713`, producer trace on).
   - **(operator, loopback)** `curl -sS http://127.0.0.1:39713/poawx/pool-identity` → `pool_pubkey`, `network_id`, `fee_bps=0`.
   - **(miner, offline)** `irium-wallet poawx-register --emit-only --pool-pubkey <66hex> --network-id <id> --addr <addr> --worker w1 --expiry-height 1000 --fee-bps 0 > poawx-delegation.json`
   - **(operator, loopback)** `curl -sS -X POST http://127.0.0.1:39713/poawx/delegation -H 'content-type: application/json' --data @poawx-delegation.json`
   - **(miner)** `minerd -a sha256d -o stratum+tcp://127.0.0.1:39712 -u <addr>.w1 -p x -t 3` → mine **block 2** (mode-1).

## 5. Success criteria (17 required proofs)

1. Isolated node + stratum on fresh `$TROOT`.
2. RPC/status/delegation/metrics/stratum all bound `127.0.0.1` only (`ss -ltnp`).
3. `poawx-register --emit-only` produced `poawx-delegation.json`.
4. Operator POSTed it via loopback curl to `/poawx/delegation`.
5. Stock cpuminer unchanged.
6. Pool built the mode-1 delegated receipt from the registry (producer trace).
7. `submit_block_extended … receipts=1` → `BLOCK_ACCEPTED` height 2.
8. Committed block has a non-zero `irx1_root`.
9. Block receipt carries the embedded 226-byte delegation.
10. Coinbase pays the miner pkh (single p2pkh output).
11. Delegate pkh is **not** an output.
12. Fee 0% (pool identity + delegation `fee_bps=0`; single output).
13. No private key/seed in the emit-only output (`poawx-delegation.json` keys are exactly `delegation,worker,miner_pkh`; wallet privkey hex absent from output + stderr).
14. Emit-only made no GET/POST/network call (takes no `--pool`; strict re-run with the delegation server stopped still succeeds).
15. Delegation endpoint stayed loopback-only (`IRIUM_POAWX_DELEGATION_BIND=127.0.0.1:39713`; non-loopback is refused by code).
16. Chain difficulty/LWMA-144 code untouched (`git diff --name-only` vs the Phase 18 base = wallet + docs only). See §6.
17. Mainnet/prod PIDs unchanged before and after.

## 6. Difficulty clarification (three distinct concepts — kept separate)

- **Chain/block difficulty = automatic via LWMA-144 target adjustment.** Consensus code,
  **untouched**, never manually set in this E2E.
- **PoAW-X puzzle difficulty** (`IRIUM_POAWX_PUZZLE_DIFFICULTY_BITS`) — a PoAW-X *puzzle*
  knob for the isolated bring-up only, not chain difficulty.
- **Stratum share difficulty** (`STRATUM_DEFAULT_DIFF=0.001`, vardiff off) — a non-mainnet
  *share-acceptance* floor for the E2E only, not chain difficulty.

The two-phase bootstrap and these knobs are **isolated-E2E artifacts only**, never part of
normal or mainnet operation.

## 7. Abort criteria

Any preflight failure; any pilot port already in use; mainnet/prod PID or path appears as a
kill target; block 2 not mode-1 (no `irx1_root`/no embedded delegation); delegate pkh paid;
`fee_bps>0` anywhere; private key found in emit-only output; delegation endpoint bound
non-loopback; emit-only fails offline. On abort: stop by exact pidfile, `rm -rf $TROOT`,
verify mainnet/prod intact.

## 8. Mainnet/prod safety

Record before and confirm after: `219530` (VPS-1 mainnet, `/home/irium/mainnet/bin/iriumd-current`)
and `4042500-4042503` (prod pool) all alive; ports/paths never touched. Cleanup is guarded by
a pid-allowlist + `/proc/<pid>/exe` path check so a prod pid/path can never be killed. No
`sudo`/firewall/systemd. No `pkill`/`killall` — exact pidfiles only.

## 9. Repository policy (local-only)

Phase 19C is execution + verification only: **no git commits of run output, no `git push`,
no remote branch recreation, no tags/PR/merge.** PoAW-X work stays local on the VPS until the
owner explicitly approves a final push/release. The only git usage during the run is
read-only preflight.

## 10. After this E2E

A green Phase 19C completes the local validation chain (18C single-node, 18D two-node sync,
19C emit-only registration). The remaining gate before a **real external miner pilot** is
purely operational and requires explicit operator approval: source-restricted firewall for
the stratum port (operator-run `sudo ufw`), the out-of-band exchange of the public pool
identity, and live monitoring per the trusted-miner runbook/checklist. No code change is
expected to be required.
