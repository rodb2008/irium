# PoAW-X Phase 14-F — Post-Remediation Full Two-VPS Validation

**Date:** 2026-06-15
**Result: PASS** — full PoAW-X stack re-validated on two VPS after Phase 14-C fixes, Phase 14-E main reconciliation, mainnet binary remediation, and latest-main (5d4604c) merge.

---

## 1. Identity

| Item | Value |
|---|---|
| Branch | `testnet/poawx-phase12-completion-rc-hardening` |
| Pre-test HEAD | `c4a1a7b` |
| **HEAD after latest-main merge** | `94b1cc4` (merge of `5d4604c`) + doc commit (this) |
| origin/main at test time | `5d4604c` (v1.9.115) |
| **Latest-main `5d4604c` merged?** | **YES** — `c934d6d` "Add coinbase_tag field to rpc/blocks response" (non-consensus RPC field). Clean auto-merge; rustfmt normalized; folded into merge commit `94b1cc4` (2 parents `c4a1a7b`+`5d4604c`). |
| Backup ref | `backup/pre-14f-merge-c4a1a7b` |

---

## 2. Mainnet Safety Preflight (VPS-1, official isolated binary)

| Item | Value |
|---|---|
| ExecStart | `/home/irium/mainnet/bin/iriumd-current` |
| PID | 4042499 (unchanged throughout) |
| Running hash | `7c07ae2c30dd1c5ade6a23e99af4a132e4a4bbe8504c3b3ec4c342cfeb133cae` (official v1.9.115) |
| Height / peers | 31895 → 31897 advancing / 9 |
| PoAW-X | OFF (no `IRIUM_POAWX_MODE`) |

**Build isolation confirmed:** dev/devnet builds write to `/home/irium/irium/target/…`; the mainnet service binary is `/home/irium/mainnet/bin/iriumd-current` (separate). `cargo build` during this phase did **not** affect the mainnet binary.

---

## 3. Build / Test (VPS-1, branch binary v1.9.115)

| Check | Result |
|---|---|
| `cargo fmt --check` | clean |
| `cargo build --release` | OK (v1.9.115; 3 pre-existing warnings) |
| `cargo test` (multi) | **1588 passed, 0 failed** |
| `cargo test -- --test-threads=1` | **1588 passed, 0 failed** (iriumd 248 in 785s) |
| Targeted: poawx / irx1 / timestamp | 99 / 19 / 9 |
| Targeted: seed isolation (test_12l) / reorg (phase13c) | 5 / 10 |
| Targeted: lane-cpu regression / reward_split / worker_sig | 1 / 9 / 3 |
| Targeted: carrier filter | code present (6 refs); build-verified (no unit test) |

---

## 4. Two-VPS Devnet Topology

| Node | Binary | Config |
|---|---|---|
| VPS-1 (207.244.247.86) | repo branch binary `/home/irium/irium/target/release/iriumd` (`cc7f79f0…`) | devnet, PoAW-X active, activation height 1, difficulty 4 bits, RPC 127.0.0.1:39511 (private), P2P 0.0.0.0:39510, data dir under $HOME |
| VPS-2 (157.173.116.134) | **isolated** branch binary `/home/irium/devnet-bin/iriumd-poawx-cc7f79f0` | devnet, PoAW-X active, P2P 0.0.0.0:39514, seed → VPS-1:39510, RPC private, data dir under $HOME |

**VPS-2 isolated-binary method:** VPS-2's repo is on `main` (no PoAW-X) and its mainnet service runs from its repo target (same binary-collision vuln as VPS-1 pre-remediation). To avoid touching VPS-2 mainnet, VPS-1's built branch binary was copied to an isolated path on VPS-2 (sha256 `cc7f79f00f358612e7e15ee3539ad3c3a48eaffe5243409212e745e4c489d93c`, **matched** source exactly) and the devnet node ran from there. **VPS-2 repo, mainnet binary, and `iriumd.service` were not touched.**

---

## 5. E2E Results — two independent runs, both 37/37 PASS

### Run A — standard (lane="A")
`Total: 37 | PASS: 37 | FAIL: 0 | SKIP: 0 — VERDICT: PASS`

### Run B — lane="cpu" (B-1 live regression proof)
`Total: 37 | PASS: 37 | FAIL: 0 | SKIP: 0 — VERDICT: PASS`

