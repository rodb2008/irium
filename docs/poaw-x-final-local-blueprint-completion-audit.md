# PoAW-X — final local blueprint completion audit (Phase 22F)

**Status: LOCAL TECHNICAL IMPLEMENTATION COMPLETE against the blueprint — NOT
mainnet-ready.** Local-only (branch `testnet/poawx-phase20-blueprint-completion-local`; not
pushed; remote branch absent; `main` untouched). Every PoAW-X mechanism is implemented,
gated, and **mainnet hard-off** (`network_id == 0` ⇒ off). This document is **not** a claim
of audit, production-readiness, or mainnet-readiness. External security review, a public
testnet, an independent audit, and a governance/activation decision remain **pending**.

This audit reflects HEAD after Phase 22E (true-VRF E2E wiring). It is docs-only.

## A. Final blueprint matrix

### 1. Implemented locally and gated (testnet/devnet only; mainnet hard-off)

| Mechanism | Gate(s) (env) / notes |
|---|---|
| Multi-role rewards | `MULTI_ROLE_REWARD_ACTIVATION_HEIGHT` |
| 55/22/13/10 split (PRIMARY/COMPUTE/VERIFY/SUPPORT) | bps constants sum to 10000 (test-asserted) |
| Official 0% fee | default; fee-0 canonical coinbase |
| Third-party fee | `THIRD_PARTY_FEE_ACTIVATION_HEIGHT` + `THIRD_PARTY_POOL_MODE`; cap 2.00% (`THIRD_PARTY_FEE_CAP_BPS`) |
| Delegated / non-custodial receipts | `DELEGATION_ACTIVATION_HEIGHT`; wallet signs, no custody |
| Hidden precommit | `HIDDEN_PRECOMMIT_ACTIVATION_HEIGHT` |
| Role precommit/reveal | testnet role protocol (`ROLE_*`) |
| Role gossip | `ROLE_GOSSIP_ENABLED` + `ROLE_GOSSIP_WINDOW` (in-memory + reserved P2P) |
| Tickets / Sybil resistance | `TICKETS_ACTIVATION_HEIGHT` + `TICKETS_REQUIRED` + `TICKET_SYBIL_BITS` |
| Penalty enforcement | `PENALTY_STATE_ACTIVATION_HEIGHT` + `PENALTY_STATE_REQUIRED` |
| Persistent anti-domination | `ANTI_DOMINATION_{ACTIVATION_HEIGHT,REQUIRED,WINDOW,LOOKBACK}` (reorg-safe) |
| Candidate set | `CANDIDATE_SET_{ACTIVATION_HEIGHT,REQUIRED}` |
| Candidate admission / gossip | `CANDIDATE_ADMISSION_{ACTIVATION_HEIGHT,REQUIRED,WINDOW}` (also §2: needs public-network review) |
| Chain-committed admission | `COMMITTED_ADMISSION_{ACTIVATION_HEIGHT,REQUIRED,WINDOW}` |
| Puzzle work modes | `PUZZLE_WORK_{ACTIVATION_HEIGHT,REQUIRED}` + `PUZZLE_*_BITS` (ASSIGNED work, NOT chain PoW; also §2) |
| Wallet emit helpers | emit-only; no private key / seed phrase in output |
| Pool production mirrors | byte-for-byte mirrors with parity tests; node authoritative |
| Node authoritative validation | `connect_block` re-validates every gated section; fail-closed |
| Mainnet hard-off | every gate returns false when `network_id == 0` |

### 2. Implemented but requires external security review before any non-test network

| Mechanism | Gate(s) | Why review |
|---|---|---|
| **True VRF `AssignmentProofV2`** | `TRUE_VRF_{ACTIVATION_HEIGHT,REQUIRED}` | depends on `vrf_fun 0.12.1` / `secp256kfun 0.12` (pre-1.0, not formally audited); secp256k1 RFC 9381 ECVRF; review the crate + the V2 wiring |
| Finality committee | `FINALITY_COMMITTEE_{ACTIVATION_HEIGHT,REQUIRED}` + `FINALITY_THRESHOLD_{NUM,DEN}` | consensus-finality logic + threshold economics need review |
| Finality vote gossip | `FINALITY_GOSSIP_{ACTIVATION_HEIGHT,REQUIRED,WINDOW}` | public-network propagation/DoS behavior needs review |
| Candidate admission / gossip (public-network) | `CANDIDATE_ADMISSION_*` | "best among admitted to THIS node in the window" is propagation-sensitive; public-network admission tuning needs review |
| Puzzle work modes | `PUZZLE_WORK_*` | assigned-work difficulty model needs review (does NOT touch chain difficulty/LWMA) |

