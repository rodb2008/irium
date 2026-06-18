# PoAW-X Phase 20 — Step 7B-Self: self-operated two-VPS complete final rehearsal (PASS)

**Classification: self-operated two-VPS complete PoAW-X final rehearsal — NOT the public/external
non-self-operated miner test.** Both VPSs are operator-controlled. This rehearsal re-validated the
full PoAW-X path end-to-end on fresh ports/roots. **Not claimed:** external miner test complete,
public activation complete, mainnet-ready.

## Setup
- Branch `testnet/poawx-phase20-blueprint-completion-local` @ `4ee4c46` (no code change for the run).
- VPS-1 `207.244.247.86` — node + pool + stratum + block producer.
- VPS-2 `157.173.116.134` — trusted miner identity + role-gossip origin + observer Node B + remote cpuminer.
- Ports: VPS-1 status 40308 / RPC 40311 / delegation 40313 / metrics 40314 **loopback**; P2P 40310 +
  stratum 40312 `0.0.0.0`, **source-restricted to VPS-2 via operator UFW** (opened then removed).
  VPS-2 status 40318 / P2P 40320 / RPC 40321 loopback. Activation height 2; synthetic OFF.

## Results
- block 1 bootstrap `00000000e293fe22ed465d7b1b32ef831a8f5242f336e9f78c9b76132fba40a8`.
- **block 2 — official fee-0** `0000000022fbe4a5f32685e076027c321ea14608e0a95218452bfb526acade8f`,
  irx1_root `033b453d68839ffd0e2d10d148b6804e3eda1bea39f0665d43e686b390fa3f49`; 5 outputs
  irx1 + PRIMARY 2,750,000,000 + COMPUTE 1,100,000,000 + VERIFY 650,000,000 + SUPPORT 500,000,000
  (all to VPS-2 miner pkh `bb925faf…`); collected role source (P2P-gossiped); accepted via
  `submit_block_extended`.
- **block 3 — third-party fee 200 bps** `00000000598f16b543912212c973ec90cbddb0d22e2490ba53b09600b5c6c9b3`,
  irx1_root `bdc328067a37320750f389f74bf8648bc2621cc49bce09545912670f1843a76f`; 6 outputs
  irx1 + PRIMARY_net 2,695,000,000 + COMPUTE 1,100,000,000 + VERIFY 650,000,000 + SUPPORT 500,000,000
  + FEE 55,000,000 → fee_pkh `00112233445566778899aabbccddeeff00112233`; fee = floor(2.75e9×200/10000)
  from PRIMARY only, roles untaxed, no delegate/hidden output; **hidden-precommit enforced**
  (block-2 precommit_root validated block-3 reveals); accepted via `submit_block_extended`.

## Bridge / observer / persistence
- Cross-VPS bridge: VPS-2 wallet → VPS-2 node loopback RPC → real P2P → VPS-1 node cache →
  VPS-1 pool fetch → block (synthetic OFF). Non-custodial emit-only delegations (VPS-2 signs
  offline; only signed payloads to VPS-1 loopback).
- **Observer Node B (VPS-2)** independently synced + validated block 2 and block 3 **byte-identical**
  (hash + irx1_root).
- **Restart/reload (VPS-1):** height 3 + tip + both block hashes + irx1_roots preserved.

## Remote cpuminer result — SUCCESS this run
The **VPS-2 stock cpuminer landed BOTH blocks** over the real external, source-restricted stratum
(block 2 as worker `w1`, block 3 as `w2`; "accepted 1/1 (yay!!!)" each). The slow-remote-CPU
low-devnet PoW caveat from Steps 5B/6F **did not recur** in this run — no local-cpuminer fallback
was needed. (The caveat may still occur on slower remote hardware; it is an environment limitation,
not a Phase 20 logic defect.)

## Safety
Mainnet hard-off; chain difficulty LWMA-144 automatic; only stratum + P2P public (source-restricted
to VPS-2); delegation/RPC/status/metrics loopback-only (verified blocked from VPS-2). Test roots +
staged bins removed; ports clear both VPS; cpuminer preserved; mainnet `219530` + 4 prod workers +
VPS-2 `1851441` untouched. Agent ran no sudo/ufw — firewall operator-only, temporary rules removed.
Artifacts at `/home/irium/phase20-step7b-self-artifacts`. Nothing pushed.
