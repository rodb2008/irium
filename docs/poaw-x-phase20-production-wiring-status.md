# PoAW-X Phase 20 — Production Wiring Status (Steps 1–3 COMPLETE: extension threaded + consensus-enforced + root-committed + pool official-fee-0 production; wallet/third-party/E2E follow-up)

**Status:** **PARTIAL** (advancing). **Step 1** (threading `Phase20ReceiptExt` through node storage,
persistence, P2P/block sync, reorg) is COMPLETE; **Step 2** (`connect_block` /
`submit_block_extended` enforcement + receipts-root inclusion, gated by
`phase20_production_active(height)`) is COMPLETE and ENFORCED; **Step 3 — pool canonical multi-role
coinbase production in OFFICIAL fee-0 mode — is now COMPLETE** (synthetic testnet/devnet role-claim
source). After activation (testnet/devnet only; mainnet hard-off) the pool builds a valid
`Phase20ReceiptExt` + the canonical 55/22/13/10 coinbase + the gated root the node enforces. Before
activation the Phase 18/19 path is byte/logically identical. Still remaining: third-party fee block
production, wallet third-party-fee CLI, pool registry fee relaxation, a live (non-synthetic)
hidden-precommit role-claim protocol + commitment root, and a live loopback E2E.

### Step 3 (this pass) — pool canonical multi-role coinbase production (OFFICIAL fee-0): COMPLETE
After all Phase 20 production gates are active on testnet/devnet (multi-role + fairness; mainnet
hard-off), the stratum pool builds a valid `Phase20ReceiptExt`, the canonical multi-role coinbase,
and the gated root that matches `connect_block` / `submit_block_extended`.
- **Mirror primitives** (`pool/irium-stratum/src/delegation.rs`): byte-for-byte stratum-local
  mirrors of the node consensus primitives — `multi_role_amounts`, `fairness_assignment_digest`,
  `assign_lane_id`, `role_claim_digest`, and `RoleRewardMirror` / `PoawxRoleClaimMirror` /
  `Phase20ReceiptExtMirror`. Parity tests assert equality vs the dev-dep node lib (any drift fails).
- **Gate** `phase20_production_active(height)` (mainnet hard-off via `network_id_from_env()==0`;
  requires both `IRIUM_POAWX_MULTI_ROLE_REWARD_ACTIVATION_HEIGHT` and
  `IRIUM_POAWX_FAIRNESS_MATRIX_ACTIVATION_HEIGHT`).
- **Synthetic role-claim builder** `build_synthetic_phase20_ext(...)`, gated by
  `IRIUM_POAWX_SYNTHETIC_ROLE_CLAIMS=1` (testnet/devnet-only, mainnet hard-off, disabled by
  default). Deterministic per-role nonce/secret; assigned lane via `assign_lane_id`; a verifying
  `role_claim_digest`; solver pkh from registered workers if supplied, else the primary miner pkh
  (MVP single-miner). **This is local/testnet-only for production-wiring validation — NOT the live
  hidden-precommit role-claim protocol, which remains pending. No hidden-precommit is claimed.**
  If production is active but synthetic claims are disabled, the pool attaches NO extension — it does
  not fake claims; the node then fails closed on the missing extension.
- **Canonical coinbase** (`build_native_rewardable_coinbase`): after activation + an ext-bearing
  receipt, emits `irx1 OP_RETURN` + PRIMARY/COMPUTE/VERIFY/SUPPORT p2pkh in fixed order with the
  55/22/13/10 split (remainder → PRIMARY; exact sum), OFFICIAL fee-0 (no fee output, no delegate
  output). Duplicate pkhs (MVP: all role pkhs == primary) stay separate. The irx1 root is the
  GATED root. The mining.notify split rebuilds the same bytes (18C invariant preserved).
- **Gated root** (`compute_receipts_root_from_pending_gated`) + the submit paths use it so the
  pool-committed root equals the node's; pre-activation it equals the legacy root (byte-identical).
