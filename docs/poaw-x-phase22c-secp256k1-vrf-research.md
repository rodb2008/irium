# PoAW-X Phase 22C — secp256k1 true-VRF path research (for AssignmentProofV2)

**Status:** Research only — **no consensus code, no `Cargo.toml`/`Cargo.lock` change, no
VRF implementation, no homemade VRF.** Local-only; not pushed; remote branch absent.
**Mainnet hard-off; not mainnet-ready.** `AssignmentProofV1` **remains the testnet/devnet
placeholder** until a true-VRF V2 is implemented + reviewed + approved. A viable Option-A
(secp256k1, no-OpenSSL) path was found and proof-of-built in a scratch project **outside**
the repo; **implementation is deferred to Phase 22D** pending review/approval.

## A. Baseline (repo)

- Crypto deps: `k256 0.13` (secp256k1) + `sha2 0.10` + `digest 0.10` + `generic-array` +
  `typenum` + `rand_core 0.6` + `proc-macro2`/`quote`. **No VRF crate; no OpenSSL.**
- Key model: 33-byte compressed secp256k1 assignment public key; `solver_pkh =
  HASH160(secp256k1 pubkey)`; ECDSA `sign_prehash`/`verify_prehash`.

## B. Candidate evaluation (`cargo search` + `cargo info`)

| Crate | Curve | True VRF? | OpenSSL? | Pure Rust? | License | Verdict |
|---|---|---|---|---|---|---|
| `vrf 0.2.5` (witnet) | secp256k1 | yes (ECVRF) | **YES (OpenSSL)** | no | — | **REJECT** (OpenSSL vs rustls policy) |
| `libecvrf 1.2.1-beta.0` | secp256k1+keccak | yes | unclear, **beta**, EVM-oriented | partial | Apache-2.0 | **REJECT for now** (beta, unclear maintenance/scope) |
| `vrf-rfc9381 0.0.5` | multi (early) | yes (RFC9381) | no | yes | MIT/Apache | **DEFER** (0.0.5, very early) |
| `vrf-r255 0.1.0` | ristretto255 | yes | no | yes | — | N/A (not secp256k1 → Option B, not A) |
| `ecvrf 0.4.4` | curve25519 | yes | no | yes | — | N/A (not secp256k1) |
| **`vrf_fun 0.12.1`** (secp256kfun / LLFourn) | **secp256k1** | **yes — RFC 9381 ECVRF (TAI + SSWU)** | **NO** | **YES** | **0BSD** | **VIABLE — recommended Option A** |

Rejected per policy: OpenSSL paths (`vrf`/witnet), unaudited homemade math, signature-only
pseudo-VRF, non-building crates, incompatible key models, unclear-maintenance/beta crates.

## C. Scratch build (outside the repo — `/tmp/vrf_scratch`, repo Cargo untouched)

`cargo init` + `cargo add vrf_fun@0.12.1 secp256kfun@0.12 sha2@0.10 hex` → **built and ran
cleanly** on rustc 1.92.0.

- **No OpenSSL, no `secp256k1-sys` (C), no `bindgen`, no `ring`** — verified via `cargo tree`.
- **~23 transitive crates total**; net-new vs the repo's existing tree (≈8): `vrf_fun`,
  `secp256kfun`, `secp256kfun_arithmetic_macros`, `sigma_fun`, `subtle-ng`, `rand_chacha`,
  `ppv-lite86`, `zerocopy`. (The rest — `generic-array`, `typenum`, `digest`, `sha2`,
  `rand_core`, `proc-macro2`, `quote`, `crypto-common`, `block-buffer` — are already in the
  repo tree.)
- **`sha2 0.10` matches the repo's `sha2 0.10`** (same `digest 0.10`) — no version conflict.
  (Note: `sha2 0.11` is incompatible with secp256kfun's `digest 0.10` — must pin `sha2` to
  `0.10`.)
