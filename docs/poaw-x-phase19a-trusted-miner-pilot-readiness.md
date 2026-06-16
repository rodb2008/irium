# PoAW-X Phase 19A — Trusted External Miner Pilot Readiness

**Version:** 1.0 (Phase 19A — planning/readiness, docs-only)
**Branch:** `testnet/poawx-phase19-trusted-miner-pilot-readiness`
**Base checkpoint:** `origin/testnet/poawx-phase18-auto-receipts-zero-fee-rewards` @ `491a4de`
**Status:** Readiness documentation only. **No code in Phase 19A.** The Phase 19B
`--emit-only` wallet change is **implemented locally in Phase 19B commit `1628843`**
(branch `testnet/poawx-phase19b-wallet-emit-only-delegation` — **local-only checkpoint,
not pushed**). A real external miner pilot **still requires explicit operator approval
before live testing**.
**Network scope:** testnet / devnet only. Mainnet delegated (mode-1) path is
consensus-gated and hard-rejected.

> **TESTNET ONLY.** No real Irium coins, no real rewards, no mainnet compatibility.
> The chain may reset at any time. Never use mainnet wallets/keys/addresses.

---

## 0. What changed since the v2.0 pilot docs

The earlier pilot docs (runbook/checklist/invite/guide, all v2.0) describe the
**`native_rewardable` + manually-seeded-receipt** route, where the operator seeds a
receipt for `miner == worker`. **Phase 18 replaced that with the non-custodial
delegated (mode-1) flow:** the miner signs a one-time delegation authorizing the
pool delegate key to produce receipts on the miner's behalf; the miner wallet stays
the sole payout identity; the official pool is 0% fee and never a payout identity.

This route is proven end-to-end:
- **Phase 18C** — single-node E2E: stock cpuminer, no manual seed, mode-1 block
  accepted via `submit_block_extended`, `irx1_root` present, miner paid, delegate not
  paid, fee 0%.
- **Phase 18D** — two-node cross-VPS sync: Node B independently validated the
  delegated receipt; identical hash / `irx1_root` / embedded 226-byte delegation.

Full proof chain: `docs/poaw-x-phase18-delegated-receipts-validation-summary.md`.

---

## 1. What is ready for a trusted external miner

- **Stock cpuminer support** — unchanged `minerd -a sha256d …` command; no miner-side signing.
- **Wallet delegation registration** — `irium-wallet poawx-register` signs in memory; the private key never leaves the wallet; the registry stores no private key.
- **Pool mode-1 receipt production** — the pool builds the mode-1 receipt from the stored delegation via `/poawx/assignment`.
- **`submit_block_extended`** with `receipts=N` (mode-1); legacy `submit_block` is 405-gated when PoAW-X is active.
- **Cross-node validation** — an independent peer validates the delegated receipt on sync.
- **Official 0% fee** — `fee_bps == 0` enforced at wallet, pool, and consensus; `fee_bps > 0` fails closed.
- **Direct miner payout** — single p2pkh output to the miner pkh; the delegate key is signer-only and never paid.

## 2. What is NOT ready for a public testnet

- **Broad public endpoint exposure** — stratum stays source-restricted; RPC/status/delegation/metrics stay loopback-only. No open-access hardening validated.
- **Many concurrent miners** — the pool currently commits a mode-1 block only when all pending receipts belong to the connected miner (single-miner clean; multi-miner needs per-worker coinbase payouts).
- **Long soak** — only short E2E runs so far; no sustained production/reorg soak.
- **Third-party (fee-bearing) pools** — intentionally unsupported; `fee_bps > 0` is rejected.
- **Metrics / dashboard** — only gated `IRIUM_POAWX_PRODUCER_TRACE` logs.
- **Broader miner compatibility** — no ASIC/firmware matrix on the mode-1 path.
- **Mainnet activation** — mode-1 is hard-off on mainnet; activation is a later governance step.

## 3. Trusted pilot topology

| Host | Role | Exposure |
|---|---|---|
| **VPS-1** | testnet node A + stratum + loopback delegation server | stratum port: **source-restricted to the miner IP**; node P2P: source-restricted to VPS-2 (only if an observer node is used); RPC / status / delegation / metrics: **loopback only** |
| **External miner** | stock cpuminer | outbound TCP to the VPS-1 stratum port only |
| **VPS-2 (optional)** | observer / sync node B | dials the VPS-1 public P2P; no inbound rule on VPS-2 |

