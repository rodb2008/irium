# PoAW-X Phase 21E — Mandatory candidate admission + gossip

**Status:** Implemented (gated, testnet/devnet only, **mainnet hard-off**, default
off; old behavior byte-identical when off). Local-only; not pushed. Builds on Phase
21A–21D. PoAW-X is **consensus/network-level**; the pool is one miner interface, not
the owner — the node re-validates everything.

## What this closes (and what it does not)

Phase 21D validated "the selected role is the best candidate **within the included
candidate set**" but could not tell whether the producer omitted candidates. Phase 21E
adds a **candidate admission + gossip** layer so a block's candidate set must **equal
the set of candidates ADMITTED to the validating node** during a deterministic window.
This moves "best within the included set" → **"best among candidates admitted to this
node in the window."**

**HONEST LIMITATION (still open).** It does **not** prove that unknowable
offline/never-gossiped miners did not exist. The node validates against its **local
admitted-cache snapshot**, so equality is propagation-sensitive and is **testnet/devnet
only**; public-network admission-window tuning (and making admission provably complete)
remains future work. This is the correct, enforceable goal — *best among admitted*, not
*best among the unknowable*.

## Candidate admission payload (`CandidateAdmissionV1`)

`src/poawx_admission.rs`: one canonical `RoleCandidate` (Phase 21D) bound to
`(version, network_id, target_height, seed)` plus a domain-separated digest. Fixed
249-byte wire; `validate()` recomputes the candidate self-consistency (assignment-proof
digest + penalty weight + effective score) and the admission digest, rejecting wrong
network/height and any mutation. **No private key material.** The `seed` is the parent
block hash (`prev_hash`) — the deterministic, non-wall-clock admission context.

## Admission window + node cache

`NodeCandidateAdmissionCache` (process-global): `ingest_bytes` runs
validate → window (`target ∈ [tip, tip+window]`, default 64) → dedupe-by-digest →
store keyed by `(target_height, role_id, solver_pkh)`; a conflicting same-key digest is
rejected. The outcome (`AcceptedNew`/`Duplicate`/`Rejected`) drives the rebroadcast
decision (only newly-accepted rebroadcasts). `candidates_for` / `admitted_candidate_set`
return the canonical admitted set for `(height, seed)`; `prune` + the window bound
memory. The admission "window" is the cache snapshot at validation time — deterministic,
not wall-clock.

## P2P gossip

`protocol.rs`: `PoawxCandidateAdmission = 28` + `PoawxCandidateAdmissionPayload`
(opaque canonical wire bytes). `p2p.rs`: `broadcast_candidate_admission` + receive-loop
ingest on both paths — validate/dedupe via the cache, **rebroadcast only newly-accepted**,
gated by `candidate_admission_gossip_enabled()` (testnet/devnet + gate configured),
mainnet hard-off, malformed dropped (no panic), older/mainnet peers drop the unknown
type.

## Loopback RPC bridge

`iriumd`: loopback-only `candidate_admission_bridge_guard` +
`POST /poawx/candidate-admission` (ingest + rebroadcast) +
`GET /poawx/candidate-admissions?target_height=H` (hex admitted set, window-bounded).
**No public ports.**

## Node enforcement

`connect_block` runs candidate-set validation when `candidate_set_enforced` OR
`candidate_admission_enforced`. When `candidate_admission_enforced(height)`
(`IRIUM_POAWX_CANDIDATE_ADMISSION_ACTIVATION_HEIGHT` + `…_REQUIRED=1`, mainnet hard-off),
`validate_block_candidate_sets`:

- requires the included candidate set to **EQUAL** the node's admitted set for
  `(height, parent prev_hash seed)`, compared via canonical serialization — a **missing**
  admitted candidate or an **extra** non-admitted candidate both **reject**;
- combined with the Phase 21D best-in-set check, the selected role is the **best among
  admitted** candidates;
- **fails closed** when a selected role has no admitted candidate.

Gate off ⇒ unchanged Phase 21D behavior; mainnet hard-off.

## Pool support

`pool/irium-stratum` mirrors the wire (`RoleCandidateMirror::deserialize`,
`decode_admission_candidate`, `build_admitted_candidate_set`,
`CANDIDATE_ADMISSION_WIRE`). The producer loop calls `refresh_pool_admitted_cache`
(loopback `GET /poawx/candidate-admissions`) before production. When
`pool_candidate_admission_enforced`, `build_synthetic`/`build_collected` build the
candidate set **strictly from node-admitted candidates** and **fail closed** (no ext) if
the admitted cache is unavailable/empty — no fake set. Official fee-0 and third-party fee
paths preserved; synthetic fallback only when explicitly enabled. A parity test proves
the pool's admitted set byte-matches the node's admitted set. **The pool is one interface,
not the owner — the node re-validates exact set equality.** (Multi-pool best-candidate
selection / role_reward alignment across competing pools is production follow-up.)

## Wallet helper

`irium-wallet poawx-candidate-admission --network-id <id> --target-height <H> --role
<…> --solver <addr|40hex> --ticket-digest <64hex> --seed <64hex> [--role-claim-digest]
[--penalty-status] [--dominance-weight] [--assignment-pubkey]` emits the admission JSON +
`wire_hex` to POST to the node's loopback `/poawx/candidate-admission`. **No private key /
no seed phrase**, testnet/devnet only, mainnet hard-off. The `wire_hex` deserializes +
validates via the node lib.

## Gates (all default off, mainnet hard-off)

- `IRIUM_POAWX_CANDIDATE_ADMISSION_ACTIVATION_HEIGHT`,
  `IRIUM_POAWX_CANDIDATE_ADMISSION_REQUIRED=1`, `IRIUM_POAWX_CANDIDATE_ADMISSION_WINDOW`.

Each gate returns false on mainnet (`network_id == 0`). Chain difficulty remains
**LWMA-144 automatic**.

## Tests

- `poawx_admission`: admission wire round-trip + digest sensitivity + validate
  accept/reject; cache ingest/dedupe/window/malformed/deterministic-root/prune; gate
  logic + mainnet hard-off.
- `protocol`: `PoawxCandidateAdmission = 28` round-trip.
- `chain`: `phase21e_admission_enforcement` (exact-match accepts; missing/extra/empty
  reject; gate-off 21D behavior; mainnet hard-off).
- pool: `phase21e_pool_admitted_candidate_set_parity_and_failclosed` (pool admitted set
  byte-matches node; decode round-trip; wrong-seed None; builder fail-closed on empty
  cache + attaches on populated; mainnet hard-off).
- wallet: `phase21e_candidate_admission_emit_json_no_secret_mainnet_off`.

## Remaining technical steps

- **True cryptographic VRF** (replace the `AssignmentProofV1` placeholder).
- **Provably-complete admission** for public networks (admission-window timing/finality
  so the admitted set is known-complete, not just locally-observed).
- Puzzle work-mode primitives beyond the simplified role path.
- Finality-committee integration with the SUPPORT/finality role.
- **Excluded (not in this track):** public testnet with outside miners, independent
  security audit, community vote, mainnet activation.