- **Pre-activation unchanged:** legacy single-output (or mode-1) coinbase + legacy root; existing
  native_rewardable / delegation behavior untouched (keyed on `phase20_ext` presence).
- Node parity for the test: two pure node validators (`validate_phase20_production_payout`,
  `validate_poawx_coinbase_payout`) were made `pub` so the pool dev-test asserts the AUTHORITATIVE
  node validator accepts the pool-produced fixture.
- Tests: pool `phase20_mirror_wire_parity_vs_node`, `phase20_gate_mainnet_off_and_heights`,
  `phase20_synthetic_disabled_or_mainnet_returns_none`,
  `phase20_synthetic_builder_valid_and_node_validator_passes` (delegation);
  `phase20_gated_root_byte_identity_and_node_parity` (block);
  `phase20_native_coinbase_canonical_multi_role_official` + `phase20_preactivation_coinbase_is_legacy`
  (stratum). Pre-existing delegation/native_rewardable/wallet suites unchanged.

### Step 2 — connect_block / submit_block_extended enforcement + receipts-root: COMPLETE
- **Receipts-root inclusion (gated).** `irx1_root_from_block_receipts_gated(receipts, phase20_active)`
  (lib) and `compute_poawx_receipts_root_gated(receipts, phase20_active)` (iriumd) bind
  `Phase20ReceiptExt::digest()` into each receipt's inner hash **after** the optional mode-1
  delegation digest, **only when `phase20_active`**. The old public functions are thin wrappers
  (`..., false`), so every pre-activation / non-production caller is **byte-identical**. The hex
  pending `phase20_ext` is exactly `serialize()`, so the submit-path root equals the connect-path
  root. Mutating any extension field (role claim, RoleReward, fee_bps, fee_pkh) changes the root.
- **`connect_block` enforcement.** `validate_poawx_block_receipts` now recomputes the root with the
  gate and, after activation, runs `validate_phase20_production_block` (per receipt:
  `validate_phase20_production_payout` with PRIMARY = receipt `worker_pkh`, total = block subsidy,
  `prev_hash` = parent hash, `third_party_mode = third_party_fee_active && third_party_pool_mode_enabled`).
  Pre-activation it runs the legacy 10%/receipt floor check unchanged. A missing extension after
  activation **fails closed**.
- **`submit_block_extended` enforcement.** Uses the gated root for the irx1 commitment check and
  adds an early reject when production is active but a receipt is missing the extension; the
  authoritative validation remains `connect_block` (called from the handler).
- **Reject coverage** (all via the integrated validator): missing extension, bad role claim,
  RoleReward mismatch, wrong coinbase amount/order, hidden extra payout, fee output in official
  mode, fee without third-party mode, fee over the 200 bps cap, root/extension mismatch, and
  mainnet (hard-off — the gate is false, so enforcement never runs and the root stays legacy).
- **Coinbase-only assumption (documented).** The production payout check uses the block subsidy as
  the distributable total; the supported single-miner producer builds a coinbase-only block (no
  fee-bearing txs). Fee-aware totals for fee-bearing blocks are a follow-up (no such producer
  exists yet — pool production is out of scope here).

### Step 1 — receipt-wire / storage / P2P / reorg threading: COMPLETE

### Step 1 (this pass) — receipt-wire / storage / P2P / reorg threading: COMPLETE
- **Receipt wire (`PoawxBlockReceipt.phase20_ext: Option<Phase20ReceiptExt>`)** + a **present-only
  v3 receipt section** (`POAWX_RECEIPT_SECTION_MAGIC_V3`): a block uses v3 only when a receipt
  carries the extension; v1/v2 (mode-0/mode-1) blocks are **byte-identical** to before
  (`serialize_v3` = `serialize_v2` + a `0` flag when absent). Round-trips through block
  serialize/deserialize (the **P2P / binary-persist path**).
