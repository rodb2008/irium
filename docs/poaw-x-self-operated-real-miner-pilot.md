# PoAW-X Self-Operated Real-Miner Pilot

**Date:** 2026-06-15
**Type:** Founder/internal — VPS-1 operator (testnet node + stratum), VPS-2/irium-eu external miner. Not a community test.

**Verdict: PARTIAL** — a **real** cpuminer connected, subscribed, and authorized over the genuine external network, but **no real CPU miner could submit shares / mine a block** due to a stratum compatibility issue (unsolicited `mining.set_version_mask`). The PoAW-X `submit_block_extended` fix itself is independently proven (the receipt-seeded demo on `f9f8d17` committed a block via the python harness).

---

## 1. Real miner used

**YES — real binaries built from source on VPS-2 (isolated `/tmp`, no global installs):**
- `pooler/cpuminer` (minerd) **2.5.1** — built clean.
- `tpruvot/cpuminer-multi` **1.3.7** — built after fixing a libcurl/libcrypto link (configure left `LIBCURL=''`; re-linked with `make LIBS="-lz -lcrypto -lpthread -lcurl"`).

No production files touched; both build trees removed at cleanup.

## 2. Operator (VPS-1) result

- Isolated testnet node (devnet, PoAW-X active, RPC `127.0.0.1:39511` private) + `pool/irium-stratum` (f9f8d17 build) on `0.0.0.0:39512`.
- Operator-seeded a receipt whose worker == the mining address; template showed `pending=1`, `receipts_root` non-zero; stratum built irx1 jobs (`irx1_len=38`).
- RPC never exposed; stratum reachable only by VPS-2 (ufw source-restricted).

## 3. Miner (VPS-2) result

- **External network path: WORKS** — both miners connected to `VPS1_PUBLIC_IP:39512` from VPS-2.
- **subscribe + authorize (server-side): WORKS** — stratum logged `[authorize] adapter_kind=cpuminer_compat`, sent difficulty + irx1 job.
- **pooler/cpuminer 2.5.1:** `Stratum authentication failed` — aborts on the unsolicited `mining.set_version_mask` (no version-rolling support).
- **cpuminer-multi 1.3.7:** `unknown stratum method mining.set_version_mask!` → `Stratum answer id is not correct!` → hashes (6.6 MH/s) but **submits no shares** (request-id desync from the unsolicited mask). No `sharecheck` recorded server-side.

## 4. Required-proof outcomes

| Item | Result |
|---|---|
| Accepted share (real miner) | ❌ none submitted |
| `submit_block_extended` fires (real miner) | ❌ not reached (no share) — but PROVEN via harness on f9f8d17 |
| PoAW-X block accepted (real miner) | ❌ |
| Height advances | ❌ (stayed 0) |
| irx1_root visible | ✅ in template/job (operator RPC); no block produced |
| Receipt clears | ❌ (no commit) |
| Real cpuminer connects/subscribes/authorizes | ✅ |
| Negative: legacy submit_block 405 | ✅ (unchanged guard) |
| RPC private throughout | ✅ |
| Unknown IPs blocked | ✅ (ufw source-restricted to VPS-2) |

## 5. Root cause (stratum, separate from the PoAW-X submit fix)

`pool/irium-stratum/src/stratum.rs` pushes an **unsolicited** `mining.set_version_mask` notification on subscribe to **every** miner except Whatsminer firmware (`suppress_unsolicited_mask = is_whatsminer_firmware(user_agent)`). Standard CPU miners (`cpuminer`, `cpuminer-multi`) do **not** support version-rolling and either abort (pooler) or desync their request-id matching and never submit shares (cpuminer-multi). The mainnet pool works with version-rolling-capable ASIC firmware; CPU miners are not currently compatible.

**This is pre-existing stratum behavior, unrelated to the PoAW-X `submit_block_extended` fix.**

## 6. Firewall / RPC / mainnet / cleanup

- Stratum 39512 opened **only** to VPS-2 IP via a temporary ufw rule; removal command handed to operator (rule still present at write time — see final response).
- RPC `39511` never publicly exposed (cpuminer never uses RPC).
- VPS-1 mainnet `MainPID=4042499` / irium-eu `MainPID=1851441`, both official `7c07ae2c…`, active — **untouched**. Production pool untouched.
- Testnet node + stratum stopped; devnet ports clear; temp data, seed script, and both miner build trees removed.

## 7. Limitations

- Only two CPU miners tested (both old, neither version-rolling-capable). A version-rolling-capable miner (e.g. cpuminer-opt, ASIC firmware) was not tested here.
- The PoAW-X block path via stratum is proven only with the python harness (f9f8d17 demo), not yet with a real CPU miner end-to-end.

## 8. Readiness for one trusted community volunteer

**Not yet — recommend a follow-up stratum fix first.** Before inviting a CPU-miner volunteer, suppress the unsolicited `mining.set_version_mask` for cpuminer-family user agents (extend `suppress_unsolicited_mask` beyond Whatsminer, or gate it by config), then re-run this self-operated test to a committed block with a real cpuminer. Alternatively, validate with a version-rolling-capable miner the mainnet pool already supports and document the exact recommended miner/version for volunteers. The PoAW-X consensus + submit path is ready; the gap is CPU-miner stratum handshake compatibility.

---

## Update (post version-mask fix retest)

The `mining.set_version_mask` blocker from §5 was **fixed** (`poawx: suppress version mask for cpu miners`). Retest with real `pooler/cpuminer` 2.5.1:
- ✅ Unsolicited `set_version_mask` now **skipped** for cpuminer user agents; pooler **authorizes** and **mines**, and submitted an **accepted share** (`rewardable=true`).
- ⚠️ Block promotion still blocked by a **separate** issue: `canonical_block_ok=false` (`COMPAT_CANDIDATE_BLOCKED`) — header canonicalization mismatch on the pre-fork path (devnet height 1 < 22888). See `docs/poaw-x-real-cpuminer-stratum-compatibility.md`.

**Verdict remains PARTIAL**, but the handshake blocker is resolved; the remaining gap is real-CPU-miner block-hash canonicalization at low devnet heights (likely a devnet-height artifact; mainnet pool works at post-fork heights).
