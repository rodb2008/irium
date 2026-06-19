# PoAW-X Phase 22B — True VRF key-model & dependency decision package

**Status:** Decision package only — **no code, no dependency, no `Cargo.toml`/`Cargo.lock`
change, no VRF implementation.** Local-only; not pushed; remote branch absent.
**Mainnet hard-off; not mainnet-ready.** `AssignmentProofV1` remains a documented
**VRF-style placeholder** — **true VRF is PENDING** (true VRF pending) and PoAW-X is **not full
blueprint-complete** until this VRF decision is approved and implemented. **No homemade
VRF.** This document exists so the project can choose the key model + dependency *before*
any code is written.

## 1. Current key model

- **Curve / library:** secp256k1 via `k256 = 0.13` (features `ecdsa`, `ecdh`). No VRF
  crate anywhere in the tree (Phase 21G inspection).
- **Assignment public key:** a **compressed 33-byte secp256k1 point**
  (`TicketProof.assignment_public_key[33]`).
- **Payout / solver identity:** `solver_pkh = HASH160(compressed secp256k1 pubkey)`
  (`hash160` = SHA-256 then RIPEMD-160), the same identity used for coinbase payout.
- **Wallet signing model:** secp256k1 ECDSA `sign_prehash` / `verify_prehash` (e.g.
  `Delegation`, and the Phase 21H finality votes). Wallet helpers are emit-only and never
  output a private key or seed phrase.
- **Pool / node validation path:** the pool mirrors consensus wire byte-for-byte and
  produces proofs; the **node is authoritative** and re-validates everything. There is no
  OpenSSL in the dependency tree by deliberate policy (rustls-only; dependencies pinned to
  keep OpenSSL/webpki RustSec advisories out).

## 2. Why `AssignmentProofV1` is NOT a true VRF

`AssignmentProofV1` (Phase 21D) is a deterministic, domain-separated, public-key-bound,
**hash-based** digest over `network_id ‖ target_height ‖ role_id ‖ solver_pkh ‖
assignment_public_key ‖ ticket_digest ‖ seed`, with `assignment_score = first 8 bytes`.

- It is **recomputable by anyone** who knows the (public) bound fields — so it is **not
  unpredictable-before-reveal**, which is the defining property of a VRF output.
- It requires **no secret key** to produce; it cannot bind unpredictability to a keypair.
- It is genuinely useful as a **testnet/devnet deterministic assignment binding** (and is
  what candidate sets / admissions / puzzle challenges bind to today), but it is **not a
  cryptographic VRF** and is labeled as a placeholder throughout.

## 3. Required properties for a true VRF (`AssignmentProofV2`)

1. **Public verifiability** — `verify(pk, msg, output, proof) → bool`, deterministic, no
   network/wall-clock.
2. **Secret-key-generated proof** — only the holder of the VRF secret key can produce a
   valid `(output, proof)` for a message.
3. **Deterministic output** — same `(sk, msg)` ⇒ same output (no per-call randomness in
   the output).
4. **Binding** — the proof/output must bind `network_id, target_height, role_id,
   ticket_digest, seed/prev_hash, miner` (the VRF message is the canonical concatenation).
5. **Unpredictable without the secret key** — output indistinguishable from random to
   anyone lacking `sk`.
6. **Stable consensus serialization** — fixed, versioned wire; mutation changes the
   verification result; bound into the ext digest/receipts-root.
7. **No OpenSSL** — must respect the project's rustls-only, no-OpenSSL posture (unless that
   policy is explicitly changed).
8. **No private-key leakage** — wallet emits proof/output only; secret key never echoed.
9. **No homemade crypto** — must be a reviewed/audited implementation of a standard VRF
   (e.g. RFC 9381 ECVRF, or an audited sr25519/Ristretto VRF); hand-rolling is forbidden.

## 4. Options

### Option A — keep the secp256k1 key model (audited ECVRF)
Use a safe, reviewed **secp256k1 ECVRF** (RFC 9381 ECVRF-SECP256K1-SHA256-TAI) crate.
- **Pros:** least wallet/identity disruption — the existing 33-byte assignment key + the
  secp256k1 wallet model are reused; one key model.
- **Cons / blocker:** **no safe, cached, no-OpenSSL, secp256k1 ECVRF crate is currently in
  the dependency tree or cargo cache** (Phase 21G). The best-known secp256k1 ECVRF crate
  (`vrf`, witnet) binds to **OpenSSL**, which violates the rustls-only policy. A
  pure-Rust, no-OpenSSL, reviewed secp256k1 ECVRF crate would have to be identified +
  vetted before this becomes viable.

