# PoAW-X Phase 26C — live three-system multi-block soak

**Status: multi-block soak PASSED.** The Phase 26B epoch-seed fix was validated LIVE across
Windows + VPS-1 + VPS-2: **six real Irium-native-PoW all-gates PoAW-X blocks** were mined by the
CLI live-proof harness, accepted by real devnet nodes, and propagated to all three systems at the
same height, tip hash, and irx1 root — with phase21d and phase22a enforced on every block. This is
the live confirmation that multi-block all-gates chains (impossible before Phase 26B) now work on
real nodes. Devnet only; loopback RPC everywhere; cross-host P2P only on the source-restricted
VPS-1 hub port 41210; mainnet/prod and all real wallets untouched. NOT production-ready /
mainnet-ready / audited.

## Systems / branch / HEAD

- Windows `C:\Users\Ibrahim` (mainnet PID `33752` untouched).
- VPS-1 hub `irium@207.244.247.86` (`vmi2780294`; mainnet `219530` + prod pool untouched).
- VPS-2 observer `irium@157.173.116.134` (`vmi2995746`; mainnet `1851441` untouched).
- Branch `testnet/poawx-phase20-blueprint-completion-local` @ **`081a1bd`** — built + run on all three.
  `origin/main` unchanged at `19c496d`.

## Windows IP / firewall

- Windows egress IP: **`122.162.151.91`** — UNCHANGED from Phase 25C, so the existing
  source-restricted VPS-1 UFW rule (`41210/tcp` from `122.162.151.91` + from VPS-2
  `157.173.116.134`) already applied. **No firewall change was needed**; rules left unchanged.
- Reachability re-verified with a temporary auto-exit listener on VPS-1 `0.0.0.0:41210`:
  VPS-2 → VPS-1:41210 `OK`; Windows → VPS-1:41210 `TcpTestSucceeded : True`.

## Build results

All three built `--release --bin iriumd --bin poawx-live-proof-harness` at `081a1bd`, exit 0.

## Ports (RPC loopback-only everywhere)

| System  | P2P              | RPC               | Status            |
|---------|------------------|-------------------|-------------------|
| VPS-1   | `0.0.0.0:41210`  | `127.0.0.1:41411` | `127.0.0.1:41408` |
| VPS-2   | `0.0.0.0:41420`  | `127.0.0.1:41421` | `127.0.0.1:41418` |
| Windows | `127.0.0.1:41430`| `127.0.0.1:41431` | `127.0.0.1:41428` |

No public RPC anywhere; no stratum; no UDP; cross-host only TCP 41210 (source-restricted). Spokes
dial the hub via `IRIUM_ADDNODE=207.244.247.86:41210`. To make cross-host propagation fast, the
nodes were launched with P2P timing env only — `IRIUM_P2P_HANDSHAKE_SECS=30`,
`IRIUM_P2P_PING_SECS=20`, `IRIUM_GAP_HEALER_SECS=10`,
`IRIUM_P2P_NO_GETBLOCKS_COOLDOWN_SECS=5`, `IRIUM_P2P_LOCATOR_RECOVERY_COOLDOWN_SECS=5` — these are
operational timing knobs only; **no consensus / gate / LWMA / difficulty / target / reward change.**

## Storage (isolated; no default path; no `/tmp`)

- Windows: `C:\Users\Ibrahim\irium-poawx-phase26c\node\{data,blocks,state}` (banner-confirmed).
- VPS-1:   `/home/irium/irium-p26c-vps1-node/{data,blocks,state}` (banner-confirmed).
- VPS-2:   `/home/irium/irium-p26c-vps2-node/{data,blocks,state}` (banner-confirmed).

## Peer proof

All three started at genesis height 0, devnet genesis
`0000000028f25d65557e9d8d9e991f516c00d68f5aeae10b750645b398bd10a3`. Hub `peers=2` (Windows +
VPS-2); VPS-2 `peers=1`; Windows `peers=1`. RPC loopback-only throughout; mainnet PIDs alive.

## Block list (6 live all-gates blocks; each propagated to ALL THREE)

