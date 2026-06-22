# Auditor Outreach Message (template)

Fill the `[...]` placeholders. **Do not send without explicit approval and confirmed recipient
details.** Contains no credentials, secrets, or machine-private data. Do not state or imply the system
is audited, production-ready, or mainnet-ready.

---

**To:** `[Contact Email]`
**Subject:** Independent security review — Irium PoAW-X (Phase 26), testnet/devnet scope

Hi `[Auditor Name]`,

I'm reaching out from Irium Labs to ask whether `[Auditor Company]` is available to perform an
independent security review of a bounded set of changes to our node software ("PoAW-X" — a multi-role
proof-of-aligned-work consensus overlay).

**Scope is testnet/devnet only.** PoAW-X is **hard-off on mainnet** (`network_id == 0`), and mainnet
is explicitly out of scope. A public testnet launch is **gated on this review** — we will not launch
publicly before an independent assessment.

**What changed (Phase 26):**
1. **Epoch-seed alignment** — corrected which seed the candidate-set gate (phase21d/21e) expects so
   multi-block chains are satisfiable, while leaving the committed-admission gate (phase22a) unchanged.
2. **Candidate-admission persistence** — a node persists already-validated admissions and reloads +
   re-validates them at startup (restart cold-resync).
3. **Historical-admission serving** — when serving block bodies during sync, a node also sends the
   matching admissions, re-validated by the receiver (fresh-node sync from scratch).

The premise we want independently verified is that **none of this weakens the validation gates** —
persistence/serving change admission *availability*, not validity.

**Key review areas:**
- Epoch-seed alignment correctness (`admission_epoch_seed`; grandparent seed; genesis boundary).
- Candidate-admission persistence (corruption/stale-state safety, re-validation on reload).
- Historical-admission serving (bounded send, receiver re-validation).
- phase21d / phase21e / phase22a invariants (especially: phase21e equality still required; phase22a
  unchanged; no block accepted without a matching validated admission).
- P2P risk surface: DoS, replay, and cache-poisoning.

**Baseline:**
- Repo: `https://github.com/iriumlabs/irium.git` (public)
- Branch: `testnet/poawx-phase20-blueprint-completion-local`
- HEAD `22dfde8` (docs); last source change `0208368`; full source range `30bce64..0208368` (8 files).
- `origin/main` unchanged.

We have prepared a kickoff package (scope, review guide, reproduction commands, deliverables, findings
tracker) and an internal self-review (not an audit) to speed you up; we'll share the exact paths on
confirmation.

**Could you let us know:**
- Your **availability** and earliest start.
- **Scope confirmation** (is the above the right boundary, or would you adjust it?).
- **Estimated timeline** to a report.
- Expected **deliverables** (we've drafted a deliverables list and can align to your format).
- A **cost estimate**.
- Your **preferred secure handoff method** for any non-public materials (the repo itself is public; we
  do not intend to share secrets, keys, or private logs).

Proposed timeline on our side: `[Timeline]`. Scope/budget notes: `[Budget/Scope Notes]`.

Thanks very much,
`[Your Name / Irium Labs]`

---

> Reviewer note: we are **not** claiming the system is audited, production-ready, or mainnet-ready —
> obtaining your independent assessment is the purpose of this engagement.
