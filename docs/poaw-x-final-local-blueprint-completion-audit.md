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
