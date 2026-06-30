# PoAW-X Phase 23A — true-VRF internal security review

**This is an INTERNAL security review by Claude Code. It is NOT an independent third-party
audit and does NOT replace one.** Local-only (branch
`testnet/poawx-phase20-blueprint-completion-local`; not pushed; remote branch absent; `main`
untouched). Scope: the true-VRF `AssignmentProofV2` dependencies + wiring from Phase 22D/22E.
**Mainnet hard-off; not mainnet-ready.**

## 1. Review scope

- Dependencies: `vrf_fun 0.12.1`, `secp256kfun 0.12.1`, `sha2 0.10.9`, `bincode 2.0.1`, and
  transitive crates.
- `AssignmentProofV2` primitive (`src/poawx_candidate.rs`).
- AVR2 ext section (`src/poawx.rs`).
- Candidate admission V2 carrying (`src/poawx_admission.rs`).
- Committed-admission root binding (`src/poawx_committed_admission.rs`).
- Wallet proof generation/submission (`src/bin/irium-wallet.rs`).
- Pool opaque bundling (`pool/irium-stratum/src/delegation.rs`).
- Node admission validation + `connect_block` VRF enforcement (`src/chain.rs`).

## 2. Threat model

Adversaries considered: (a) a miner trying to forge/steal a favorable VRF assignment, replay
another party's proof, or reuse a proof across height/role/miner/ticket/seed; (b) a pool
trying to fabricate, omit, or substitute proofs; (c) a network peer sending malformed/oversized
admissions or block ext sections (DoS / parser abuse); (d) accidental mainnet activation;
(e) secret-key leakage via wallet output/logs. Out of scope: the security of the underlying
secp256k1/RFC 9381 construction itself (assumed correct per the spec; the implementing crate's
correctness is the subject of the still-required EXTERNAL audit), and network-layer DoS beyond
parser bounds.

## 3. Dependency review

| Crate | Version (Cargo.lock) | License | build.rs | unsafe | notes |
|---|---|---|---|---|---|
| `vrf_fun` | 0.12.1 | 0BSD | no | 0 | `#![no_std]`; RFC 9381 ECVRF (tai); direct dep only |
| `secp256kfun` | 0.12.1 | 0BSD | no | 0 real (1 comment) | `#![no_std]`; **vendors k256 field arithmetic** (`src/vendor/k256`) |
| `sigma_fun` | 0.9.0 | 0BSD | no | 0 | sigma protocols; pulled by vrf_fun |
| `secp256kfun_arithmetic_macros` | 0.2.0 | 0BSD | no | 1 | proc-macro; emits one `unsafe` NonZero-scalar marker |
| `subtle-ng` | 2.5.0 | BSD-3-Clause | no | 1 | constant-time primitives (standard `unsafe`) |
| `sha2` | 0.10.9 | — | — | — | matches repo (digest 0.10.7) |
| `bincode` | 2.0.1 | — | — | — | fixed-config encode/decode of the 81-byte VrfProof |

- `cargo tree -i vrf_fun` → only `irium-node-rs`. `cargo tree -i secp256kfun` → `irium-node-rs`
  + `sigma_fun` ← `vrf_fun`. No other consumers.
- `cargo tree | grep -Ei 'openssl|secp256k1-sys|bindgen|native-tls'` → **NONE**. (`ring` is the
  pre-existing rustls TLS backend, unrelated to VRF.)
- **No build scripts**, **no proc-macro network/runtime deps**, both VRF crates are `#![no_std]`
  (no `std::net`/`tokio`/`reqwest`/`std::process`/`asm!`).
- `unsafe`: `vrf_fun`/`secp256kfun`/`sigma_fun` have **no real `unsafe`** (the one secp256kfun
  hit is a code comment); the macro crate emits a single NonZero marker `unsafe`; `subtle-ng`
  uses standard constant-time `unsafe`. No FFI.