### 3. Excluded from the local track (non-code; out of scope here)

- External security review of `vrf_fun`/`secp256kfun` + finality + admission.
- Public testnet deployment.
- Independent third-party audit.
- Governance / community vote.
- Mainnet activation decision (height + flip of the gates).

### 4. Not mainnet-ready (applies to the whole PoAW-X track)

All of the above is **mainnet hard-off** and gate-disabled by default. Nothing in this track
is mainnet-ready. No mainnet activation is claimed, scheduled, or possible without an explicit
operator decision to set activation heights + `*_REQUIRED=1` on a non-zero network — which is
gated behind the pending review/testnet/audit/vote items in §3.

## B. Security review required

- **`vrf_fun` / `secp256kfun`** (the true-VRF dependency) require **external review before any
  non-test network** — pre-1.0, not formally audited; pin + (ideally) vendor before mainnet.
- **Finality committee** logic (vote validation, threshold, finalization of the parent)
  requires review.
- **Candidate admission / gossip** behavior requires **public-network** review (propagation,
  windowing, anti-flood).
- **Finality vote gossip** requires public-network review.
- **All PoAW-X gates are testnet/devnet only.** **No mainnet activation is claimed.**

## C. Gate audit

- **~17 gate families**, all read from `IRIUM_POAWX_*` env vars (names enumerated in §A and in
  the per-phase docs).
- **Default off:** each `*_ACTIVATION_HEIGHT` defaults to `None` (inactive) and each
  `*_REQUIRED` defaults to `false`; absent env ⇒ mechanism inactive.
- **Mainnet hard-off:** `network_id == 0` (mainnet/unset) forces every gate false — 44
  `network_id == 0 / network_id_byte() == 0 / network_id_from_env() == 0` guards across the
  node + pool. Mainnet-off is unit-tested per module (`*_gate(0, …) == false`,
  `mainnet hard-off` assertions).
- **Env names documented** in the per-phase docs (21c–21i, 22a, 22b/22c/22d/22e) and listed
  here.
- **Cannot activate accidentally:** activation requires BOTH a configured activation height
  AND (for enforcement) `*_REQUIRED=1` AND a non-zero network id — three independent
  conditions, none defaulted on.

## D. Code safety scan

- **No private key / seed output:** wallet helpers are emit-only; VRF/signing secrets are
  inputs (`--secret-hex`) and never echoed; per-feature tests assert no `secret`/`private`/
  `mnemonic` value leaks.
- **No pool-held VRF secrets:** the pool has **no** `vrf_fun`/`secp256kfun` dependency and
  never proves; its production code only mirrors + bundles miner-supplied proofs (the only
  `AssignmentProofV2::prove` references in the pool are in `#[cfg(test)]` via the node
  dev-dependency). The node is authoritative.
- **No homemade VRF:** the VRF is `vrf_fun::rfc9381::tai` over `secp256kfun` (5 references,
  all in `src/poawx_candidate.rs`); no hand-rolled curve math.
- **No OpenSSL / secp256k1-sys / bindgen:** `cargo tree` shows none. (`ring` is the
  pre-existing rustls TLS backend, unrelated.)
- **No LWMA / difficulty / target changes:** no `lwma`/`difficulty`/`target`/`pow` file
  touched across the PoAW-X track; chain difficulty / LWMA-144 untouched.
- **No wall-clock consensus dependency:** no `SystemTime`/`Instant::now`/`UNIX_EPOCH` in the
  PoAW-X consensus modules.
- **No floats in consensus:** the only `f64` in the PoAW-X modules is inside a `#[cfg(test)]`
  distribution test (`phase20_fairness_distribution_34_33_33`); consensus math is
  integer/fixed-point.
- **Bounded deserialization:** all wire types use fixed `*_WIRE` sizes and `*_MAX_BYTES`/
  `*_CAP` anti-oversize bounds (17+ such consts) with explicit length checks.
- **Deterministic ordering:** canonical sorts + `BTreeMap`/`BTreeSet`; no hash-map iteration
  in consensus output.
