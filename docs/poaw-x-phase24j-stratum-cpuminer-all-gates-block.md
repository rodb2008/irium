# PoAW-X Phase 24J — stratum cpuminer real all-gates block run (PARTIAL — PoW-tooling blocker)

**Status: PARTIAL. No real block was mined.** The full coordination + a live cpuminer session
were achieved, but **stock cpuminer does not produce valid Irium PoW**, so no share/block was
found. Wallet-path isolation (the Phase 24I lesson) was fixed and held throughout. Local-only;
not pushed; `main` untouched; loopback-only; isolated storage; mainnet hard-off. No fake, no
weakened gates, no synthetic fallback. Not production-ready; not mainnet-ready.

## Wallet-path isolation (Phase 24I lesson — fixed + proven)

All wallet commands ran with an isolated `HOME=/home/irium/irium-p24j-wallet-home`, so the
wallet store resolved to `…/irium-p24j-wallet-home/.irium/wallet.json`. Verified throughout:
**real `~/.irium/wallet.json` was never created** (absent at every check), and the isolated
`wallet.json` held the test identity. The stateless `poawx-*` emit commands (`--secret-hex`)
write no wallet file.

## What succeeded (live, isolated devnet, all gates)

- Fresh binaries (HEAD `479d353`); node up, isolated (`/home/irium/irium-p24j-node/{blocks,
  state}`); `/poawx/assignment` → HTTP 200 at genesis.
- **Single coordinated identity P** generated via the isolated wallet:
  address `A = Q7ugEAbmUSv1pw1mymSnfHPJZXMCMCxzxg`, pkh `P = 74beece1…57cc`; WIF exported and
  decoded to the raw secret; verified `finality member_pkh == P` and `address→pkh == P`.
- Node material (all solver=P, seed=genesis): **H=1 + H=2 candidate admissions** (3 roles each,
  true-VRF V2) + **SUPPORT finality vote (member=P)** → all **200 OK**; node cached **H1=3,
  H2=3, finality=1** under all gates.
- Pool launched **collected mode** (loopback, isolated, role protocol, pointed at node); **9/9
  role precommit/reveal accepted** (3 roles, solver=P, H=1 reveal + H=1/H=2 precommit).
- **cpuminer session established:** minerd connected, `subscribe` (cpuminer/2.5.1), `authorize`
  worker = `A` (→ session pkh P).

## What was NOT achieved — definitive PoW-tooling blocker

- minerd hashed **~900M sha256d hashes** (2 threads, ~60 s) against the easy devnet job target
  (`bits 207fffff`) and submitted **ZERO shares**. At that target millions of hashes should
  qualify — finding none means **stock cpuminer's sha256d-over-standard-header ≠ Irium's actual
  block PoW hashing**. Irium ships its own **`irium-miner`** (RPC-based, mines against the node
  directly), and the pool's stratum adapter is `native_rewardable_reserved` — i.e. Irium uses a
  custom mining scheme that stock cpuminer/minerd cannot satisfy.
- No valid share ⇒ no per-session collected ext built ⇒ no `submit_block_extended` ⇒ no block.
  Node height stayed 0.

### The remaining gap (precise)
A real mined all-gates block needs **a miner that produces valid Irium PoW through the pool's
stratum path** (so the pool can attach the per-session collected ext). The only Irium-compatible
miner today (`irium-miner`) mines via **RPC against the node**, which **bypasses the pool** and
thus the all-gates ext (the node's internal template does not build the PoAW-X ext — only the
pool does). Bridging this requires one of:
1. an Irium-PoW-compatible **stratum** miner (or a cpuminer patched to Irium's header/algo), or
2. building the all-gates ext on the **node's internal block template** so the RPC `irium-miner`
   path includes it, or
3. a custom mining harness that submits a valid PoW block with the ext to
   `/rpc/submit_block_extended`.

This is a mining-tooling/integration task, not a PoAW-X consensus gap (consensus validation of a
full all-gates block is already covered in-process by `chain::phase22e_true_vrf_e2e_block` + the
per-section tests).

## Cleanup

- minerd (timeout-exited), pool, and node stopped by exact PIDs (the node port-owner PID was
  identified via `ss` and confirmed to be `iriumd`, not mainnet 219530). No `pkill`/`killall`.
- Removed `/home/irium/irium-p24j-{node,pool,wallet,wallet-home}`; preserved
  `/home/irium/phase24j-all-gates-artifacts/` (logs, evidence, JSON) +
  `phase24i-all-gates-artifacts/pool.log` (the p24j pool logged there due to an artifact-path
  sed quirk; harmless).
- No p24j ports bound; **real `~/.irium/wallet.json` absent**; `~/.irium` block store intact (2
  pre-existing orphan dirs); mainnet 219530 + prod pool (4) alive; repo clean at `479d353`.

## Claim status

- **Real mined all-gates block accepted? NO.**
- **Production-candidate for controlled public testnet? NO.**
- **Mainnet production-ready? NO.**
- Allowed: wallet-path isolation fixed + proven; full coordinated all-gates material (single
  identity P) validated by the node + accepted by the pool's collected path; live cpuminer
  session established. The remaining blocker is mining-tooling (valid Irium PoW via stratum).

## Remaining blockers

- Mining tooling: an Irium-PoW-compatible stratum miner (or node-template ext-build for the RPC
  miner, or a custom submit harness).
- Cross-host P2P provider/firewall; independent audit; public testnet; governance/mainnet
  activation.