**Positive flow (both runs):** worker keypair + puzzle solved → receipt accepted (200) → receipt in pending → block mined → block accepted by VPS-1 (200) → height 1 → irx1_root in block JSON matches submitted root → 7-rule consensus validated → receipt cleared after commit.

**Negative checks (N-1…N-13, both runs, all PASS):** legacy submit_block 405 · empty receipts 400 · missing irx1 400 · zero irx1 400 · wrong irx1 root 400 · bad sig 400 · spoofed pkh 400 · wrong nonce 400 · insufficient PoW 400 · missing payout 400 · wrong payout pkh 400 · mainnet PoAW-X 404 · RPC 39511 not public (refused).

**P2P sync (both runs):** VPS-2 synced to height 1 via P2P (attempt=1), irx1_root matched VPS-1, both nodes same tip hash.

**Reorg/restore:** Phase 13-C 10 unit tests pass (empty/add/dedup/expiry/boundary/idempotent/no-invalid-reintroduction); structural `reorg_orphaned_blocks` compiled.

---

## 6. Targeted Verifications

- **lane="cpu" B-1 regression — PROVEN (live + unit):** Run B submitted a `lane="cpu"` receipt; VPS-1 accepted the block (P-10), its computed irx1_root matched the canonical-first-byte root (P-13), connect_block's `validate_poawx_block_receipts` passed (P-14), and VPS-2 synced it (P-18/20/21). An unfixed node would reject on the internal 3-byte (`compute_poawx_receipts_root`) vs 1-byte (`irx1_root_from_block_receipts`) mismatch. Also covered by unit test `test_compute_receipts_root_lane_cpu_matches_block_receipt_root`.
- **MTP — verified via code + unit/build, not live-activated:** `MTP_ACTIVATION_HEIGHT=32_000`, `median_time_past()`, and timestamp validation present in `chain.rs`; block-template MTP timing in `iriumd.rs`. Devnet runs at height 1 (« 32000) so MTP does not trigger there — boundary behaviour is logic/build-verified. (Mainnet, now on official v1.9.115, enforces the same rule; it is ~100 blocks from activation.)
- **Carrier filter — verified via code + build:** `is_btc_carrier`/`is_ltc_carrier` stale relay-tip drop present in `iriumd.rs`. No BTC/LTC carriers exist in devnet, so not exercised live; build-verified.
- **N-1 cap 255 / N-2 saturating_add / N-3 shared reward const:** present and unit-covered.

---

## 7. Shutdown & Cleanup

- Devnet nodes stopped on both VPS; devnet data/state dirs removed on both.
- Devnet ports clear on both: VPS-1 (39510/39511/39508), VPS-2 (39514/39511/39508).
- VPS-2 isolated binary retained at `/home/irium/devnet-bin/iriumd-poawx-cc7f79f0` (record/future runs).

---

## 8. Mainnet Untouched (both hosts)

| Host | PID before | PID after | Hash | Result |
|---|---|---|---|---|
| VPS-1 | 4042499 | 4042499 | `7c07ae2c…` | unchanged, height advancing |
| VPS-2 | 1836431 | 1836431 | `e6cbe44c…` | unchanged, ExecStart unchanged |

No restart/reconfigure of either mainnet. RPC 39511 never publicly exposed.

---

## 9. Outstanding Follow-up (not done here)

- **VPS-2 binary-collision vulnerability still exists:** VPS-2's `iriumd.service` runs `/home/irium/irium/target/release/iriumd` (its repo target). It should receive the same remediation as VPS-1 (move the mainnet binary to a stable path isolated from the repo) in a controlled maintenance window. Deferred per instruction.

---

## 10. Verdict

- **PASS.** Total across both E2E runs: **74/74 checks PASS, 0 FAIL, 0 SKIP**; unit/integration **1588 tests, 0 failed** (single-threaded and multi).
- MTP present and consensus-aligned with official v1.9.115; carrier filter present; PoAW-X positive/negative/P2P/reorg all green; B-1 lane="cpu" proven live.
- Both mainnets untouched; build isolation confirmed.
- **Branch is ready to push after explicit approval.** **Trusted miner pilot would be unblocked after push** (still gated on explicit approval).
- Push status: **BLOCKED — not pushed.** No PR, no merge-to-main.
