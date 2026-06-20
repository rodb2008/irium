# PoAW-X Phase 24F — pool/cpuminer all-gates block-production harness hardening

**Status: harness blocker FIXED (code/test phase). NOT production-ready; NOT mainnet-ready.**
Local-only; not pushed; remote branch absent; `main` untouched. No live nodes/miners launched
this phase. Mainnet hard-off preserved. This does **not** replace an external audit, and makes
**no** claim of a real mined all-gates block.

## A. Harness gap audit (findings)

The full all-gates block-production path already exists and is wired:
- **Wallet** → ticket proof + `AssignmentProofV2` + candidate admission + finality vote (proven
  live in Phase 24E).
- **Node** validates + caches admissions/votes and exposes them
  (`/poawx/candidate-admissions`, `/poawx/finality-votes`); `/poawx/assignment` provides the
  per-height seed / puzzle difficulty / pow_bits.
- **Pool** fetches admitted candidates + finality votes and builds the all-gates Phase 20 ext
  (`build_synthetic_phase20_ext` / `build_collected_phase20_ext`) with the `TPK1` ticket, `DOM1`
  dominance, `CND1` candidate-set, `PZL1` puzzle, `FIN1` finality, `CAC1` committed-admission,
  and `AVR2` true-VRF sections — **fail-closed**, bundling **miner-supplied** V2 proofs (the
  **pool holds no VRF secret**).
- **Submit** path `/rpc/submit_block_extended` accepts `poawx_receipts[].phase20_ext`, is
  mainnet-hard-off, requires the ext under phase-20 production, and defers to **`connect_block`
  as the authoritative validator**.

### Exact blocker found
`poawx_get_assignment` (node) returned **HTTP 404 when `tip_height == 0`**. On a fresh
devnet/testnet (tip at genesis) the pool could never obtain an assignment for block 1, so it
could not build the first job — the 14-F "genesis `/poawx/assignment` 404" wall. Everything at
`tip ≥ 1` already worked; only the first block was blocked. (`/poawx/assignment` already
required `is_active && is_non_mainnet`, so mainnet/inactive were already excluded.)

## B/C. Fix implemented (minimal, devnet/testnet-gated)

Removed only the `tip_h == 0 → 404` special case in `poawx_get_assignment`. The genesis
assignment seed now derives from the genesis tip hash via the **same path** used for
`tip ≥ 1` (`chain.last()` / `hash_for_height(0)` / `target_for_height(0)` all work at height 0).

Preserved unchanged:
- `is_active && is_non_mainnet` gate → **mainnet still returns 503; inactive still returns 503
  (mainnet hard-off preserved).**
- `connect_block` (authoritative validation), difficulty, LWMA, target, PoW — **untouched.**
- **No pool change** — the preferred design (pool consumes node-validated data; no separate
  pool assignment endpoint; no VRF secret in the pool) already holds.

## D. Tests

- `iriumd::phase24f_assignment_served_at_genesis_devnet_only` — devnet+active serves a
  well-formed assignment at the genesis tip (`height 0`, 32-byte `seed`, `pow_bits`,
  `puzzle_difficulty`); **mainnet rejects**; **inactive rejects**.
- Existing pool all-gates coverage stays green: `delegation::phase22e_pool_e2e_bundle_and_
  failclosed` (AVR2 bundle + official fee-0 + third-party fee + fail-closed),
  `phase22d_pool_true_vrf_parity_and_failclosed`, `phase20` (ticket/fee/reward), plus the node
  in-process all-gates block validators (`chain::phase22e_true_vrf_e2e_block`,
  `phase22d_true_vrf_enforcement`, `phase22e_wrong_candidate_score_rejects`, and the per-section
  phase21 enforcement tests).
- The fuller single end-to-end all-gates *block* integration test was deliberately **not**
  added this phase (consensus-level all-gates validation is already covered by the tests above);
  it remains optional/future.

## Result / status

- **Live all-gates block production is now unblocked at the assignment layer:** a fresh
  devnet/testnet can obtain a genesis assignment → the pool can build the all-gates ext → submit
  via `/rpc/submit_block_extended` → `connect_block` validates. The assembly + submit + validate
  path is complete in code.
- **Not yet demonstrated:** a **real cpuminer-mined all-gates block** end-to-end (requires
  running the pool + cpuminer live; not done in this code/test phase).
- **Cross-host P2P remains blocked** by the provider/firewall layer (Phase 24E).
- **No mainnet-ready / production-candidate claim.** External audit missing; public testnet
  pending; governance/mainnet activation pending.

## Remaining for Phase 24G (live rehearsal)

1. Resolve the provider firewall for cross-host P2P (operator).
2. Run pool/stratum + cpuminer live on an isolated devnet (genesis assignment now served) to
   produce a real all-gates block; verify observer sync + restart/reload.
3. Then (only after external audit + public testnet + governance) consider mainnet.

## Phase 24G update (single-VPS real mined block rehearsal — PARTIAL)

Phase 24G validated LIVE: the 24F genesis /poawx/assignment fix (200 at tip 0) and the full
wallet->node all-gates block-material path (3-role true-VRF V2 candidate admissions seeded with
the genesis hash + finality vote, all validated under all gates and cached). A real
cpuminer-mined accepted all-gates block was NOT demonstrated: it requires a miner<->pool
onboarding/coordination layer (admitted role solvers == pool primary_pkh for all 3 roles;
finality vote from a committee member; admitted candidates for H+1; dominance matching genesis
state) + live PoW mining, and the synthetic producer path is disallowed for the claim. No fake,
no weakened gates. NOT production-ready/mainnet-ready. See
docs/poaw-x-phase24g-single-vps-real-mined-all-gates-block.md.