- **Exposed (source-restricted only):** the stratum port (→ miner IP); Node A P2P (→ VPS-2, only if an observer is used).
- **Loopback only (never exposed):** RPC, status, **the delegation server**, metrics.
- **Firewall:** at most two temporary source-restricted `ufw allow from <IP> to any port <P> proto tcp` rules; **never `Anywhere`**; removed and verified absent after the pilot.
- **Operator handoff:** every `sudo ufw` open/close (the agent prints, the operator runs).

## 4. Chosen registration model — `--emit-only` (LOCKED)

The delegation endpoint **must stay loopback-only.** We do **not** expose
`/poawx/delegation`, and we do **not** require giving external miners SSH/tunnel
access to VPS-1. The selected external-pilot design is the wallet `--emit-only` mode:

1. Operator runs the pool locally with the loopback-only delegation endpoint.
2. Operator reads the pool identity locally (`GET http://127.0.0.1:<delegation-port>/poawx/pool-identity`): `pool_pubkey`, `network_id`, `fee_bps=0`, `domain`.
3. Operator sends that **public** identity info to the trusted miner out-of-band.
4. Miner runs the wallet locally in `--emit-only` mode and **signs the delegation locally**.
5. Miner sends back **only the signed delegation JSON/payload** (no private key).
6. Operator POSTs that payload to the loopback-only `/poawx/delegation` endpoint.
7. Miner runs stock cpuminer, unchanged.

Guarantees: the miner private key never leaves the miner wallet; the operator only
ever receives a signed delegation payload; the delegation endpoint stays loopback-only;
RPC stays private; the official fee stays 0%; `fee_bps > 0` stays rejected.

> **SSH tunnel is NOT the recommended pilot path.** It may be documented only as an
> emergency/dev fallback, never as the primary trusted-miner flow.

### 4.1 Phase 19B implementation (implemented locally in commit `1628843`)

`--emit-only` is **implemented locally in Phase 19B commit `1628843`** (branch
`testnet/poawx-phase19b-wallet-emit-only-delegation` — **local-only checkpoint, not
pushed**). The online `irium-wallet poawx-register` still performs GET pool-identity →
sign → POST in one shot; `--emit-only` adds a build-and-print mode that signs locally
and emits the canonical 226-byte delegation payload **without** contacting the pool.
The pool delegation server **still refuses any non-loopback bind** — the endpoint
stays loopback-only. The code is implemented and unit-tested; **a real external pilot
still requires explicit operator approval before live testing.**

**Phase 19B CLI shape (implemented and tested in commit `1628843`):**

Miner side (offline; needs only the wallet + the operator-supplied public identity):
```
irium-wallet poawx-register --emit-only \
  --pool-pubkey <66hex> --network-id <1|2> \
  --addr <miner-address> --worker <worker> \
  --expiry-height <N> --fee-bps 0 > poawx-delegation.json
```

Operator side (loopback only):
```
curl -sS -X POST http://127.0.0.1:<delegation-port>/poawx/delegation \
  -H 'content-type: application/json' --data @poawx-delegation.json
```

Phase 19B preserves all current fail-closed behavior: `--fee-bps` other than 0 is
rejected; `--emit-only` performs the same self-verify (`verify_signature`) before
printing; the output contains no private key (the miner private key never leaves the
wallet — the operator receives only the signed delegation payload); stdout carries
only the JSON payload. The official fee remains 0%.

## 5. Miner command (stock cpuminer, unchanged)

```
minerd -a sha256d -o stratum+tcp://<VPS-1-public-ip>:<stratum-port> -u <wallet-address>.<worker> -p x -t <threads>
```
- `<worker>` must match the worker named in the registered delegation.
- Suggested threads: 2–3 for a CPU pilot.
- **Operator-side success signals:** `adapter_kind=native_rewardable`; accepted share (`{"result":true}`); `submit_block_extended … receipts=1`; `[BLOCK_ACCEPTED] accepted_height=N`; non-zero `irx1_root`; single p2pkh output to the miner pkh.

## 6. Stratum environment for the delegated mode-1 pilot

