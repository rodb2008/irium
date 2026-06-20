# PoAW-X known limitations & non-goals

State plainly, up front:

- **NOT mainnet-ready.** Every PoAW-X gate is default-off and `network_id == 0` hard-off.
- **NOT independently audited yet.** The only review to date is the internal Claude Code review
  (Phase 23A, `docs/poaw-x-phase23a-true-vrf-internal-security-review.md`). This package exists
  to enable an external audit.
- **Public testnet not yet done.** No live multi-party PoAW-X network has been run.
- **Governance / community vote not yet done.** No on-chain or community decision has been made.
- **External security review is REQUIRED** before any public testnet, non-test network, or
  mainnet activation.

## Specific limitations

- **`vrf_fun` / `secp256kfun` are pre-1.0 (0.12.x) and not formally audited.** Their correctness
  (incl. the vendored k256 field arithmetic) is a primary audit target; pin + vendor before
  mainnet.
- **Candidate admission is propagation-sensitive.** Enforcement proves "best among candidates
  ADMITTED to THIS node within the window", NOT "best among all unseen/offline/never-gossiped
  miners". Public-network admission windowing/tuning requires a testnet review.
- **Finality committee + gossip** public-network behavior (propagation, threshold economics,
  liveness under churn) requires testnet review.
- **Economic parameters** (55/22/13/10 split, 2% fee cap, thresholds) may require governance
  review; they are not claimed to be economically final.
- **Puzzle work modes are ASSIGNED work, not chain PoW.** They do not touch chain
  difficulty/LWMA-144; they are not a replacement for the chain's proof-of-work.
- **Role precommit/reveal gossip** has the in-memory + reserved-P2P plumbing; full cross-process
  live E2E is testnet work.

## Non-goals (explicitly out of this package)

- Mainnet activation / height selection.
- Governance / community vote mechanics.
- Exchange listing, liquidity, market structure.
- Non-PoAW-X mainnet services.
- Public testnet operations and external miner operations (unless requested later).
- A claim that the internal review substitutes for an external audit (it does not).

## Phase 24F update (genesis assignment harness fix)

Phase 24F found + fixed the exact live-block-production blocker: poawx_get_assignment returned
404 at tip_height==0, so a fresh devnet could not get an assignment for block 1 (the 14-F
genesis /poawx/assignment wall). Fix: serve the assignment at the genesis tip on
devnet/testnet only (mainnet + inactive still 503; connect_block/LWMA/difficulty untouched).
Live all-gates block production is now unblocked at the assignment layer (assembly + submit +
connect_block validate path is complete in code); a real cpuminer-mined all-gates block is
still not demonstrated, and cross-host P2P remains firewall-blocked. NOT production-ready, NOT
mainnet-ready. See docs/poaw-x-phase24f-pool-cpuminer-all-gates-harness.md.