- **Pre-1.0** (0.12.1) — API/behavior may change; pin (done, exact in Cargo.lock) and vendor
  before mainnet.
- No duplicate versions of the VRF crates. `cargo-deny`/`cargo-geiger` not installed (noted;
  not installed globally per the rules).
- **Pool depends on NO VRF crate** — it treats proofs as opaque 273-byte wire.

## 4. Code review findings

### Cryptographic (AssignmentProofV2)

- **Message binding** (`vrf_message`): domain separator `IRIUM_POAWX_VRF_MESSAGE_V2` ‖
  network_id ‖ target_height ‖ role_id ‖ solver_pkh ‖ ticket_digest ‖ seed ‖
  assignment_public_key. **All eight required fields are bound.** ✅
- **Verification** (`validate`): version/network/height checked; digest recomputed; the 33-byte
  key is parsed to a `secp256kfun::Point`; the 81-byte bincode `VrfProof` is decoded with an
  exact-length check; `tai::verify` runs against the bound message; the carried `vrf_output` is
  checked against the recomputed output. Wrong message/key/proof/output/digest all reject
  (tested). ✅
- **Replay / substitution resistance:** every distinguishing field (height, role, solver,
  ticket, seed, key) is inside `alpha`, so a proof for one (height/role/miner/ticket/seed)
  fails verification for any other — cross-context reuse is rejected (tested per field). ✅
- **Malleability / canonicality:** fixed 273-byte wire; `serialize`/`deserialize` enforce the
  exact length; the proof body is a fixed 81-byte bincode encoding with an exact-length decode
  (no trailing bytes); ext AVR2 and admission trailing wires enforce exact/bounded lengths.
  Malformed/truncated/oversized inputs reject (Phase 23A tests). ✅
- **Output / score:** `assignment_v2_score_from_output` = first 8 bytes of `vrf_output`,
  little-endian (documented); RFC 9381 deterministic nonce ⇒ identical output across
  wallet/node; tie-breaks are deterministic (existing candidate sort); integer-only, no floats.
  ✅

### Wallet secret handling
- `poawx-assignment-proof-v2` and `poawx-candidate-admission --secret-hex` take the secret as
  **input only**; it is never written to the JSON, never logged, and not present in error
  strings (errors are static messages). Self-verify before emit. Submit path POSTs only the
  public `wire_hex`. Mainnet (network_id 0) disabled. Tests assert the secret value never
  appears in output. ✅ (The wallet's general WIF/key-management code handles private keys for
  ordinary wallet operations — that is OUTSIDE the V2 emit scope and does not echo the V2
  secret.)

### Node validation
- Admission `validate` (called by `ingest_bytes` before caching): under `true_vrf_active` it
  REQUIRES a V2 proof bound to the candidate (role/solver/ticket/key/seed + vrf_output ==
  candidate digest) and VRF-verifies it; a V1-only admission is rejected; malformed rejects
  pre-cache. ✅
- `CandidateAdmissionV1` binds the V2 proof into the admission digest; the candidate digest is
  the VRF output; the committed-admission root binds the candidate digest ⇒ binds the VRF
  output (tested: committed root changes when the output changes). ✅
- `connect_block` `validate_block_true_vrf`: requires the AVR2 section + candidate set under
  enforcement; each proof VRF-verified + bound to the SELECTED candidate; candidate
  assignment_proof_digest must equal the VRF output; V1-only block rejected; mutated AVR2 and
  wrong-score rejected. `validate_block_candidate_sets` uses `validate_scoring` under the gate
  (pure `validate_self` otherwise) — one block satisfies both. ✅

### Pool
- No VRF dependency; never proves. `decode_admission_v2` + `build_admitted_v2_proofs` +
  `build_pool_true_vrf_section` bundle the SELECTED candidates' miner-supplied proofs; the
  builders FAIL CLOSED if a selected role lacks a proof. Official fee-0 and third-party fee
  paths both attach AVR2; the node re-verifies (pool cannot bypass validation). ✅

