# Audit Kickoff — Email / Message Draft

A ready-to-send intro for an independent reviewer. Fill the `[...]` placeholders. Contains **no
credentials, secrets, or machine-private data**. Do not state or imply that the system is audited,
production-ready, or mainnet-ready.

---

**Subject:** Independent security review — PoAW-X (Phase 26) consensus + P2P + storage changes

Hi [Auditor name],

We'd like to engage you for an independent security review of a bounded set of changes to the Irium
node ("PoAW-X" — a multi-role proof-of-aligned-work overlay). The changes are **devnet/testnet only**;
PoAW-X is **hard-off on mainnet** and mainnet is not in scope.

**What PoAW-X is (short):** an overlay validated by gated sections inside the node's `connect_block`
pipeline (dominance, candidate-set/admission, puzzle, finality, committed-admission, true-VRF). It is
enforced only on non-mainnet networks (`network_id != 0`).

**What needs review (Phase 26):**
1. *Epoch-seed reconciliation* — made multi-block chains satisfiable by correcting which seed the
   candidate-set gate (phase21d/21e) expects, while leaving the committed-admission gate (phase22a)
   unchanged.
2. *Admission-cache persistence* — a node persists already-validated candidate admissions and reloads
   + re-validates them at startup, so it can re-validate persisted blocks after a restart.
3. *Historical-admission serving* — when serving block bodies during sync, a node also sends the
   matching admissions (re-validated by the receiver), so a fresh node can sync from scratch.

The core premise to verify: **none of this weakens the validation gates** — persistence/serving change
admission *availability*, not validity; phase21e equality still gates block connection.

**Baseline:**
- Repo: `https://github.com/iriumlabs/irium.git` (public)
- Branch: `testnet/poawx-phase20-blueprint-completion-local`
- HEAD: `972bb9c` (docs); last source change `0208368`.
- Full source diff range: `30bce64..0208368` (8 source files; the rest is tests + docs).
- `origin/main` unchanged.

**Where to start (in the repo):**
- `docs/audit/phase26h-kickoff/README.md` → `AUDIT_SCOPE.md` → `AUDITOR_REVIEW_GUIDE.md` →
  `REPRO_COMMANDS.md`.
- Deeper background: `docs/audit/poawx-phase26-independent-audit-package.md`,
  `...-technical-appendix.md`, `...-auditor-checklist.md`.
- Record findings in `docs/audit/phase26h-kickoff/FINDINGS_TRACKER.md`.

**Requested deliverables** (see `AUDIT_DELIVERABLES.md`):
- A summary report, findings with severity + exploitability notes, recommended fixes, retest
  requirements, and a final sign-off / non-sign-off statement with stated assumptions/limitations.

**Specific questions we want answered** (full list in `auditor-checklist.md`):
- Does the epoch-seed change preserve BOTH phase21d and phase22a (phase22a unchanged)?
- Can admissions be forged, replayed, or cross-network reused?
- Is admission persistence safe against corruption/stale state?
- Can historical-admission serving be abused for DoS?
- Can a node be tricked into accepting a block without a matching, validated admission?
- Does mainnet remain unaffected?

**Timeline:** [proposed start date] – [proposed report date]; we're flexible.

**Scope boundaries:** mainnet, governance, real-value rewards, and a live public testnet are out of
scope. We are not claiming the system is audited, production-ready, or mainnet-ready — that is the
purpose of this engagement.

**Logistics / contact:** [contact name + channel]. The repo is public; no credentials are required.
We will not send (and please do not request) any secrets, private keys, wallet data, or
machine-private logs.

Thanks,
[Your name / org]

---
