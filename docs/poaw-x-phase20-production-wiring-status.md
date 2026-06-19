# PoAW-X Phase 20 — Production Wiring Status (Steps 1–4 + 6A + 6B + 6C + 6D + 6E + 6F COMPLETE: extension threaded + consensus-enforced + root-committed + official+third-party-fee production + hidden-precommit root + role collection + role gossip plumbing + live node↔pool bridge + loopback live E2E + two-VPS live E2E; only public/external miner test remains)

## Step status summary (PoAW-X role/fairness track)
- **Step 6A — hidden-precommit commitment root: COMPLETE** (primitives + connect_block enforcement, mainnet hard-off)
- **Step 6B — local/testnet role precommit/reveal collection: COMPLETE** (loopback endpoints + RoleProtocolStore)
- **Step 6C — role gossip payloads + in-memory relay: COMPLETE** (forward-compat wire types + pool gossip engine)
- **Step 6D — live cross-process node↔pool bridge: COMPLETE** (P2P receive/rebroadcast + loopback RPC bridge + pool fetch)
- **Step 6E — local loopback live role-gossip E2E: COMPLETE / PASS** (blocks 2 official + 3 third-party from collected data; restart/reload PASS; found+fixed the `fee_terms_from_ext_hex` × `precommit_root` bug in `cdbe24c`). See `poaw-x-phase20-step6e-loopback-role-gossip-e2e.md`.
- **Step 6F — two-VPS live role-gossip E2E: COMPLETE / PASS** (role gossip VPS-2 → real P2P → VPS-1 → block production; observer Node B validated both blocks byte-identical; restart/reload PASS; operator-only source-restricted UFW, removed after). See `poaw-x-phase20-step6f-two-vps-role-gossip-e2e.md`.
- **Step 7A — external trusted-miner test PACK + preflight: PREPARED (docs only; test NOT run).** Two-tier plan (Tier 1 external stock-cpuminer connect/auth/work/PoW-attempt; Tier 2 emit-only role precommit/reveal, no private-key transfer), operator preflight checklist, fresh Step-7 port plan (stratum [+optional P2P] source-restricted; delegation/RPC/status/metrics/role loopback-only), and a simple miner invite. **No endpoints exposed, no ports bound, no invites sent, no firewall changes.** See `poaw-x-phase20-external-miner-test-plan.md` + `poaw-x-phase20-trusted-miner-invite.md`.
- **Step 7B-Self — self-operated two-VPS complete PoAW-X final rehearsal: PASS** (*not* the external miner test). Blocks 2 (official) + 3 (third-party fee) produced on VPS-1 from P2P-gossiped collected role data (synthetic OFF); observer Node B validated both byte-identical; restart/reload PASS; operator-only source-restricted UFW (removed after). **In this run the remote VPS-2 cpuminer landed BOTH blocks over the real external path** (the slow-PoW caveat did not recur). See `poaw-x-phase20-step7b-self-two-vps-rehearsal.md`.
- **Phase 21A — blueprint gap-closure FOUNDATION primitives: IMPLEMENTED (data-only, gated, mainnet hard-off).** Miner Work Tickets + Sybil-work, anti-domination recent-reward tracker + fairness weight, adaptive mode state machine, penalty/fraud state — node-lib modules `src/poawx_{ticket,dominance,adaptive,penalty}.rs` with tests; not wired into live acceptance (Phase 21B). **PoAW-X is consensus/network-level; the pool is only one miner interface, not the owner.** See `poaw-x-phase21-blueprint-gap-closure.md`.
- **Phase 21B — gated ticket + penalty ENFORCEMENT: IMPLEMENTED (testnet/devnet-gated, mainnet hard-off, old behavior unchanged when off).** Compact `TicketProof` bound into `Phase20ReceiptExt` (optional trailing section, byte-identical when absent); `connect_block` enforces per-role ticket proofs when `tickets_enforced(height)` and blocks suspended/slashed from high-trust roles when `penalty_state_enforced(height)`; pool attaches byte-identical ticket proofs (node-parity tested); wallet `poawx-ticket-proof` emit. **Dominance remains data-only — full persistent/reorg-safe dominance enforcement deferred to Phase 21C (not faked).** See `poaw-x-phase21-blueprint-gap-closure.md`.
- **Remaining:** (1) **run** the public/external non-self-operated miner test (blocked on operator collecting the miner inputs + firewall handoff); (2) remote slow-cpuminer low-devnet-height PoW caveat (it did not recur in Step 7B-Self, but remains possible on slow remote hardware); (3) Phase 21B+ enforcement + VRF/private-assignment, puzzle work-modes, finality-committee integration.

**Safety (unchanged):** mainnet hard-off on every gate; chain difficulty remains LWMA-144 automatic; delegation/RPC/status/metrics loopback-only; mainnet + production pool untouched throughout. **Not claimed:** mainnet-ready, public activation complete, external miner test complete, or remote cpuminer PoW solved. Nothing pushed (local branch only).

## Step 6D — live cross-process node↔pool role-gossip bridge: COMPLETE (loopback bridge + P2P receive/rebroadcast wired)

Step 6D closes the Step 6C gap: the node P2P bus and the pool `RoleProtocolStore`
are now joined over loopback so externally-gossiped role data reaches pool
production, and pool-local submissions reach the P2P network.

