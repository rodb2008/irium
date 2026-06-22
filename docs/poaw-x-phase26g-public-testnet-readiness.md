# PoAW-X Phase 26G — Public Testnet Readiness

**Docs-first readiness package. This document does NOT launch a public testnet** — it defines scope,
prerequisites, success/abort criteria, metrics, assumptions, and the communication plan for a future,
separately-approved public testnet. **Production-ready: NO. Mainnet-ready: NO. Audited: NO.**

Companion docs: `poaw-x-phase26g-public-testnet-rollout-checklist.md`,
`poaw-x-phase26g-public-testnet-risk-register.md`,
`poaw-x-phase26g-public-testnet-operator-runbook.md`.

## Baseline

- Repo: `https://github.com/iriumlabs/irium.git`
- Branch: `testnet/poawx-phase20-blueprint-completion-local`
- Commit baseline: **`c15c436`** (`origin/main` unchanged at `19c496dc5f2fa08981a109b10eeb257105c28c43`).
- Audit package: `docs/audit/poawx-phase26-*` (Phase 26F).

## Executive summary

PoAW-X is a multi-role proof-of-aligned-work overlay enforced by gated sections inside `connect_block`
(phase21c dominance, phase21d/21e candidate set/admission, phase21f puzzle, phase21h finality,
phase22a committed admission, phase22d true-VRF). It is **hard-off on mainnet** (`network_id == 0`).
Phase 26 made a multi-block all-gates chain satisfiable and fixed cold-resync end to end:

- 26B — epoch-seed alignment (multi-block satisfiable; phase21d preserved, phase22a unchanged).
- 26C — live three-system 6-block soak (same final height/tip/irx1 across Windows + VPS-1 + VPS-2).
- 26D — admission-cache persistence (restart / keep-storage cold replay).
- 26E — historical-admission serving (fresh-wipe node syncs from scratch).
- 26F — independent-audit prep package.

A public testnet is the natural next validation: it exercises the system with **independent
operators, untrusted peers, real network conditions, scale, and adversarial behavior** — none of
which the controlled three-node devnet runs cover.

## What is now validated (controlled devnet + repo-local tests)

- Multi-block all-gates chains accepted by the real `connect_block` pipeline; 6+ sequential blocks.
- Repo-local suite green serialized: 26B **744/0**, 26D **747/0**, 26E **748/0**; release builds pass.
- Live three-system propagation: 6 all-gates blocks, same height/tip/irx1, incl. a spoke-originated
  block.
- Restart / keep-storage cold replay reaches the tip from disk (reloaded, re-validated admissions).
- Fresh-wipe node syncs a 6-block chain from scratch via served, re-validated historical admissions.
- Mainnet/prod processes and storage untouched throughout; all gate equality logic unchanged.

## What is still unvalidated (public-testnet targets)

- **Independent audit** of the Phase 26 changes (package exists; audit not performed).
- **Untrusted multi-operator network:** independent node operators, unknown peers, churn, NAT/firewall
  diversity, clock skew, and Sybil/spam pressure.
- **Scale:** more than three nodes; many miners; deeper chains (beyond a handful of blocks and beyond
  one admission window per getblocks batch).
- **Adversarial behavior:** malformed/forged/replayed admissions at volume, eclipse/partition
  attempts, getblocks/headers flooding, withholding, and reorg pressure.
- **Performance / DoS** under sustained load (admission serving, gossip fan-out, sync bursts).
- **phase21e propagation sensitivity** at scale ("admitted to THIS node in the window" — see
  Limitations).
- **Governance** of activation parameters and any future mainnet path.

## Why a public testnet is the next step

Controlled devnet proves the mechanism; only a public testnet exposes the assumptions that matter for
a real network: untrusted peers, propagation under churn, admission availability at scale, and
adversarial resistance — feeding both the audit and any future governance decision. It remains
**non-mainnet**; mainnet stays hard-off.

## Proposed public-testnet scope (for later approval)

- Network: a dedicated **testnet** (`network_id == 1`), explicitly NOT mainnet. PoAW-X active via the
  documented activation env (see rollout checklist).
- Participants: a small set of named operators first, then opened to external testers/miners.
- Mining: **Irium-native** `poawx-live-proof-harness` / the node's native path only — **no stock
  cpuminer/minerd**.
- Interfaces: **RPC and status loopback-only on every node**; cross-host **P2P only on
  source-restricted ports**; no public RPC/stratum; no `0.0.0.0/0`; no UDP.
