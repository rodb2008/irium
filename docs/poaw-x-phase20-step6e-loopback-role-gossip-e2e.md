# PoAW-X Phase 20 — Step 6E: local loopback live role-gossip E2E (PASS)

**Status: PASS (2026-06-18).** Single-VPS, loopback-only live end-to-end run proving that
Phase 20 blocks are produced from **collected role-gossip data** flowing through the live
node↔pool bridge (Step 6D), with **synthetic fallback OFF**. This is a testnet/devnet
validation only — **not** mainnet activation and **not** an external/public miner test.

## Branch / HEAD
- Branch: `testnet/poawx-phase20-blueprint-completion-local`
- HEAD at start of run: `285f8fd` (Step 6D). The run surfaced a real pool-only bug;
  the fix landed as **`cdbe24c`** (final HEAD for Step 6E).

## Environment (all loopback `127.0.0.1`)
- status `40108`, P2P `40110`, RPC `40111`, stratum `40112`, delegation/role `40113`, metrics `40114`
- isolated test root (removed after pass); release binaries; stock cpuminer
  `/home/irium/phase13-devnet/cpuminer-src/minerd`; peers = 0 throughout
- activation height **2** for all gates (PoAW-X / delegation / multi-role / fairness /
  third-party-fee / hidden-precommit); `IRIUM_POAWX_ROLE_PROTOCOL_ENABLED=1`,
  `IRIUM_POAWX_ROLE_GOSSIP_ENABLED=1`, `IRIUM_POAWX_ROLE_GOSSIP_NODE_RPC=http://127.0.0.1:40111`,
  puzzle bits 4, `STRATUM_DEFAULT_DIFF=1` (share difficulty only), vardiff off
- two-phase bootstrap: node inactive → legacy block 1 → restart active

## Synthetic fallback OFF
`IRIUM_POAWX_SYNTHETIC_ROLE_CLAIMS` was never set. The stratum trace logged
`phase20 COLLECTED role-protocol ext attached` for both blocks; the dataless height-4
attempt logged `no collected/synthetic claims … node will fail closed`, confirming synthetic
never substitutes.

## Block 2 — official fee-0 (collected role data via the pool endpoint → node forward)
- height **2**, hash `00000000e50519f4623563a142cc0abdbe24e6da284bc3413e29f5ce7aff1c7e`
- irx1_root `ace94a4862fa215da0ab9b369376745088bdf3330965d10648abf63334809ca9`
- role source: **collected** (miner submitted precommit/reveal to the pool loopback
  `/poawx/role-precommit|reveal`; pool forwarded to the node)
- 5 outputs (55/22/13/10), no fee, no delegate:
  irx1 + PRIMARY 2,750,000,000 + COMPUTE 1,100,000,000 + VERIFY 650,000,000 + SUPPORT 500,000,000 = 5e9
- accepted via `submit_block_extended`

## Block 3 — third-party fee 200 bps (collected role data via node RPC → pool fetch)
- height **3**, hash `000000004744ba24ccd5d5e1fc566de547c0f91c2a9de9cbcdd80e5dc0963744`
- irx1_root `51a0d3216f905d2b8af2a9cc0446f7d490a8999d09794458b8ba8d2ee8b65661`
- parent-root validation: **PASS** — block-2's `precommit_root(3)` validated block-3's
  reveals (hidden-precommit enforced)
- role source: **collected, node-direct** (submitted only to the node
  `/poawx/role-gossip/{reveal,precommit}`; pool `bridge_fetch_into_store` pulled them)
- 6 outputs, fee-aware:
  irx1 + PRIMARY_net 2,695,000,000 + COMPUTE 1,100,000,000 + VERIFY 650,000,000 +
  SUPPORT 500,000,000 + FEE 55,000,000 → fee_pkh `00112233445566778899aabbccddeeff00112233`
- fee = floor(2,750,000,000 × 200 / 10000) = 55,000,000 from PRIMARY only;
  primary_net = primary_gross − fee; compute/verify/support untaxed; fee paid only to fee_pkh;
  **no delegate output, no hidden extra**; total 5e9
- accepted via `submit_block_extended`

## Restart/reload — PASS
Node stopped (exact pidfile) and restarted from the same test root: height 3 + tip preserved;
block 2 + block 3 re-queried with hashes and irx1_roots identical → Phase20ReceiptExt /
precommit_root / irx1_root reload intact.

## Bug found and fixed (`cdbe24c`, pool-only)
The E2E surfaced a real production bug in the pool coinbase builder:
- `fee_terms_from_ext_hex` read the third-party fee terms from the **last 22 bytes** of the
  serialized `Phase20ReceiptExt` (`fee_bps(2) || fee_pkh(20)`).
- Step 6A appended an **optional trailing `precommit_root` (flag + 32)** *after* `fee_pkh`.
  With hidden-precommit active, the helper misparsed 22 bytes of the precommit-root hash as a
  spurious fee → the multi-role coinbase grew a bogus 6th output → `connect_block` rejected it
  (`poawx coinbase: expected 4 payout outputs, found 5`).
- Why it was missed: Step 5A predates Step 6A (no `precommit_root`); unit tests validate the
  ext via the node deserializer, not via this pool helper, and never exercised fee +
  `precommit_root` together.
- **Fix:** parse the fee from the **front**, skipping `version(1) + RoleReward(60) + the three
  variable-length claims` to land exactly on `fee_bps`/`fee_pkh`, so the optional trailing
  `precommit_root` is never misread. Node validators were already correct (full deserialize).
  A regression test was added (fee parses correctly with/without `precommit_root`; official
  fee-0 stays fee-0 when `precommit_root` is present).

## Verification (pool-only change; no node source touched)
- pool `cargo fmt -- --check`: clean
- pool full `cargo test`: **85/0**
- pool `cargo test phase20`: **21/0**
- pool `cargo test delegation`: **31/0**
- pool `cargo test native_rewardable`: **6/0**
- node source unchanged since `285f8fd` (iriumd bin 256/0 from Step 6D still holds)

## Safety
Mainnet hard-off on every gate; chain difficulty remains **LWMA-144 automatic** (untouched);
loopback-only (no public ports); no external miner; mainnet `219530` + 4 production pool
workers + VPS-2 mainnet `1851441` untouched. Artifacts at `/home/irium/phase20-step6e-artifacts`.
Nothing pushed.

## Not claimed
Not mainnet-ready; public activation not complete; external/public miner test not done.
