# PoAW-X Phase 21H — Finality committee for the 10% SUPPORT/finality role

**Status:** Implemented (gated, testnet/devnet only, **mainnet hard-off**, default
off; old behavior byte-identical when off). Local-only; not pushed. Builds on Phase
21A–21G. PoAW-X is **consensus/network-level**; the pool is one miner interface, not
the owner — the node re-verifies everything.

## Existing finality code reused / not reused

**Inspection (Section A):** there was **no prior finality-committee / BFT vote system**
to reuse. What exists is unrelated: a confirmation-**depth** notion (`proof_finality_depth`,
default 6, for HTLC/proof maturity), the AnchorManager block-hash **checkpoints**, a
`checkpoint_height`/`checkpoint_hash` handshake field, and the adaptive
`finality_available` bool. Phase 21H therefore adds a **new** committee path but
**reuses**: the project's **secp256k1 ECDSA** signing (`k256` `sign_prehash`/
`verify_prehash` — the same primitive as `Delegation`), the **SUPPORT role** (id 3, 10% /
1000 bps), and the Phase 21D/21E candidate-set/admission path for committee membership.

## Committee model

A finality committee = the **SUPPORT-role candidates** (role id 3) in the block's
candidate set. Each member signs a `FinalityVoteV1` with their secp256k1 key over a
domain-separated vote digest binding network/height/`block_hash`/`parent_hash`/
`committee_epoch`/`member_pkh`/`ticket_digest`/`vote_type`; `member_pkh = HASH160(
member_pubkey)`. `vote_type` ∈ {Precommit, Commit, Checkpoint}. A `FinalityProofV1`
bundles a **canonical (sorted by member_pkh, deduped)** set of votes. Wire is fixed/
bounded (vote 232 bytes; ≤ 256 votes); mutation changes the proof digest.

The proof in block H **finalizes the parent** (`block_hash = the carrying block's
prev_hash`), so votes are over an already-known hash — **no circularity** with the block
that carries them.

## Threshold

Deterministic integer N-of-M: `required_votes = ceil(committee_size × num / den)`,
clamped to `1..=committee_size`. Gates `IRIUM_POAWX_FINALITY_THRESHOLD_NUM` /
`…_DEN` (default **1/1** = unanimous of the present committee; **1-of-1** supported for
low participation; tests use **2-of-3**). Only **Commit** votes count toward
finalization. The threshold is **node-authoritative**: a producer cannot weaken it (the
node requires `proof.threshold_num/den == configured`).

**Low-participation behavior:** with a committee of 1, 1-of-1 finalizes; the chain is not
halted by finality on its own. When the finality gate is **required** and the threshold
is not met, the block **fails closed** (rejected) — finality is then a hard requirement,
as configured.

## Ext binding

`Phase20ReceiptExt` gains an OPTIONAL trailing **FIN1** section (`finality_proof`);
**byte-identical to pre-21H when absent**, bound into the ext digest → receipts root →
irx1 when present. The deserialize magic-dispatch handles `TPK1`/`DOM1`/`CND1`/`PZL1`/
`FIN1` in any order, strict.

## Node validation

When `finality_committee_enforced(height)` (`IRIUM_POAWX_FINALITY_COMMITTEE_ACTIVATION_HEIGHT`
+ `…_REQUIRED=1`, mainnet hard-off), `connect_block` calls `validate_block_finality`:

- requires the finality proof + the candidate set;
- the committee = the SUPPORT-role candidate solver pkhs;
- the proof must use the node-authoritative threshold; every vote must verify (signature),
  bind to (network, height, `block_hash == prev_hash`, epoch), and be from a committee
  member; duplicate members are rejected (non-canonical); and the Commit votes must meet
  the threshold;
- ⇒ the **SUPPORT/finality 10% reward stands only with a valid finality proof** (an
  invalid/missing proof rejects the block).
- **Fails closed** on missing proof / missing candidate set / non-member / insufficient
  or weakened threshold / wrong block hash.

