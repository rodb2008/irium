# PoAW-X Phase 24E — two-VPS production-candidate validation (PARTIAL)

**Status: PARTIAL. NOT production-ready; NOT mainnet-ready.** Local-only; not pushed; remote
branch absent; `main` untouched. Cross-host P2P was attempted but **blocked at the
firewall/provider layer**; a single-host loopback admission/finality demo was run instead.
External audit is still missing and a public testnet is still pending. This phase makes **no**
cross-host, block-production, or mainnet-readiness claim.

## Cross-host P2P — attempted, blocked (no claim)

The operator opened an OS `ufw allow from 157.173.116.134 to port 40610` rule on VPS-1. Despite
that:
- Node A was listening on `0.0.0.0:40610` (verified via `ss`); loopback connect worked.
- VPS-2 egress IP = `157.173.116.134` (matches the rule).
- **VPS-2 → VPS-1:22 (SSH) succeeded** (baseline IPv4 path OK), but **VPS-2 → VPS-1:40610 timed
  out** (dropped) — repeatedly.

Conclusion: port 40610 is dropped at a firewall layer even though `:22` from the same source is
open and node A is listening — most likely a **provider-level firewall/security group** that
also needs `40610/tcp` opened from VPS-2 (or a ufw ordering issue). This is operator-only to
resolve; the agent did not run sudo/ufw or touch any provider firewall. **No cross-host P2P or
cross-host gossip validation is claimed.**

## Single-host loopback demo (VPS-1) — what was run

With node A running under **all PoAW-X gates** (devnet, isolated `$HOME`-rooted storage):
- Wallet (VPS-1, isolated path, throwaway devnet secret) generated: **ticket proof**,
  **AssignmentProofV2** (real secp256k1 RFC 9381 ECVRF; `assignment_proof_digest` = VRF output,
  public key derived from the secret), a **V2-bound candidate admission** (`true_vrf:true`), and
  a **member-signed finality vote**.
- **Candidate admission submitted to node A's loopback RPC → HTTP 200 OK**; node A's
  `GET /poawx/candidate-admissions?target_height=1` returns the admission wire ⇒ node A
  **validated** (true-VRF verify + role/solver/ticket/assignment-key/seed/output binding, under
  all gates) and **cached** it.
- **Finality vote submitted to node A's loopback RPC → HTTP 200 OK**; node A's
  `GET /poawx/finality-votes?target_height=1` returns the vote wire ⇒ validated + cached.
- **Secret never leaked** (0 emitted JSON files contain the secret).

### Honest scope of the loopback demo
- A **second local node (node C)** was started and pointed at node A over `127.0.0.1:40610`,
  but **no P2P peer link formed**: the node **filters same-host peers** (anti-self-connection;
  any address in the host's own IP set, including loopback, is dropped — `filtered_local`). This
  is a structural property, separate from the cross-host firewall block.
- Therefore what was validated is the **admission/finality ingest → validation → cache** path on
  a live all-gates node via the loopback RPC bridge (the *same* `validate → window → dedupe →
  store` logic the P2P gossip-receive path uses), **not** node-to-node P2P gossip transport.

## Claims

**Allowed (validated this phase):**
- Single-host, live, all-gates **admission + finality ingest/validation/cache** via the loopback
  RPC bridge (true-VRF V2 admission accepted + cached; member-signed finality vote accepted +
  cached).
- Phase 24C **storage isolation remained safe** — node A, node B (VPS-2), and node C all used
  only `/home/irium/irium-p24e-*` dirs; **no process touched `~/.irium`** on either host.
- The wallet VRF **secret was never leaked**.

**NOT claimed (not validated):**
- cross-host P2P; cross-host gossip; node-to-node P2P gossip (any host);
- all-gates block production; official fee-0 live block; third-party fee live block;
- observer block validation; restart/reload block validation.

## Setup / transfer

- VPS-2 received the local branch via **git bundle (no push)**: `irium-p24e.bundle`
  sha256 `5f960754245b881fa73337b176925bff588426d11152dc3ecaf9d1d2a7cf60b7`, cloned to
  `/home/irium/irium-p24e-src` (HEAD `31680ec`), release-built (13m23s).
- Node A (VPS-1) + node B (VPS-2) + node C (VPS-1) all launched with verified isolated
  `$HOME`-rooted storage banners and all gates.

## Cleanup

- node A / node C stopped by exact pidfiles (VPS-1); node B stopped by exact pidfile (VPS-2);
  no `pkill`/`killall`; mainnet PIDs 219530 / 1851441 untouched.
- Removed `/home/irium/irium-p24e-nodeA`, `irium-p24e-nodeC`, `irium-p24e-wallet` (VPS-1);
  `irium-p24e-nodeB`, `irium-p24e-src` (VPS-2). Preserved `phase24e-all-gates-artifacts/` on
  both hosts (logs, bundle checksum, evidence files).
- No Phase24E ports bound on either host; no new `~/.irium` orphan dirs; both mainnets + VPS-1
  prod pool (4 workers) alive.
- **Operator close handoff still required** for the leftover OS ufw 40610 rule (the agent does
  not run ufw): `sudo ufw delete allow from 157.173.116.134 to any port 40610 proto tcp`.

## Remaining blockers / status

- **Cross-host P2P:** provider/firewall layer must open `40610/tcp` from VPS-2 (operator).
- **Full all-gates live block production:** needs the pool/stratum + cpuminer harness with
  `/poawx/assignment` seeding (`iriumd` has no internal miner; 14-F harness wall). Not attempted.
- **Independent audit:** missing. **Public testnet:** pending. **Governance / mainnet
  activation:** pending.
- **Production-candidate for controlled public testnet?** Not established by this phase — the
  cross-host and live-block portions did not complete. The all-gates consensus behavior remains
  exercised in-process by the test suite (e.g. `chain::phase22e_true_vrf_e2e_block`), and live
  single-host admission/finality validation now also passed; but the cross-host + block-
  production validation is still outstanding.
- **NOT production-ready. NOT mainnet-ready.**

## Phase 24F update (genesis assignment harness fix)

Phase 24F found + fixed the exact live-block-production blocker: poawx_get_assignment returned
404 at tip_height==0, so a fresh devnet could not get an assignment for block 1 (the 14-F
genesis /poawx/assignment wall). Fix: serve the assignment at the genesis tip on
devnet/testnet only (mainnet + inactive still 503; connect_block/LWMA/difficulty untouched).
Live all-gates block production is now unblocked at the assignment layer (assembly + submit +
connect_block validate path is complete in code); a real cpuminer-mined all-gates block is
still not demonstrated, and cross-host P2P remains firewall-blocked. NOT production-ready, NOT
mainnet-ready. See docs/poaw-x-phase24f-pool-cpuminer-all-gates-harness.md.
