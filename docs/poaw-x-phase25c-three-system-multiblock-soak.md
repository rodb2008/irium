# PoAW-X Phase 25C — three-system multi-block devnet soak

**Status:**
- **Single-block three-system propagation: PASSED.** One real Irium-native-PoW all-gates
  PoAW-X block was built + submitted by the CLI live-proof harness, accepted by a real devnet
  node, and observed at the same height + hash across **Windows + VPS-1 + VPS-2**.
- **Multi-block (≥5 blocks) soak: BLOCKED / NOT MET.** A second block (height 2) cannot be made
  valid under the all-gates-required configuration because of a **node-level (consensus)
  contradiction between two REQUIRED gates** (phase21d candidate-set seed vs phase22a
  committed-admission freeze seed). Documented below. No gate was weakened and no consensus
  logic was changed.

NOT production-ready. NOT mainnet-ready. NOT audited.

## Systems / branch / HEAD

- Windows `C:\Users\Ibrahim` (submitter + originating node; mainnet PID `33752` untouched).
- VPS-1 hub `irium@207.244.247.86` (`vmi2780294`; mainnet `219530` + prod pool untouched).
- VPS-2 observer `irium@157.173.116.134` (`vmi2995746`; mainnet `1851441` untouched).
- Branch `testnet/poawx-phase20-blueprint-completion-local` @ **`0f82616d5c9af7e7d96a026614fbdb40df7220ce`**
  — verified checked out and built on all three. `origin/main` unchanged at `19c496d…`.
- Windows repo dir used: `C:\Users\Ibrahim\irium-poawx-windows-test` (the prompt's
  `irium-poawx-three-system-test` did not exist; the existing clone on the same branch was used).
  The two HEADs differed only by docs (`1ca7d89..0f82616` = 7 `.md` files, no `.rs`/`.toml`), so
  the already-present Windows release binaries are source-equivalent to the target HEAD.

## Firewall (Windows egress IP changed)

- Windows egress IP **changed**: Phase 25B `122.162.148.238` → Phase 25C **`122.162.151.91`**
  (dynamic home-ISP address). Operator was asked before any UFW change.
- VPS-1 host UFW updated (reuse port `41210`, source-restricted, no broad rules):
  - removed the stale rule for the old Windows IP `122.162.148.238`;
  - added `allow from 122.162.151.91 to any port 41210 proto tcp` (Windows);
  - kept `allow from 157.173.116.134 to any port 41210 proto tcp` (VPS-2).
  - sudo was used only for these `ufw` commands via `sudo -S` over SSH stdin; the password was
    never printed, echoed, written to a file/log/doc, or committed.
- Reachability verified with a temporary auto-exit listener on VPS-1 `0.0.0.0:41210`:
  - VPS-2 → VPS-1:41210 → `OK`;
  - Windows → VPS-1:41210 → `TcpTestSucceeded : True`.
  - The probe listener auto-exited (30s) and was confirmed gone; 41210 left free until the node bound it.
- These UFW rules were **left in place** at cleanup (per operator instruction). The Windows IP is
  dynamic and may need re-adding for any future run.

## Ports (RPC loopback-only everywhere)

| System  | P2P              | RPC               | Status            |
|---------|------------------|-------------------|-------------------|
| VPS-1   | `0.0.0.0:41210`  | `127.0.0.1:41311` | `127.0.0.1:41308` |
| VPS-2   | `0.0.0.0:41320`  | `127.0.0.1:41321` | `127.0.0.1:41318` |
| Windows | `127.0.0.1:41330`| `127.0.0.1:41331` | `127.0.0.1:41328` |

No public RPC anywhere; no stratum opened; no UDP; no `0.0.0.0/0`; only TCP 41210 cross-host
(source-restricted). Windows P2P bound loopback (dialer-only behind NAT). Spokes dial the hub via
`IRIUM_ADDNODE=207.244.247.86:41210`.

## Storage (isolated; no default path; no `/tmp`)

- Windows: `C:\Users\Ibrahim\irium-poawx-phase25c\node\{data,blocks,state}` (banner-confirmed).
- VPS-1:   `/home/irium/irium-p25c-vps1-node/{data,blocks,state}` (banner-confirmed).
- VPS-2:   `/home/irium/irium-p25c-vps2-node/{data,blocks,state}` (banner-confirmed).
- Nodes started from their repo cwd so `./bootstrap/trust/allowed_anchor_signers` resolves; storage
  stayed isolated via explicit `IRIUM_DATA_DIR` / `IRIUM_BLOCKS_DIR` / `IRIUM_STATE_DIR`.

## Build results

- All three built `--release --bin iriumd --bin poawx-live-proof-harness` at `0f82616`, exit 0
  (cargo on PATH via `~/.cargo/bin`). VPS binaries present; Windows binaries source-equivalent.

## Three-system mesh (peer proof)

- All three started at genesis height 0, devnet genesis
  `0000000028f25d65557e9d8d9e991f516c00d68f5aeae10b750645b398bd10a3`.
- Hub `peers=2` (unique_ips=2), VPS-2 `peers=1` (dialed hub, handshake OK), Windows `peers=1`
  (dialed hub). RPC loopback-only on all three; mainnet PIDs alive throughout.

## Block 1 — PASSED (the three-system propagation proof)

- Origin: **Windows harness → Windows node** (`http://127.0.0.1:41331`), Irium-native PoW, no stock
  cpuminer. Harness run with the full gate env so the builder's `default_profile()` /
  finality / activation params match the node (without it the puzzle proof digest mismatches).
