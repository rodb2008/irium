# PoAW-X Phase 20 — Production Wiring Status (PARTIAL: consensus validator + extension done; live node/pool/wallet integration follow-up)

**Status:** **PARTIAL.** The Phase 20 **consensus-validation + wire layer** is implemented and
tested (the integrated production validator + the receipt-extension type), gated and mainnet-off,
with **zero changes to the proven `iriumd` / pool / wallet / storage paths** (zero regression to
the validated Phase 18/19/19D trusted-miner flow). The **live integration** (threading the
extension through the node pending/persistence/reorg/P2P path, the `connect_block`/
`submit_block_extended` call site, pool coinbase production, wallet CLI, and registry
relaxation) is the **documented remaining step** — deliberately staged, not faked.

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
1. **Node receipt-wire threading** — carry `Phase20ReceiptExt` in the block receipt section
   (v3, present-only so v1/v2 stay byte-identical) through `iriumd` pending receipts,
   `storage` JSON persist/reload, reorg pending↔block mappers, and P2P block ser/de.
2. **connect_block / submit_block_extended** — call `validate_phase20_production_payout` when
   `phase20_production_active(height)` and the extension is present; reject missing/malformed
   extension after activation; keep pre-activation Phase 18/19 blocks valid.
3. **Pool production** — build the canonical multi-role (+ optional fee) coinbase in the
   stratum native_rewardable path; assemble the extension from registered delegations / role
   claims.
4. **Role-claim source** — real claims from miners, or a clearly-named testnet/devnet-only
   `IRIUM_POAWX_SYNTHETIC_ROLE_CLAIMS=1` synthetic builder (mainnet-impossible). Not added yet.
5. **Wallet CLI** — `--third-party-pool` / `--fee-bps` / `--fee-pkh` on `poawx-register`/
   `--emit-only` (fee terms already round-trip in the signed delegation).
6. **Pool registry** — relax `verify_and_store` (currently fail-closed on `fee_bps>0`) to accept
   capped third-party fees + persist `fee_pkh`, gated on third-party mode; reject mismatch/mutation.
7. **Observer + loopback smoke** — two-node + isolated `$TROOT` E2E (operator-approved, loopback).

## Why staged (honest)
The live integration is a multi-thousand-line change across `iriumd` (~25k lines), `chain.rs`,
`stratum.rs`, `storage.rs`, `delegation.rs`, and the wallet — the exact paths the validated
trusted-miner flow depends on. Landing the consensus validator + extension first (this pass,
zero-regression) makes the live integration a smaller, reviewable, bisectable next step rather
than one risky mega-change. **Production wiring is therefore PARTIAL, not COMPLETE — not faked.**