- **JSON persistence** (`storage::JsonPoawxReceipt.phase20_ext`, `write_block_json`) +
  **JSON reload** (`iriumd` block-load reconstruction) carry the extension hex (omitted when absent).
- **Pending receipt** (`iriumd PoawxPendingReceipt.phase20_ext`) + both mappers
  (`pending_receipt_to_block_receipt` / `block_receipt_to_pending`) preserve it, so **reorg
  rollback/reapply** keeps the extension (malformed → fail-closed, like delegation).
- **NOT enforced:** the extension is only preserved, never validated/required in this step; the
  receipts root is unchanged (root/digest inclusion + validation belong to the enforcement step).
- Tests: v3 element round-trip + byte-identity-when-absent (poawx); v3 block wire round-trip +
  old-block-no-v3-magic (block); reorg mapper preserves ext + plain→None (iriumd).

### Tests added in Step 2
- **poawx** `phase20_root_gating_and_mutation_sensitivity`: gate-off byte-identity (extension
  ignored == no-ext root == wrapper); gate-on differs and is deterministic; mutating role
  claim / RoleReward / fee_bps / fee_pkh each changes the gated-on root; malformed/truncated
  extension fails to deserialize.
- **iriumd** `phase20_gated_root_parity_pending_vs_block_and_byte_identity`: gate-off equals the
  legacy root; gate-on submit-path (pending) root equals connect-path (block) root; gate-on
  differs from legacy.
- **chain** `phase20_connect_block_production_enforcement`: valid Phase 20 block accepted;
  rejects bad role claim, RoleReward mismatch, wrong coinbase order, wrong amount, hidden extra
  payout, fee-without-mode; accepts third-party fee with fee gate + mode; rejects fee over cap;
  rejects missing extension after activation; mainnet hard-off skips enforcement.
- submit_block_extended handler accept/reject is exercised through the gated-root parity +
  the authoritative `connect_block` tests; a live running-node loopback E2E is **Step 5**.

> Mainnet hard-off for all three features. Chain difficulty automatic via LWMA-144. Local-only;
> not pushed. Hidden-precommit commitment root remains a separate PARTIAL (see fairness doc).

## What is implemented this pass (COMPLETE, tested, safe)
- **`Phase20ReceiptExt`** (`src/poawx.rs`) — the versioned production receipt extension carrying
  the three role claims (compute/verify/support), the `RoleReward` payout pkhs, and the signed
  third-party fee terms (`fee_bps` + `fee_pkh`). Canonical `serialize`/`deserialize` (length-
  prefixed claims) + `digest` + round-trip/truncation/unknown-version tests.
- **`validate_phase20_production_payout`** (`src/chain.rs`) — the INTEGRATED consensus validator
  that ties the existing primitives together (the future `connect_block` entry point):
  1. validates each role claim against the deterministic fairness assignment (slot 0 per role;
     wrong role/lane/height/prev/digest reject; distinct expected role_ids reject a duplicate
     claim for the same role);
  2. requires the `RoleReward` pkhs to equal the validated claim solver pkhs;
  3. validates fee terms (`validate_fee_terms`: official 0% / third-party cap 2% / mode / pkh);
  4. validates the canonical fee-aware multi-role coinbase (`validate_poawx_coinbase_payout`).
- **`phase20_production_active(height)`** — gate requiring both multi-role + fairness active
  (mainnet hard-off); third-party fee layered separately.

## Tests added (this pass)
poawx: `phase20_receipt_ext_wire_roundtrip` (round-trip, truncation, unknown version, digest
sensitivity). chain: `phase20_integrated_production_validator` (official accept; third-party-fee
accept; fee-without-mode reject; wrong role; tampered lane; RoleReward mismatch; wrong height;
coinbase tamper; fee-in-official reject; over-cap reject) and
`phase20_production_gate_requires_multirole_and_fairness_mainnet_off`. Plus all prior Phase 20
primitive/validator tests. Full suite green: lib poawx 45, phase20 23, reward 6 (single-thread),
wallet 420, stratum delegation 14, native_rewardable 6, fmt clean.

