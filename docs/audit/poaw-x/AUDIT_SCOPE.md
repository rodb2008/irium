# PoAW-X audit scope

Branch `testnet/poawx-phase20-blueprint-completion-local` @ `4a3c596`. Local-only;
mainnet hard-off; not mainnet-ready.

## In scope

| Area | Primary files |
|---|---|
| Phase 20 receipt / reward / receipts-root (irx1) wiring | `src/poawx.rs`, `src/chain.rs`, `src/block.rs` |
| 55/22/13/10 reward split (PRIMARY/COMPUTE/VERIFY/SUPPORT) | `src/poawx.rs` (`MULTI_ROLE_*_BPS`, `multi_role_amounts`) |
| Delegated / non-custodial receipts | `src/poawx.rs` (`Delegation`), `src/bin/irium-wallet.rs` |
| Official 0% fee + third-party fee (cap 2.00%) | `src/poawx.rs` (`apply_fee`, `THIRD_PARTY_FEE_CAP_BPS`) |
| Hidden precommit | `src/poawx.rs` (`role_precommit_*`), `src/chain.rs` |
| Role precommit/reveal + role gossip | `pool/irium-stratum/src/delegation.rs`, `src/protocol.rs`, `src/p2p.rs` |
| Tickets / Sybil primitives | `src/poawx_ticket.rs` |
| Penalty enforcement | `src/poawx_penalty.rs`, `src/chain.rs` |
| Persistent anti-domination (reorg-safe) | `src/poawx_dominance.rs`, `src/chain.rs` |
| Candidate set | `src/poawx_candidate.rs` |
| Candidate admission / gossip | `src/poawx_admission.rs`, `src/protocol.rs`, `src/p2p.rs`, `src/bin/iriumd.rs` |
| Chain-committed admission | `src/poawx_committed_admission.rs`, `src/chain.rs` |
| **AssignmentProofV2 true VRF** | `src/poawx_candidate.rs` (`AssignmentProofV2`), `src/chain.rs` (`validate_block_true_vrf`) |
| Puzzle work modes (assigned work, NOT chain PoW) | `src/poawx_puzzle.rs`, `src/chain.rs` |
| Finality committee + finality vote gossip | `src/poawx_finality.rs`, `src/protocol.rs`, `src/p2p.rs`, `src/bin/iriumd.rs` |
| Wallet emit helpers | `src/bin/irium-wallet.rs` |
| Pool production mirrors (byte-parity) | `pool/irium-stratum/src/delegation.rs`, `…/stratum.rs`, `…/block.rs` |
| Node authoritative validation | `src/chain.rs` (`connect_block` + `validate_block_*`) |
| Mainnet hard-off gates | all `IRIUM_POAWX_*` gates (every module) |

## Out of scope

- Mainnet activation (height selection + flipping gates on a non-zero network).
- Governance / community vote.
- Economic review of the reward percentages (the auditor MAY review if they choose; not
  required).
- Exchange listing / liquidity.
- Non-PoAW-X mainnet services.
- Public testnet operations.
- External miner operations (unless requested later).

## Crypto-correctness boundary

The auditor is asked to assess the **use** of `vrf_fun`/`secp256kfun` (RFC 9381 ECVRF over
secp256k1) and the surrounding wiring. The internal correctness of the ECVRF crate
implementation (incl. the vendored k256 field arithmetic in `secp256kfun/src/vendor/k256`) is a
**key audit target** — these crates are pre-1.0 (`0.12.x`) and not formally audited.
