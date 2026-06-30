# PoAW-X Phase 24L — Windows local live-proof package

**Goal:** a Windows-safe, local, devnet-only live proof:

> Windows local devnet `iriumd` → `poawx-live-proof-harness` → real RPC submit →
> node accepts an all-gates block → height advances.

This packages the flow. **The actual live proof is run by the user on Windows.**
Code/test-first: this phase adds the binary, the runner script, tests, and docs;
it launches **no** live VPS nodes/miners. Local-only; not pushed; mainnet
untouched.

## What this builds on

Phase 24K proved (in-process) that an Irium-native-PoW mined all-gates block is
accepted by full `connect_block`. Phase 24L moves that to a **real node over
RPC**, reusing the SAME proven construction:
`irium_node_rs::poawx_mining_harness::build_devnet_all_gates_block`. A
`connect_block` test (`chain::phase24l_lib_builder_connect_block`) proves the
binary's exact builder output is node-acceptable, so the only Windows-only
unknown is the local RPC round-trip.

## Components

- **Binary** `poawx-live-proof-harness` (`src/bin/poawx-live-proof-harness.rs`):
  GET `/poawx/assignment` + `/rpc/getblocktemplate` → build the all-gates block
  with Irium-native PoW → POST candidate admissions → POST
  `/rpc/submit_block_extended` → GET `/status` and verify height advanced → print
  a proof summary + write `poawx-live-proof.json` under the work dir.
- **Runner** `scripts/windows/poawx-live-proof.ps1`: creates isolated dirs,
  starts a loopback devnet node, runs the harness, stops the node, confirms the
  default `~/.irium` was not created.

### Safety guards (fail-closed)

- rejects mainnet (`network_id == 0`) — `guard_network`;
- requires an explicit `--devnet`/`--testnet` flag matching `IRIUM_NETWORK`;
- requires a **loopback** `--rpc-url` (127.0.0.1 / localhost / ::1) — no public RPC;
- requires an explicit, existing `--work-dir` that is NOT the production default
  (`%USERPROFILE%\.irium` / `$HOME/.irium`) — `guard_isolated_storage`;
- never prints private keys, seeds, or VRF secrets; artifacts are public block
  data only;
- the node binds loopback only (no `0.0.0.0`); P2P is loopback (`127.0.0.1:41010`).

### Path note (important)

The node fails closed (exit 78, Phase 24C hardening) on storage dirs that do not
resolve under the user's home. On Windows that home is `%USERPROFILE%`, so the
isolated root is created at **`%USERPROFILE%\irium-poawx-live-proof`** (not
`C:\...`). This both satisfies the storage guard and stays off the production
default `%USERPROFILE%\.irium`.

## Transfer to Windows (no push, no GitHub)

On the VPS (current branch, HEAD recorded in the final report):

```sh
cd /home/irium/irium
git bundle create poawx-phase24l.bundle HEAD
sha256sum poawx-phase24l.bundle > poawx-phase24l.bundle.sha256
```

Copy both files to Windows (scp/USB/RDP share). On Windows (Git for Windows):

```powershell
# verify integrity (compare to the .sha256 from the VPS)
Get-FileHash .\poawx-phase24l.bundle -Algorithm SHA256

git clone .\poawx-phase24l.bundle irium-poawx
cd irium-poawx
git checkout testnet/poawx-phase20-blueprint-completion-local   # or the bundled HEAD
git rev-parse HEAD    # must equal the commit from the final report
```

## Build (Windows)

