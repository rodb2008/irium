# PoAW-X Phase 24G — single-VPS real mined all-gates block rehearsal (PARTIAL)

**Status: PARTIAL. A real mined all-gates block was NOT demonstrated.** Two major live results
were achieved; the final mined+accepted block requires a miner↔pool orchestration layer beyond
this rehearsal. Local-only; not pushed; remote branch absent; `main` untouched. Loopback-only;
isolated `$HOME`-rooted storage; mainnet hard-off. Not production-ready; not mainnet-ready; does
not replace external audit.

## What succeeded (live, on isolated devnet, all gates enabled)

1. **Phase 24F genesis-assignment fix validated LIVE.** With a freshly rebuilt release `iriumd`,
   `GET /poawx/assignment` returns **HTTP 200** at genesis (`tip_height == 0`) with a well-formed
   body (`height:0`, 32-byte `seed`, `commitment_nonce`, `pow_bits:1d00ffff`,
   `puzzle_difficulty:4`). The 14-F "genesis 404" wall is gone. (Earlier the stale 24D-era binary
   still 404'd; rebuilding with the 24F fix resolved it.)
2. **Full wallet → node all-gates block-material path validated LIVE.** Using the isolated
   wallet, a throwaway devnet secret, and `seed = genesis hash` (= block-1 `prev_hash`):
   - 3 candidate admissions (compute/verify/support), each carrying a real true-VRF
     `AssignmentProofV2` (`true_vrf:true`), submitted to the node loopback RPC → **200 OK** each;
     node **validated under all gates** (true-VRF verify + role/solver/ticket/key/seed/output
     binding) and **cached all 3** (`GET /poawx/candidate-admissions?target_height=1` → 3).
   - 1 member-signed finality vote submitted → **200 OK** → validated + cached.
   - Secret leak check: the VRF/vote secrets were never written to emitted JSON.
- **Storage isolation safe:** node banners showed only `/home/irium/irium-p24g-node/{blocks,
  state}`; `~/.irium` untouched (no new orphan dirs); mainnet 219530 + prod pool (4 workers)
  alive throughout.

## What was NOT demonstrated (honest)

**A real cpuminer-mined, node-accepted all-gates block was not produced.** No synthetic fallback
was used (it is explicitly disallowed for the success claim), and no gate was weakened.

### Exact remaining blocker (from the pool code path)
Producing one *accepted* all-gates block requires orchestrated, mutually-consistent inputs that
a single wallet/cpuminer rehearsal does not provide:
- In `build_synthetic_phase20_ext` / `build_collected_phase20_ext`,
  `role_assignment_v2 = build_pool_true_vrf_section(height, prev_hash, &role_reward)` looks up
  admitted V2 proofs **by `role_reward.solver`**, and `synth_role_solver` yields the **same**
  pkh (the pool's `primary_pkh`) for all three roles. ⇒ the admitted candidates' solvers for
  compute/verify/support must **all equal the pool's `primary_pkh`** (the rehearsal used distinct
  solvers aa/bb/cc).
- The bundled **finality proof** must contain ≥threshold Commit votes from **committee members**
  (member `pkh == primary_pkh`); the rehearsal's vote used an unrelated key.
- The **committed-admission** section commits height **H+1**, so admitted candidates for height
  H+1 must also be present (only height-H admissions were submitted).
- **Dominance weights** in the ext must match the node's genesis-derived persisted state.
- The **synthetic** producer path is disallowed for the claim; the **collected** path
  additionally requires the role precommit/reveal protocol populated with matching solvers.

In short: the missing piece is a **miner↔pool onboarding/coordination layer** that registers the
miner identity `P` with the pool and supplies all matching proof material (3-role admissions +
finality-committee vote for `P`, for heights H and H+1, with consistent dominance) — plus the
live PoW mining. This is a larger integration task than a single live rehearsal, and is the
correct subject of a future phase.

## Cleanup

- Node stopped by exact pidfile (no `pkill`/`killall`). p24g runtime dirs removed
  (`irium-p24g-node`; no pool/wallet runtime left). Artifacts preserved under
  `/home/irium/phase24g-all-gates-artifacts/` (node log, assignment.json, admission/finality
  JSON + cache responses, `evidence-vps1.txt`).
- No p24g ports bound; `~/.irium` untouched (2 pre-existing orphan dirs); mainnet 219530 + prod
  pool alive.

## Claim status

- **Real mined all-gates block accepted? NO** (not demonstrated; exact blocker documented).
- **Production-candidate for controlled public testnet? NO.**
- **Mainnet production-ready? NO.**
- Allowed: 24F genesis-assignment fix validated live; full wallet→node all-gates block-material
  path (3-role V2 admissions + finality) validated live under all gates; storage isolation safe.

## Remaining blockers

- Miner↔pool onboarding/coordination layer for a complete all-gates block (above) + live PoW
  mining (real cpuminer-mined block).
- Cross-host P2P provider/firewall (Phase 24E).
- Independent external audit; public testnet; governance / mainnet activation.

## No source change

This phase made **no** source/consensus change (HEAD stays `b7d6341`); the release `iriumd` was
rebuilt to include the already-committed Phase 24F fix. Docs-only commit.

## Phase 24H update (miner-pool coordination fix)

Phase 24H fixed the exact 24G blocker: build_synthetic_phase20_ext now derives per-role solvers
(and role_reward + the AVR2 lookup) from the node-validated admitted candidate set instead of
the pool primary_pkh, so role rewards + per-role V2 proofs key to the actual admitted miners;
fail-closed if any role lacks an admitted candidate. New helper pool_role_reward_from_admitted.
build_collected already derives role_reward from reveals (real path, unchanged). No pool-local
onboarding endpoint needed (node RPC suffices); pool holds no miner secret. Test
delegation::phase24h_role_reward_derived_from_admitted_candidates. Remaining for 24I: one
coordinated live run (matching admission/V2/ticket/finality-member/reveal/H+1 + dominance) +
live PoW mining. NOT production-candidate; NOT mainnet-ready. See
docs/poaw-x-phase24h-miner-pool-onboarding-coordination.md.