- **Domain separators present:** every digest is domain-separated (28 distinct
  `IRIUM_POAWX_*_V1/_V2` tags).
- **network/height/role/seed binding present:** tickets, candidates, admissions, puzzle
  challenges, finality votes, and V2 proofs all bind `network_id`, `target_height`, `role_id`,
  and the seed/prev_hash.

## E. Verification (this audit)

- `cargo tree | grep -Ei 'openssl|secp256k1-sys|bindgen'` → **NONE**
- `cargo fmt -- --check` (lib) → clean; (pool) → clean
- `cargo test --lib poawx` → **125 passed** (parallel)
- `cargo test --lib phase20 -- --test-threads=1` → **33 passed**
- `cargo test --lib reward -- --test-threads=1` → **9 passed**
- `cargo test --bin irium-wallet poawx` → **6 passed**
- `cargo test --bin irium-wallet` → **428 passed**
- `cargo test --bin iriumd -- --test-threads=1` → **256 passed**
- pool: `cargo fmt -- --check` → clean; full `cargo test` → **95 passed**; `phase20` → **21**;
  `delegation` → **41**; `native_rewardable` → **6**
- (full `cargo test --lib -- --test-threads=1` → **724 passed**, recorded at Phase 22E.)

## F. Wording

- ✅ "PoAW-X **local technical implementation is complete** against the blueprint."
- ✅ "True VRF is **implemented but requires external review**."
- ✅ "Public testnet / independent audit / governance vote **pending**."
- ❌ NOT "mainnet-ready." ❌ NOT "audited." ❌ NOT "production-ready."

## G. Conclusion

The PoAW-X blueprint is **locally, technically complete**: all mechanisms — multi-role
rewards + 55/22/13/10 split, fees, delegation, precommit/reveal + gossip, tickets, penalties,
anti-domination, candidate set + admission + chain-committed admission, true-VRF
`AssignmentProofV2` (E2E: wallet → admission → node → pool bundle → block → connect_block),
puzzle work modes, and the finality committee + vote gossip — are implemented, gated, and
mainnet hard-off, with node-authoritative validation and pool byte-parity. It is **not
mainnet-ready**, and it **requires external security review and a public testnet (then
independent audit + a governance/activation decision) before any push, merge, or mainnet
activation.**

## Phase 24E update (two-VPS production-candidate validation — PARTIAL)

Phase 24E attempted the full two-VPS all-gates validation. Cross-host P2P was BLOCKED at the
firewall/provider layer (port 40610 dropped despite an OS ufw allow; SSH:22 from the same source
worked). A single-host loopback demo validated, under all gates on a live node, the admission +
finality ingest/validation/cache path (true-VRF V2 admission + member-signed finality vote both
accepted [200 OK] and cached); Phase 24C storage isolation stayed safe; the VRF secret never
leaked. NOT validated: cross-host P2P/gossip, node-to-node P2P gossip (same-host peers are
filtered), all-gates block production, fee blocks, observer/restart block validation. NOT
production-ready; NOT mainnet-ready. See docs/poaw-x-phase24e-two-vps-production-candidate-
validation.md.

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

## Phase 24L Windows live proof — RESULT: PASSED

The Windows local devnet live proof ran end-to-end and PASSED: a real Irium-native-PoW all-gates
block was submitted to a real local devnet node over RPC and accepted, advancing the chain
height 0 -> 1 (block 31df881052b05dc6319c5915ca938b282df60ab7e823aba44ee5edd20dfd23bf, irx1 root
772e1cd700af122e5bc2a586a1eb94d4dc33bdd2ab819dba435df9875c7ed9bd, official 0% fee, all-gates
sections present). Post-fix HEAD 1ca7d89. Two genuine bugs were fixed to make it work on Windows:
cef587d (preserve Windows drive prefix in the storage guard) and 1ca7d89 (initialize the
standard-header activation global in the standalone harness). Mainnet node (PID 33752) and the real
%USERPROFILE%\.irium wallet/config were untouched; no proof listeners remained. Allowed claim:
"Local Windows devnet live proof succeeded: a real Irium-native-PoW all-gates block was submitted
to a real node and accepted, advancing the chain." NOT production-ready / mainnet-ready / audited.
Remaining: cross-host P2P provider/firewall, independent audit, public testnet, governance/mainnet
activation. See docs/poaw-x-phase24l-windows-live-proof-result.md.
