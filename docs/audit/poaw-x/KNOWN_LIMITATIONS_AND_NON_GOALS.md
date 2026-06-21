# PoAW-X known limitations & non-goals

State plainly, up front:

- **NOT mainnet-ready.** Every PoAW-X gate is default-off and `network_id == 0` hard-off.
- **NOT independently audited yet.** The only review to date is the internal Claude Code review
  (Phase 23A, `docs/poaw-x-phase23a-true-vrf-internal-security-review.md`). This package exists
  to enable an external audit.
- **Public testnet not yet done.** No live multi-party PoAW-X network has been run.
- **Governance / community vote not yet done.** No on-chain or community decision has been made.
- **External security review is REQUIRED** before any public testnet, non-test network, or
  mainnet activation.

## Specific limitations

- **`vrf_fun` / `secp256kfun` are pre-1.0 (0.12.x) and not formally audited.** Their correctness
  (incl. the vendored k256 field arithmetic) is a primary audit target; pin + vendor before
  mainnet.
- **Candidate admission is propagation-sensitive.** Enforcement proves "best among candidates
  ADMITTED to THIS node within the window", NOT "best among all unseen/offline/never-gossiped
  miners". Public-network admission windowing/tuning requires a testnet review.
- **Finality committee + gossip** public-network behavior (propagation, threshold economics,
  liveness under churn) requires testnet review.
- **Economic parameters** (55/22/13/10 split, 2% fee cap, thresholds) may require governance
  review; they are not claimed to be economically final.
- **Puzzle work modes are ASSIGNED work, not chain PoW.** They do not touch chain
  difficulty/LWMA-144; they are not a replacement for the chain's proof-of-work.
- **Role precommit/reveal gossip** has the in-memory + reserved-P2P plumbing; full cross-process
  live E2E is testnet work.

## Non-goals (explicitly out of this package)

- Mainnet activation / height selection.
- Governance / community vote mechanics.
- Exchange listing, liquidity, market structure.
- Non-PoAW-X mainnet services.
- Public testnet operations and external miner operations (unless requested later).
- A claim that the internal review substitutes for an external audit (it does not).

## Phase 24F update (genesis assignment harness fix)

Phase 24F found + fixed the exact live-block-production blocker: poawx_get_assignment returned
404 at tip_height==0, so a fresh devnet could not get an assignment for block 1 (the 14-F
genesis /poawx/assignment wall). Fix: serve the assignment at the genesis tip on
devnet/testnet only (mainnet + inactive still 503; connect_block/LWMA/difficulty untouched).
Live all-gates block production is now unblocked at the assignment layer (assembly + submit +
connect_block validate path is complete in code); a real cpuminer-mined all-gates block is
still not demonstrated, and cross-host P2P remains firewall-blocked. NOT production-ready, NOT
mainnet-ready. See docs/poaw-x-phase24f-pool-cpuminer-all-gates-harness.md.

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

## Phase 24I update (coordinated live mined block attempt — PARTIAL)

Phase 24I validated LIVE: 24F genesis assignment (200); the full coordinated node material under
all gates (single identity P: H=1+H=2 candidate admissions [3 roles, solver=P, true-VRF V2] +
SUPPORT finality vote member=P, all 200 OK + cached); the pool running in COLLECTED mode
(loopback, isolated, all gates, role protocol) accepting 9/9 role precommit/reveal + building
jobs. A real mined accepted all-gates block was NOT produced: the pool builds the PoAW-X ext
per-miner-SESSION (needs a connected stratum miner), and the cpuminer step was stopped after a
minor ~/.irium incident (new-address created a stray ~/.irium/wallet.json because the wallet CLI
defaults its store to ~/.irium; removed; operator wallets + mainnet untouched; lesson: isolate
the wallet path too). NOT production-candidate; NOT mainnet-ready. See
docs/poaw-x-phase24i-single-vps-live-mined-all-gates-block.md.

## Phase 24J update (stratum cpuminer attempt — PoW-tooling blocker)

Phase 24J fixed + proved wallet-path isolation (isolated HOME; real ~/.irium/wallet.json never
created) and ran the full coordinated path with a single identity P: H1+H2 admissions + finality
(member=P) cached by node; 9/9 role precommit/reveal accepted by the pool (collected mode); and
a live cpuminer SESSION (subscribe+authorize, worker A -> pkh P). But NO block: stock cpuminer
hashed ~900M sha256d vs an easy target and found 0 valid shares -> stock cpuminer PoW != Irium's
custom block hashing (Irium ships an RPC-based irium-miner; stratum adapter is
native_rewardable_reserved). Definitive remaining blocker = mining tooling: need an
Irium-PoW-compatible stratum miner (or node-template ext-build for the RPC miner, or a custom
submit harness). Not a PoAW-X consensus gap. NOT production-candidate; NOT mainnet-ready. See
docs/poaw-x-phase24j-stratum-cpuminer-all-gates-block.md.

## Phase 24K update (Irium-native all-gates mining harness — block mined + connect_block-accepted)

Phase 24K closed the Phase 24J mining-tooling blocker IN-PROCESS. New mainnet-hard-off harness
`src/poawx_mining_harness.rs` (`guard_network`, `guard_isolated_storage`, `mine_pow` — grinds the
nonce via Irium's REAL `hash_for_height` + `meets_target`, never touches LWMA/difficulty). Two
deterministic tests in `src/chain.rs`: Stage 1 mines an all-gates block and every authoritative
validator accepts it (+ E13–E17 negatives); Stage 2 drives the FULL `connect_block` to acceptance,
advancing the chain to height 2 (real PoW, gated `irx1` root, production payout, dominance,
candidate set + admission, puzzle, finality, committed admission, true-VRF, 0%-fee coinbase).
Mainnet hard-off (devnet `network_id=2`); no validator weakened; no PoW bypass; hidden-precommit /
ticket-proof / mode-1 delegation gates left off as independent (separately tested). Still NOT
production-candidate / mainnet-ready: a live cross-host run (Phase 24L), independent audit, public
testnet, and governance activation remain. See
docs/poaw-x-phase24k-irium-native-mining-submit-harness.md.

## Phase 24L update (Windows local live-proof package)

Phase 24L packages a Windows-safe, devnet-only LIVE proof: a loopback `iriumd` +
the new `poawx-live-proof-harness` binary that builds an all-gates block with
Irium-native PoW (via `poawx_mining_harness::build_devnet_all_gates_block`),
ingests candidate admissions, submits through the real `/rpc/submit_block_extended`
path, and verifies the node advanced height. A `connect_block` test
(`chain::phase24l_lib_builder_connect_block`) proves the binary's exact builder
output is node-acceptable, so only the local RPC round-trip is Windows-verified by
the user. Safety: rejects mainnet, requires loopback RPC + an explicit isolated
work dir (not `%USERPROFILE%\.irium` / `$HOME/.irium`), no public bind, no secrets
in logs. Isolated root is under `%USERPROFILE%` (Phase 24C storage hardening fails
closed on dirs outside the user home). Runner: `scripts/windows/poawx-live-proof.ps1`;
guide: `docs/poaw-x-phase24l-windows-live-proof.md`. The actual Windows live proof
is run by the user; allowed claim if it passes: "Local Windows devnet live proof
succeeded: a real Irium-native-PoW all-gates block submitted to a real node and
accepted." NOT mainnet-ready / production-ready / audited.