```
IRIUM_NETWORK=testnet
IRIUM_STRATUM_POAWX=1
IRIUM_STRATUM_NATIVE_REWARDABLE_ENABLED=1
IRIUM_STRATUM_ADAPTER_MODE=native_rewardable
IRIUM_STRATUM_VARDIFF_ENABLED=0
STRATUM_DEFAULT_DIFF=1   # share diff = block target; avoids cpuminer flood on native_rewardable; NOT chain difficulty (LWMA-144)
IRIUM_POAWX_DELEGATION_BIND=127.0.0.1:<delegation-port>   # loopback-only; non-loopback is refused
IRIUM_POAWX_STATE_DIR=<state-dir>
IRIUM_POAWX_DELEGATE_KEY_PATH=<state-dir>/poawx_delegate_key.hex   # signer-only, 0600
IRIUM_POAWX_DELEGATIONS_PATH=<state-dir>/poawx_delegations.json    # no private keys
# optional, no secrets: IRIUM_POAWX_PRODUCER_TRACE=1
```

## 7. Observability checklist (operator, private)

delegation stored (registry entry, no private key) → assignment fetched
(`/poawx/assignment`) → mode-1 receipt built → `irx1_root` present in coinbase →
`submit_block_extended` used → `BLOCK_ACCEPTED` → miner pkh paid (single p2pkh) →
**delegate pkh NOT paid** → **fee_bps == 0** → embedded delegation present in the
block receipt → peer sync verified (same height/hash on the observer node) →
**no compat / variant-sweep promotion**.

## 8. Operator caveats (carried from Phase 18C/18D)

- **Activation height is a testnet/devnet config value, never a mainnet value.** 18C/18D used activation height 2 only for an isolated bring-up.
- **The two-phase bootstrap (poawx-inactive → block 1 → active) is an isolated-E2E artifact**, not part of a pilot against an already-live testnet chain.
- **A standalone sync node (copied binary, no repo) must run from a CWD containing `bootstrap/anchors.json` and `bootstrap/trust/allowed_anchor_signers`** — otherwise iriumd exits with "Failed to load anchors". Genesis is embedded in the binary; anchors are not.
- **On testnet/devnet, feed peers via node-config `p2p_seeds` or env `IRIUM_MANUAL_PEERS` / `IRIUM_ADDNODE`.** `IRIUM_STATIC_PEERS` is mainnet-only for the dialer and is ignored on non-mainnet networks.
- **Never `pkill -f` the node binary path** (it can match the SSH command's own argv and kill the session, and the bare `iriumd` substring reaches the production node). Kill by exact PID after confirming `/proc/<pid>/cmdline`.
- Source-restricted UFW only; exact-pidfile cleanup; no `Anywhere` rules; no production paths.

## 9. Abort / rollback plan

1. Miner stops mining (Ctrl+C).
2. Operator stops the stratum, then the node, **by exact pidfile** (never `pkill`/`killall`).
3. Remove the temporary testnet run directory; verify all pilot ports clear.
4. Operator deletes each temporary UFW rule and verifies absent.
5. Preserve **sanitized** node + stratum logs (mask IPs, strip tokens/auth) under a dated artifact dir.
6. Verify mainnet/prod untouched on both hosts (PIDs unchanged, official binary hash intact, services active).

## 10. Phase 19A vs 19B boundary

- **Phase 19A (this phase):** docs only — this readiness doc plus updates to the
  runbook, checklist, invite, guide, acceptance-criteria, and stop-conditions to the
  delegated mode-1 route with `--emit-only` as the selected design. No code, no live
  processes.
- **Phase 19B (implemented locally in commit `1628843`; local-only, not pushed):** the
  wallet `--emit-only` mode is implemented and unit-tested, so a trusted external miner
  can register a delegation without exposing the loopback endpoint or granting SSH
  access. A real external pilot still requires explicit operator approval before live
  testing; chain difficulty remains automatic via LWMA-144 (never manually controlled).
- **Phase 19C (local E2E — EXECUTED + PASSED 2026-06-16):** see
  `docs/poaw-x-phase19c-local-e2e-test-plan.md`. The isolated, loopback-only, single-VPS
  end-to-end validation of the `--emit-only` path passed all 17 proofs (block 2 committed
  mode-1 via `submit_block_extended`, miner-paid/delegate-unpaid/fee-0%, embedded
  delegation, registry has no private key). Local-only, not pushed.
- **Live external trusted-miner pilot (operator-approved; not started):** the firewall
  handoff is **operator-run only** (the agent never runs `sudo`/`ufw`) — stratum is the
  only miner-facing port, source-restricted to the trusted-miner IP, never `Anywhere`;
  `/poawx/delegation`, RPC, status, metrics stay loopback-only. Exact open/verify/close
  commands + pre-open/post-close checklists: runbook **Appendix B** and operator checklist
  **Section Q**.
