# PoAW-X architecture overview

## Trust boundaries (read this first)

- **PoAW-X is consensus/network-level**, not a pool feature. The rules live in the node.
- **The node is the authoritative validator.** Every gated section is re-validated in
  `connect_block` (`src/chain.rs`); validation is fail-closed.
- **The pool (`pool/irium-stratum`) is only ONE miner interface.** It mirrors consensus wire
  byte-for-byte (parity tests) and bundles miner-supplied data. It does **not** own validation
  and **holds no VRF secret**.
- **The wallet signs / produces miner proofs.** It is emit-only for PoAW-X helpers; secrets are
  inputs and are never echoed.
- **Mainnet hard-off.** `network_id == 0` disables every gate.

Because `irium-node-rs` is only a dev-dependency of the pool crate, the pool MIRRORS the
consensus types; parity tests assert byte-for-byte equality against the canonical node types.

## Reward split

Coinbase reward splits PRIMARY 55% / COMPUTE 22% / VERIFY 13% / SUPPORT 10% (bps 5500/2200/
1300/1000, summing to 10000; integer division remainder → PRIMARY). Official path fee = 0%;
third-party pool mode allows a fee capped at 2.00% (200 bps), else fail-closed to 0%.

## Receipt / commitment flow

A production block carries `poawx_receipts`, each with an optional `Phase20ReceiptExt`. The ext
serializes the role reward + role claims + fee + a set of present-only trailing magic-dispatch
sections (each byte-identical when absent): `TPK1` tickets, `DOM1` dominance weights, `CND1`
candidate set, `PZL1` puzzle proofs, `FIN1` finality proof, `CAC1` committed admission, `AVR2`
true-VRF proofs. The ext digest feeds the receipts-root (irx1) committed in the block.

## Role flow

Roles: PRIMARY (the miner), COMPUTE, VERIFY, SUPPORT. Role claims use a hidden
precommit/reveal (commit in block H-1, reveal at H); role precommit/reveal + gossip plumbing
exists (testnet). Each role's solver is selected as the best candidate for that role.

## Candidate admission & committed admission flow

1. A miner builds a `RoleCandidate` and wraps it in a `CandidateAdmissionV1`; under the true-VRF
   gate the admission also carries the `AssignmentProofV2`.
2. Admissions are gossiped (P2P `PoawxCandidateAdmission`) and POSTed via loopback RPC
   (`/poawx/candidate-admission`); the node validates + caches them
   (`NodeCandidateAdmissionCache`).
3. Under enforcement, a block's candidate set must EQUAL the node's admitted set for
   (height, seed). Chain-committed admission (`AdmissionCommitmentV1`) commits the next
   height's admitted root in block H-1 (freeze seed = grandparent hash, no circularity).

## Finality flow

A SUPPORT-role finality committee casts member-signed secp256k1 votes (`FinalityVoteV1`),
gossiped (`PoawxFinalityVote`) + collected; a `FinalityProofV1` bundles ≥ threshold Commit
votes for the parent block. The node re-verifies every vote + membership + threshold; the pool
only bundles.

## True-VRF V2 flow (Phase 22D/22E)

1. **Wallet/miner** (only holder of the VRF secret, a secp256k1 key): `prove` →
   `AssignmentProofV2` (273-byte wire); the candidate's `assignment_proof_digest` = the VRF
   output, so the effective score derives from the VRF output.
2. The proof rides in the candidate admission (bound into its digest) → node validates at
   ingest → admitted candidates exposed over loopback RPC.
3. **Pool** fetches admitted proofs and bundles the `AVR2` section for the selected candidates;
   fail-closed if a selected role lacks a proof. The pool has no VRF dependency.
4. **Node** `connect_block` re-verifies the `AVR2` section against the selected candidates
   (`validate_block_true_vrf`); V1-only blocks are rejected under V2 enforcement.

Gate: `IRIUM_POAWX_TRUE_VRF_{ACTIVATION_HEIGHT,REQUIRED}`; off ⇒ V1 placeholder accepted
(byte-identical wire); mainnet hard-off.
