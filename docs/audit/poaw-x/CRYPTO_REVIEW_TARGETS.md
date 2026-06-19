# PoAW-X crypto review targets

The highest-value target. The true-VRF construction is the newest and rests on a pre-1.0
dependency.

## Dependencies

| Crate | Version | License | Notes |
|---|---|---|---|
| `vrf_fun` | 0.12.1 | 0BSD | RFC 9381 ECVRF-SECP256K1-SHA256-TAI; `#![no_std]`; direct dep only |
| `secp256kfun` | 0.12.1 | 0BSD | secp256k1 group ops; `#![no_std]`; **vendors k256 field arithmetic** (`src/vendor/k256`) |
| `sigma_fun` | 0.9.0 | 0BSD | sigma protocols (pulled by vrf_fun) |
| `secp256kfun_arithmetic_macros` | 0.2.0 | 0BSD | proc-macro (one NonZero-marker `unsafe`) |
| `subtle-ng` | 2.5.0 | BSD-3-Clause | constant-time primitives (standard `unsafe`) |
| `sha2` | 0.10.9 | — | must stay 0.10 (digest 0.10); 0.11 breaks secp256kfun |
| `bincode` | 2.0.1 | — | fixed-config encode/decode of the 81-byte `VrfProof` |

- **No build scripts; no OpenSSL/secp256k1-sys/bindgen/native-tls; no FFI; `#![no_std]`**
  (no network/runtime). `ring` in the wider tree is the pre-existing rustls backend, unrelated.
- **Pre-1.0 (0.12.x), not formally audited** — a primary reason for the external audit.

## `AssignmentProofV2` — `src/poawx_candidate.rs`

- **Message binding** (`vrf_message`): `b"IRIUM_POAWX_VRF_MESSAGE_V2"` ‖ network_id ‖
  target_height ‖ role_id ‖ solver_pkh ‖ ticket_digest ‖ seed ‖ assignment_public_key.
- **`prove(secret, …)`:** builds a `secp256kfun::KeyPair` from the secret; derives the 33-byte
  compressed public key; `tai::prove::<Sha256>`; self-verifies; bincode-encodes the proof
  (exactly 81 bytes = gamma 33 + challenge 16 + response 32); computes the struct digest. The
  secret is never stored.
- **`validate(net, height)`:** version/net/height checks; digest recompute; parse the 33-byte
  key to `secp256kfun::Point`; bincode-decode the proof with an exact-length check;
  `tai::verify::<Sha256>` against the message; check `output == vrf_output`.
- **Output/score:** `assignment_v2_score_from_output` = first 8 bytes of the VRF output,
  little-endian (documented); RFC 9381 deterministic nonce ⇒ identical output across
  wallet/node; integer-only.
- **Wire:** fixed 273 bytes; `serialize`/`deserialize` enforce the exact length.

## Domain separators

28 distinct `IRIUM_POAWX_*_V1/_V2` tags (one per digest). Confirm each digest is domain-
separated and that no two digests can collide across types.

## Wallet secret handling — `src/bin/irium-wallet.rs`

`poawx-assignment-proof-v2` and `poawx-candidate-admission --secret-hex`: the secret is input
only, never echoed/logged/in JSON or error strings; self-verify before emit; submit posts only
public wire; mainnet disabled.

## No homemade VRF

The VRF is `vrf_fun::rfc9381::tai` over `secp256kfun` (5 references, all in
`src/poawx_candidate.rs`). No hand-rolled curve/field math in PoAW-X code.

## Questions for the external reviewer

1. Is the `vrf_fun`/`secp256kfun` RFC 9381 ECVRF implementation correct and constant-time
   where it must be (incl. the vendored k256 field arithmetic)?
2. Is the bincode encoding of `VrfProof` strictly canonical (single valid encoding; no
   alternate/short/long encodings accepted)?
3. Does the domain-separated message fully prevent cross-context proof reuse?
4. Is first-8-bytes-LE of the VRF output an acceptable score derivation (bias/grindability)?
5. Any side channels in `prove`/`validate` that could leak the secret on a malicious host?
6. Pinning/vendoring recommendation for a pre-1.0 dependency before mainnet.