### Gate / activation
- `IRIUM_POAWX_TRUE_VRF_ACTIVATION_HEIGHT` (None = off) + `IRIUM_POAWX_TRUE_VRF_REQUIRED`
  (false = off); `network_id == 0` hard-off. Gate off ⇒ V1 accepted, byte-identical wire; gate
  on ⇒ V2 required, V1-only rejected. Mainnet cannot activate accidentally (needs a non-zero
  network + activation height + required flag — three independent conditions). Docs match. ✅

## 5. Test / verification results

- `cargo tree | grep -Ei 'openssl|secp256k1-sys|bindgen|native-tls'` → NONE.
- fmt clean (lib + pool).
- `cargo test --lib poawx -- --test-threads=1` → 128 passed.
- `cargo test --lib phase20 -- --test-threads=1` → 33; `reward` → 9.
- `cargo test --bin irium-wallet poawx` → 6; `cargo test --bin irium-wallet` → 428.
- `cargo test --bin iriumd -- --test-threads=1` → 256 passed.
- pool: fmt clean; full → 96; `phase20` → 21; `delegation` → 42; `native_rewardable` → 6.

### Negative test coverage (H) — all present
wrong network, height, role, solver, ticket, seed, public key, mutated output, mutated proof,
malformed proof length, wrong digest, V1-only under V2 required, missing AVR2, wrong candidate
score, secret-not-printed, mainnet hard-off, gate-off compatibility. Phase 23A added bounded-
deserialization/malformed-input tests for the V2 proof, ext AVR2 section, admission trailing
wire, and the pool mirror.

## 6. Risk rating

**Overall: LOW for local/devnet use; NOT acceptable for public/non-test/mainnet without an
external audit.** The wiring is gated, mainnet-hard-off, node-authoritative, fail-closed, and
covered by positive + negative tests. The residual risk is concentrated in (a) the maturity of
the pre-1.0, non-audited VRF crates, and (b) public-network behavior of finality/admission
gossip — both of which require external review.

## 7. Open issues

| ID | Sev | Area | Description |
|---|---|---|---|
| 23A-INFO-1 | Informational | dependency | `vrf_fun`/`secp256kfun` are pre-1.0 (0.12.1) and not formally audited; correctness of the ECVRF implementation is assumed pending external audit. |
| 23A-INFO-2 | Informational | dependency | `secp256kfun` vendors k256 field arithmetic (`src/vendor/k256`); include this in the external audit scope. |
| 23A-LOW-1 | Low | admission | "best among candidates admitted to THIS node in the window" is propagation-sensitive (documented honest limitation); needs public-network review/tuning. |
| 23A-INFO-3 | Informational | dependency | One `unsafe` in the proc-macro (NonZero marker) + `subtle-ng` (constant-time); standard, no FFI. |

**No Critical, High, or Medium findings. No consensus change was required.**

## 8. Recommended fixes

- Before any non-test network: obtain an **external audit** of `vrf_fun`/`secp256kfun` (incl.
  the vendored k256 arithmetic); **vendor + pin** the chosen versions.
- Public-network review of candidate admission/gossip + finality gossip (propagation,
  windowing, anti-flood) before public testnet.
- No code fix is required for local/devnet testing.

## 9. Acceptability for local/devnet testing

**Phase 22D/22E is acceptable for LOCAL / DEVNET testing** behind the (default-off,
mainnet-hard-off) true-VRF gate: the proof construction is a standard RFC 9381 ECVRF over a
fully-bound message, the node is authoritative and fail-closed, the pool holds no secret, and
malformed input is rejected with bounded deserialization.

## 10. External audit still required

**An independent external audit is still REQUIRED before any public testnet, non-test network,
or mainnet activation.** This internal review does not replace it. No mainnet-ready / audited /
production-ready claim is made.
