# PoAW-X Receipt-Seeded Stratum Block Demo

**Date:** 2026-06-15
**Type:** Founder/internal, **VPS-1 local-only** (firewall stayed closed; 39512 not reopened to VPS-2).

**Verdict: FAIL** (for the stated objective: accepted stratum share → pending receipt → irx1 root → **committed PoAW-X block via the stratum flow**). A real blocker was found in the **stratum** (not the node). Documented honestly per instruction; **no success was faked**.

---

## 1. Identity / Scope

- Branch/HEAD: `testnet/poawx-phase12-completion-rc-hardening` @ `645cae1`.
- VPS-1 local-only. Firewall: **not opened**; 39512 bound `127.0.0.1` only; confirmed closed externally before and after.
- Testnet node binary `cc7f79f0…` (validated branch, isolated from mainnet); stratum `pool/irium-stratum` v0.1.1 `4856c31b…`.
- RPC `127.0.0.1:39511` private throughout; P2P/stratum/status all localhost.

## 2. Genesis Workaround (SUCCEEDED)

The prior rehearsal failed because `/poawx/assignment` returns 404 at height 0. This time the receipt was seeded **directly** (operator-side, private RPC), replicating the validated Phase 14-F derivation, without the assignment endpoint:
- commitment nonce = `SHA256(SHA256(GENESIS || (h-1) LE || "poawx_assignment_seed_v1") || "commitment_nonce")` for target height 1 (parent = genesis).
- generated a throwaway worker keypair, brute-forced the puzzle (4 leading-zero bits), signed the challenge `SHA256(solution || nonce || height_LE)`, and `POST /poawx/receipt`.

Result: **HTTP 200 accepted**; `getblocktemplate` then showed `poawx_pending_receipts: [1]` and `receipts_root: d79d0f76…` (non-zero). Pending receipt confirmed.

## 3. Results vs Required Proof

| # | Required | Result |
|---|---|---|
| 1 | Stratum receives/accepts share | ✅ share accepted (1/0) |
| 2 | Pending receipt available | ✅ seeded + confirmed in template |
| 3 | Stratum builds coinbase with non-zero irx1 root | ✅ `to_job mode=active pending=1 irx1_len=38 receipts_root=d79d0f76…` (every job) |
| 4 | `submit_block_extended` used (not legacy) | ❌ **NO** — stratum used legacy `/rpc/submit_block` |
| 5 | PoAW-X block accepted | ❌ **NO** — node returned **405** (legacy submit blocked under PoAW-X active); height stayed 0 |
| 6 | irx1_root visible via private RPC | ⚠️ visible in template/job, but no block committed |
| 7 | Receipt clears after commit | ❌ N/A — no commit; receipt remained pending |
| 8 | Block contains receipt section | ❌ N/A — no block |
| 9 | No consensus rejection | ⚠️ node behaved correctly (405 is the intended guard); block simply never reached `submit_block_extended` |
| 10 | Height advances | ❌ **NO** — stayed 0 |

## 4. Root-Cause Blocker (stratum, not node)

- The stratum **builds the irx1 job correctly** (290 `to_job` logs, all `pending=1`, `irx1_len=38`, matching `receipts_root`), and the miner share was accepted.
- At block submission, the stratum chose the **legacy** branch. The `submit_block_extended` info log (`src/stratum.rs:3486`) fired **0 times**.
- Node log: `[submit_block] reject: poawx mode=active; use /rpc/submit_block_extended with puzzle receipts`. Stratum log: `[block] submit failed reason=http_status=405`.
- Runtime `config.poawx_enabled = true` (startup warning `IRIUM_STRATUM_POAWX=1: PoAW-X receipt path enabled`; main.rs wires it at line 214). Every job logged `pending=1`.
- **Therefore:** the submit-time condition `config.poawx_enabled && !job.poawx_pending_receipts.is_empty()` (`src/stratum.rs:3472`) evaluated **false** at submit, even though job-build populated receipts and the coinbase carried irx1. The job's `poawx_pending_receipts` was not effective on the submit path.
- **Conclusion:** a stratum job-state/submit-path defect — the solved block is submitted via the legacy endpoint and rejected, so **no PoAW-X block can be committed through the stratum** in the current build. This is a code bug in `pool/irium-stratum` (out of scope to fix in this docs-only demo); needs investigation + fix + re-test.

The **node** consensus path is not implicated: it correctly accepted the receipt, advertised `receipts_root`, and correctly rejected legacy submit under PoAW-X active. The full node receipt→irx1→block path is proven in Phase 14-F via `submit_block_extended` (direct RPC).

## 5. Negative Checks (PASS)

- Bogus share **rejected** (`stale share`, height unchanged).
- Legacy `submit_block` **405** under PoAW-X active (the very guard that blocked the stratum here) — intended behavior confirmed.
- No invalid block accepted (height stayed 0; nothing committed).

## 6. Mainnet Untouched / Firewall / Cleanup

- VPS-1 mainnet: PID 4042499, `7c07ae2c…` (unchanged); irium-eu mainnet: PID 1851441, `7c07ae2c…` (unchanged).
- Firewall stayed closed; 39512 `127.0.0.1`-only and confirmed externally CLOSED before/after.
- Testnet node + stratum stopped; devnet ports clear; temp data + seed script removed.

## 7. Limitations

- VPS-1 local-only; no external miner this round (intentional).
- Demo used the python stratum harness as the share source.

## 8. Does this close the gap before one trusted community miner?

**No.** The gap (committed PoAW-X block **through the stratum**) is **not** closed. The stratum builds irx1 jobs and accepts shares, but its block-submission does not use `submit_block_extended`, so it cannot commit PoAW-X blocks. Before a trusted community miner can produce real PoAW-X blocks via this stratum:
1. **Fix the stratum** so a solved share with pending receipts submits via `/rpc/submit_block_extended` (investigate why `job.poawx_pending_receipts` is empty on the submit path despite job-build populating it).
2. Re-run this exact demo (seed receipt → stratum share → committed irx1 block, height advances, receipt clears).
3. Then confirm end-to-end with a real `cpuminer` and the trusted-miner runbook/checklist.

Until the stratum fix lands, the trusted community miner pilot should remain **not started** for block production (share submission alone would work, but no PoAW-X blocks would be committed).