### Option B — add a separate VRF key model (sr25519 / Ristretto)
Adopt an **audited** Ristretto/sr25519 VRF (e.g. `schnorrkel`, used by Polkadot).
- **Pros:** mature, audited, pure-Rust, deterministic VRF; cryptographically clean.
- **Cons:** introduces a **separate VRF keypair** (32-byte Ristretto public key) distinct
  from the secp256k1 payout identity → a new field in the ticket, wallet must **manage an
  extra key**, migration/UX complexity, and a heavier dependency surface
  (`curve25519-dalek`, `merlin`) that is **not currently cached** (offline-build risk).

### Option C — vendor + review a VRF implementation
Vendor a chosen VRF crate into the tree and **security-review** it before any consensus use.
- **Pros:** full control over the exact code; reproducible offline build.
- **Cons:** **heavy security review required**; not a quick implementation; must be audited
  before any mainnet consideration. Appropriate as the *gate* for Option A or B, not a
  shortcut.

### Option D — keep `AssignmentProofV1` for testnet/devnet only (status quo)
- **Pros:** unblocks all current local/testnet/devnet work; everything that binds to it
  (candidate sets, admissions, puzzle challenges, committed admission) keeps working.
- **Cons:** **not blueprint-complete** for true VRF; must remain clearly labeled
  **pending**; not acceptable as the final mainnet VRF.

## 5. Recommendation

1. **Do not write any VRF code until the key model + dependency are explicitly approved.**
   This is a cryptographic-foundation decision, not an implementation detail.
2. **Near-term (now):** keep **Option D** — `AssignmentProofV1` as the labeled placeholder
   for local/testnet/devnet only. **Do not block local non-mainnet testing on true VRF**,
   and **do not claim full blueprint completion**.
3. **Path to true VRF (choose one, then do Option C review before consensus use):**
   - **Preferred if a safe crate exists:** **Option A** (secp256k1 ECVRF, no OpenSSL,
     pure-Rust, reviewed) — minimal wallet disruption, single key model. *Blocker:*
     identify such a crate; none is currently in-tree/cached.
   - **Otherwise:** **Option B** (audited sr25519/Ristretto VRF) with explicit ticket +
     wallet support for a separate VRF key — cryptographically clean but more surface +
     UX.
   In **either** case: vendor + security-review (**Option C**) before enabling on any
   non-test network; gate behind `IRIUM_POAWX_TRUE_VRF_{ACTIVATION_HEIGHT,REQUIRED}`
   (reserved, mainnet hard-off); keep `AssignmentProofV1` accepted only when the V2 gate is
   off; bind V2 into the candidate/admission digests; wallet emits proof/output only.
4. **Decision owners must confirm:** (a) the key-model choice (A vs B), (b) the specific
   crate + version, (c) the rustls/no-OpenSSL policy stance, (d) the review/audit plan —
   **before** Phase 22C (implementation) is started.

## 6. Remaining blocker

**No safe true-VRF dependency is currently available in the tree/cache**, and the
key-model + dependency choice is **not yet approved**. Until that decision is made and the
crate is vendored + reviewed, true VRF stays **pending** and PoAW-X is **not full
blueprint-complete** (one gap remains). Everything else in the Phase 20/21/22A track is
implemented + gated + tested, mainnet hard-off.

## 7. Confirmations

- `AssignmentProofV1` **remains a placeholder** — unchanged by this phase.
- **No homemade VRF**, no VRF dependency added, no `Cargo.toml`/`Cargo.lock` change, no
  code change (docs-only).
- Mainnet hard-off; not mainnet-ready; no push; remote branch absent; no
  merge/PR/tag/release; mainnet/prod untouched; chain difficulty/LWMA-144 untouched.


## Phase 22C — secp256k1 true-VRF research (Option A viable)

Research found a VIABLE Option A path: `vrf_fun 0.12.1` (secp256kfun) is a pure-Rust,
no-OpenSSL **secp256k1** RFC 9381 ECVRF that scratch-built + ran outside the repo
(prove/verify/output deterministic; wrong-message rejected). This is a real **true VRF**
candidate for a future `AssignmentProofV2` — but `AssignmentProofV1` remains the
**placeholder** (no homemade VRF; **no dependency added to the repo**; **mainnet hard-off**;
not mainnet-ready). Implementation is deferred to Phase 22D pending explicit approval +
security review. Details: `docs/poaw-x-phase22c-secp256k1-vrf-research.md`.
