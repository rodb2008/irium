# PoAW-X Phase 21G — True cryptographic VRF: feasibility report (OUTCOME B)

**Outcome: B — no safe real-VRF implementation path exists in this repository at this
time.** Per the Phase 21G crypto rule (no invented/homemade VRF math, no renaming the
placeholder, only a safe dependency or existing primitive qualifies), **no consensus code
was changed.** `AssignmentProofV1` (Phase 21D) **remains an explicit VRF-style
placeholder**; this phase is **docs-only**. True cryptographic VRF remains **PENDING**.
This is not a completion claim.

Local-only; not pushed. Mainnet remains hard-off for all PoAW-X/Phase 21 gates;
LWMA-144 and the block PoW target are untouched.

## What a "true VRF" would require here

`AssignmentProofV1` is a deterministic, domain-separated, public-key-bound, hash-based
digest — **recomputable by everyone**, therefore NOT a VRF (a VRF output must be
unpredictable to everyone except the secret-key holder, yet publicly verifiable). A real
`AssignmentProofV2` needs an audited VRF construction with: keygen, `prove(sk, msg) ->
(output, proof)`, `verify(pk, msg, output, proof)`, deterministic verification, stable
serialization, no private-key leakage, no network/runtime dependency, an acceptable
license, and ideally **compatibility with the existing secp256k1 key model**.

## Inspection (evidence)

- **Crypto dependencies (node `Cargo.toml`):** `k256 = 0.13` (secp256k1, features
  `ecdsa`, `ecdh`), `sha2`, `hmac`, `ripemd`, `rand_core`, `bip39`, `aes-gcm`, `pbkdf2`,
  `scrypt`, `subtle`, `zeroize`. **No VRF crate.** Pool mirrors the same minimal set
  (`k256` ecdsa only).
- **Key model:** secp256k1 throughout — wallet/miner identity is a secp256k1 keypair;
  `solver_pkh = HASH160(compressed secp256k1 pubkey)`; the ticket's
  `assignment_public_key[33]` is a compressed secp256k1 point.
- **`Cargo.lock` (node + pool):** grep for `vrf`, `ecvrf`, `schnorrkel`, `curve25519`,
  `ed25519`, `x25519`, `ristretto`, `merlin`, `openssl`, `p256`, `frost`, `bulletproofs`
  → **none present**. Only `k256` + `elliptic-curve` for EC crypto.
- **Cargo registry cache (`~/.cargo/registry`):** **none** of the above VRF/curve crates
  are cached, so adding one would require a network fetch with no guarantee of a clean,
  reproducible offline build.
- **`k256` 0.13:** exposes field/scalar/point arithmetic but **has no VRF feature** and
  no ECVRF API.

## Options evaluated, and why none is safe *here* now

1. **Existing repo primitive / crate already in the tree** — none. `k256` provides no
   VRF. **Rejected: nothing to use.**

2. **`schnorrkel` (sr25519 VRF, audited, used by Polkadot)** — the strongest audited
   pure-Rust VRF. But it uses a **Ristretto25519 key model**, *incompatible* with the
   project's secp256k1 identity; it pulls a heavy `curve25519-dalek` + `merlin`
   dependency tree that is **not cached** (clean offline build not guaranteed); and
   adopting it is a large consensus-affecting change that would need an explicit
   key-model decision + dependency/security review beyond this phase's scope. **Rejected
   here: incompatible key model + uncached heavy dep + needs review.**

3. **`vrf` (witnet, ECVRF over secp256k1)** — key-model-compatible, but it binds to
   **OpenSSL** (`libssl`), which this workspace **deliberately avoids**: the tree is
   rustls-only and dependencies are explicitly pinned to keep OpenSSL/webpki RustSec
   advisories out (see the `tokio-tungstenite`/`rustls` notes in `Cargo.toml`). Adding
   OpenSSL contradicts the repo's stated security posture and is a build/portability
   risk. **Rejected: OpenSSL system dependency conflicts with the rustls-only posture.**

4. **Hand-roll RFC 9381 ECVRF-SECP256K1-SHA256-TAI on `k256`** — technically possible
   with `k256` point/scalar/hash-to-curve, and key-model-compatible. But a by-hand ECVRF
   implementation is, by definition, **unaudited homemade VRF math** — which the Phase
   21G crypto rule **explicitly forbids** ("Do not invent custom VRF cryptography… Do not
   implement unaudited homemade VRF math"). **Rejected: forbidden by the crypto rule.**

## Conclusion

There is **no safe path** to a true VRF in this repo right now: nothing suitable is in
the dependency tree or cache; the only audited option changes the key model and is
heavy/uncached; the secp256k1 ECVRF crate needs OpenSSL (against the repo posture); and
hand-rolling ECVRF is forbidden. Per the rule, we **stop and report** rather than ship
unsafe or incompatible crypto. `AssignmentProofV1` remains the gated placeholder; the
Phase 21D/21E/21F enforcement (candidate set, admission, puzzle work) is unaffected.

## What a future Outcome A would need (recommended path)

1. **Decide the VRF key model** (a project-level decision, not a code detail):
   - **Option A1 — adopt `schnorrkel` (sr25519 VRF):** introduce a *separate* per-miner
     VRF keypair (32-byte Ristretto public key), carried alongside the secp256k1 payout
     identity (e.g. a new fixed field, not the 33-byte secp256k1
     `assignment_public_key`). Pros: audited, well-known, deterministic verify. Cons: new
     key type for the wallet to manage; heavy dep tree; build must be vendored/cached.
   - **Option A2 — audited secp256k1 ECVRF crate (RFC 9381):** only if a reputable,
     pure-Rust, no-OpenSSL, secp256k1 ECVRF crate becomes available and reviewable.
     Keeps the existing key model. Currently no such crate is in-tree/cached.
2. **Dependency/security review + vendoring:** confirm license, audit status, RustSec
   clean, and a reproducible offline build (vendor into the cache) before adding.
3. **Then implement `AssignmentProofV2`** binding {domain, network_id, target_height,
   role_id, solver_pkh, ticket_digest, parent-seed, VRF public key, VRF output, VRF
   proof}; derive the assignment score from the VRF output; gate behind
   `IRIUM_POAWX_TRUE_VRF_ACTIVATION_HEIGHT` + `IRIUM_POAWX_TRUE_VRF_REQUIRED=1` (mainnet
   hard-off); keep V1 accepted when the gate is off; bind V2 into the candidate/admission
   digests/roots; wallet emits **proof only, never the secret key**.

## Safety confirmations (unchanged this phase)

- **Mainnet hard-off:** no gate added/changed; all existing PoAW-X/Phase 21 gates remain
  `network_id == 0` hard-off. No consensus code changed.
- **Private-key safety:** nothing changed; `AssignmentProofV1` and all wallet helpers
  remain no-private-key/no-seed-phrase.
- **Pool is one interface, not the owner:** unchanged; the node remains authoritative.
- **Chain difficulty:** LWMA-144 and block PoW target untouched.

## Remaining technical steps (still open)

- **True cryptographic VRF (this gap):** pending a key-model decision + a safe,
  reviewable, cleanly-building VRF dependency (see Outcome-A path above).
- Full finality-committee integration (Phase 21F `FinalityWorkPlaceholder` is a
  placeholder).
- Provably-complete public-network candidate admission (Phase 21E validates
  best-among-admitted, not best-among-unseen).
- **Excluded (not in this track):** public testnet with outside miners, independent
  security audit, community vote, mainnet activation.
