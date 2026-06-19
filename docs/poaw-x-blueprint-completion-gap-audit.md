# PoAW-X — Blueprint completion gap audit

**Scope:** audit the local implementation (Phase 20 + Phase 21A–21J + Phase 22A) against
the original PoAW-X blueprint. Local-only; nothing pushed. **Mainnet hard-off** for every
PoAW-X/Phase-2x gate. PoAW-X is **consensus/network-level**; the **pool is one miner
interface only**. **Not a mainnet-ready claim; true VRF is NOT complete.** Gaps are listed
honestly, not hidden.

Legend: **IMPL** = implemented (non-gated/base); **GATED** = implemented behind a
testnet/devnet gate, mainnet hard-off; **PLACEHOLDER** = deterministic stand-in, not the
final cryptographic primitive; **PENDING** = not implemented; **EXCLUDED** = out of this
track.

| Blueprint item | Status | Where / note |
|---|---|---|
| Multi-role rewards | **IMPL** | `poawx.rs multi_role_amounts`; coinbase validator (`chain.rs`) |
| Proposer / PRIMARY role | **IMPL** | receipt `worker_pkh` = payout identity; PRIMARY 55% |
| Assigned worker role (COMPUTE) | **GATED** | role claim + fairness lane; `poawx.rs` / fairness matrix |
| Verification role (VERIFY) | **GATED** | role claim; puzzle VerificationWork mode (21F) |
| Finality / SUPPORT role | **GATED** | finality committee (21H) + SUPPORT 10% reward gating |
| 55/22/13/10 reward split | **IMPL** | `MULTI_ROLE_*_BPS` 5500/2200/1300/1000 |
| Official pool 0% fee | **IMPL** | default fee-0 path |
| Third-party fee policy (≤200bps, PRIMARY-only, signed) | **GATED** | `THIRD_PARTY_FEE` gate + mode + delegation terms |
| Non-custodial delegation (mode-1) | **IMPL/GATED** | `Delegation` 226B; connect_block verify; payout identity stays miner |
| Hidden precommit root | **GATED** | prior-block `precommit_root`; connect_block (Step 6A) |
| Role gossip (precommit/reveal) | **GATED** | `poawx_gossip.rs`; P2P (26/27) + loopback RPC bridge |
| Candidate admission | **GATED** | 21E local-cache admission + 22A **chain-committed** admission |
| Candidate set | **GATED** | 21D `CandidateSet`; node best-in-set validation |
| Ticket / Sybil resistance | **GATED** | 21A/21B `TicketProof` + sybil-work; connect_block enforce |
| Anti-domination | **GATED** | 21C persistent reorg-safe per-(miner,window) state + weight enforce |
| Adaptive mode | **GATED (data)** | 21A state machine; consumed where applicable; no hardware class |
| Penalty state | **GATED** | 21A/21B status + high-trust gating |
| Puzzle work modes | **GATED** | 21F 5 modes; assigned-work proofs, NOT chain PoW (LWMA untouched) |
| Finality committee | **GATED** | 21H real secp256k1 votes + N-of-M threshold + node enforce |
| Finality vote gossip/collection | **GATED** | 21I P2P (29) + node cache + loopback RPC + pool fetch |
| Chain-committed admission | **GATED** | **22A (this phase)** — admitted set root committed in parent, matched at H |
| **True cryptographic VRF** | **PLACEHOLDER / PENDING** | `AssignmentProofV1` is a documented VRF-style placeholder; 21G Outcome B found no safe dep/key-model path |
| **Public-network admission completeness** | **PENDING (strengthened)** | 22A chain-commits the admitted set before selection, but does NOT prove offline/never-gossiped miners existed |
| Public testnet with outside miners | **EXCLUDED** | out of track |
| Independent security audit | **EXCLUDED** | out of track |
| Community vote | **EXCLUDED** | out of track |
| Mainnet activation path | **EXCLUDED / hard-off** | every gate `network_id == 0` ⇒ false |

## Honest residual gaps (after Phase 22A)

1. **True cryptographic VRF** — still a placeholder; needs a reviewed/vendored VRF
   dependency + a key-model decision (Phase 21G recommended path). Not solved here.
2. **Provably-complete public-network candidate admission** — 22A makes the admitted set
   **chain-committed** (so block H cannot deviate from the set committed at H-1), which
   strengthens integrity and removes the per-node-cache divergence at selection time. It
   still cannot prove that a miner who never gossiped an admission did not exist — that is
   an open distributed-systems limit, not a bug.
3. Public testnet, independent audit, community vote, mainnet activation — excluded.

No mainnet-ready claim. No full-production claim. True VRF not complete.
