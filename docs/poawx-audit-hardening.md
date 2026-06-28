# PoAW-X Pre-Mainnet Audit Hardening

All fixes below are gated behind `IRIUM_POAWX_AUDIT_HARDENING_ACTIVATION_HEIGHT`. The gate is
hard-off on mainnet (`network_id == 0`) and the code is byte-identical to pre-audit behaviour
when off; the gate activates from block 1 on a freshly-wiped devnet/testnet.

## Fixes
1. Deterministic receipts root: the audit root sorts receipts by their full inner hash (total
   order), eliminating `sort_unstable_by`'s undefined order on equal sort keys (chain-split risk
   with adversarial multi-receipt blocks). Byte-identical for single-receipt (honest) blocks.
2. Finality parent_hash binding: every vote in a proof must share the proof's parent lineage.
3. Finality equivocation detection: a member casting a Commit vote for a different block at the
   same height is rejected by the gossip cache (cannot assemble two conflicting proofs).
4. VRF identity binding: `AssignmentProofV2::validate` now enforces
   `solver_pkh == hash160(assignment_public_key)`, so solver_pkh cannot be ground for priority.
5. Receipt signature coverage: `worker_pubkey`/`worker_sig` are bound into the receipts root and
   canonical low-S ECDSA is required (no block-hash-stable wire variants).
6. Inbound sibling delivery: the inbound block-gossip path refreshes the peer tip so an
   equal-height competing fork is resolved at shallow depth (completes the v0.1.9 fix).
7. Ticket epoch binding: a ticket proof's `epoch` must equal its `target_height`.
8. Role distinctness -- see below (documented protocol property, no validity rule).
9. Proposer-key-mismatch diagnostic: the ineligibility rejection now lists the eligible proposer
   pkhs so an operator can see when a miner signs with an unregistered key.
10. Lane validation: the receipt `lane` (in the consensus hash) must be the canonical value.
11. Strict leaf decoding: leaf structs reject trailing bytes (canonical-encoding hardening).
12. GetHeaders responder per-IP throttle (anti-CPU-DoS under the chain lock).
13. Unsolicited competing-tip headers trigger a gated locator fetch instead of being dropped.
14. submit() pool insertion re-validates announce candidates before inclusion.

## #8 Role distinctness -- documented protocol design (Option A)

PoAW-X produces ONE block per VRF-selected proposer; that proposer fills all three roles
(COMPUTE / VERIFY / SUPPORT) of its OWN block, and the role-reward solver pkhs are the same
identity. This is intentional: blocks are not collaboratively assembled from three separate
nodes, so a per-block "three distinct role solvers" validity rule would reject every
honestly-produced block and halt the chain. Role/identity separation is provided where it is
security-relevant instead:

- Finality (Fix #2 of the v0.1.8 hardening / `block_finality_has_genuine_quorum`) requires a
  committee of DISTINCT, on-chain-REGISTERED keys meeting a 2/3 threshold, so finalization
  cannot be self-certified by one identity.
- Fairness across producers comes from VRF proposer rotation over time, not within a block.

Therefore role distinctness is NOT enforced as a per-block consensus rule. This is explicit and
intentional for mainnet.