**Architecture now:**
```
peer gossip → iriumd P2P receive → node role-gossip cache → (loopback RPC GET) → pool RoleProtocolStore → Phase20 block production
pool local role-precommit/reveal endpoint → pool store → (loopback RPC POST) → node cache → iriumd P2P broadcast/rebroadcast
```

- **Node cache** (`src/poawx_gossip.rs`, new): `NodeRoleGossipCache` — a process-global
  singleton (`global_cache()`) shared by the P2P task(s) and the axum RPC handlers (no Arc
  threaded through `P2PNode`). Validates the SAME Step 6C wire envelopes
  (`RolePrecommitGossip`/`RoleRevealGossip`, v1), dedupes by stable digest, bounds by the
  height window `[tip, tip+window]` (`IRIUM_POAWX_ROLE_GOSSIP_WINDOW`, default 64), stores
  precommits/reveals by target height, prunes old heights. **Mainnet hard-off**
  (`activation::network_id_byte()==0`), disabled unless `IRIUM_POAWX_ROLE_GOSSIP_ENABLED=1`.
  Reveal requires a matching precommit (graceful reject otherwise). **No consensus effect**
  (Step 6A enforcement stays block-driven).
- **P2P receive dispatch** (`src/p2p.rs`): `MessageType::PoawxRolePrecommit`/`PoawxRoleReveal`
  are now handled in **both** receive sites (inbound + outbound), mirroring `OfferBroadcast`:
  gate → `global_cache().ingest_*` → **rebroadcast only when newly accepted** (`broadcast_raw`).
  Duplicate/invalid are neither stored nor rebroadcast; malformed never panics. Unknown/old
  message handling and existing block/tx/receipt gossip are unchanged.
- **P2P broadcast** (`src/p2p.rs`): `P2PNode::broadcast_role_precommit`/`broadcast_role_reveal`
  flood an envelope to all peers (mirror of `broadcast_offer`).
- **Loopback RPC bridge** (`src/bin/iriumd.rs`): four routes on the existing RPC listener,
  each **loopback-only** (`addr.ip().is_loopback()` → 403 otherwise) + gated + mainnet-off:
  `POST /poawx/role-gossip/precommit`, `POST /poawx/role-gossip/reveal` (validate/dedupe/store
  → best-effort P2P broadcast on `AcceptedNew`; 400 on reject), `GET /poawx/role-gossip/precommits?target_height=H`
  and `GET /poawx/role-gossip/reveals?target_height=H` (return stored envelopes for a
  window-bounded height). No new port bound.
- **Pool bridge** (`pool/irium-stratum/src/delegation.rs` + `stratum.rs`): on a valid local
  role submission the pool stores it (Step 6B) **and** best-effort forwards the envelope to the
  node (`forward_precommit_to_node`/`forward_reveal_to_node`); a node failure never affects the
  local store/response. Before producing a block the pool `bridge_fetch_into_store` pulls
  node-collected precommits (height + height+1) and reveals (height) and ingests them into the
  local store, then builds the collected Phase 20 ext. Node RPC base defaults to the pool's
  existing node RPC; override via `IRIUM_POAWX_ROLE_GOSSIP_NODE_RPC`. Production priority is
  unchanged: **collected (now bridged) real role data → synthetic fallback
  (`IRIUM_POAWX_SYNTHETIC_ROLE_CLAIMS=1`) → fail closed.**
- **P2P receive wired:** YES (both dispatch sites). **P2P rebroadcast wired:** YES
  (`broadcast_raw` on newly-accepted, mirroring OfferBroadcast). **Loopback RPC bridge:** YES
  (POST/GET, loopback-enforced). **Public exposure:** OUT OF SCOPE — all bridge endpoints are
  loopback-only; no public ports were bound and **no live E2E was run** this step.
- **Env (new this step):** `IRIUM_POAWX_ROLE_GOSSIP_WINDOW` (default 64),
  `IRIUM_POAWX_ROLE_GOSSIP_NODE_RPC` (default = pool node RPC). Existing:
  `IRIUM_POAWX_ROLE_GOSSIP_ENABLED=1` (mainnet hard-off, default off).
- **Tests:** node `poawx_gossip` (envelope/version/oversize/malformed + hides secret;
  ingest validate/window/dedupe; reveal-needs-precommit; GET-able; prune; rebroadcast policy;
  mainnet+disabled hard-off); node `protocol` (26/27 forward-compat unchanged); pool
  `phase20_role_gossip_bridge_*` (real-HTTP fetch → store → build → **node validator accepts**;
  official + third-party fee; real-HTTP forward POSTs the envelope to the bridge path;
  unreachable-node error-safety; base override; mainnet hard-off; synthetic gating). All Step
  6A/6B/6C tests unchanged.
- **NOT done (follow-up):** local loopback live role-gossip E2E (stock miner → real
  cross-process bridge → block), then a two-VPS live role-gossip E2E **only with the operator
  firewall handoff** (source-restricted, as in Step 5B). Mainnet remains disabled; chain
  difficulty remains **LWMA-144 automatic**; the external-miner low-devnet-height PoW caveat
  remains separate.



