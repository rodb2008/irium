# PoAW-X Phase 21I — Live finality-vote P2P gossip + pool collection

**Status:** Implemented (gated, testnet/devnet only, **mainnet hard-off**, default
off; old behavior byte-identical when off). Local-only; not pushed. Completes the Phase
21H follow-up (live finality-vote propagation). PoAW-X is **consensus/network-level**;
the pool is one miner interface, not the owner — the node re-verifies everything.

## Reused patterns

Phase 21I mirrors the existing gossip stack **exactly** — the candidate-admission
(Phase 21E) and role-gossip flows: a P2P message type + opaque payload, a process-global
node cache with the same validate → window → dedupe → store → rebroadcast-only-if-new
flow, a loopback-only RPC bridge, and a pool fetch helper. No second/incompatible gossip
style was introduced.

## Finality vote gossip

- **P2P:** `protocol.rs` `PoawxFinalityVote = 29` + `PoawxFinalityVotePayload` (opaque
  canonical `FinalityVoteV1` wire). `p2p.rs` `broadcast_finality_vote` + receive-loop
  ingest on both paths: validate the **member secp256k1 signature** + dedupe via the node
  cache, **rebroadcast only newly-accepted**, gated by `finality_gossip_enabled`, mainnet
  hard-off, malformed dropped (no panic), older/mainnet peers drop the unknown type.
- **Votes are member-signed, not pool/node-signed.** The node only validates +
  stores/forwards; it never creates votes.

## Node finality-vote cache

`NodeFinalityVoteCache` (process-global, in `src/poawx_finality.rs`): `ingest_bytes` runs
validate(signature) → window (`target ∈ [tip, tip+window]`, default 64) → dedupe by the
signed vote digest → store keyed by `(target_height, block_hash, vote_type, member_pkh)`;
a conflicting same-key digest is rejected. Outcome (`AcceptedNew`/`Duplicate`/`Rejected`)
drives rebroadcast. `votes_for` (sorted by member_pkh) / `votes_for_height` (RPC export) /
a deterministic `root` / `prune` bound memory; safe under repeated duplicates.

## Loopback RPC bridge

`iriumd` (loopback-only): `finality_vote_bridge_guard` +
`POST /poawx/finality-vote` (ingest + rebroadcast) +
`GET /poawx/finality-votes?target_height=H` (deterministic hex votes, window-bounded).
**No public ports;** testnet/devnet only; mainnet hard-off (disabled unless the finality
gossip gate is configured).

## Pool collection

`fetch_node_finality_votes` (async loopback `GET /poawx/finality-votes`) +
`refresh_pool_finality_votes` populate the existing (Phase 21H) `pool_finality_vote_cache`;
the stratum producer loop refreshes the finality-vote cache before production (after the
admitted-cache refresh), best-effort, no-op unless the finality committee gate is enforced.
`build_pool_finality_proof` (Phase 21H) then **bundles the fetched member-signed votes**
into the `FinalityProofV1` it attaches; it **fails closed** if the threshold cannot be met.
The pool **never signs votes** and **does not bypass node validation**. Official fee-0 and
third-party fee flows preserved; the manual/bundled vote path (21H) still works when the
gossip gate is off.

## Wallet

`irium-wallet poawx-finality-vote …` remains **emit-only by default**. An explicit
`--submit --node-rpc <loopback-url>` POSTs the signed vote wire to the loopback node's
`/poawx/finality-vote` (best-effort). No public remote posting by default; the signing key
stays an **input** and is **never echoed**; testnet/devnet only; mainnet hard-off.

## What is enforced

Gossip/cache/RPC/fetch are **transport** — they do not change consensus. Block-level
finality enforcement is unchanged from **Phase 21H**: `connect_block` still
authoritatively validates the `FinalityProofV1` (committee = SUPPORT candidates,
node-authoritative threshold, member signatures, finalizes the parent) when
`finality_committee_enforced`. The node re-verifies every gossiped vote's signature before
storing, and re-verifies the bundled proof at block acceptance. The SUPPORT/finality 10%
reward still stands only with a valid finality proof.

## What remains pending

- **True cryptographic VRF** (Phase 21G Outcome B; `AssignmentProofV1` placeholder).
- Provably-complete public-network candidate admission (Phase 21E is best-among-admitted).

## Gates (all default off, mainnet hard-off)

- `IRIUM_POAWX_FINALITY_GOSSIP_ACTIVATION_HEIGHT`, `IRIUM_POAWX_FINALITY_GOSSIP_REQUIRED=1`,
  `IRIUM_POAWX_FINALITY_GOSSIP_WINDOW` (transport) — composes with the Phase 21H
  `IRIUM_POAWX_FINALITY_COMMITTEE_*` enforcement gates.

Each gate returns false on mainnet (`network_id == 0`). When the gossip gate is on the
pool prefers node-fetched votes; the node still authoritatively validates the finality
proof. Chain difficulty remains **LWMA-144 automatic**.

## Tests

- `protocol`: `message_type_try_from_finality_vote_29` (round-trip).
- `poawx_finality`: `finality_gossip_cache_ingest_dedupe_window_prune` (store valid,
  reject malformed/invalid-sig/out-of-window, dedupe, deterministic sorted export + root,
  prune) + the Phase 21H sign/verify/threshold tests.
- pool: the Phase 21H `phase21h_pool_finality_proof_parity_and_failclosed` proves the
  bundle-from-cache → node-accept path; the async fetch is a thin reqwest mirror of the
  admission fetch (operationally exercised).
- wallet: `phase21h_finality_vote_emit_no_secret_mainnet_off` extended — `--submit`/
  `--node-rpc` accepted by the JSON builder; key never echoed; mainnet hard-off.

## Remaining technical steps

- **True cryptographic VRF** (Phase 21G Outcome B).
- **Provably-complete public-network candidate admission**.
- **Excluded (not in this track):** public testnet with outside miners, independent
  security audit, community vote, mainnet activation.
