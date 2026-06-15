# PoAW-X Real CPU-Miner Stratum Compatibility

**Date:** 2026-06-15
**Crate:** `pool/irium-stratum` (PoAW-X-gated; mainnet pool unaffected)
**Status:** Compatibility fix applied + verified — a real `pooler/cpuminer` now completes the stratum handshake (subscribe → authorize → jobs → mining) and submits an **accepted share**. Full real-cpuminer *block production* at devnet heights is blocked by a **separate, pre-existing** header-canonicalization issue (§4), not by this fix.

---

## 1. Root cause

The stratum pushed an **unsolicited** `mining.set_version_mask` (BIP310 version-rolling) notification on subscribe to **every** miner except Whatsminer/BTMiner firmware (`suppress_unsolicited_mask = is_whatsminer_firmware(user_agent)`). Standard CPU miners don't support version-rolling:
- `pooler/cpuminer` 2.5.1 → aborts with "Stratum authentication failed".
- `tpruvot/cpuminer-multi` 1.3.7 → "unknown stratum method mining.set_version_mask!" → request-id desync → submits no shares.

(Found in the self-operated real-miner pilot; the PoAW-X `submit_block_extended` fix was already proven separately on `f9f8d17`.)

## 2. Fix

`pool/irium-stratum/src/stratum.rs`:
- Added `is_cpuminer_family(ua)` (matches `cpuminer`, `cpuminer-multi`, `cpuminer-opt`).
- Added `should_suppress_unsolicited_mask(user_agent)` = Whatsminer/BTMiner **or** cpuminer-family.
- The subscribe handler now calls `should_suppress_unsolicited_mask(...)` instead of the Whatsminer-only check.

Version-rolling is **not** lost for miners that support it: they negotiate via `mining.configure`, whose handler still sends the mask. Only the **unsolicited** subscribe-time push is suppressed, and only for non-version-rolling families. Version-rolling-capable ASIC firmware (cgminer/Antminer/Bitaxe/Whatsminer-via-configure) is unchanged. **No change** to PoAW-X receipt/irx1/`submit_block_extended` logic or to iriumd.

## 3. Tests added (`cargo test`: 38 passed, 0 failed)

- `cpuminer_family_detected_from_user_agent` — cpuminer/cpuminer-multi/cpuminer-opt true; bitaxe/cgminer false.
- `unsolicited_mask_suppressed_for_cpuminer_and_whatsminer_only` — cpuminer + whatsminer suppressed; bitaxe/cgminer/none not suppressed.

## 4. Real-miner retest (VPS-1 stratum, VPS-2 = pooler/cpuminer 2.5.1, external path)

- ✅ Stratum logged `user_agent=cpuminer/2.5.1 skipped unsolicited set_version_mask` — fix confirmed.
- ✅ pooler **authorized cleanly** (no "authentication failed"), received the irx1 job, and started hashing (~5 MH/s/thread). The pre-fix abort is gone.
- ✅ pooler submitted an **accepted share**: `[SHARE_ACCEPTED] worker=… rewardable=true` (after ~4 cores × a few minutes — diff-1 share, see note). The real-miner share path works end-to-end.
- ⚠️ Block promotion **blocked**: `[COMPAT_CANDIDATE_BLOCKED] share_block_like=true canonical_block_ok=false action=no_candidate_promotion`. The miner's raw submitted hash meets the block target, but the **canonical** hash iriumd would validate does **not** — a header byte-order mismatch on the **pre-fork** path (devnet height 1 < `STANDARD_HEADER_ACTIVATION_HEIGHT=22888`). The stratum correctly refuses to submit a block iriumd would reject. **Separate from the version-mask fix and pre-existing.**
- ✅ The PoAW-X `submit_block_extended` → committed-block path is independently proven via the python harness (`f9f8d17`).

### Difficulty note
The stratum's minimum share difficulty is clamped to **1**. On this devnet (block target is trivially easy, `POW_LIMIT 7fffff…`), a diff-1 share is actually *harder* than a block, so a real CPU miner needs ~2³² hashes (minutes at a few MH/s) to find one — whereas the python harness constructs a share directly. This is a **difficulty/time artifact of the test**, not a compatibility issue. The handshake/authorize fix is the substantive result; the `submit_block_extended` → block path is independently proven (harness, `f9f8d17`).

## 5. Safety

- RPC `39511` never publicly exposed (cpuminer never uses RPC).
- Stratum `39512` opened only to VPS-2 IP via temporary ufw rule; removed after test (see pilot doc / final response).
- Both mainnets untouched (VPS-1 `4042499` / irium-eu `1851441`, official `7c07ae2c…`); production pool untouched. Miner build trees in `/tmp` removed.

## 6. Readiness for one trusted CPU-miner volunteer

**Handshake/authorize/mining: UNBLOCKED** — the version-mask fix lets real CPU miners connect, authorize, receive jobs, and submit accepted shares (verified with pooler/cpuminer 2.5.1).

**Full real-cpuminer block production: NOT yet at devnet heights** — a separate header-canonicalization mismatch (pre-fork merkle byte order, height < `STANDARD_HEADER_ACTIVATION_HEIGHT=22888`) blocks candidate-block promotion. This is almost certainly a **devnet-height artifact** (the mainnet pool, at post-fork heights, produces real-miner blocks routinely). 

**Recommended follow-ups (separate tasks):**
1. Investigate the cpuminer canonical-hash variant for **pre-fork** heights (the `canonical_block_ok=false` case), or run the PoAW-X pilot at **post-fork** heights so the standard header byte order applies.
2. Optionally lower the devnet share difficulty so a real CPU miner finds shares in seconds rather than minutes (the diff-1 floor currently makes CPU shares slow).

The PoAW-X consensus + `submit_block_extended` path is ready; remaining work is real-CPU-miner block-hash canonicalization at low heights.