## Interaction with Phase 21F puzzle work

The Phase 21F `FinalityWorkPlaceholder` puzzle mode remains an assigned-work primitive,
but it is **not sufficient on its own** when finality is required: the finality gate
requires the **full committee proof** (`validate_block_finality` rejects a block that
carries only puzzle proofs and no `finality_proof`). The test
`phase21h_finality_enforcement` covers this (missing-finality-proof ⇒ reject).

## Pool support

The pool does **not** sign votes (no member keys) and does **not** verify finality (the
node is authoritative) — it **BUNDLES** real member-signed votes into a byte-identical
`FinalityProofMirror`. It mirrors the gate + threshold, holds a process-global vote cache
(collected vote wire per height), and `build_pool_finality_proof` decodes + filters by
network/height/block_hash + dedups by member + sorts canonical. `build_synthetic`/
`build_collected` attach the bundled proof when `pool_finality_committee_enforced` and
**fail closed** (no ext) if no collected committee votes. Official fee-0 and third-party
fee paths preserved; candidate/admission/ticket/dominance/penalty enforcement remain
compatible. A parity test proves a pool-bundled proof verifies via the node lib.

## Wallet helper

`irium-wallet poawx-finality-vote --network-id <id> --target-height <H> --block-hash
<64hex> [--parent-hash] [--committee-epoch] [--ticket-digest] [--vote-type
precommit|commit|checkpoint] --secret-hex <64hex>` builds + signs a **real secp256k1**
vote. The signing key is an **input** (testnet throwaway) and is **never echoed**; the
output carries only the public key, member_pkh, signature, vote_digest, and `wire_hex`
(to POST to the pool/operator finality collector). Testnet/devnet only; mainnet hard-off.

## True VRF note

`AssignmentProofV1` remains a **VRF-style placeholder** (Phase 21G documented that no
safe true-VRF dependency exists in-tree). The finality committee uses real ECDSA
signatures; candidate selection still relies on the V1 placeholder under the existing
gate-off/testnet rules. **True VRF remains pending.**

## Gates (all default off, mainnet hard-off)

- `IRIUM_POAWX_FINALITY_COMMITTEE_ACTIVATION_HEIGHT`, `IRIUM_POAWX_FINALITY_COMMITTEE_REQUIRED=1`,
  `IRIUM_POAWX_FINALITY_THRESHOLD_NUM`, `IRIUM_POAWX_FINALITY_THRESHOLD_DEN`.

Each gate returns false on mainnet (`network_id == 0`). Chain difficulty remains
**LWMA-144 automatic** — finality never touches PoW/target/interval.

## Tests

- `poawx_finality`: vote sign/verify + rejects (network/height/block/sig/pkh mismatch),
  threshold pass/fail + 1-of-1 + non-member + duplicate + wrong-block + wire round-trip +
  digest mutation, required-votes math, gate logic + mainnet hard-off.
- `chain`: `phase21h_finality_enforcement` (2-of-3 accepts; insufficient/missing-proof/
  missing-candidate-set/weakened-threshold reject; mainnet hard-off).
- `poawx`: `phase21h_ext_finality_section_roundtrip` (absent byte-identical, present +
  mutation change digest, finality+candidate+dominance together).
- pool: `phase21h_pool_finality_proof_parity_and_failclosed` (real member vote bundled by
  the pool verifies via the node lib, build_synthetic attaches + node reads FIN1,
  fail-closed without votes, mainnet hard-off).
- wallet: `phase21h_finality_vote_emit_no_secret_mainnet_off`.

## Remaining technical steps

- **True cryptographic VRF** (replace the `AssignmentProofV1` placeholder; Phase 21G
  Outcome B).
- Provably-complete public-network candidate admission (Phase 21E is best-among-admitted).
- Live finality-vote gossip/collection over P2P (the bundling + cache exist; live
  propagation mirrors the admission pattern as an operational follow-up).
- **Excluded (not in this track):** public testnet with outside miners, independent
  security audit, community vote, mainnet activation.
