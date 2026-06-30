# PoAW-X Phase 20 — Step 6F: two-VPS live role-gossip E2E (PASS)

**Status: PASS (2026-06-18).** Two-VPS live end-to-end run proving role gossip across a real
cross-VPS P2P link: VPS-2 submits role precommits/reveals to its local node, the VPS-2 node
gossips them over real P2P to the VPS-1 node, the VPS-1 pool fetches them from the VPS-1 node
cache, and VPS-1 produces Phase 20 blocks from that **collected, P2P-gossiped** role data with
**synthetic fallback OFF**. The VPS-2 observer node independently validates the blocks. This is a
self-operated testnet/devnet validation — **not** mainnet activation and **not** an external
(non-self-operated) public miner test.

## Branch / HEAD
- Branch: `testnet/poawx-phase20-blueprint-completion-local`, HEAD `cdbe24c` — **no code change
  in Step 6F** (pure live run).

## VPS roles / IPs
- VPS-1 `207.244.247.86` — node + pool + miner
- VPS-2 `157.173.116.134` — gossip node + observer Node B

## Ports
- VPS-1: status `40208`, RPC `40211`, delegation/role `40213`, metrics `40214` = **loopback**;
  P2P `40210` + stratum `40212` = `0.0.0.0` (UFW source-restricted to VPS-2; the local
  cpuminer connects via `127.0.0.1`)
- VPS-2: status `40218`, P2P `40220`, RPC `40221` = **loopback**; dials VPS-1 via
  `IRIUM_MANUAL_PEERS=207.244.247.86:40210`

## Firewall (operator-run only; agent ran no sudo/ufw)
- **OPEN** (VPS-1): `ufw allow from 157.173.116.134 to any port 40212 proto tcp` +
  `… port 40210 …` → `[24] 40212/tcp ALLOW IN 157.173.116.134`, `[25] 40210/tcp ALLOW IN 157.173.116.134`.
  Post-open: VPS-2 → 40210/40212 reachable; VPS-2 → RPC/status/delegation/metrics **blocked**.
- **CLOSE** (VPS-1): `ufw delete allow …` ×2 → `phase20 step6f rules absent`.
  Post-close: VPS-2 → 40210/40212 **blocked**; temporary rules removed.
- No public RPC/status/delegation/metrics at any point (loopback-only, verified blocked from VPS-2).

## Synthetic fallback OFF
`IRIUM_POAWX_SYNTHETIC_ROLE_CLAIMS` never set on either node/pool. Stratum trace logged
`phase20 COLLECTED role-protocol ext attached` for both blocks.

## Cross-VPS bridge data flow (proven live)
VPS-2 wallet → VPS-2 node loopback RPC `/poawx/role-gossip/{precommit,reveal}` (accepted) →
VPS-2 node cache + **P2P broadcast** (`MessageType::PoawxRolePrecommit/Reveal`) → VPS-1 node
**P2P receive** (`recv PoawxRolePrecommit/Reveal`) → VPS-1 node cache (GET returned all three
roles: precommits@2/reveals@2/precommits@3 for block 2; reveals@3/precommits@4 for block 3) →
VPS-1 pool `bridge_fetch_into_store` → `build_collected` → block. Non-custodial delegations were
emit-only (VPS-2 signs offline with VPS-1's pool pubkey; payload POSTed to VPS-1's loopback
delegation endpoint).

## Block 2 — official fee-0 (P2P-gossiped collected role data)
- height **2**, hash `000000001bdaa8f1ef49b2a19a1fd1b6194efa7bcd9c09bf69e1605a2c36c39d`
- irx1_root `c9bb41691352970a0d1af222c57fa19077cceb8d6d91414afc3a48b1d2ffcb40`
- role source: **P2P-gossiped collected** (origin VPS-2)
- 5 outputs (55/22/13/10), no fee, no delegate:
  irx1 + PRIMARY 2,750,000,000 + COMPUTE 1,100,000,000 + VERIFY 650,000,000 + SUPPORT 500,000,000
  (all to the VPS-2 miner pkh `815854…`) = 5e9
- accepted via `submit_block_extended`

## Block 3 — third-party fee 200 bps (P2P-gossiped, node-direct)
- height **3**, hash `00000000daba210e08659e9cd052d856a5bca7d5d3b02c0937c8a274c479ae2f`
- irx1_root `506d4a16352f8dd1417b7af0f1c2cb9d085f5fcf0767bc80d48be3822929c02f`
- parent-root validation: **PASS** — block-2's `precommit_root(3)` validated block-3's reveals
  (hidden-precommit enforced)
- role source: **P2P-gossiped, node-direct** (submitted only to the VPS-2 node)
- 6 outputs, fee-aware:
  irx1 + PRIMARY_net 2,695,000,000 + COMPUTE 1,100,000,000 + VERIFY 650,000,000 +
  SUPPORT 500,000,000 + FEE 55,000,000 → fee_pkh `00112233445566778899aabbccddeeff00112233`
- fee = floor(2,750,000,000 × 200 / 10000) = 55,000,000 from PRIMARY only; primary_net =
  primary_gross − fee; compute/verify/support untaxed; fee paid only to fee_pkh; **no delegate,
  no hidden output**; total 5e9
- accepted via `submit_block_extended`
- (confirms the Step 6E `fee_terms_from_ext_hex` fix `cdbe24c` is correct in the two-VPS path)

## Observer Node B validation — byte-identical
The VPS-2 node independently synced and re-served both blocks via its loopback RPC,
**byte-identical** to VPS-1: block 2 hash `000000001bda…` / irx1_root `c9bb4169…`; block 3 hash
`00000000daba…` / irx1_root `506d4a16…`. No mismatch.

## Restart/reload (VPS-1) — PASS
Node stopped (exact pidfile) and restarted from the same test root: height 3 + tip preserved;
block 2 + block 3 hashes and irx1_roots identical → Phase20ReceiptExt / precommit_root /
irx1_root reload intact.

## Remote cpuminer caveat (honest, same as Step 5B)
The VPS-2 stock cpuminer **connected, subscribed** (unsolicited `set_version_mask` correctly
skipped for the cpuminer family), **authorized** (`Q94J7A6…w1`), and **received correct work**
over the source-restricted external stratum — but did **not land a diff-1 PoW share** within the
bounded window. The blocks were therefore produced by the **VPS-1 local cpuminer using the VPS-2
identity** (`-u ADDR2.w1/.w2`) and **VPS-2-signed delegations**, while the **role gossip still
originated on VPS-2 over P2P** and the observer validated. This is a low-devnet-height / slow
remote-CPU mining-environment limitation, **not** a Phase 20 logic failure (stratum, job,
authorization, delegation, and validation all succeeded).

## Safety
Mainnet hard-off on every gate; chain difficulty remains **LWMA-144 automatic** (untouched);
only stratum + P2P were public and only source-restricted to VPS-2; delegation/RPC/status/metrics
stayed loopback-only. Test roots and staged binaries removed; test ports clear on both VPS;
cpuminer preserved; mainnet `219530` + 4 production pool workers + VPS-2 mainnet `1851441`
untouched. Artifacts at `/home/irium/phase20-step6f-artifacts`. The agent ran no sudo/ufw —
firewall was operator-only and temporary rules were removed. Nothing pushed.

## Not claimed
Not mainnet-ready; public activation not complete; external (non-self-operated) public miner
test not done; remote cpuminer PoW not solved at low devnet heights.