**Status:** **PARTIAL** (advancing). Steps **1** (ext threading), **2** (`connect_block`/`submit_block_extended`
enforcement + receipts-root), **3** (official fee-0 multi-role coinbase), **4** (third-party-fee
production), **6A** (hidden role-precommit commitment root), and **6B** (local/testnet role precommit +
reveal COLLECTION protocol) are COMPLETE; **Step 6C — testnet/devnet role precommit/reveal
gossip/P2P PLUMBING — is now COMPLETE at the payload + validation + in-memory-relay level.** The node
P2P layer reserves two forward-compatible wire message types (`PoawxRolePrecommit = 26`,
`PoawxRoleReveal = 27`); old/mainnet peers drop them safely via the `Unknown` sentinel + `_ => {}`
catch-all (existing block/tx/receipt gossip unaffected). The pool gains a versioned gossip envelope
(`RolePrecommitGossip`/`RoleRevealGossip`, v1) and a conservative `RoleGossipEngine`
(validate → dedupe-by-stable-digest → height-window → store in the Step 6B `RoleProtocolStore` →
rebroadcast **only** when newly accepted), tested with an in-memory multi-node relay and full
production parity. **Honest scope:** the *live cross-process bridge* (node P2P receive → pool store,
and pool → node broadcast) is **NOT wired** in this step — the node P2P bus and the pool store live in
separate crates/processes joined only by RPC; bridging them is the documented follow-up. **No public
ports were bound, no live E2E was run.** Official pool stays **0%**; third-party fee opt-in/capped
2%/PRIMARY-only/miner-signed; **mainnet hard-off** for every gate (`role_gossip_enabled()` is false on
mainnet and unless both `IRIUM_POAWX_ROLE_PROTOCOL_ENABLED=1` and `IRIUM_POAWX_ROLE_GOSSIP_ENABLED=1`).
Chain difficulty remains **LWMA-144 automatic** (untouched). The external-miner low-devnet-height PoW
caveat (Steps 5B / earlier) remains a separate, unrelated item. Steps 5A/5B results unchanged.

### Step 6C (this pass) — testnet/devnet role gossip plumbing: COMPLETE (payload + validation + in-memory relay; live bridge = future)
- **Node wire envelope** (`src/protocol.rs`): `MessageType::PoawxRolePrecommit = 26` and
  `PoawxRoleReveal = 27` + `TryFrom<u8>` arms + `PoawxRolePrecommitPayload`/`PoawxRoleRevealPayload`
  (opaque JSON bytes, same pattern as `OfferBroadcast`/`ProofGossip`). **No receive-dispatch change** —
  unknown/old peers and mainnet nodes drop these via the existing forward-compat path, so adding the
  variants is inert until a future step bridges them. Pure gossip, **no consensus effect** (Step 6A
  enforcement is driven by block contents, not by message receipt).
- **Pool gossip layer** (`pool/irium-stratum/src/delegation.rs`): `ROLE_GOSSIP_VERSION=1`,
  `ROLE_GOSSIP_MAX_BYTES=4096`; versioned envelopes `RolePrecommitGossip`/`RoleRevealGossip`
  (`encode`/`decode`, reject unsupported version / oversize / malformed); `role_gossip_enabled()`
  (mainnet hard-off; requires `IRIUM_POAWX_ROLE_PROTOCOL_ENABLED=1` **and**
  `IRIUM_POAWX_ROLE_GOSSIP_ENABLED=1`; default off).
- **Engine** (`RoleGossipEngine`): `ingest_precommit` / `ingest_reveal` — validate first (reuses the
  Step 6B DTO `validate`), enforce height window `[tip, tip+ROLE_PROTOCOL_HEIGHT_WINDOW]` (stale and
  far-future reject), dedupe by stable digest (precommit = `leaf()`, reveal = domain-tagged hash;
  any mutation changes it), then store via the **unchanged** Step 6B store. Returns
  `GossipOutcome::{AcceptedNew, Duplicate, Rejected}`; **only `AcceptedNew` rebroadcasts** — invalid
  payloads are never stored or relayed. A reveal with no matching precommit is rejected gracefully
  (no crash), per the Step 6B store policy. The seen-digest set is held separately from the store so
  6B semantics are untouched; the store is pruned by height via `prune`.
- **Store/dedupe/prune**: validate → dedupe → store-if-valid-and-in-window → rebroadcast-only-valid →
  prune old heights. Conservative size bound `ROLE_GOSSIP_SEEN_CAP` on the seen set.
- **Env (new)**: `IRIUM_POAWX_ROLE_GOSSIP_ENABLED=1` (in addition to `IRIUM_POAWX_ROLE_PROTOCOL_ENABLED=1`).
  Both default off; both mainnet-hard-off.
- Tests (pool): `phase20_role_gossip_envelope_roundtrip_and_versioning` (encode/decode round-trip,
  precommit hides secret/nonce, reveal reconstructs commitment, mutation changes digest, bad-version /
  oversize / malformed reject), `phase20_role_gossip_validation_window_dedupe` (wrong-network / stale /
  far-future / malformed / duplicate-dedupe / reveal-without-precommit-safe / reveal-with-precommit),
  `phase20_role_gossip_inmemory_relay` (valid precommit stores once across a 3-node flood + converges,
  duplicate stores once, invalid not stored/relayed, valid reveal stores once, invalid reveal not
  stored/relayed), `phase20_role_gossip_production_parity` (gossip-collected precommits build the parent
  root + reveals build the child ext that the node validator accepts; official fee-0 + third-party fee;
  synthetic fallback only when enabled; mainnet hard-off). Tests (node): `message_type_try_from_role_gossip_26_27`
  (26/27 map; unknown 200 still errors → graceful drop), payload round-trip + wire serialization for
  both, wrong-message-type rejection.
