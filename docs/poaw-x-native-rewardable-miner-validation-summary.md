# PoAW-X native_rewardable Miner Path — Validation Summary

**Status:** Internally proven end-to-end (Phases 13-15). No external party yet.
**Proven code commit:** `fad21c4` "poawx: native_rewardable CPU-miner block route (testnet/devnet, gated)"
**Code branch:** `origin/testnet/poawx-phase13-native-rewardable-cpuminer-e2e` @ `fad21c461ee996e85f8c612b4265f3228d65f8a9`

> **TESTNET/DEVNET ONLY.** No real coins, no rewards, no mainnet compatibility. Mainnet stayed untouched throughout (proof below).

---

## 1. The proven route

```
cpuminer/minerd -> native_rewardable -> submit_block_extended -> irx1_root -> BLOCK_ACCEPTED -> peer sync
```

- Rewardable CPU mining uses the gated **native_rewardable** route, **not** rewardable `cpuminer_compat`.
- `cpuminer_compat` stays **non-rewardable** on PoAW-X (compatibility / share accounting only).
- **No variant sweep** may promote a PoAW-X block; promotion is gated on a single deterministic canonical reconstruction (no byte-order guessing).
- Rewardable blocks come only from the deterministic native path; `submit_block_extended` commits the receipt and the coinbase carries the matching `irx1_root`.
- **Mainnet byte-identical / unaffected** when `poawx_enabled=false` (legacy routing, diff-1 floor, single-output coinbase all unchanged).

## 2. Proof chain

| Phase | Proof | Result |
|---|---|---|
| **13** | Real cpuminer (pooler 2.5.1) -> native_rewardable -> committed local height-1 block with correct `irx1_root`. Includes the empirical finding that a real cpuminer header is byte-identical to the canonical reconstruction (the old `v_be` suspicion was a trivially-easy-target sweep artifact). | PASS |
| **14** | Same path synced across two VPS nodes over real cross-VPS P2P (Node A mines, Node B syncs). Same height, same block hash, same `irx1_root`. | PASS |
| **15** | Self-hosted remote miner on VPS-2 connected to VPS-1 stratum over the **public internet** (source-restricted firewall), mined through native_rewardable, produced a block accepted by Node A and synced to Node B; payout/worker address recorded correctly. | PASS |

Phases 14 and 15 were **validation-only and added no code commits** — the proven code is `fad21c4`.

## 3. Stratum config that activates the route (testnet/devnet)

```
IRIUM_NETWORK=devnet
IRIUM_STRATUM_ADAPTER_MODE=auto
IRIUM_STRATUM_NATIVE_REWARDABLE_ENABLED=1
IRIUM_STRATUM_POAWX=1
IRIUM_STRATUM_MINER_FAMILY=cpuminer
STRATUM_DEFAULT_DIFF=0.001     # sub-1 floor allowed only under the non-mainnet gate
STRATUM_CARRIERS=off
IRIUM_POAWX_MODE=active
```

## 4. Mainnet safety proof (held across all three phases)

- VPS-1 mainnet: `iriumd.service` active, MainPID unchanged, official binary hash `7c07ae2c30dd1c5a` (`/home/irium/mainnet/bin/iriumd-current`).
- VPS-2/irium-eu mainnet: active, MainPID unchanged, official hash `7c07ae2c30dd1c5a`.
- All VPS-1 production pool services (`irium-pool-api`, `irium-stratum`, `-443`, `-legacy`, `-solo`) active throughout.
- RPC/status kept private on `127.0.0.1`; only the devnet P2P port (VPS-2-restricted) and the operator-selected stratum port (miner-IP-restricted) were ever exposed, both temporary; firewall rules removed and verified absent after each pilot.
- Devnet teardown by exact pidfile/port only — never `pkill`/`killall`/process-name matching (ref `poaw-x-mainnet-cleanup-incident.md`).

## 5. Final result

The **native_rewardable CPU-miner path is internally proven end-to-end**: a standard cpuminer mines through the deterministic gated route, the block is accepted with the correct `irx1_root`, and it propagates/syncs to a second node over real P2P — with mainnet and the production pool untouched.

## 6. Remaining step

Invite a **trusted external miner** only when one actually exists, and only under **source-restricted, temporary** firewall rules (require the miner public IP first; never `Anywhere`; remove immediately after). See `poaw-x-trusted-miner-pilot-runbook.md` (sections 0, 14-18) and `poaw-x-trusted-miner-operator-checklist.md` (sections N, O).
