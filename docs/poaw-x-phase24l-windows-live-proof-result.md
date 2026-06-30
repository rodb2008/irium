# PoAW-X Phase 24L Windows Local Live Proof Result

**Status: PASSED.** The Windows local devnet live proof ran end-to-end on the user's Windows
machine: a real Irium-native-PoW all-gates block was built, submitted to a real local devnet node
over RPC, and **accepted**, advancing the chain from height 0 to height 1. Local devnet only;
loopback only; mainnet and the real wallet untouched. Not production-ready / mainnet-ready /
audited.

## Branch / HEAD

- Remote branch: `testnet/poawx-phase20-blueprint-completion-local`
- Post-fix HEAD: `1ca7d89d445afdb5c3323e6a19215a262013e8e5`

## Fixes that made the Windows proof pass (two genuine bugs)

- `cef587d fix: preserve Windows drive prefix in storage guard` — `normalize_under` returned
  `None` on `Component::Prefix`, so the Phase 24C storage guard rejected EVERY absolute path on
  Windows (the `C:` drive prefix), which would force the forbidden `~/.irium` default. The prefix
  is now preserved; the caller's `starts_with(home)` check still confines the path under HOME.
  Unix is unaffected (no `Prefix` component).
- `1ca7d89 poawx: initialize header activation in live proof harness` —
  `build_devnet_all_gates_block` now resolves the standard-header activation height into the
  process global the same way the node does at startup. A standalone live-proof binary has no
  `ChainState` to set it, so the global fell back to the mainnet constant (22_888) and height-1
  headers hashed pre-activation, mismatching the node (devnet/testnet resolve it to 1). Idempotent
  `OnceLock`; no LWMA/difficulty/target/PoW change.

## Windows environment

- Repo: `C:\Users\Ibrahim\irium-poawx-windows-test`
- Proof root (isolated, under `%USERPROFILE%`): `C:\Users\Ibrahim\irium-poawx-live-proof`
- Toolchain: git 2.50.1, rustc 1.93.0, cargo 1.93.0

## Build (passed)

`cargo build --release --bin iriumd --bin poawx-live-proof-harness` — succeeded.

## Tests (passed, on Windows)

- harness bin: **5/0**
- lib poawx: **136/0**
- lib `phase24l_lib_builder_connect_block`: **1/0**
- `cargo fmt -- --check`: clean

## Live proof rerun (passed)

`powershell -ExecutionPolicy Bypass -File scripts\windows\poawx-live-proof.ps1` — exit **0**.

- node accepted the block (`{"accepted":true,"height":1,...}`)
- before height: **0**
- after height: **1**
- block hash: `31df881052b05dc6319c5915ca938b282df60ab7e823aba44ee5edd20dfd23bf`
- irx1 root: `772e1cd700af122e5bc2a586a1eb94d4dc33bdd2ab819dba435df9875c7ed9bd`
- official fee: **0%** (no fee output)
- all-gates sections present: candidate_set, candidate_admission, committed_admission,
  true_vrf(AVR2), role_puzzle_proofs, finality_proof, role_dominance_weights,
  multi_role_reward_0pct_fee

## Isolation / safety (verified)

- Windows mainnet node **untouched**: PID 33752 (`…\Irium Core\iriumd.exe`), listening on
  38291/38300/8080 — pre-existing, left running, not part of the proof.
- `%USERPROFILE%\.irium` **untouched**: `wallet.json` (2418 B @ 2026-06-02), `node.conf`
  (34172 B @ 2026-05-09), `anchors.json` (1177 B @ 2026-05-05) all identical before/after;
  plaintext-backup count unchanged (3); the proof never created the default dir.
- No proof listeners left on 41008 / 41010 / 41011 (only transient client `TIME_WAIT`).
- The proof used only the explicit isolated dirs under `irium-poawx-live-proof`.

## Allowed claim

> Local Windows devnet live proof succeeded: a real Irium-native-PoW all-gates block was submitted
> to a real node and accepted, advancing the chain.

## Not allowed

- production-ready
- mainnet-ready
- audited

## Remaining blockers

- cross-host P2P provider/firewall
- independent audit
- public testnet
- governance / mainnet activation
