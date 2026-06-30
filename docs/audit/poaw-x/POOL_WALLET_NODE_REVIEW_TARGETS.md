# PoAW-X pool / wallet / node review targets

## Wallet — `src/bin/irium-wallet.rs`

- **Proof emission:** `poawx-assignment-proof-v2`, `poawx-candidate-admission` (with optional
  `--secret-hex` to bind a V2 proof), plus `poawx-ticket-proof`, `poawx-assignment-proof` (V1),
  `poawx-puzzle-challenge/solve`, `poawx-finality-vote`, `poawx-committed-admission`. All
  emit-only; testnet/devnet; mainnet (network_id 0) disabled.
- **Secret non-leakage:** secrets are inputs (`--secret-hex`); never written to JSON, logs, or
  error strings; self-verify before emit. Review every print/format path for accidental
  inclusion. Tests assert the secret value is absent from output.
- **Submit path:** optional `--submit --node-rpc <loopback-url>` POSTs ONLY the public
  `wire_hex` to the node's loopback RPC; no secret sent; no default public posting.

## Node — `src/bin/iriumd.rs` + `src/p2p.rs` + caches

- **Loopback-only RPC:** `/poawx/candidate-admission` (POST), `/poawx/candidate-admissions`
  (GET), `/poawx/finality-vote` (POST), `/poawx/finality-votes` (GET) are guarded by a
  loopback bridge guard + gossip-enabled gate. Review that they cannot be reached off-loopback
  and are testnet/devnet only.
- **Caches:** `NodeCandidateAdmissionCache`, `NodeFinalityVoteCache` — validate → window →
  dedupe → store; pruned by height; bounded seen-set. Review window/dedupe/bounds.
- **P2P gossip:** `PoawxCandidateAdmission` (=28), `PoawxFinalityVote` (=29) — receive arms
  ingest (validate) + rebroadcast only when newly accepted; payload size-capped. Review
  validation-before-store and rebroadcast amplification.

## Pool — `pool/irium-stratum/src/delegation.rs` (+ `stratum.rs`, `block.rs`)

- **No VRF secret / no proving:** the pool has no `vrf_fun`/`secp256kfun` dependency; production
  code never proves (the only `AssignmentProofV2::prove` references are in `#[cfg(test)]` via
  the node dev-dependency). Confirm there is no path where the pool fabricates a proof.
- **Fetch / bundle / fail-closed:** `refresh_pool_admitted_cache` fetches admitted candidates;
  `decode_admission_v2` + `build_admitted_v2_proofs` + `build_pool_true_vrf_section` bundle the
  SELECTED candidates' proofs into the `AVR2` section; `build_synthetic_phase20_ext` /
  `build_collected_phase20_ext` FAIL CLOSED (emit no ext) if a selected role lacks a proof.
- **Byte-parity mirrors:** all consensus types are mirrored byte-for-byte; parity tests assert
  equality vs the canonical node types (e.g. `phase22d_pool_true_vrf_parity_and_failclosed`,
  `phase22e_pool_e2e_bundle_and_failclosed`). Review that mirrors cannot diverge silently.
- **Fees:** official fee-0 and third-party fee (cap 2.00%, fail-closed to 0% on invalid terms)
  both produce valid exts; review the fee split + that the node re-validates.
- **No bypass:** the pool cannot bypass node validation — the node re-verifies every section.

## Cross-cutting

- **No public RPC/port exposure** introduced by this work; loopback-only bridges.
- **Trust boundary:** node authoritative; pool/wallet untrusted producers; all producer output
  is re-validated by the node.

## Phase 24F update (genesis assignment harness fix)

Phase 24F found + fixed the exact live-block-production blocker: poawx_get_assignment returned
404 at tip_height==0, so a fresh devnet could not get an assignment for block 1 (the 14-F
genesis /poawx/assignment wall). Fix: serve the assignment at the genesis tip on
devnet/testnet only (mainnet + inactive still 503; connect_block/LWMA/difficulty untouched).
Live all-gates block production is now unblocked at the assignment layer (assembly + submit +
connect_block validate path is complete in code); a real cpuminer-mined all-gates block is
still not demonstrated, and cross-host P2P remains firewall-blocked. NOT production-ready, NOT
mainnet-ready. See docs/poaw-x-phase24f-pool-cpuminer-all-gates-harness.md.

## Phase 24H update (miner-pool coordination fix)

Phase 24H fixed the exact 24G blocker: build_synthetic_phase20_ext now derives per-role solvers
(and role_reward + the AVR2 lookup) from the node-validated admitted candidate set instead of
the pool primary_pkh, so role rewards + per-role V2 proofs key to the actual admitted miners;
fail-closed if any role lacks an admitted candidate. New helper pool_role_reward_from_admitted.
build_collected already derives role_reward from reveals (real path, unchanged). No pool-local
onboarding endpoint needed (node RPC suffices); pool holds no miner secret. Test
delegation::phase24h_role_reward_derived_from_admitted_candidates. Remaining for 24I: one
coordinated live run (matching admission/V2/ticket/finality-member/reveal/H+1 + dominance) +
live PoW mining. NOT production-candidate; NOT mainnet-ready. See
docs/poaw-x-phase24h-miner-pool-onboarding-coordination.md.

## Phase 24K update (Irium-native all-gates mining harness — block mined + connect_block-accepted)

Phase 24K closed the Phase 24J mining-tooling blocker IN-PROCESS. New mainnet-hard-off harness
`src/poawx_mining_harness.rs` (`guard_network`, `guard_isolated_storage`, `mine_pow` — grinds the
nonce via Irium's REAL `hash_for_height` + `meets_target`, never touches LWMA/difficulty). Two
deterministic tests in `src/chain.rs`: Stage 1 mines an all-gates block and every authoritative
validator accepts it (+ E13–E17 negatives); Stage 2 drives the FULL `connect_block` to acceptance,
advancing the chain to height 2 (real PoW, gated `irx1` root, production payout, dominance,
candidate set + admission, puzzle, finality, committed admission, true-VRF, 0%-fee coinbase).
Mainnet hard-off (devnet `network_id=2`); no validator weakened; no PoW bypass; hidden-precommit /
ticket-proof / mode-1 delegation gates left off as independent (separately tested). Still NOT
production-candidate / mainnet-ready: a live cross-host run (Phase 24L), independent audit, public
testnet, and governance activation remain. See
docs/poaw-x-phase24k-irium-native-mining-submit-harness.md.
