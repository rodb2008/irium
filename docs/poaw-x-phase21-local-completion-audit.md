# PoAW-X Phase 20/21 — Local completion audit + push-readiness package

**Status:** Local audit only. **Nothing pushed.** Local branch
`testnet/poawx-phase20-blueprint-completion-local`. PoAW-X is **consensus/network-level**;
the **pool is one miner interface only**, not the owner — the node authoritatively
validates everything. **Mainnet remains hard-off** for every PoAW-X/Phase-21 gate. This is
**not** a mainnet-ready or full-production claim.

## A. Repository audit

- **Branch:** `testnet/poawx-phase20-blueprint-completion-local`
- **HEAD:** `3821fe8` (+ this docs commit) — was `89a290d` before the audit
- **Upstream:** none configured (branch never pushed; absent on `origin`)
- **`origin/main`:** `19c496d` (merge base with this branch: `5d4604c`, v1.9.115)
- **`git status --short`:** clean
- **Local range:** 166 PoAW-X Phase 20/21 commits since `origin/main` (oldest `ef754f0`
  "testnet: validate PoAW-X stratum TCP miner path" → `89a290d`), plus the two Phase 21J
  audit commits.
- **No PR, no tag at HEAD, no release, no push.** (138 pre-existing local tags, none on
  this branch's HEAD; none created here.)

## B. Feature-status matrix

### Implemented (local, gated, mainnet hard-off)
| Feature | Where | Enforcement |
|---|---|---|
| Multi-role rewards (55/22/13/10) | `poawx.rs` `multi_role_amounts` | coinbase validator |
| Official fee-0 path | `poawx.rs`/`chain.rs` | default |
| Third-party fee path (≤200bps, PRIMARY-only) | `poawx.rs`/`chain.rs` | gated, miner-signed |
| Delegated / non-custodial receipts (mode-1) | `poawx.rs` `Delegation` | connect_block verify |
| Hidden-precommit root | `poawx.rs`/`chain.rs` | connect_block (gated) |
| Role precommit/reveal collection | `poawx_gossip.rs`/pool | loopback store |
| Role gossip bridge | `protocol.rs`/`p2p.rs`/iriumd | P2P + loopback RPC |
| Ticket primitives + enforcement (21A/21B) | `poawx_ticket.rs`/`chain.rs` | connect_block (gated) |
| Penalty primitives + enforcement (21A/21B) | `poawx_penalty.rs`/`chain.rs` | connect_block (gated) |
| Persistent reorg-safe anti-domination (21C) | `poawx_dominance.rs`/`chain.rs` | connect_block (gated) |
| Candidate set + AssignmentProofV1 (21D) | `poawx_candidate.rs`/`chain.rs` | connect_block (gated) |
| Candidate admission + gossip (21E) | `poawx_admission.rs`/`p2p`/iriumd | P2P + RPC + connect_block |
| Puzzle work modes (21F) | `poawx_puzzle.rs`/`chain.rs` | connect_block (gated) |
| Finality committee proof + enforcement (21H) | `poawx_finality.rs`/`chain.rs` | connect_block (gated) |
| Finality vote P2P gossip + collection (21I) | `poawx_finality.rs`/`p2p`/iriumd/pool | P2P + RPC + fetch |
| Wallet emit helpers | `irium-wallet.rs` | emit-only, no key output |
| Pool production mirrors | `pool/irium-stratum/src/delegation.rs` | byte-identical, parity-tested |
| Node authoritative validation | `chain.rs` connect_block | re-verifies all of the above |

### Pending / excluded (NOT implemented or out of track)
| Item | Status |
|---|---|
| True cryptographic VRF | **PENDING** — Phase 21G **Outcome B**: no safe VRF dep/key-model path in-tree; `AssignmentProofV1` remains a documented VRF-style placeholder |
| Provably-complete public-network candidate admission | **PENDING** — Phase 21E validates best-among-admitted-to-this-node, not best-among-unseen |
| Public testnet with outside miners | **EXCLUDED** |
| Independent security audit | **EXCLUDED** |
| Community vote | **EXCLUDED** |
| Mainnet activation | **EXCLUDED** (hard-off) |

**Explicit:** PoAW-X is consensus/network-level; the pool is one miner interface only;
mainnet remains hard-off; **no mainnet-ready claim; no full-production claim.**

## C. Gate audit

Every PoAW-X gate is **default off** and **hard-off when `network_id == 0`** (mainnet/
unset). The per-module `network_id == 0` hard-off guard is present and unit-tested in each
module (`poawx_admission`, `poawx_candidate` ×2, `poawx_dominance` ×2, `poawx_finality`
×2, `poawx_penalty` ×2, `poawx_puzzle`, `poawx_ticket` ×3, `poawx_adaptive`,
`poawx_gossip`); Phase 20 gates hard-off via `activation::network_id_byte() == 0`.

| Gate (activation) | REQUIRED flag | Other | Module |
|---|---|---|---|
| `IRIUM_POAWX_ACTIVATION_HEIGHT` (+`_MODE`) | — | — | activation/poawx |
| `IRIUM_POAWX_MULTI_ROLE_REWARD_ACTIVATION_HEIGHT` | — | — | poawx/chain |
| `IRIUM_POAWX_FAIRNESS_MATRIX_ACTIVATION_HEIGHT` | — | — | poawx/chain |
| `IRIUM_POAWX_HIDDEN_PRECOMMIT_ACTIVATION_HEIGHT` | — | — | poawx/chain |
| `IRIUM_POAWX_THIRD_PARTY_FEE_ACTIVATION_HEIGHT` | `…_THIRD_PARTY_POOL_MODE` | — | poawx/chain |
| (role) | — | `IRIUM_POAWX_ROLE_PROTOCOL_ENABLED`, `IRIUM_POAWX_ROLE_GOSSIP_ENABLED`(+`_WINDOW`), `IRIUM_POAWX_SYNTHETIC_ROLE_CLAIMS` | poawx_gossip |
| `IRIUM_POAWX_TICKETS_ACTIVATION_HEIGHT` | `IRIUM_POAWX_TICKETS_REQUIRED` | `IRIUM_POAWX_TICKET_SYBIL_BITS` | poawx_ticket |
| `IRIUM_POAWX_PENALTY_STATE_ACTIVATION_HEIGHT` | `IRIUM_POAWX_PENALTY_STATE_REQUIRED` | — | poawx_penalty |
| `IRIUM_POAWX_ANTI_DOMINATION_ACTIVATION_HEIGHT` | `IRIUM_POAWX_ANTI_DOMINATION_REQUIRED` | `…_WINDOW`, `…_LOOKBACK` | poawx_dominance |
| `IRIUM_POAWX_ADAPTIVE_MODE_ACTIVATION_HEIGHT` | — | — | poawx_adaptive |
| `IRIUM_POAWX_CANDIDATE_SET_ACTIVATION_HEIGHT` | `IRIUM_POAWX_CANDIDATE_SET_REQUIRED` | — | poawx_candidate |
| `IRIUM_POAWX_ASSIGNMENT_PROOF_ACTIVATION_HEIGHT` | `IRIUM_POAWX_ASSIGNMENT_PROOF_REQUIRED` | — | poawx_candidate |
| `IRIUM_POAWX_CANDIDATE_ADMISSION_ACTIVATION_HEIGHT` | `IRIUM_POAWX_CANDIDATE_ADMISSION_REQUIRED` | `…_WINDOW` | poawx_admission |
| `IRIUM_POAWX_PUZZLE_WORK_ACTIVATION_HEIGHT` | `IRIUM_POAWX_PUZZLE_WORK_REQUIRED` | `IRIUM_POAWX_PUZZLE_BITS` | poawx_puzzle |
| `IRIUM_POAWX_FINALITY_COMMITTEE_ACTIVATION_HEIGHT` | `IRIUM_POAWX_FINALITY_COMMITTEE_REQUIRED` | `…_THRESHOLD_NUM/DEN` | poawx_finality |
| `IRIUM_POAWX_FINALITY_GOSSIP_ACTIVATION_HEIGHT` | `IRIUM_POAWX_FINALITY_GOSSIP_REQUIRED` | `…_WINDOW` | poawx_finality |

`IRIUM_POAWX_TRUE_VRF_{ACTIVATION_HEIGHT,REQUIRED}` are reserved by the Phase 21G
feasibility doc for a future `AssignmentProofV2`; **not implemented** (Outcome B).

Each gate has a `gate_logic_pure*` / `*mainnet hard-off` unit test asserting
`network_id == 0 ⇒ false`.

## D. Mainnet safety audit

- **All PoAW-X gates hard-off on mainnet** (`network_id == 0`), verified per module.
- **Language audit:** every doc occurrence of "mainnet-ready / production / public
  activation complete / true VRF complete" is a **negation** under a "What is NOT claimed"
  / "Not claimed:" heading. **No misleading positive claims found**; no doc fixes needed.
- **No mainnet/prod service touched**; no systemd/firewall/sudo; **no public ports bound**;
  no services or miner started.
- **Mainnet seed PID `219530` alive**; **prod pool workers `1804806`, `4042500/1/2` alive**
  (untouched throughout Phase 21).
- Chain difficulty remains **LWMA-144 automatic**; no PoW/target/bits/interval code touched
  by any Phase 21 commit.

## E. Security / consensus audit

- **Private-key / seed safety:** wallet emit helpers output **no private key / no seed
  phrase**. `poawx-finality-vote` takes the signing key as an **input** (`--secret-hex`,
  testnet throwaway) that is **never echoed** (unit-tested: the secret never appears in
  output). Assignment/admission/puzzle/ticket helpers require no key at all.
- **No floats in consensus math** (only one `f64` exists, in a `#[cfg(test)]` distribution
  assertion). **No wall-clock** in any PoAW-X module (deterministic height/seed windows).
- **No panics on malformed wire** in non-test code: deserializers return `Result` with
  bounded length checks (`ROLE_CANDIDATE_WIRE`, `FINALITY_VOTE_WIRE`,
  `CANDIDATE_ADMISSION_MAX_BYTES`, `FINALITY_VOTE_MAX_BYTES`, `MAX_CANDIDATES`,
  `FINALITY_MAX_VOTES`, puzzle profile bounds). The only non-test `expect`s are
  `FinalityVoteV1::signed` (signer-side, valid key) and `PuzzleMode::from_id(i%5)`
  (infallible) — neither is reachable from untrusted consensus input.
- **Deterministic ordering:** admission + finality caches use `BTreeMap`/`BTreeSet`;
  candidate/finality proofs sort canonically (by pkh/member) before digest/root.
- **Domain separators + binding:** every digest is domain-separated and binds
  network/height/role/seed (and signature where applicable); mutation changes the
  digest/root (unit-tested).
- **Fix applied (test-only):** unified the PoAW-X test env lock so
  `cargo test --lib poawx` is race-free (commit `3821fe8`). No non-test code changed.
- **Documented residual (pre-existing, not a regression):** the full unfiltered
  `cargo test --lib` run is occasionally flaky **in parallel** due to a pre-existing
  chain-test `IRIUM_NETWORK` env race (`test_validate_poawx_coinbase_rejects_zero_root`);
  it is **green single-threaded (708/0)**, which is the verification method used
  throughout. Not a consensus issue.

## F. Test results (this audit, HEAD `3821fe8`)

- `cargo fmt -- --check`: **clean** (lib + pool).
- lib full: **708 / 0** (`--test-threads=1`). `cargo test --lib poawx`: **113 / 0**
  (parallel, repeated). Modules: poawx_finality 5, poawx_puzzle 8, poawx_admission 3,
  poawx_candidate 7, poawx_ticket 6, poawx_dominance 12, poawx_adaptive 6, poawx_penalty 5.
  phase20 **33 / 0**; reward **9 / 0**.
- `cargo test --bin irium-wallet`: **425 / 0** (poawx 6).
- `cargo test --bin iriumd -- --test-threads=1`: **256 / 0** (at `89a290d`; the only
  change since is the `#[cfg(test)]` lib lock, which the iriumd binary does not compile).
- pool: `cargo fmt --check` clean; full **92 / 0**; phase20 21, delegation 38,
  native_rewardable 6.
- Docs grep sanity: "true VRF pending", "mainnet hard-off", "not mainnet-ready",
  "pool is one miner interface", and "public testnet/audit/vote excluded" all present.

## Push-readiness

The branch is a clean, self-contained, fully-tested local feature branch with **mainnet
hard-off** throughout and **no production impact**. It is suitable to push **for remote
backup / review only** (not for merge to `main`, not for mainnet). Recommended (operator
runs, not executed here):

```
git push -u origin testnet/poawx-phase20-blueprint-completion-local
```

Do **not** merge to `main`, tag, release, or activate mainnet from this branch.

## Update — Phase 22A

Phase 22A (chain-committed candidate admission) has since been added on top of this audit
(commits `2e97f5e`..`de43acf` + docs): admitted candidate-set roots are now chain-committed
in the parent block and re-validated at the target block (gated, mainnet hard-off). It
strengthens 21E without claiming offline-miner discovery; true VRF remains pending. See
`poaw-x-phase22a-committed-admission.md` and `poaw-x-blueprint-completion-gap-audit.md`.


## Phase 22B — true VRF decision package (PENDING)

True VRF remains **pending** (true VRF pending): `AssignmentProofV1` is a **placeholder**,
**mainnet hard-off**, **not mainnet-ready**, and **no homemade VRF** will be added. The
key-model + dependency decision (Option A secp256k1 ECVRF without OpenSSL, vs Option B a
separate audited sr25519/Ristretto VRF key, then Option C vendor + security review) is
captured in `docs/poaw-x-phase22b-true-vrf-decision-package.md`. No code/dependency/Cargo
change in Phase 22B (docs-only). PoAW-X is **not full blueprint-complete** until this VRF
decision is approved and implemented; no push, no mainnet, no audit/vote.


## Phase 22C — secp256k1 true-VRF research (Option A viable)

Research found a VIABLE Option A path: `vrf_fun 0.12.1` (secp256kfun) is a pure-Rust,
no-OpenSSL **secp256k1** RFC 9381 ECVRF that scratch-built + ran outside the repo
(prove/verify/output deterministic; wrong-message rejected). This is a real **true VRF**
candidate for a future `AssignmentProofV2` — but `AssignmentProofV1` remains the
**placeholder** (no homemade VRF; **no dependency added to the repo**; **mainnet hard-off**;
not mainnet-ready). Implementation is deferred to Phase 22D pending explicit approval +
security review. Details: `docs/poaw-x-phase22c-secp256k1-vrf-research.md`.

## Phase 22E — true-VRF E2E wiring (update)

Production wiring for `AssignmentProofV2` is complete (local-only, gated, mainnet hard-off):
wallet/miner emits the proof (`poawx-candidate-admission --secret-hex`, secret never echoed),
it is carried in the candidate admission and committed-admission root, the node validates at
ingest + block acceptance, and the pool fetches + bundles it into the Phase 20 ext AVR2
section (fail-closed; no VRF secret in the pool). Both official fee-0 and third-party fee
production paths pass with miner-supplied proofs. Not mainnet-ready (external security review
of `vrf_fun`/`secp256kfun` pending).