- **API (RFC 9381 ECVRF-SECP256K1-SHA256-TAI):**
  - `rfc9381::tai::prove::<Sha256>(&keypair, alpha) -> VrfProof<U16>`
  - `rfc9381::tai::verify::<Sha256>(public_key, alpha, &proof) -> Option<VerifiedRandomOutput>`
  - `rfc9381::tai::output::<Sha256>(verified) -> [u8; 32]`
  - keys are `secp256kfun::KeyPair` (secp256k1).
- **Proof-of-build assertions all passed:** `prove` then `verify` returns a 32-byte output;
  **deterministic** (two `prove` calls → identical output, RFC 9381 deterministic nonce);
  **wrong message → `verify` returns `None`** (bound + unpredictable). Sample run output:
  `vrf_output=c8ad10832b15a2218996e81c44fc3bfd8b2262a63e2ffe573ed7ff839ef59a00`.
- Scratch dir at `/tmp/vrf_scratch` (ephemeral; the repo and its `Cargo.toml`/`Cargo.lock`
  were **not** modified — only the shared cargo registry cache gained downloaded crates,
  which does not affect repo builds).

## E. Viability + risks

**Viable.** `vrf_fun`/`secp256kfun` is a pure-Rust, no-OpenSSL, secp256k1 RFC 9381 ECVRF
that builds + runs, keeps the existing secp256k1 key model, and produces a deterministic,
publicly-verifiable output bound to the message — exactly the V2 requirements (Phase 22B §3).

Risks / open items (must be resolved before consensus use):
1. **Pre-1.0 (`0.12.1`)** — API may change; pin exactly and vendor if needed.
2. **Audit status** — `secp256kfun` is well-regarded and widely used (LLFourn), but this is
   **not a claim of formal audit**; a **security review is required before any non-test
   network** (Phase 22B Option C gate still applies).
3. **Key interop** — both this and `k256` are secp256k1, but they are different Rust types;
   V2 must convert the 33-byte compressed pubkey (`assignment_public_key`) and the miner
   secret between `k256` and `secp256kfun` (byte-level, same curve) — to be designed/tested
   in 22D.
4. **Proof wire** — `VrfProof` (gamma + compact proof) needs a fixed, versioned consensus
   serialization; `secp256kfun`/`vrf_fun` expose `serde`/`bincode` features — 22D must
   define + test a canonical fixed-size encoding (do not rely on bincode defaults for
   consensus).
5. **MSRV 1.85** — fine (repo toolchain is 1.92).

## Phase 22D plan (implementation — only after review/approval)

1. Add `vrf_fun = "0.12.1"` (+ `secp256kfun = "0.12"`, `sha2` stays `0.10`) to `Cargo.toml`
   **(explicit approval required; not done in 22C)**.
2. Implement `AssignmentProofV2` (a new gated section), VRF message =
   domain ‖ network_id ‖ target_height ‖ role_id ‖ ticket_digest ‖ seed ‖ solver_pkh ‖
   assignment_public_key; carry `(vrf_output[32], vrf_proof)` with a fixed wire; node
   `verify` + derive `assignment_score` from `vrf_output`.
3. Gate behind `IRIUM_POAWX_TRUE_VRF_{ACTIVATION_HEIGHT,REQUIRED}` (mainnet hard-off);
   keep `AssignmentProofV1` accepted when the V2 gate is off; bind V2 into the candidate/
   admission digests.
4. Wallet emits proof/output only (never the VRF secret); pool mirrors + the node
   re-validates (pool is one interface, not the owner).
5. Security review of `secp256kfun`/`vrf_fun` + the V2 wiring before enabling on any
   non-test network.

## Conclusion

- **Option A remains viable and is now evidenced** — a safe, pure-Rust, no-OpenSSL,
  secp256k1 RFC 9381 VRF (`vrf_fun`/`secp256kfun`) builds + runs in a scratch project.
- **Option B (separate sr25519/Ristretto VRF key) stays the fallback** only if Option A is
  rejected at review (e.g. audit concerns).
- `AssignmentProofV1` **is still a placeholder** (no homemade VRF). **No dependency was
  added to the repo**; implementation is deferred to **Phase 22D** pending explicit
  approval + security review. Mainnet hard-off; not mainnet-ready.
