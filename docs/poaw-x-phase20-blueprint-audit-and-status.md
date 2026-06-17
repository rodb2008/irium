# PoAW-X Phase 20 — Blueprint Completion Audit & Status

**Version:** 1.0 (Phase 20 — local blueprint completion)
**Branch:** `testnet/poawx-phase20-blueprint-completion-local` (from `4172e1f`) — **local-only, not pushed.**
**Scope:** Honest audit of the remaining PoAW-X blueprint items against what the repository
actually defines, and completion of everything that can be implemented **without inventing
consensus-critical parameters**. Items whose consensus parameters are **undefined in the
repo** are marked **BLOCKED** with a precise design-gap doc — per the standing rule "do not
fake completion."

> Chain difficulty is automatic via **LWMA-144**; `STRATUM_DEFAULT_DIFF` is the stratum
> **share** difficulty only. Mainnet PoAW-X mode-1 remains **hard-disabled** (see §K).

---

## 1. What the repo actually defines today (ground truth)

- **Reward split (consensus):** `validate_poawx_reward_split_from_block` (chain.rs) enforces
  `worker_due = block_reward × POAWX_WORKER_REWARD_PERMILLE / 1000` = **10% per receipt**,
  paid to each receipt's `worker_pkh`. There is **no multi-role (e.g. 55/22/13/10) split**
  anywhere in the repo. (The 55/22/13/10 figure exists only in external "blueprint memory,"
  not in versioned consensus.)
- **Lanes:** the assignment carries a `lane`; the implemented path is **`lane="cpu"` only**
  (`EXPECTED_LANE_FIRST=b'c'`; `gpu` is explicitly rejected in tests). There is **no GPU or
  ASIC lane**, and **no commit-reveal / hidden-assignment scheme** in code.
- **Delegation registry (pool):** `JsonDelegationStore` keys by `(miner_pkh, worker)` and
  already supports **many** workers/miners; `all_active(tip)` filters active + unexpired;
  stores **no private keys**. (Phase 20 adds a multi-worker registry test — see §D.)
- **Mainnet gating:** `connect_block` **hard-rejects** mode-1 on mainnet regardless of env;
  `poawx_delegation_active(height)` returns false on mainnet; a regression test
  (`mode-1 on mainnet must hard-reject`) exists. Testnet/devnet gate on
  `IRIUM_POAWX_DELEGATION_ACTIVATION_HEIGHT`.
- **Fee:** the 226-byte `Delegation` carries `fee_bps`, currently **must be 0** (fails closed
  at wallet, pool, consensus). No fee-output consensus path exists.
- **Stratum block production:** **single-miner** — the coinbase pays one address (the
  connected miner). Multi-miner concurrent block production (per-worker coinbase outputs) is
  **not** implemented.

## 2. Blueprint audit table

| # | Blueprint requirement | Current status | Missing (code / tests / docs) | Risk | Phase 20 action |
|---|---|---|---|---|---|
| B | CPU/GPU/ASIC fairness matrix | Owner spec supplied; lane assignment + role-claim primitives + 34/33/33 distribution + activation gate implemented (mainnet-off) | Live hidden-precommit enforcement needs a future on-chain commitment root; connect_block wiring | MED | **PARTIAL** — primitives/validation/tests COMPLETE; hidden-precommit + wiring follow-up (design-gap doc, now partial-resolved) |
| C | Multi-role reward split | Owner spec supplied (55/22/13/10); consensus primitives + validator implemented (gated, mainnet-off) | Pool production + node receipt-wire/persist threading + live connect_block enforcement | MED | **PARTIAL** — primitives/validator/tests COMPLETE; production follow-up (see design-gap doc, now resolved) |
| D | Many-miner / multi-worker pool | Registry: multi-worker ✅; block production: single-miner | Per-worker coinbase outputs in stratum (depends on reward model) | MED | **PARTIAL** — registry test added; production gap documented |
| E | Third-party pool fee | Owner spec supplied (cap 200bps); fee primitives + fee-aware canonical validator + gates implemented (mainnet-off); delegation already binds fee terms | Wallet CLI flags + pool registry relax + live connect_block enforcement | MED | **PARTIAL** — primitives/validator/tests COMPLETE; wallet/pool/production follow-up (see design-gap doc, now resolved) |
| F | Long soak / restart / reorg testing | Proven ad-hoc in 18C/18D/19C/19D | A reusable loopback harness + documented full-soak commands | LOW | **COMPLETE** — harness + doc |
| G | Metrics / monitoring | `/metrics` exists (aggregate, loopback) | PoAW-X-specific counters + safe-monitoring doc | LOW | **PARTIAL** — monitoring doc + counter plan (code deferred, non-consensus) |
| H | Public miner onboarding at scale | 19A–19D docs + emit-only validator | Consolidated onboarding package + helper scripts | LOW | **COMPLETE** — doc + scripts |
| I | Broad public testnet readiness | Scattered notes | Readiness package (capacity, abuse, rollback, activation policy) | LOW | **COMPLETE** — doc |
| J | Governance / community activation | None | Activation process doc | LOW | **COMPLETE** — doc |
| K | Mainnet activation safety framework | Gating implemented + tested | Far-future-height policy, operator/rollback checklists, default-off confirmation | MED | **COMPLETE** — framework doc (no activation) |

## 3. Consensus blockers — NONE remaining (all three resolved to PARTIAL)

> **Update:** all three previously-blocked consensus items (B fairness matrix, C multi-role
> reward split, E third-party fee) now have owner-supplied specs and are implemented to
> **PARTIAL** — consensus primitives + validators + activation gates (mainnet-off) + tests are
> done (testnet/devnet-gated); each has remaining production-wiring follow-up. **No item is
> BLOCKED on an undefined consensus parameter anymore.**

- ~~B — CPU/GPU/ASIC fairness matrix~~ — **PARTIAL** (assignment + role-claim primitives + 34/33/33 distribution + gate done; live **hidden-precommit** needs a future on-chain commitment root).
- ~~C — Multi-role reward split~~ — **PARTIAL** (primitives/validator/tests done; pool production + receipt-wire threading follow-up).
- ~~E — Third-party pool fee~~ — **PARTIAL** (fee primitives + fee-aware canonical validator + gates done, cap 2%, mainnet-off; wallet CLI + pool registry + live enforcement follow-up).

**Remaining work is implementation/integration, not blocked parameters:** (1) a fairness
**commitment-root** design decision to make hidden-precommit consensus-enforceable; (2)
**production wiring** for each — pool building canonical (multi-role/fee) coinbases, node
receipt-wire/persist threading, and live `connect_block`/`submit_block_extended` enforcement
gated on the activation heights; (3) wallet CLI fee flags. Each follows the
proven mode-1 pattern.

## 4. What Phase 20 delivers (safe, local)

- Multi-worker delegation **registry test** (Part D registry layer).
- Master audit (this doc) + three **design-gap docs** (B, C, E).
- **Multi-worker pool** behavior doc (registry vs block-production gap).
- **Soak/reorg harness** script + doc (loopback-only, exact pidfiles, documented full runs).
- **Metrics/monitoring** doc (existing surface + safe PoAW-X counter plan).
- **Public testnet readiness**, **miner onboarding at scale**, **governance activation**,
  and **mainnet activation safety framework** docs.
- Helper scripts: pool-identity package printer, pilot-readiness checker, log PASS/FAIL
  scanner, firewall-command template printer (never executes).

Everything is local on the VPS; nothing pushed; no mainnet activation; chain difficulty stays
LWMA-144 automatic.
