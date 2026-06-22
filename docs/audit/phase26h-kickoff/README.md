# PoAW-X Phase 26 — Independent Audit Kickoff

This folder is the **kickoff package** for an independent security review of the PoAW-X Phase 26
changes. It is a starting point for an auditor; it does not itself assert any result.

**NOT audited. NOT production-ready. NOT mainnet-ready.** Mainnet remains hard-off
(`network_id == 0`). No live testnet has been launched.

## Purpose

Enable an independent reviewer to assess the consensus, P2P, and storage changes that made
multi-block PoAW-X chains satisfiable and fixed cold-resync, and to record findings against this
codebase — **without weakening any gate** as a premise to verify.

## Baseline

- Repo: `https://github.com/iriumlabs/irium.git`
- Branch: `testnet/poawx-phase20-blueprint-completion-local`
- Commit baseline (HEAD): **`972bb9c`** (docs). Last **source** change: **`0208368`**.
- `origin/main` unchanged at `19c496dc5f2fa08981a109b10eeb257105c28c43`.
- Full source audit range: **`30bce64..0208368`** (8 source files, +1006/−47; rest is tests + docs).

## Expected reviewer profile

A reviewer comfortable with: blockchain consensus validation, Rust, P2P gossip/sync protocols, and
basic applied cryptography (signatures, digests, VRF outputs as opaque values). Deep VRF-internals
expertise is not required — the audit treats VRF outputs as validated opaque digests; the focus is
on the *gating logic*, *admission availability*, and *DoS/abuse surface*.

## How to navigate the docs

1. Start here (`README.md`).
2. `AUDIT_SCOPE.md` — exact diff ranges + files to prioritize (and the phase22a "must be unchanged"
   check).
3. `AUDITOR_REVIEW_GUIDE.md` — recommended order, invariants, threat model, attacks, tests, limits.
4. `REPRO_COMMANDS.md` — non-live commands to check out, diff, test, and build.
5. `FINDINGS_TRACKER.md` — record findings here (severity/status table).
6. `AUDIT_DELIVERABLES.md` — what the audit should produce.
7. `AUDIT_KICKOFF_EMAIL_DRAFT.md` — a ready-to-send intro for the reviewer.

Delivery wrapper (how this package is handed to an external reviewer):
- `docs/audit/phase26j-external-handoff/` — `PACKAGE_MANIFEST.md`, `SEND_READY_SUMMARY.md`,
  `AUDITOR_OUTREACH_MESSAGE.md`, `AUDITOR_HANDOFF_CHECKLIST.md`, `EXTERNAL_FINDINGS_TRACKER_COPY.md`.

Background (already written; the auditor should read these too):
- `docs/audit/poawx-phase26-independent-audit-package.md` — full package (invariants, threat model,
  test/live matrices).
- `docs/audit/poawx-phase26-technical-appendix.md` — per-change analysis (A epoch-seed, B persistence,
  C serving, D P2P/DoS, E mainnet safety).
- `docs/audit/poawx-phase26-auditor-checklist.md` — 14 questions to answer.
- `docs/poaw-x-phase26g-public-testnet-readiness.md` — the (separately gated) public-testnet plan.

## In scope

- Epoch-seed reconciliation (phase21d/21e expected candidate-set seed; the `admission_epoch_seed`
  invariant) and confirmation that **phase22a is unchanged**.
- Candidate-admission validation, persistence (`candidate_admissions.dat`), and reload.
- Historical-admission serving during block sync and the receiver re-validation path.
- The bounded P2P send and its DoS/abuse surface.
- Confirmation that PoW/LWMA/difficulty/target/reward and mainnet behavior are unchanged.

## Out of scope

- Mainnet enablement, real-value rewards, governance, and any future mainnet path.
- The hidden-precommit / role-ticket-proof / mode-1 delegation paths (separately tested, unchanged
  here).
- A live public testnet (separate, approval-gated; see the readiness package).
- The devnet/test block **builders** (`poawx_mining_harness.rs`, `poawx-live-proof-harness.rs`) as
  consensus authorities — they are NOT validators; the node validates every block independently. They
  are in scope only to confirm they are mainnet-hard-off and not on the validation path.

## Claims policy

This package and any review based on it must not claim production-ready, mainnet-ready, or audited.
A completed audit's sign-off (or non-sign-off) is the only basis for an "audited" statement.