## Coverage of the requested test list
The integrated validator + extension cover, at the consensus-validation/wire-type level:
role-claim cases (11–18), coinbase cases (19–26), third-party fee cases (27–39), and the
extension round-trip portions of (48–49). The remaining items — wallet CLI (40–42), pool
identity/registry (43–47), and live persistence/P2P/reorg/observer (48–52 at the running-node
level) — depend on the live integration below.

## Remaining live integration (follow-up — NOT done; the bulk of A/C/D/E/F/G/H/I)
Each touches the validated Phase 18/19/19D code and is staged to avoid regressing it:
1. **Node receipt-wire threading** — ✅ **DONE (Step 1)**: `Phase20ReceiptExt` is carried in the
   present-only v3 receipt section through `iriumd` pending receipts, `storage` JSON persist/reload,
   reorg pending↔block mappers, and P2P block ser/de (data only, not enforced).
2. **connect_block / submit_block_extended** — ✅ **DONE (Step 2)**: `validate_phase20_production_payout`
   runs in `connect_block` when `phase20_production_active(height)`; the extension is bound into the
   receipts root; missing extension after activation fails closed; pre-activation Phase 18/19 blocks
   remain valid (byte-identical). submit path uses the gated root + early missing-ext reject.
3. **Pool production** — ✅ **DONE (Step 3, OFFICIAL fee-0)**: the stratum native_rewardable path
   builds the canonical multi-role coinbase + `Phase20ReceiptExt` + gated root after activation,
   using the gated synthetic role-claim builder (testnet/devnet-only). Third-party-fee block
   production is NOT done (Step 4).
4. **Role-claim source** — real claims from miners, or a clearly-named testnet/devnet-only
   `IRIUM_POAWX_SYNTHETIC_ROLE_CLAIMS=1` synthetic builder (mainnet-impossible). Not added yet.
5. **Wallet CLI** — `--third-party-pool` / `--fee-bps` / `--fee-pkh` on `poawx-register`/
   `--emit-only` (fee terms already round-trip in the signed delegation).
6. **Pool registry** — relax `verify_and_store` (currently fail-closed on `fee_bps>0`) to accept
   capped third-party fees + persist `fee_pkh`, gated on third-party mode; reject mismatch/mutation.
7. **Observer + loopback smoke** — two-node + isolated `$TROOT` E2E (operator-approved, loopback).
   **(Step 5 — NOT done; submit_block_extended live handler accept/reject is covered here.)**

### Still NOT done after Step 3 (explicit)
- third-party-fee block production (Step 4) — pool builds OFFICIAL fee-0 only
- wallet third-party-fee CLI (`--third-party-pool` / `--fee-bps` / `--fee-pkh`)
- pool registry fee relaxation (`verify_and_store` still fail-closed on `fee_bps>0`)
- a LIVE (non-synthetic) role-claim protocol — Step 3 uses a gated testnet/devnet synthetic builder
- hidden-precommit commitment root (fairness matrix remains PARTIAL — assignment uses `prev_hash`,
  known at block time; a prior-block commitment root is required for true hidden-before-reveal)
- public/external miner test
- live loopback / two-node E2E (Step 5)

Mainnet remains disabled for all Phase 20 features; chain difficulty remains automatic via LWMA-144.

## Why staged (honest)
The live integration is a multi-thousand-line change across `iriumd` (~25k lines), `chain.rs`,
`stratum.rs`, `storage.rs`, `delegation.rs`, and the wallet — the exact paths the validated
trusted-miner flow depends on. Landing the consensus validator + extension first (this pass,
zero-regression) makes the live integration a smaller, reviewable, bisectable next step rather
than one risky mega-change. **Production wiring is therefore PARTIAL, not COMPLETE — not faked.**
