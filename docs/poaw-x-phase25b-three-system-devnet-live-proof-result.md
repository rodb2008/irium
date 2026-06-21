# PoAW-X Phase 25B — three-system devnet live proof RESULT: PASSED

**Status: PASSED.** A real Irium-native-PoW all-gates PoAW-X block, built and submitted by the CLI
live-proof harness, was accepted by a real devnet node and **observed across all three systems**
(Windows + VPS-1 + VPS-2) at the same height and the same block hash. Devnet only; loopback RPC
everywhere; cross-host P2P only on the source-restricted VPS-1 hub port 41210; mainnet/prod and all
real wallets untouched. NOT production-ready / mainnet-ready / audited.

## Systems / branch

- Windows `C:\Users\Ibrahim` (submitter + originating node; mainnet PID 33752 untouched).
- VPS-1 hub `207.244.247.86` (`vmi2780294`; mainnet 219530 + prod pool untouched).
- VPS-2 observer `157.173.116.134` (`vmi2995746`; mainnet 1851441 untouched).
- Branch `testnet/poawx-phase20-blueprint-completion-local` @ `be6e6b7…` — built on all three.

## Firewall

- Windows egress IP: `122.162.148.238`.
- Host UFW rules added by the operator (interactively; password never seen by the agent) on VPS-1:
  inbound TCP `41210` from `122.162.148.238` and from `157.173.116.134` (source-restricted).
- Verified open: VPS-2 → VPS-1:41210 `OK`; Windows → VPS-1:41210 `TcpTestSucceeded: True` (the
  host UFW rule was the blocker; no provider-level change was needed this time).

## Ports (RPC loopback-only everywhere)

- VPS-1: P2P `0.0.0.0:41210`, RPC `127.0.0.1:41211`, status `127.0.0.1:41208`.
- VPS-2: P2P `0.0.0.0:41220`, RPC `127.0.0.1:41221`, status `127.0.0.1:41218`, dials VPS-1.
- Windows: P2P `127.0.0.1:41230`, RPC `127.0.0.1:41231`, status `127.0.0.1:41228`, dials VPS-1.

## Storage (isolated; no default path used)

- VPS-1: `/home/irium/irium-p25b-vps1-node/{data,blocks,state}` (banner-confirmed).
- VPS-2: `/home/irium/irium-p25b-vps2-node/{data,blocks,state}` (banner-confirmed).
- Windows: `C:\Users\Ibrahim\irium-poawx-phase25b\node\{data,blocks,state}` (banner-confirmed).
- Nodes were started from their repo cwd only so `./bootstrap/trust/allowed_anchor_signers`
  resolves (storage stays isolated via explicit `IRIUM_*_DIR`).

## Peer topology (hub)

Windows → VPS-1 and VPS-2 → VPS-1. VPS-1 hub reported `peers=2`; VPS-2 `peers=1`; Windows `peers=1`.
All three on devnet genesis `0000000028f25d65557e9d8d9e991f516c00d68f5aeae10b750645b398bd10a3`.

## CLI harness result (Windows, Irium-native PoW — no stock cpuminer)

`poawx-live-proof-harness --devnet --rpc-url http://127.0.0.1:41231 --work-dir
…\irium-poawx-phase25b\harness-work`:
- node response `{"accepted":true,"height":1,"tip":"7de18583…871"}`
- before height **0** → after height **1**
- **block hash** `7de18583540de933c6b0efe127c955cbda23078b0ebc975e3986deaad01ab871`
- **irx1 root** `772e1cd700af122e5bc2a586a1eb94d4dc33bdd2ab819dba435df9875c7ed9bd`
- official fee **0%**; all-gates sections present (candidate_set, candidate_admission,
  committed_admission, true_vrf/AVR2, role_puzzle_proofs, finality_proof, role_dominance_weights).

## Propagation (the three-system proof)

- **Windows (originator): height 1, tip `7de18583…871`.**
- **VPS-1: height 1, tip `7de18583540d…`** (received from Windows; heartbeat `tip=7de18583540d`).
- **VPS-2: height 1**, accepted via headers→getblocks sync from VPS-1
  (`accepted block height 1 hash 7de18583540d`, persisted); `/rpc/block?height=1` hash =
  `7de18583540de933c6b0efe127c955cbda23078b0ebc975e3986deaad01ab871`, **irx1_root =
  `772e1cd7…d9bd`** (exact match — the all-gates ext propagated intact).

All three nodes hold the identical all-gates block at height 1.

## Optional pool

Not run (optional; not needed for the proof). No stock cpuminer used anywhere.

## Cleanup / safety

- All three Phase25B nodes stopped by exact pidfile PIDs (no pkill/killall). Ports
  41208/41210/41211 (VPS-1), 41218/41220/41221 (VPS-2), 41228/41230/41231 (Windows) all closed.
- Mainnet untouched and alive: Windows 33752, VPS-1 219530, VPS-2 1851441; VPS-1 prod pool
  (irium-pool-api + irium-stratum) alive.
- `~/.irium` / `%USERPROFILE%\.irium` untouched (Windows wallet.json/node.conf/anchors unchanged;
  VPS wallets unchanged). Test used isolated storage throughout. Artifacts preserved under
  `phase25b-artifacts-vps1/vps2` and the Windows `irium-poawx-phase25b\artifacts` + `harness-work`.

## Claim

> Three-system devnet proof succeeded: Windows, VPS-1, and VPS-2 participated in a live PoAW-X
> devnet where a real Irium-native-PoW all-gates block was submitted to a real node, accepted, and
> observed across the devnet.

NOT claimed: production-ready, mainnet-ready, audited.

## Remaining blockers

- Independent audit; public testnet; governance / mainnet activation.
- (Firewall for cross-host P2P is now resolved at the host-UFW level for these source IPs; the
  Windows egress IP is dynamic and may need re-adding for future runs.)
