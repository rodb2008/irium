# PoAW-X Phase 24I — coordinated single-VPS live mined all-gates block (PARTIAL)

**Status: PARTIAL. A real mined all-gates block was NOT produced.** Significant live progress
was made and a minor `~/.irium` incident occurred and was remediated (below). Local-only; not
pushed; remote branch absent; `main` untouched. Loopback-only; isolated `$HOME`-rooted storage;
mainnet hard-off. No fake, no weakened gates, no synthetic fallback. Not production-ready; not
mainnet-ready; does not replace external audit.

## What succeeded (live, isolated devnet, all gates)

- **Fresh binaries** rebuilt from HEAD `d8ea027` (pool with the 24H fix; iriumd with the 24F
  fix).
- **Node up, isolated** (`/home/irium/irium-p24i-node/{blocks,state}`); `/poawx/assignment`
  returns **HTTP 200 at genesis** (24F).
- **Coordinated miner material, single identity P** (`P = fc7250a2…cef =
  hash160(pubkey(K))` = the SUPPORT solver / finality member):
  - candidate admissions for **H=1 and H=2** (3 roles each, solver = P, with true-VRF V2),
    submitted to the node → **200 OK**; node validated under all gates and cached **3 (H=1) + 3
    (H=2)**.
  - SUPPORT finality vote (`member_pkh == P`) → **200 OK** → validated + cached.
- **Pool launched in COLLECTED mode** (loopback `127.0.0.1:40812`, admin bridge `:40813`,
  metrics `:40814`, isolated dirs, all gates, role protocol on, pointed at node RPC): delegate
  signer key auto-loaded (pool holds **no miner secret**), SSE connected, building jobs for
  height 1 (prev = genesis).
- **Role precommit/reveal accepted:** 9/9 — precommit+reveal (H=1) + precommit (H=2) for all 3
  roles (solver P) POSTed to the pool's loopback `/poawx/role-precommit` + `/poawx/role-reveal`
  → all `{"status":"accepted"}`.

## What was NOT achieved (honest)

**No real cpuminer-mined, node-accepted all-gates block.** Key finding: the pool builds the
PoAW-X receipt/ext **per miner session** (keyed to the share submitter's pkh), not from the
template loop — so a **stratum miner must be connected** for `build_collected` to run and attach
the ext (until then the pool logs "no receipts_root … legacy submit", as observed). Irium's PoW
is **sha256d** and the devnet job target is easy (`bits 207fffff`), so mining itself is fast;
the remaining steps were: connect a stratum cpuminer (session) → pool builds the per-session
collected ext → mine → `/rpc/submit_block_extended` → `connect_block` validate all sections.
The cpuminer step was **not run** (see the incident below; stopped per the `~/.irium` rule).

## Incident: stray `~/.irium/wallet.json` (minor, remediated)

While generating a miner address, `irium-wallet new-address` defaulted its wallet path to the
storage runtime root, which is `~/.irium` — so it **created `~/.irium/wallet.json`** (1000 B, 3
throwaway devnet addresses). This touched `~/.irium`, which is forbidden.
- **Impact:** the operator's real wallets (`irium-wallet.json`, `wallet.core.json`) were
  **untouched** (old mtimes); the mainnet node does not hold `wallet.json` open and stayed alive;
  the `~/.irium` block store was intact (36,532 blocks, only the 2 pre-existing orphan dirs).
- **Remediation:** per the rules, stopped immediately, reported, and removed the stray
  `wallet.json` I created (no prior `wallet.json` existed; the operator uses differently-named
  wallets), restoring the prior state.
- **Root cause + lesson:** the wallet CLI's **stateful** commands (`new-address`,
  `list-addresses`) use the storage root for the wallet file and default to `~/.irium`. Unlike
  the Phase 24B/24C blocks-dir case, the 24C fail-closed guard does **not** catch this because
  `~/.irium` IS under `$HOME` (a valid configured path) — it's the *default*, not an invalid
  path. **Future hardening:** for any wallet CLI use in a test/dev rehearsal, set an explicit
  isolated `IRIUM_DATA_DIR`/wallet path under `/home/irium/irium-p24X-wallet`, and/or have the
  wallet CLI warn/refuse to use `~/.irium` when a test/dev rehearsal env is active. (The
  stateless `poawx-*` emit commands take `--secret-hex` and write no wallet file — they were
  safe.)

## Cleanup

- Pool stopped by pidfile; the node stopped by its exact PID (the pidfile had raced empty, so
  the port owner PID was identified via `ss` and confirmed to be the p24i node, not mainnet
  219530, before `kill`). No `pkill`/`killall`.
- Removed `/home/irium/irium-p24i-{node,pool,wallet}`; preserved
  `/home/irium/phase24i-all-gates-artifacts/` (node + pool logs, assignment/admission/finality
  JSON, role precommit/reveal evidence).
- No p24i ports bound; `~/.irium` intact (stray `wallet.json` removed); mainnet 219530 + prod
  pool (4) alive; repo clean at `d8ea027`.

## Claim status

- **Real mined all-gates block accepted? NO.**
- **Production-candidate for controlled public testnet? NO.**
- **Mainnet production-ready? NO.**
- Allowed: 24F genesis assignment live; full coordinated node material (solver-aligned H=1/H=2
  admissions + finality member=P) validated under all gates; pool runs collected mode and
  accepts the role precommit/reveal + builds jobs; the 24H role-solver coordination is in place.

## Remaining for a real mined block

1. Connect a stratum cpuminer (sha256d) session so the pool builds the **per-session** collected
   ext (the ext is keyed to the miner's pkh); verify the producer attaches the all-gates ext
   (`receipts_root` present / "COLLECTED … ext attached").
2. Mine the easy devnet target → `/rpc/submit_block_extended` → iterate any `connect_block`
   binding mismatches (finality `block_hash`, committed-admission seed, dominance weights vs
   genesis state, fairness lane, hidden-precommit grace).
3. Use an explicit isolated wallet path for all wallet CLI use (lesson above).
- Plus the standing blockers: cross-host P2P provider/firewall; independent audit; public
  testnet; governance/mainnet activation.
