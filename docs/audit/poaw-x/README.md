# Irium PoAW-X — external audit package

**Project:** Irium PoAW-X (Proof-of-Allocated-Work, extended) — consensus/network-level
mechanism for the Irium chain (`irium-node-rs` + `pool/irium-stratum`).

**Branch under review:** `testnet/poawx-phase20-blueprint-completion-local`
**HEAD:** `4a3c596` (Phase 23A) — see `git log` for the Phase 20→23A history.

**Status (read first):**
- **Local-only.** This branch is NOT pushed; the remote branch is absent; `main` is untouched.
- **Mainnet hard-off.** Every PoAW-X gate is disabled when `network_id == 0` (mainnet/unset)
  and default-off otherwise.
- **NOT mainnet-ready. NOT independently audited.** The only review so far is an *internal*
  Claude Code review (Phase 23A) — this package exists to enable a proper **external** audit.

## Purpose of this package

Give an external security auditor everything needed to review the PoAW-X local technical
implementation (Phase 20 through Phase 23A): scope, threat model, architecture, crypto and
consensus review targets, build/test reproduction, known limitations, internal-review
findings, and a checklist.

## What an auditor should review first

1. `AUDIT_SCOPE.md` — what is in/out of scope.
2. `THREAT_MODEL.md` — attackers, protections, and the files/tests for each.
3. `ARCHITECTURE_OVERVIEW.md` — how node/pool/wallet relate (the node is authoritative; the
   pool is only one miner interface and holds no VRF secret).
4. `CRYPTO_REVIEW_TARGETS.md` — `vrf_fun`/`secp256kfun` + `AssignmentProofV2` (the highest-
   value target; pre-1.0 dependency).
5. `CONSENSUS_REVIEW_TARGETS.md` — ext serialization, `connect_block` enforcement, roots.
6. `POOL_WALLET_NODE_REVIEW_TARGETS.md`, `KNOWN_LIMITATIONS_AND_NON_GOALS.md`,
   `FINDINGS_FROM_INTERNAL_REVIEW.md`, `AUDITOR_CHECKLIST.md`.

## Where important docs live

- This package: `docs/audit/poaw-x/`.
- Per-phase implementation docs: `docs/poaw-x-phase21c…21i`, `…22a`, `…22b/22c` (true-VRF
  decision/research), `…22d` (V2 primitive), `…22e` (E2E wiring), `…phase23a` (internal review).
- Final local completion audit: `docs/poaw-x-final-local-blueprint-completion-audit.md`.

## How to reproduce tests

See `BUILD_AND_TEST_GUIDE.md`. Quick start (from the repo root, on the branch above):

```
cargo fmt -- --check
cargo test --lib -- --test-threads=1
cargo test --bin irium-wallet
cargo test --bin iriumd -- --test-threads=1
( cd pool/irium-stratum && cargo test )
cargo tree | grep -Ei 'openssl|secp256k1-sys|bindgen|native-tls' || echo NONE
```

All PoAW-X mechanisms are gated; see `CONSENSUS_REVIEW_TARGETS.md` for the env-var gate names
and `THREAT_MODEL.md` for the mainnet-hard-off guarantee.

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