Prerequisites: Rust **stable** (`rustup` + the MSVC toolchain / "Desktop
development with C++" build tools), Git for Windows.

```powershell
cargo build --release --bin iriumd --bin poawx-live-proof-harness
```

## Run the live proof

```powershell
powershell -ExecutionPolicy Bypass -File scripts\windows\poawx-live-proof.ps1
```

### Expected output (success)

```
[i] isolated root: C:\Users\<you>\irium-poawx-live-proof
[i] starting node: ...\target\release\iriumd.exe  (RPC 127.0.0.1:41011)
[i] /poawx/assignment OK (height=0, pow_bits=207fffff)
[i] running harness...
PoAW-X LIVE PROOF OK
network_id        : 2 (non-mainnet)
before_height     : 0
after_height      : 1
submitted_height  : 1
block_hash        : <hex>
irx1_root         : <hex>
official_fee      : 0% (no fee output)
poawx_sections    : candidate_set, candidate_admission, committed_admission, true_vrf(AVR2), role_puzzle_proofs, finality_proof, role_dominance_weights
node_response     : {"status":"accepted",...}
artifact          : C:\Users\<you>\irium-poawx-live-proof\artifacts\poawx-live-proof.json
[i] production default C:\Users\<you>\.irium not created by this run (good)
[OK] Phase 24L Windows live proof SUCCEEDED
```

## Success criteria

1. node starts with isolated Windows dirs (under `%USERPROFILE%\irium-poawx-live-proof`);
2. node does not use `%USERPROFILE%\.irium`;
3. `/poawx/assignment` returns at genesis;
4. harness builds all-gates material; 5. mines Irium-native PoW;
6. submits via `/rpc/submit_block_extended`; 7. node accepts; 8. height advances;
9. summary shows before/after height, block hash, irx1 root, all-gates sections,
   official 0% fee; 10. node stopped; 11. no default wallet/data path touched.

## Troubleshooting

- **node exits with code 78**: a storage dir resolved outside `%USERPROFILE%`.
  Keep the root under your user profile (the script already does).
- **harness: "refusing non-loopback RPC"**: use `http://127.0.0.1:41011`.
- **submit rejected (HTTP 400) + commitment_nonce/difficulty**: ensure the node
  and the harness share the SAME env (the script sets both); rerun via the script.
- **assignment 503**: the node is mainnet or not active — the script sets
  `IRIUM_NETWORK=devnet` + `IRIUM_POAWX_MODE=active`.

## Cleanup

Stop is automatic. To remove the proof state:
`Remove-Item -Recurse -Force $env:USERPROFILE\irium-poawx-live-proof`.

## What the proof means / does not mean

If it passes, the only allowed claim is:

> **Local Windows devnet live proof succeeded: a real Irium-native-PoW all-gates
> block was submitted to a real node and accepted, advancing the chain.**

NOT allowed: "mainnet-ready", "production-ready", "audited". This is a local
devnet proof — it does not establish cross-host networking, public-testnet
behavior, independent audit, or governance/mainnet activation.

## Phase 24L Windows live proof — RESULT: PASSED

The Windows local devnet live proof ran end-to-end and PASSED: a real Irium-native-PoW all-gates
block was submitted to a real local devnet node over RPC and accepted, advancing the chain
height 0 -> 1 (block 31df881052b05dc6319c5915ca938b282df60ab7e823aba44ee5edd20dfd23bf, irx1 root
772e1cd700af122e5bc2a586a1eb94d4dc33bdd2ab819dba435df9875c7ed9bd, official 0% fee, all-gates
sections present). Post-fix HEAD 1ca7d89. Two genuine bugs were fixed to make it work on Windows:
cef587d (preserve Windows drive prefix in the storage guard) and 1ca7d89 (initialize the
standard-header activation global in the standalone harness). Mainnet node (PID 33752) and the real
%USERPROFILE%\.irium wallet/config were untouched; no proof listeners remained. Allowed claim:
"Local Windows devnet live proof succeeded: a real Irium-native-PoW all-gates block was submitted
to a real node and accepted, advancing the chain." NOT production-ready / mainnet-ready / audited.
Remaining: cross-host P2P provider/firewall, independent audit, public testnet, governance/mainnet
activation. See docs/poaw-x-phase24l-windows-live-proof-result.md.
