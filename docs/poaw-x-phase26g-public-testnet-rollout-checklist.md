# PoAW-X Phase 26G — Public Testnet Rollout Checklist

Operational checklist for a future, separately-approved public testnet. **Docs-only; do not launch
without explicit approval.** Pair with the readiness doc, risk register, and operator runbook.
**Production-ready: NO. Mainnet-ready: NO. Audited: NO.**

Legend: each item is a gate to confirm `[ ]` before proceeding.

## 1. Branch / HEAD verification
- [ ] All operators on branch `testnet/poawx-phase20-blueprint-completion-local`.
- [ ] `git rev-parse HEAD` == the agreed baseline (currently `c15c436`) on every node.
- [ ] `git ls-remote origin main` == `19c496dc5f2fa08981a109b10eeb257105c28c43` (unchanged).
- [ ] Clean working tree; release build reproduces (`cargo build --release --bin iriumd --bin poawx-live-proof-harness`).

## 2. Activation env
- [ ] `IRIUM_NETWORK=testnet` (NOT mainnet; `network_id == 1`).
- [ ] `IRIUM_POAWX_MODE=active`, `IRIUM_POAWX_ACTIVATION_HEIGHT=1`.
- [ ] Gate env consistent on EVERY node and harness:
      `IRIUM_POAWX_PUZZLE_DIFFICULTY_BITS`, `IRIUM_POAWX_PUZZLE_BITS`,
      `*_MULTI_ROLE_REWARD_*`, `*_FAIRNESS_MATRIX_*`, `*_ANTI_DOMINATION_*`,
      `*_CANDIDATE_SET_*`, `*_ASSIGNMENT_PROOF_*`, `*_CANDIDATE_ADMISSION_*`,
      `*_PUZZLE_WORK_*`, `*_FINALITY_COMMITTEE_*`, `*_FINALITY_THRESHOLD_NUM/DEN`,
      `*_COMMITTED_ADMISSION_*`, `*_TRUE_VRF_*` (activation heights + required flags).
- [ ] No env that disables/weakens a gate or stages activation to bypass a gate.
- [ ] No change to PoW/LWMA/difficulty/target/reward env.
- [ ] Gate env is identical between each node and the harness that submits to it (mismatch ⇒
      validation failures, e.g. puzzle/seed mismatches).

## 3. Isolated storage
- [ ] `IRIUM_DATA_DIR`, `IRIUM_BLOCKS_DIR`, `IRIUM_STATE_DIR` set to per-node isolated dirs.
- [ ] None is `/tmp`, `~/.irium`, `%USERPROFILE%\.irium`, or a symlink to a default.
- [ ] Storage banners confirm the isolated dirs on startup.
- [ ] `candidate_admissions.dat` will live under the data root (26D); confirm the data root is
      isolated and writable.
- [ ] Real wallets/keys/config are NOT under any test dir.

## 4. Ports / firewall
- [ ] RPC loopback-only on every node (`IRIUM_NODE_HOST=127.0.0.1`).
- [ ] Status server loopback-only (`IRIUM_STATUS_HOST=127.0.0.1`).
- [ ] Cross-host P2P only on agreed source-restricted ports (`IRIUM_P2P_BIND` per node).
- [ ] Firewall allows P2P inbound ONLY from the agreed peer source IPs (no `0.0.0.0/0`, no all-ports,
      no UDP, no RPC/stratum exposure).
- [ ] Dynamic-IP participants: a documented re-scoping plan; verify current egress IP before launch.
- [ ] Reachability verified with a temporary, auto-exiting listener (then confirmed closed) — no
      persistent open test ports beyond the node's own.

## 5. Node topology
- [ ] Topology agreed (e.g. hub + spokes, or mesh) and documented.
- [ ] Spokes dial reachable peers via `IRIUM_ADDNODE=<host>:<p2p-port>` (source-restricted).
- [ ] Each node reports the expected peer count after handshake.
- [ ] All nodes share the same genesis hash.

## 6. Miner / harness
- [ ] Mining is Irium-native only (`poawx-live-proof-harness` or the node's native path).
- [ ] NO stock cpuminer/minerd anywhere.
- [ ] Harness runs with `IRIUM_NETWORK` matching the node and the FULL gate env (so the builder's
      `default_profile()`/finality/activation match the node).
- [ ] Harness work-dir is an explicit isolated dir (not a default).
- [ ] No private keys/secrets printed, stored, or committed.

## 7. Sync / cold-restart
- [ ] Incremental sync verified (a 1-block-behind node converges quickly).
- [ ] Restart / keep-storage: node reloads `candidate_admissions.dat`, re-validates, and rebuilds to
      the tip from disk (26D).
- [ ] Fresh-wipe: a fully-wiped node receives served historical admissions and syncs from scratch to
      the tip (26E); confirm tip/irx1 match peers.
- [ ] Note expected initial getblocks stall window before handshake-push delivery (~30–45 s on
      devnet); flag if it exceeds an agreed threshold at scale.

## 8. Observability / log collection
- [ ] Each node writes logs to its isolated artifacts dir; pidfile recorded.
- [ ] Collect (no secrets): height/tip/irx1, peer counts, sync-stall events, getblocks served/recv,
      admissions ingested/rejected (by reason), re-broadcast counts, propagation latency.
- [ ] Logs are summarized for any shared report; raw logs with machine-private data are NOT shared.
- [ ] No sudo passwords, private keys, wallet data, or credentials in any collected artifact.

## 9. Cleanup
- [ ] Stop ONLY testnet nodes by exact pidfiles/PIDs (no pkill/killall).
- [ ] Verify all testnet ports closed.
- [ ] Verify mainnet/prod processes + production pool still alive and untouched.
- [ ] Verify default storage (`.irium`) untouched (mtime predates the run).
- [ ] Leave firewall rules as agreed (remove temporary rules only if explicitly decided).
- [ ] Preserve artifacts (summarized, no secrets).

## 10. Post-test report
- [ ] Branch/HEAD used; per-node final height/tip/irx1; convergence confirmation.
- [ ] Block list with hashes + irx1 roots and originating operator.
- [ ] Restart and fresh-wipe sync results.
- [ ] Metrics summary + rejection-reason breakdown.
- [ ] Any abort/incident and its handling.
- [ ] Cleanup proof + mainnet-untouched proof + default-storage-untouched proof.
- [ ] Explicit claim status: testnet only — NOT production-ready / mainnet-ready / audited.