- **NOT done (live bridge, follow-up):** wiring the node P2P receive loop to call `RoleGossipEngine`
  and feed the pool store, and having the pool emit `MessageType::PoawxRolePrecommit/Reveal` to peers;
  public/external role-gossip run; two-VPS live role-gossip E2E. No public ports in this step.

### Step 6B (this pass) — local/testnet role precommit + reveal collection: COMPLETE
- **Payloads** (`pool/irium-stratum/src/delegation.rs`): `RolePrecommitDto` (hides secret/nonce —
  only `commitment_hash`) and `RoleRevealDto` (secret + nonce + lane + claim_digest), validated into
  `ValidatedPrecommit`/`ValidatedReveal` using the **Step 6A primitives** (one hashing model). Reveal
  validation enforces `commitment_hash == role_precommit_commitment(secret,nonce)` (mutation fails
  closed). Wrong network/role/solver-hex reject.
- **Store** (`RoleProtocolStore`): height-keyed in-memory store; `add_precommit` (idempotent on
  same commitment, rejects a differing duplicate), `add_reveal` (rejects a reveal with no matching
  precommit), `prune(tip)` (window `ROLE_PROTOCOL_HEIGHT_WINDOW`=64), `canonical_precommit` (one per
  role, smallest `(solver,commitment)`), `precommit_root_for(h)`, `select_reveals(h)` (one valid
  reveal per role or None). Shared via `PoawxProducer.role_store`.
- **Production** (`build_collected_phase20_ext` + stratum `build_session_poawx_receipts`): when
  hidden-precommit is active, the ext source priority is **(1) collected** (reveals for H → claims +
  RoleReward; `precommit_root` commits H+1 from collected precommits) → **(2) synthetic** only when
  `IRIUM_POAWX_SYNTHETIC_ROLE_CLAIMS=1` → **(3) fail closed** (no ext; node rejects after activation).
- **Endpoints**: `POST /poawx/role-precommit` and `POST /poawx/role-reveal` on the **existing
  loopback delegation server**, gated by `IRIUM_POAWX_ROLE_PROTOCOL_ENABLED=1` (403 when off, 503 on
  mainnet). No new public bind; loopback-only.
- **Wallet helpers**: `irium-wallet poawx-role-precommit` (emits precommit JSON, no secret/nonce) and
  `poawx-role-reveal` (emits reveal JSON with secret/nonce + lane + claim_digest; needs `--prev-hash`).
  No private key printed; same secret/nonce reconstructs the commitment.
- **Suggested env**: `IRIUM_POAWX_ROLE_PROTOCOL_ENABLED=1`; endpoints ride `IRIUM_POAWX_DELEGATION_BIND`
  (loopback-only; non-loopback refused). Synthetic fallback remains behind `IRIUM_POAWX_SYNTHETIC_ROLE_CLAIMS=1`.
- Tests (pool): `phase20_role_protocol_payloads_and_store` (round-trip, hide/reconstruct, mutation,
  wrong-field, store add/dedup/prune/select) and `phase20_role_protocol_collected_production_and_node_parity`
  (collected ext → node validates each claim + reveal-leaf root == parent committed root; missing role
  fails closed; off-path/mainnet → None).

### Step 6A — hidden role-precommit commitment root: COMPLETE (primitives + enforcement)
- **Primitives** (`src/poawx.rs`): `role_precommit_commitment(secret,nonce) = H(COMMIT_DOMAIN‖secret‖nonce)`

### Step 6A (this pass) — hidden role-precommit commitment root: COMPLETE (primitives + enforcement)
- **Primitives** (`src/poawx.rs`): `role_precommit_commitment(secret,nonce) = H(COMMIT_DOMAIN‖secret‖nonce)`
  (hides the reveal); `role_precommit_leaf(net,target_height,role,solver_pkh,commitment)`;
  `role_precommit_root(leaves)` = SHA256 over **sorted** leaves (order-independent, no Merkle proofs
  needed); `role_precommit_leaf_for_claim` validates `commitment == H(secret‖nonce)` then rebuilds
  the leaf. **Replay/mutation protection:** changing height/network/role/solver/secret/nonce changes
  the leaf; a mutated reveal fails the commitment binding.
- **Lane is intentionally NOT in the leaf** — the lane is `assign_lane(hash(H-1))`, unknowable when the
  precommit is placed in H-1 (it depends on H-1's own hash). The lane is non-grindable (deterministic
  from the assignment) and stays enforced at reveal by `validate_role_claim`. (A lane-bound variant
  would require a grandparent-shifted assignment; deferred.)
- **Receipt extension** (`Phase20ReceiptExt.precommit_root: Option<[u8;32]>`): trailing-optional wire
  (None => byte-identical to pre-6A; present appends flag+32). The ext digest — and therefore the
  gated receipts root — changes when `precommit_root` changes.