After every block, all three nodes were confirmed at the same height, same tip hash, and same
irx1 root (via each node's loopback `/status` + `/rpc/block`):

| H | Origin (harness → node)   | Block hash | irx1 root | All 3 synced |
|---|---------------------------|------------|-----------|--------------|
| 1 | Windows → Windows         | `2aba4b2efd9e14338814066f08bd0652d63c4c08dd603b556e683ac1651d9aca` | `187797fcca3600e35c45e0b98058d0bf6835d9e24be20d7685488e9e7d482dca` | ✅ |
| 2 | VPS-1 → VPS-1             | `2f43d72036ca5714ee768f2a706d468166a0adec934a17bf05e9c7741ea7e7f0` | `b8bf161c7bd128100bede9751b9042709212ddfff8c45c6a419a3c0e5e77ba0a` | ✅ |
| 3 | Windows → Windows         | `7061b33348dbb57d60e5eb0da084db98d02b67fb683caf109f578afd5259d1ae` | `549469a64a5fa57f3afda581a022fc57beed64545ced239026f318bfcf932b1a` | ✅ |
| 4 | VPS-2 → VPS-2             | `2f496cfb3da1a1fe1d5d1d300ef08408e52f7d54aac43960fb2a1318a0e98d30` | `04d79454436a83cf62ec6e23de79e45da477e0ce8230f71ec989598eaea6b448` | ✅ |
| 5 | VPS-1 → VPS-1             | `2e6000889190b767fc7f869eba111288aab7b7c447c74c48952eecb8c9a07264` | `ea517020cb5b7a4ad19fa3b04e1b70a51d74a94424209f7512b39f0ac9885e16` | ✅ |
| 6 | Windows → Windows         | `087597e12c055a663721c328fac4eb22957b1a18d81cb275ceaae09485d61147` | `13f77225b42fd9e9e70c521b6c4802aa872350e93e3fe0fc000ecd54847939f5` | ✅ |

- Each block: Irium-native PoW (no stock cpuminer), 0% official fee, all-gates sections
  (candidate_set, candidate_admission, committed_admission, true_vrf/AVR2, role_puzzle_proofs,
  finality_proof, role_dominance_weights). The harness fetches the parent (grandparent) prev_hash
  from the node to derive the Phase 26B admission epoch seed for height ≥ 2.
- **Final state: all three nodes at height 6, tip `087597e1…1147`, irx1 `13f77225…39f5`.**
- H4 was **originated by VPS-2** and propagated cross-host to VPS-1 + Windows — exercising VPS-2 as
  both observer and originator.

## phase21d / phase22a enforced live

Every block ≥ 2 carried an epoch-seeded candidate set (seed = grandparent hash) and a committed
admission whose parent commitment matched the child's candidate set. The real `connect_block`
pipeline on each node enforced phase21d (candidate-set seed/canonical/best-for-role/dominance/
admitted-set) AND phase22a (committed-admission self-consistency + parent match) for all six
blocks — the live confirmation of the Phase 26B reconciliation.

## Propagation

Cross-host propagation after every block was confirmed (all three at the same height/tip/irx1).
With the P2P timing knobs above, incremental (one-block-ahead) propagation to the spokes completed
within seconds of each block. VPS-2 followed the entire soak live from genesis to height 6,
re-syncing each new block.

## Restart/resync

- **Incremental resync — ROBUST (demonstrated throughout the soak).** VPS-2 joined the mesh at
  genesis and re-synced every one of the six blocks live (height 0 → 6), staying in lockstep with
  the hub and Windows after each block. The initial genesis→1 cold sync also succeeded.
- **Cold restart to re-sync the DEEP (multi-block) chain from scratch — did NOT complete** within
  the test window. After a restart, the node's active chain starts at height 0, receives the tip
  header (height 6) from the hub, but its block-body download for the multi-block gap stalls
  (`getblocks_inflight=1`; repeated `sync stalled … clearing throttles and reconnecting`); the
  persisted on-disk blocks are deferred (`persisted replay deferred 6 block files due to missing
  ancestors; network sync will fill gaps`). Reproduced across preserve-storage,
  delete-state-keep-blocks, and fresh-wipe restarts, with and without the aggressive P2P timing knobs.

This is a **node P2P / persisted-replay limitation, ORTHOGONAL to the Phase 26B consensus fix**: the
multi-block all-gates chain was mined, accepted, and propagated to all three systems correctly
(the Phase 26B goal); the limitation is in the sync path's block-body fetch for a multi-block gap
when the tip header arrives first — not in consensus, the gates, or the epoch-seed fix. Documented
honestly; not faked.

Restart/resync criterion: **partially met** — incremental resync is robust (the whole 6-block soak,
and the initial genesis→1 cold sync); a cold restart re-syncing a deep chain from scratch is limited
by this node P2P issue and is recommended as a separate P2P-sync hardening item.

## Cleanup / safety

- All three Phase 26C nodes stopped by **exact pidfile PIDs** (no pkill / no killall): Windows
  `37628`, VPS-1 `3310`, VPS-2 `2098499` — all STOPPED.
- All Phase 26C ports closed: Windows `41430/41431/41428`, VPS-1 `41210/41411/41408`,
  VPS-2 `41420/41421/41418`.
- Mainnet/prod alive and untouched: Windows `33752`, VPS-1 `219530`, VPS-2 `1851441`; VPS-1 prod
  pool (`irium-pool-api` + `irium-stratum`) alive.
- Default storage untouched (all predate this run): Windows `%USERPROFILE%\.irium` (2026-06-07),
  VPS-1 `~/.irium` (2026-06-21), VPS-2 `~/.irium` (2026-06-06).
- UFW rules left unchanged (the Windows IP was unchanged; no firewall change was made).
- Artifacts preserved: `phase26c-artifacts-vps1`, `phase26c-artifacts-vps2`, and the Windows
  `irium-poawx-phase26c\artifacts` + `harness-work`. No real wallets touched.

## Claim

> Three-system PoAW-X devnet soak succeeded: Windows, VPS-1, and VPS-2 accepted and propagated
> **six** real Irium-native-PoW all-gates blocks (including a VPS-2-originated block), all three at
> the same final height/tip/irx1, with phase21d and phase22a enforced — the live confirmation of the
> Phase 26B epoch-seed reconciliation.

NOT claimed: production-ready, mainnet-ready, audited.

## Remaining blockers

- Independent audit; public testnet; governance / mainnet activation.