- Storage: isolated `IRIUM_DATA_DIR`/`IRIUM_BLOCKS_DIR`/`IRIUM_STATE_DIR` per node; never `/tmp`,
  never a default `.irium`.
- Explicitly out of scope: mainnet enablement, real-value rewards, governance changes, any change to
  PoW/LWMA/difficulty/target/reward or to the phase21d/21e/22a equality logic.

## Mainnet stays hard-off

The public testnet does not touch mainnet. PoAW-X gates do not engage for `network_id == 0`; no
mainnet activation height, default behavior, or storage is changed. Existing mainnet nodes and the
production pool are protected and must remain untouched (see runbook "what not to do").

## Prerequisites before launch (gates)

1. Independent audit of the Phase 26 changes completed or formally in progress with a sign-off plan.
2. A frozen, tagged-internally (NOT a public release/tag per current rules — an agreed commit hash)
   baseline build reproducible by all operators.
3. Operator runbook + rollout checklist reviewed; each operator confirms isolated storage, loopback
   RPC, and source-restricted P2P.
4. Firewall plan agreed per operator (source-restricted P2P only); a dynamic-IP handling plan for any
   home-ISP participant.
5. Observability plan: agreed metrics + log fields to collect (no secrets).
6. Abort/rollback criteria and an incident contact path agreed.
7. Explicit written approval to launch (not granted by this document).

## Success criteria (public testnet)

- N independent nodes converge to the same height/tip/irx1 and stay converged across the test window.
- ≥ K sequential all-gates blocks accepted and propagated; blocks originate from ≥ 2 operators.
- A node can restart (keep-storage) and a fresh node can sync from scratch and converge.
- phase21d/21e/22a enforced throughout; no block accepted without a matching, validated admission.
- No mainnet/prod impact; no secret leakage; clean teardown.

## Abort / rollback criteria

- Any divergence in consensus validity (a node accepts a block another rejects on the same inputs).
- Evidence of phase21e bypass, admission forgery/replay/cross-network reuse, or block acceptance
  without a matching admission.
- A DoS/resource-exhaustion condition that destabilizes honest nodes.
- Any mainnet/prod impact, secret exposure, or storage/firewall rule outside the agreed scope.
- Rollback: stop testnet nodes by exact PIDs, preserve evidence, restore isolated dirs to a clean
  state, leave mainnet untouched, and file an incident note. No code rollback needed (testnet only).

## Metrics to collect (no secrets)

- Per node: height, tip hash, irx1 root, peer count, headers/blocks inflight, sync-stall events,
  getblocks served/received counts, admissions ingested/rejected (by reason), re-broadcast counts.
- Propagation latency per block (mine → all-converged), and per restart/fresh-sync convergence time.
- Rejection reasons (phase21d/21e/22a/etc.) and their frequency.
- Resource use (CPU/mem/disk for the admission snapshot) under load.
- Firewall/connectivity events (source-restricted only).

## Security assumptions

- Delivery-only peer trust: a node re-validates every admission (`ingest_bytes`) and every block
  (`connect_block`) — peers are trusted to deliver, never to assert validity.
- phase21e equality is the gate; persistence/serving only change admission *availability*, not
  validity (Phase 26 invariants V1/V2).
- Cross-host P2P is source-restricted; RPC/status are loopback-only.
- Mainnet is hard-off and isolated from the testnet.

## Known limitations

- **phase21e is propagation-sensitive** ("best among candidates admitted to THIS node in the
  window") — a pre-existing devnet/testnet honest limitation; public-testnet data should quantify it.
- **Admission window = 64**; deep syncs rely on per-getblocks-batch serving — untested beyond small
  chains; deep-sync behavior is a public-testnet target.
- **Multi-block-from-scratch getblocks** can briefly stall before the handshake-push delivers
  blocks+admissions (observed ~30–45 s in 26E); acceptable on devnet, to be characterized at scale.
- **Dynamic home-ISP IPs** require firewall re-scoping on change (no broad rules).
- Not audited; not production/mainnet-ready.

## Communication plan for testers

- Publish: the baseline commit hash, the operator runbook + rollout checklist, the activation env, the
  isolated-storage and loopback-RPC requirements, and the explicit "testnet only, no value, may reset"
  notice.
- Provide: a known-good config template (with placeholders, no secrets), the success/abort criteria,
  and the metrics/log fields to report.
- Forbid: stock cpuminer/minerd, public RPC/stratum exposure, broad firewall rules, default storage,
  and any mainnet interaction.
- Channel: a single coordination thread/issue for status, plus an incident path for abort conditions.
- Claims policy: testers and coordinators must NOT claim production-ready/mainnet-ready/audited.