- **Activation gate** `IRIUM_POAWX_HIDDEN_PRECOMMIT_ACTIVATION_HEIGHT` (mainnet hard-false, default
  off, testnet/devnet only). Before activation: Steps 5A/5B synthetic claims work unchanged, no
  parent-root requirement. After activation: every Phase 20 production block must carry a
  `precommit_root` (committing the next block's leaves) and its role claims must reveal leaves whose
  sorted root equals the **parent** block's `precommit_root`. **Grace:** exactly one transition block
  at the activation height (its parent predates activation) skips the parent-root match.
- **Chain enforcement** (`src/chain.rs`): `hidden_precommit_active(height)` gates
  `validate_hidden_precommit` inside the Phase 20 production validation path (`validate_phase20_production_block`
  now takes `previous` to load the parent's committed root). Missing parent root, wrong root, missing
  own root, and mutated/mismatched commitments all reject. The precommit_root persists/reloads and
  syncs with the block (it rides inside the already-threaded ext bytes — no new storage path).
- **Pool synthetic builder** (`pool/irium-stratum/src/delegation.rs`): mirror primitives (parity vs
  the dev-dep node lib) + `synthetic_precommit_root`; when hidden-precommit is active the builder
  derives prev-hash-free secret/nonce (so block H-1 can commit block H's root), sets each claim's
  `commitment_hash`, and sets `precommit_root` for the next height. **Testnet/devnet-only synthetic
  builder — NOT public role-claim networking.**
- Tests: poawx `phase20_hidden_precommit_primitives`, `phase20_ext_precommit_root_roundtrip_and_digest`;
  chain `phase20_hidden_precommit_enforcement` (grace accept; valid parent-root reveal accept; missing
  parent root / wrong root / missing own root / mutated commitment reject; mainnet hard-off); pool
  `phase20_hidden_precommit_synthetic_and_node_parity` (pool-committed root == node-reconstructed reveal
  root; mutation rejected by node; off-path unchanged).

### Step 4 — third-party-fee production wiring: COMPLETE
Third-party fee is **explicit opt-in only**; the official Irium pool remains **0%** and the entire
Phase 18/19/20-official path is byte/logically unchanged. Cap is **200 bps (2%)**, applied **only to
the PRIMARY_MINER allocation** (compute/verify/support are never taxed). Fee terms are **miner-signed**
into the 226-byte `Delegation` (`fee_bps` + `fee_pkh` are in `message_hash`), and the production
extension fee is **bound to that signed delegation**. Mainnet is hard-off throughout.
- **Wallet CLI** (`src/bin/irium-wallet.rs`): `--third-party-pool`, `--fee-bps <1..200>`,
  `--fee-pkh <base58-addr|40hex>`. `--fee-bps 0` still works with no third-party flag (official).
  `fee>0` requires `--third-party-pool` + `--fee-pkh` + cap + non-mainnet, else fails closed. The
  emit-only JSON equals the online POST body (both are the signed delegation hex, which carries the
  fee terms); online mode verifies the pool identity advertises the exact same `fee_bps`/`fee_pkh`.
  No private key/seed is printed; fee terms are logged when `fee>0`.
- **Pool identity** (`pool_identity_json` + `/poawx/pool-identity`): advertises `fee_bps` + `fee_pkh`
  only via `pool_third_party_fee_terms()` (third-party mode on + cap + valid pkh + non-mainnet);
  official advertises `fee_bps:0` and no `fee_pkh` (byte-identical to before). Invalid config fails
  closed to official 0% with a warning.
- **Pool registry** (`verify_and_store` + `expected_fee`): official mode still rejects `fee_bps>0`;
  third-party mode accepts a signed delegation ONLY when its fee terms equal the pool's configured
  terms (gated on `third_party_fee_active(tip)`). `StoredDelegation.fee_pkh` persists/reloads. Rejects
  fee mutation (`BadSignature`), `fee_bps` mismatch (`FeeMismatch`), `fee_pkh` mismatch
  (`FeePkhMismatch`), over-cap, and fee in official mode.
- **Pool coinbase** (`build_native_rewardable_coinbase`): with a fee-bearing ext, appends a 6th
  output (fee → `fee_pkh`) where `fee = floor(primary_gross * fee_bps / 10000)` and `primary_net =
  primary_gross - fee`; compute/verify/support unchanged; total == block reward; no delegate/hidden
  output. Official keeps the 5-output (irx1 + 4 role) shape byte-identical to Step 3.
- **Receipt extension**: `build_synthetic_phase20_ext` takes optional fee terms; ext digest (and thus
  the gated root) changes with `fee_bps`/`fee_pkh`.
- **Node consensus** (`src/chain.rs`, `src/bin/iriumd.rs`): the Step-1 mode-1 `fee_bps==0` hard-reject
  is relaxed to allow a delegation fee ONLY under the third-party gates (cap + nonzero `fee_pkh`;
  mainnet stays 0%); `validate_phase20_production_block` binds the extension fee terms to the signed
  delegation (`ext.fee != delegation.fee` rejects). The fee-aware coinbase validator
  (`validate_poawx_coinbase_payout`) was already present (Step 2).
- Tests: wallet (`poawx_register_build_signed_delegation_verifies` extended for cap/mainnet/valid +
  sig-binding; `poawx_third_party_fee_arg_resolution`); pool (`phase20_registry_third_party_fee`,
  `phase20_synthetic_builder_third_party_fee`, `phase20_native_coinbase_third_party_fee`, identity
  shape); node (`phase20_connect_block_mode1_third_party_fee_and_binding`).

### Step 3 — pool canonical multi-role coinbase production (OFFICIAL fee-0): COMPLETE
After all Phase 20 production gates are active on testnet/devnet (multi-role + fairness; mainnet
hard-off), the stratum pool builds a valid `Phase20ReceiptExt`, the canonical multi-role coinbase,
and the gated root that matches `connect_block` / `submit_block_extended`.
- **Mirror primitives** (`pool/irium-stratum/src/delegation.rs`): byte-for-byte stratum-local
  mirrors of the node consensus primitives — `multi_role_amounts`, `fairness_assignment_digest`,
  `assign_lane_id`, `role_claim_digest`, and `RoleRewardMirror` / `PoawxRoleClaimMirror` /
  `Phase20ReceiptExtMirror`. Parity tests assert equality vs the dev-dep node lib (any drift fails).
- **Gate** `phase20_production_active(height)` (mainnet hard-off via `network_id_from_env()==0`;
  requires both `IRIUM_POAWX_MULTI_ROLE_REWARD_ACTIVATION_HEIGHT` and
  `IRIUM_POAWX_FAIRNESS_MATRIX_ACTIVATION_HEIGHT`).
- **Synthetic role-claim builder** `build_synthetic_phase20_ext(...)`, gated by
  `IRIUM_POAWX_SYNTHETIC_ROLE_CLAIMS=1` (testnet/devnet-only, mainnet hard-off, disabled by
  default). Deterministic per-role nonce/secret; assigned lane via `assign_lane_id`; a verifying
  `role_claim_digest`; solver pkh from registered workers if supplied, else the primary miner pkh
  (MVP single-miner). **This is local/testnet-only for production-wiring validation — NOT the live
  hidden-precommit role-claim protocol, which remains pending. No hidden-precommit is claimed.**
  If production is active but synthetic claims are disabled, the pool attaches NO extension — it does
  not fake claims; the node then fails closed on the missing extension.
- **Canonical coinbase** (`build_native_rewardable_coinbase`): after activation + an ext-bearing
  receipt, emits `irx1 OP_RETURN` + PRIMARY/COMPUTE/VERIFY/SUPPORT p2pkh in fixed order with the
  55/22/13/10 split (remainder → PRIMARY; exact sum), OFFICIAL fee-0 (no fee output, no delegate
  output). Duplicate pkhs (MVP: all role pkhs == primary) stay separate. The irx1 root is the
  GATED root. The mining.notify split rebuilds the same bytes (18C invariant preserved).
- **Gated root** (`compute_receipts_root_from_pending_gated`) + the submit paths use it so the
  pool-committed root equals the node's; pre-activation it equals the legacy root (byte-identical).
- **Pre-activation unchanged:** legacy single-output (or mode-1) coinbase + legacy root; existing
  native_rewardable / delegation behavior untouched (keyed on `phase20_ext` presence).
- Node parity for the test: two pure node validators (`validate_phase20_production_payout`,
  `validate_poawx_coinbase_payout`) were made `pub` so the pool dev-test asserts the AUTHORITATIVE
  node validator accepts the pool-produced fixture.
- Tests: pool `phase20_mirror_wire_parity_vs_node`, `phase20_gate_mainnet_off_and_heights`,
  `phase20_synthetic_disabled_or_mainnet_returns_none`,
  `phase20_synthetic_builder_valid_and_node_validator_passes` (delegation);
  `phase20_gated_root_byte_identity_and_node_parity` (block);
  `phase20_native_coinbase_canonical_multi_role_official` + `phase20_preactivation_coinbase_is_legacy`
  (stratum). Pre-existing delegation/native_rewardable/wallet suites unchanged.

### Step 2 — connect_block / submit_block_extended enforcement + receipts-root: COMPLETE
- **Receipts-root inclusion (gated).** `irx1_root_from_block_receipts_gated(receipts, phase20_active)`
  (lib) and `compute_poawx_receipts_root_gated(receipts, phase20_active)` (iriumd) bind
  `Phase20ReceiptExt::digest()` into each receipt's inner hash **after** the optional mode-1
  delegation digest, **only when `phase20_active`**. The old public functions are thin wrappers
  (`..., false`), so every pre-activation / non-production caller is **byte-identical**. The hex
  pending `phase20_ext` is exactly `serialize()`, so the submit-path root equals the connect-path
  root. Mutating any extension field (role claim, RoleReward, fee_bps, fee_pkh) changes the root.
- **`connect_block` enforcement.** `validate_poawx_block_receipts` now recomputes the root with the
  gate and, after activation, runs `validate_phase20_production_block` (per receipt:
  `validate_phase20_production_payout` with PRIMARY = receipt `worker_pkh`, total = block subsidy,
  `prev_hash` = parent hash, `third_party_mode = third_party_fee_active && third_party_pool_mode_enabled`).
  Pre-activation it runs the legacy 10%/receipt floor check unchanged. A missing extension after
  activation **fails closed**.
- **`submit_block_extended` enforcement.** Uses the gated root for the irx1 commitment check and
  adds an early reject when production is active but a receipt is missing the extension; the
  authoritative validation remains `connect_block` (called from the handler).
- **Reject coverage** (all via the integrated validator): missing extension, bad role claim,
  RoleReward mismatch, wrong coinbase amount/order, hidden extra payout, fee output in official
  mode, fee without third-party mode, fee over the 200 bps cap, root/extension mismatch, and
  mainnet (hard-off — the gate is false, so enforcement never runs and the root stays legacy).
- **Coinbase-only assumption (documented).** The production payout check uses the block subsidy as
  the distributable total; the supported single-miner producer builds a coinbase-only block (no
  fee-bearing txs). Fee-aware totals for fee-bearing blocks are a follow-up (no such producer
  exists yet — pool production is out of scope here).

### Step 1 — receipt-wire / storage / P2P / reorg threading: COMPLETE

### Step 1 (this pass) — receipt-wire / storage / P2P / reorg threading: COMPLETE
- **Receipt wire (`PoawxBlockReceipt.phase20_ext: Option<Phase20ReceiptExt>`)** + a **present-only
  v3 receipt section** (`POAWX_RECEIPT_SECTION_MAGIC_V3`): a block uses v3 only when a receipt
  carries the extension; v1/v2 (mode-0/mode-1) blocks are **byte-identical** to before
  (`serialize_v3` = `serialize_v2` + a `0` flag when absent). Round-trips through block
  serialize/deserialize (the **P2P / binary-persist path**).
- **JSON persistence** (`storage::JsonPoawxReceipt.phase20_ext`, `write_block_json`) +
  **JSON reload** (`iriumd` block-load reconstruction) carry the extension hex (omitted when absent).
- **Pending receipt** (`iriumd PoawxPendingReceipt.phase20_ext`) + both mappers
  (`pending_receipt_to_block_receipt` / `block_receipt_to_pending`) preserve it, so **reorg
  rollback/reapply** keeps the extension (malformed → fail-closed, like delegation).
- **NOT enforced:** the extension is only preserved, never validated/required in this step; the
  receipts root is unchanged (root/digest inclusion + validation belong to the enforcement step).
- Tests: v3 element round-trip + byte-identity-when-absent (poawx); v3 block wire round-trip +
  old-block-no-v3-magic (block); reorg mapper preserves ext + plain→None (iriumd).

### Tests added in Step 2
- **poawx** `phase20_root_gating_and_mutation_sensitivity`: gate-off byte-identity (extension
  ignored == no-ext root == wrapper); gate-on differs and is deterministic; mutating role
  claim / RoleReward / fee_bps / fee_pkh each changes the gated-on root; malformed/truncated
  extension fails to deserialize.
- **iriumd** `phase20_gated_root_parity_pending_vs_block_and_byte_identity`: gate-off equals the
  legacy root; gate-on submit-path (pending) root equals connect-path (block) root; gate-on
  differs from legacy.
- **chain** `phase20_connect_block_production_enforcement`: valid Phase 20 block accepted;
  rejects bad role claim, RoleReward mismatch, wrong coinbase order, wrong amount, hidden extra
  payout, fee-without-mode; accepts third-party fee with fee gate + mode; rejects fee over cap;
  rejects missing extension after activation; mainnet hard-off skips enforcement.
- submit_block_extended handler accept/reject is exercised through the gated-root parity +
  the authoritative `connect_block` tests; a live running-node loopback E2E is **Step 5**.

> Mainnet hard-off for all three features. Chain difficulty automatic via LWMA-144. Local-only;
> not pushed. Hidden-precommit commitment root remains a separate PARTIAL (see fairness doc).

## What is implemented this pass (COMPLETE, tested, safe)
- **`Phase20ReceiptExt`** (`src/poawx.rs`) — the versioned production receipt extension carrying
  the three role claims (compute/verify/support), the `RoleReward` payout pkhs, and the signed
  third-party fee terms (`fee_bps` + `fee_pkh`). Canonical `serialize`/`deserialize` (length-
  prefixed claims) + `digest` + round-trip/truncation/unknown-version tests.
- **`validate_phase20_production_payout`** (`src/chain.rs`) — the INTEGRATED consensus validator
  that ties the existing primitives together (the future `connect_block` entry point):
  1. validates each role claim against the deterministic fairness assignment (slot 0 per role;
     wrong role/lane/height/prev/digest reject; distinct expected role_ids reject a duplicate
     claim for the same role);
  2. requires the `RoleReward` pkhs to equal the validated claim solver pkhs;
  3. validates fee terms (`validate_fee_terms`: official 0% / third-party cap 2% / mode / pkh);
  4. validates the canonical fee-aware multi-role coinbase (`validate_poawx_coinbase_payout`).
- **`phase20_production_active(height)`** — gate requiring both multi-role + fairness active
  (mainnet hard-off); third-party fee layered separately.

## Tests added (this pass)
poawx: `phase20_receipt_ext_wire_roundtrip` (round-trip, truncation, unknown version, digest
sensitivity). chain: `phase20_integrated_production_validator` (official accept; third-party-fee
accept; fee-without-mode reject; wrong role; tampered lane; RoleReward mismatch; wrong height;
coinbase tamper; fee-in-official reject; over-cap reject) and
`phase20_production_gate_requires_multirole_and_fairness_mainnet_off`. Plus all prior Phase 20
primitive/validator tests. Full suite green: lib poawx 45, phase20 23, reward 6 (single-thread),
wallet 420, stratum delegation 14, native_rewardable 6, fmt clean.

## Coverage of the requested test list
The integrated validator + extension cover, at the consensus-validation/wire-type level:
role-claim cases (11–18), coinbase cases (19–26), third-party fee cases (27–39), and the
extension round-trip portions of (48–49). The remaining items — wallet CLI (40–42), pool
identity/registry (43–47), and live persistence/P2P/reorg/observer (48–52 at the running-node
level) — depend on the live integration below.

## Remaining live integration (follow-up — NOT done; the bulk of A/C/D/E/F/G/H/I)
Each touches the validated Phase 18/19/19D code and is staged to avoid regressing it:
1. **Node receipt-wire threading** — ✅ **DONE (Step 1)**: `Phase20ReceiptExt` is carried in the
   present-only v3 receipt section through `iriumd` pending receipts, `storage` JSON persist/reload,
   reorg pending↔block mappers, and P2P block ser/de (data only, not enforced).
2. **connect_block / submit_block_extended** — ✅ **DONE (Step 2)**: `validate_phase20_production_payout`
   runs in `connect_block` when `phase20_production_active(height)`; the extension is bound into the
   receipts root; missing extension after activation fails closed; pre-activation Phase 18/19 blocks
   remain valid (byte-identical). submit path uses the gated root + early missing-ext reject.
3. **Pool production** — ✅ **DONE (Step 3, OFFICIAL fee-0)**: the stratum native_rewardable path
   builds the canonical multi-role coinbase + `Phase20ReceiptExt` + gated root after activation,
   using the gated synthetic role-claim builder (testnet/devnet-only). Third-party-fee block
   production is NOT done (Step 4).
4. **Role-claim source** — real claims from miners, or a clearly-named testnet/devnet-only
   `IRIUM_POAWX_SYNTHETIC_ROLE_CLAIMS=1` synthetic builder (mainnet-impossible). Not added yet.
5. **Wallet CLI** — ✅ **DONE (Step 4)**: `--third-party-pool` / `--fee-bps` / `--fee-pkh` on
   `poawx-register` + `--emit-only` (fee terms signed into the delegation; emit==online body).
6. **Pool registry** — ✅ **DONE (Step 4)**: `verify_and_store` accepts capped third-party fees that
   match the pool config + persists `fee_pkh`, gated on third-party mode + fee activation; rejects
   mismatch/mutation/over-cap (official mode still fail-closed on `fee_bps>0`).
7. **Observer + loopback smoke** — two-node + isolated `$TROOT` E2E (operator-approved, loopback).
   **(Step 5 — NOT done; submit_block_extended live handler accept/reject is covered here.)**

### Still NOT done after Step 4 (explicit)
- a LIVE (non-synthetic) role-claim protocol — Steps 3/4 use a gated testnet/devnet synthetic builder
- hidden-precommit commitment root (fairness matrix remains PARTIAL — assignment uses `prev_hash`,
  known at block time; a prior-block commitment root is required for true hidden-before-reveal)
- public/external miner test
- live loopback / two-node E2E (Step 5)

Mainnet remains disabled for all Phase 20 features; chain difficulty remains automatic via LWMA-144.

## Why staged (honest)
The live integration is a multi-thousand-line change across `iriumd` (~25k lines), `chain.rs`,
`stratum.rs`, `storage.rs`, `delegation.rs`, and the wallet — the exact paths the validated
trusted-miner flow depends on. Landing the consensus validator + extension first (this pass,
zero-regression) makes the live integration a smaller, reviewable, bisectable next step rather
than one risky mega-change. **Production wiring is therefore PARTIAL, not COMPLETE — not faked.**


## Phase 22B — true VRF decision package (PENDING)

True VRF remains **pending** (true VRF pending): `AssignmentProofV1` is a **placeholder**,
**mainnet hard-off**, **not mainnet-ready**, and **no homemade VRF** will be added. The
key-model + dependency decision (Option A secp256k1 ECVRF without OpenSSL, vs Option B a
separate audited sr25519/Ristretto VRF key, then Option C vendor + security review) is
captured in `docs/poaw-x-phase22b-true-vrf-decision-package.md`. No code/dependency/Cargo
change in Phase 22B (docs-only). PoAW-X is **not full blueprint-complete** until this VRF
decision is approved and implemented; no push, no mainnet, no audit/vote.

## Phase 22E — true-VRF E2E wiring (update)

Production wiring for `AssignmentProofV2` is complete (local-only, gated, mainnet hard-off):
wallet/miner emits the proof (`poawx-candidate-admission --secret-hex`, secret never echoed),
it is carried in the candidate admission and committed-admission root, the node validates at
ingest + block acceptance, and the pool fetches + bundles it into the Phase 20 ext AVR2
section (fail-closed; no VRF secret in the pool). Both official fee-0 and third-party fee
production paths pass with miner-supplied proofs. Not mainnet-ready (external security review
of `vrf_fun`/`secp256kfun` pending).
