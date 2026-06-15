# PoAW-X Stratum: submit_block_extended Fix (cpuminer-compat)

**Date:** 2026-06-15
**Crate:** `pool/irium-stratum` (PoAW-X-gated; mainnet pool runs `poawx_enabled=false`)
**Result: FIXED** — a `cpuminer`/`cpuminer-multi` miner can now mine a PoAW-X block that the node commits, end-to-end, via the stratum.

---

## 1. Original blocker (405)

Block submission in the stratum is per-adapter. Real CPU miners authorize as `adapter_id=cpuminer_compat` → `handle_submit_cpuminer_compat` → `submit_canonical_block`, which **always** POSTed to legacy `/rpc/submit_block`. With PoAW-X active the node returns **405** (legacy submit disabled). The `submit_block_extended` branch existed only in `handle_submit_legacy_rewardable`, a path CPU miners don't use. → No PoAW-X block could ever commit via a CPU miner.

## 2. Issues discovered while fixing (each verified live)

- **Missing pubkey/sig:** the stratum's `PoawxPendingReceipt` (template.rs) had only 5 fields; it dropped `worker_pubkey`/`worker_sig` from the template. The node's `submit_block_extended` validates these from the request directly (no pending-pool fallback), so an extended submit would 400 (worker-identity) without them.
- **Lane canonicalization:** the stratum's `compute_receipts_root_from_pending` (block.rs) hashed the **full** lane string, while the node canonicalizes to the first byte (the B-1 fix). Single-char lanes like `"A"` matched, but multi-char lanes (e.g. `"cpu"`) would mismatch the root.
- **Reward split (not a code bug):** the node requires the coinbase to pay each receipt's worker its share. The stratum pays the connected miner (`session.pkh`). In the real flow the **miner is the worker** (same address), so this is satisfied; it only fails if a receipt's worker differs from the mining identity.

## 3. Files changed

- **`pool/irium-stratum/src/template.rs`** — `PoawxPendingReceipt`: added `worker_pubkey: String` and `worker_sig: String`, both `#[serde(default)]` (parsed from `getblocktemplate`, preserved for extended submit; root computation is unaffected — it ignores these fields).
- **`pool/irium-stratum/src/block.rs`** — `compute_receipts_root_from_pending`: canonicalize lane to its first byte (`r.lane.bytes().next().unwrap_or(b'A')`) in both the sort key and the hash, matching iriumd.
- **`pool/irium-stratum/src/stratum.rs`** —
  - `CanonicalJobSnapshot`: added `poawx_pending_receipts: Vec<PoawxPendingReceipt>`.
  - `build_canonical_job_snapshot`: populate it from `job.poawx_pending_receipts`.
  - New `build_submit_variant(config, snapshot, req)` helper + `SubmitVariant` enum; `submit_canonical_block` now POSTs `/rpc/submit_block_extended` (with full receipts + root) when `config.poawx_enabled && !snapshot.poawx_pending_receipts.is_empty()`, else legacy unchanged.
- Incidental: `cargo fmt` normalized pre-existing formatting drift in `events.rs`, `main.rs`, `pow.rs` (whitespace only, no logic); `Cargo.lock` `irium-node-rs` version synced. No behavior change.

**Mainnet-pool-safe:** the new branch is gated on `poawx_enabled`, which is `false` in production (no `IRIUM_STRATUM_POAWX`), so the mainnet pool always takes the unchanged legacy path. New receipt fields are `serde(default)`.

## 4. Tests added (`cargo test`: 36 passed, 0 failed)

- `block.rs::lane_canonicalized_to_first_byte` — `"cpu"` and `"c"` produce the same root; `"cpu"` vs `"gpu"` differ.
- `stratum.rs`: template receipt deserializes `worker_pubkey`/`worker_sig`; missing fields default empty; `build_canonical_job_snapshot` carries receipts incl. pubkey/sig; `build_submit_variant` selects extended when enabled+receipts (asserts pubkey/sig present and root consistency), legacy when disabled, legacy when receipts empty.

## 5. Checks

- `cargo fmt --check`: clean
- `cargo test` (in `pool/irium-stratum`): **36 passed, 0 failed**

## 6. Receipt-seeded stratum demo (VPS-1 local-only, firewall closed)

With the fix + an operator-seeded receipt whose worker == the mining identity:
- Accepted stratum share ✓
- Pending receipt present with pubkey/sig ✓
- `[block] submit_block_extended (cpuminer_compat) height=1 receipts=1 root=55d78dd3…` fired ✓
- Node **accepted** the PoAW-X block; `[block] submitted height=1` ✓
- Height advanced **0 → 1** ✓
- `irx1_root` `55d78dd3…` visible via private RPC ✓
- Receipt **cleared** after commit (pending 0) ✓
- Lane canonicalization correct (unit-tested) ✓
- Negative: legacy `/rpc/submit_block` still **405** under PoAW-X active ✓
- Both mainnets untouched (VPS-1 4042499 / irium-eu 1851441, both `7c07ae2c…`); firewall stayed closed; cleanup complete ✓

## 7. Remaining limitations

- **Worker must equal miner (single-miner model):** the stratum coinbase pays one address (the connected miner). The reward-split rule requires paying every pending receipt's worker, so a block commits only when all pending receipts belong to the connected miner. This fits a **single trusted miner** pilot (the miner submits its own receipts and is paid). **Multiple concurrent miners** with distinct receipts would need the stratum to add per-worker payout outputs — not addressed here.
- The demo seeds the receipt operator-side (genesis `/poawx/assignment` returns 404 at h=0); a real miner past genesis uses the assignment endpoint normally.

## 8. Community-miner pilot status

**One trusted community miner is now UNBLOCKED for PoAW-X block production via the stratum** (single miner == worker; clean pending pool). Multi-miner concurrent PoAW-X block production remains a known limitation pending per-worker coinbase payout support.
