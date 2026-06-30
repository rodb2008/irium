# PoAW-X Phase 22D — true secp256k1 ECVRF `AssignmentProofV2`

**Status:** Implemented, **local-only** (not pushed; remote branch absent; no merge/PR/tag/
release). **Gated; mainnet hard-off; not mainnet-ready** (security review of `vrf_fun`/
`secp256kfun` still required before any non-test network — see Phase 22B/22C). This phase
turns the `AssignmentProofV1` *placeholder* into a real cryptographic VRF when the gate is
on; **`AssignmentProofV1` is retained** as the fallback when the gate is off. **No homemade
VRF**; no OpenSSL / `secp256k1-sys` / `bindgen`.

## 1. What landed

A true **RFC 9381 ECVRF-SECP256K1-SHA256-TAI** assignment proof (`AssignmentProofV2`) built
on `vrf_fun 0.12.1` + `secp256kfun 0.12` (pure-Rust, 0BSD, no OpenSSL), keeping Irium's
**secp256k1 key model**: the VRF key *is* a secp256k1 keypair and the 33-byte
`assignment_public_key` is its compressed SEC1 encoding (interops with `k256`).

- **Primitive** (`src/poawx_candidate.rs`): `AssignmentProofV2 { version, network_id,
  target_height, role_id, solver_pkh[20], assignment_public_key[33], ticket_digest[32],
  seed[32], vrf_output[32], vrf_proof[81], digest[32] }` — fixed **273-byte** wire.
  - `prove(secret, net, height, role, solver_pkh, ticket_digest, seed)` derives the public
    key, computes the VRF output + proof over the domain-separated message, self-verifies,
    and stamps the digest. **The secret never leaves the function**; only the public
    key/output/proof are retained.
  - `validate(net, height)` parses the 33-byte key into a `secp256kfun::Point`, decodes the
    bincode `VrfProof` (fixed 81 B = gamma 33 + challenge 16 + response 32), VRF-verifies
    against the message, and checks the carried output + digest. Deterministic; no secret.
  - VRF message (`alpha`) = `"IRIUM_POAWX_VRF_MESSAGE_V2" ‖ network_id ‖ target_height ‖
    role_id ‖ solver_pkh ‖ ticket_digest ‖ seed ‖ assignment_public_key` — so the output
    binds the full assignment context and changing any field changes the verification.
  - `assignment_v2_score_from_output` = first 8 bytes of the VRF output (LE) → the effective
    score derives from the **real VRF output**, not a recomputable hash.
- **Gates** (`src/poawx_candidate.rs`): `true_vrf_activation_height()` /
  `true_vrf_required()` / `true_vrf_active(h)` / `true_vrf_enforced(h)` via
  `IRIUM_POAWX_TRUE_VRF_ACTIVATION_HEIGHT` + `IRIUM_POAWX_TRUE_VRF_REQUIRED=1`, reusing the
  shared pure gate (`network_id == 0` → **mainnet hard-off**).
- **Ext + consensus** (`src/poawx.rs`, `src/chain.rs`): `Phase20ReceiptExt` gains an
  optional trailing **`AVR2`** section (`[compute, verify, support]` proofs) — present-only,
  **byte-identical + same digest when absent**, bound into the ext digest → receipts-root →
  `irx1` when present. `connect_block` calls `validate_block_true_vrf` when
  `true_vrf_enforced(height)`: every production receipt **must** carry the AVR2 section
  (the V1 placeholder is **rejected** under V2-required) **and** a candidate set; each V2
  proof is VRF-verified and bound to its role's **selected** candidate (role, `solver_pkh`,
  ticket digest, assignment public key, candidate-set seed), and the candidate's
  `assignment_proof_digest` **must equal the V2 VRF output**. Fails closed.
- **Pool** (`pool/irium-stratum/src/delegation.rs`): `AssignmentProofV2Mirror` — an
  **opaque** byte-for-byte mirror of the 273-byte wire (**no `vrf_fun` in the pool**; the
  pool never verifies or fabricates proofs — the node is authoritative). The ext mirror
  gains the `AVR2` section + `attach_true_vrf_section`; `pool_true_vrf_enforced` mirror gate
  (mainnet hard-off). Both production builders **fail closed** under enforcement (the pool
  holds no VRF secret). Fee paths untouched. A parity test asserts wire/digest equality vs
  the canonical node type.
- **Wallet** (`src/bin/irium-wallet.rs`): `poawx-assignment-proof-v2` emits a real V2 proof
  from `--secret-hex` (testnet throwaway). The **VRF secret is input-only and never
  echoed**; output carries only public material + `wire_hex`. Self-verifies before
  emitting; mainnet hard-off; emit-only.

## 2. Dependencies

Added (Phase 22D, approved): `vrf_fun = { version = "0.12.1", features = ["bincode"] }`,
`secp256kfun = "0.12"`, `bincode = "2"`; `sha2` stays **`0.10`** (0.11 breaks secp256kfun's
`digest 0.10`). `cargo tree` confirms **no `openssl` / `secp256k1-sys` / `bindgen`** in the
`vrf_fun` subtree; the pre-existing `ring` is the rustls TLS backend (unrelated). The pool
adds **no** VRF dependency.

## 3. Gate behaviour

| `IRIUM_POAWX_TRUE_VRF_*` | network | behaviour |
|---|---|---|
| off (default) | any | V1 accepted; AVR2 absent → byte-identical to pre-22D |
| activation+required | testnet/devnet | AVR2 required; V1-only block **rejected**; each V2 proof VRF-verified + bound to its selected candidate; score = VRF output |
| any | **mainnet (id 0)** | **hard-off** — gate never active |

## 4. Tests

- `poawx_candidate::assignment_v2_prove_verify_and_rejects` — prove/verify; deterministic
  output+digest; score-from-output; 273-byte wire round-trip; wrong
  net/height/role/solver/ticket/seed/pubkey, mutated output/proof, malformed proof, and
  digest-mismatch all reject; different key → different output.
- `poawx_candidate::true_vrf_gates` — activation/required logic + **mainnet hard-off**.
- `poawx::phase22d_ext_true_vrf_section_roundtrip_backward_compatible` — absent =>
  byte-identical; present round-trips + changes digest; mutation changes digest; combined
  with candidate-set + committed-admission round-trips.
- `chain::phase22d_true_vrf_enforcement` — valid accepts; missing-V2 (V1-only) /
  missing-candidate-set / mutated-proof / wrong-height reject; mainnet hard-off.
- `delegation::phase22d_pool_true_vrf_parity_and_failclosed` — proof wire == node; node
  round-trips mirror bytes; `AVR2` magic parity; ext-with-AVR2 wire+digest parity vs node;
  synthetic builder fails closed under enforcement; mainnet hard-off.
- `irium-wallet phase22d_assignment_proof_v2_emit_no_secret_mainnet_off` — emitted wire
  re-validates via the node lib; secret/private/mnemonic never leak; mainnet + missing
  secret error.

## 5. Limits / next

- **Security review** of `vrf_fun`/`secp256kfun` + the V2 wiring is **required before any
  non-test network** (Phase 22B Option-C gate). This phase does **not** claim mainnet
  readiness.
- The pool cannot produce V2 proofs (no VRF secret); under enforcement it fails closed and
  expects **miner-produced** proofs attached via `attach_true_vrf_section`. End-to-end
  miner→pool→node V2 production wiring is a follow-up.
- `AssignmentProofV1` remains the labeled placeholder when the gate is off.