- Node response `{"accepted":true,"height":1,"tip":"5d417b86…15fb"}`; before height **0** → after **1**.
- **Block hash** `5d417b86989244758254ba86d482160f8490cc47efcbbee4c72c031aafd215fb`.
- **irx1 root** `772e1cd700af122e5bc2a586a1eb94d4dc33bdd2ab819dba435df9875c7ed9bd`.
- Official fee **0%** (no fee output); all-gates sections present (candidate_set, candidate_admission,
  committed_admission, true_vrf/AVR2, role_puzzle_proofs, finality_proof, role_dominance_weights).
- **Propagation:** all three nodes reached height **1** with identical tip
  `5d417b86…15fb` (Windows originator → VPS-1 hub → VPS-2). Confirmed via each node's loopback
  `/status` (`best_header_tip.hash`).

## Block 2 — REJECTED (the multi-block blocker)

Attempted origin: VPS-1 harness → VPS-1 node (height 2). Node rejected with:

1. First: `phase21c: dominance weight mismatch role 0 got 1000 expected 645`.
   - Root cause: the harness builder computes anti-domination weights from a **fresh**
     `PersistentDominance::from_env()`, so it always emits genesis-relative weights (1000). The
     node's dominance state correctly **evolves** after block 1 (each block credits its role
     rewards), so at height 2 it expects the evolved weights.
   - A devnet-only, gate-preserving builder fix was implemented and **verified in isolation**
     (replay each prior height's reward events into the builder's dominance tracker before
     computing weights; height-1 path byte-identical). With the fix, **H1 connected and the
     dominance gate was satisfied at H2** — proving the dominance fix is correct. (The fix was
     subsequently **reverted**, not committed; preserved as
     `irium-poawx-phase25c/artifacts/dominance-replay-fix-UNUSED.patch`.)

2. After the dominance fix, H2 then failed deeper:
   `phase22a: candidate set does not match committed admission root`.

### The definitive multi-block blocker — a node-level seed contradiction

For any block at height `H ≥ 2`, two REQUIRED gates impose contradictory requirements on the
block's candidate-set freeze seed:

- **phase21d (candidate-set gate)** — `validate_block_candidate_set` requires
  `candidate_set.seed == block.header.prev_hash`. For H2 that is `hash(H1)`.
- **phase22a (committed-admission gate)** — `validate_block_committed_admission` requires
  `candidate_set.seed == parent_commitment.seed`, and the parent's commitment freeze seed is
  pinned to the **committing (parent) block's `prev_hash`**
  (`ca.seed == block.header.prev_hash` on the producing side;
  `pc.seed == prev.header.prev_hash` on the consuming side). For H1's commitment that seed is
  H1's prev_hash = **genesis**.

So H2's candidate set would need `seed == hash(H1)` (phase21d) **and** `seed == genesis`
(phase22a) simultaneously — impossible. Observed live: H2 **passed** phase21d (seed = `hash(H1)`)
and then **failed** phase22a (parent commitment seed = genesis). Reversing it to satisfy phase22a
would break phase21d.

This is **consensus / gate-semantics level**, not a harness bug, and explains why every prior
phase (24K / 24L / 25B) only ever produced a single block over genesis. Reaching ≥5 blocks
requires a separate consensus-design phase to reconcile the phase21d ↔ phase22a freeze-seed
semantics — which would be a consensus change and was explicitly out of scope here. No gate was
relaxed, no activation height was staggered, and no seed semantics were changed.

## Restart/resync & optional pool

Not reached — the multi-block soak was stopped at the H2 blocker before the restart/resync step.
The optional VPS-1 pool was not run (optional; success never depended on it).

## Cleanup proof

- All three Phase 25C nodes stopped by **exact pidfile PIDs** (no pkill / no killall):
  Windows `35628`, VPS-1 `4097477`, VPS-2 `2076508` — all confirmed STOPPED.
- All Phase 25C ports closed: Windows `41330/41331/41328`, VPS-1 `41210/41311/41308`,
  VPS-2 `41320/41321/41318`.
- Mainnet/prod alive and untouched: Windows `33752`, VPS-1 `219530`, VPS-2 `1851441`; VPS-1 prod
  pool (`irium-pool-api` + `irium-stratum` workers) alive.
- Default storage untouched (all predate this run): Windows `%USERPROFILE%\.irium` (2026-06-07),
  VPS-1 `~/.irium` (2026-06-21), VPS-2 `~/.irium` (2026-06-06).
- Local Windows working tree restored to `0f82616` (the dominance fix reverted, not committed).
- Artifacts preserved: `irium-poawx-phase25c\artifacts` (Windows), `phase25c-artifacts-vps1`,
  `phase25c-artifacts-vps2`.

## Claim status

- Phase 25C single-block three-system propagation: **PASSED** — Windows, VPS-1, and VPS-2
  participated in a live PoAW-X devnet where a real Irium-native-PoW all-gates block was submitted
  to a real node, accepted, and propagated across all three.
- Phase 25C multi-block (≥5) soak: **BLOCKED / NOT MET** — phase21d/phase22a cross-block seed
  contradiction.
- Production-ready: **NO.** Mainnet-ready: **NO.** Audited: **NO.**

## Remaining blockers / next phase

- **Multi-block:** a consensus-design phase to reconcile the phase21d candidate-set seed and the
  phase22a committed-admission freeze seed so a multi-block all-gates chain is satisfiable without
  weakening either gate. (The dominance-replay builder fix is a necessary but not sufficient
  prerequisite, preserved as an artifact.)
- Independent audit; public testnet; governance / mainnet activation.
