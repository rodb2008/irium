# PoAW-X Phase 20 — Third-Party Pool Fee (FULLY WIRED — Step 4 complete)

**Status:** **COMPLETE (Step 4, 2026-06-17).** The earlier follow-ups are now implemented end to
end: wallet `--third-party-pool`/`--fee-bps`/`--fee-pkh` CLI, pool identity advertising, pool
registry relaxation (`verify_and_store` accepts capped, signed, config-matching fees + persists
`fee_pkh`), fee-aware coinbase production (6th fee output from PRIMARY only), fee-aware receipt
extension, and live `connect_block` enforcement (mode-1 fee relaxed under the third-party gates +
ext↔delegation fee binding). See `poaw-x-phase20-production-wiring-status.md` (Step 4) for the full
inventory + tests. Official pool stays 0%; cap 2%; fee on PRIMARY only; miner-signed; mainnet
hard-off; chain difficulty remains LWMA-144 automatic. Local-only; not pushed.

> Historical note (pre-Step-4): only the consensus primitives + canonical fee-aware coinbase
> validator + activation/mode gates + delegation fee-binding were implemented; wallet CLI, pool
> registry relaxation, and live production enforcement were the documented follow-up — now done.

## Policy (implemented / enforced by the primitives)
- **Official Irium pool fee remains 0%**; `fee_bps = 0` is the default everywhere and is
  always allowed with no fee output.
- **Third-party fee cap = 200 bps (2.00%)** (`THIRD_PARTY_FEE_CAP_BPS`); `fee_bps > 200` rejects.
- A nonzero fee is allowed **only** with explicit third-party opt-in (`validate_fee_terms`
  requires `third_party_mode == true`) AND a non-zero `fee_pkh`.
- **Fee terms are signed by the miner and cannot be mutated:** the 226-byte `Delegation`
  already binds `fee_bps` + `fee_pkh` inside `message_hash()`. Test
  `phase20_delegation_binds_fee_terms` proves any change to `fee_bps` or `fee_pkh` both alters
  the hash and **breaks the signature**.
- **No hidden fee:** the validator rejects any value-bearing non-P2PKH output and any extra/
  mis-ordered P2PKH output.
- **Mainnet hard-off:** `chain::third_party_fee_active` and `third_party_pool_mode_enabled`
  return false on mainnet regardless of env. No mainnet/production activation.
- **Fee applies to PRIMARY_MINER only:** when multi-role is active the fee is taken from the
  PRIMARY allocation; COMPUTE/VERIFY/SUPPORT are never taxed.
- Chain difficulty remains automatic via LWMA-144 (untouched).

## What is implemented (this commit set)
`src/poawx.rs`:
- `THIRD_PARTY_FEE_CAP_BPS = 200`, `THIRD_PARTY_FEE_DOMAIN_V1`.
- `validate_fee_terms(fee_bps, fee_pkh, third_party_mode)` — fee-policy rules (cap, mode, pkh).
- `apply_fee(gross, fee_bps) -> (net, fee)` — `fee = floor(gross*bps/10000)`, miner keeps the
  remainder, `net + fee == gross` exactly.

`src/activation.rs` + `src/chain.rs`:
- `poawx_third_party_fee_activation_height()` (env `IRIUM_POAWX_THIRD_PARTY_FEE_ACTIVATION_HEIGHT`).
- `third_party_fee_active(height)` (mainnet hard-false) + `third_party_pool_mode_enabled()`
  (env `IRIUM_POAWX_THIRD_PARTY_POOL_MODE=1`, mainnet hard-false).
- `validate_poawx_coinbase_payout(outputs, primary_pkh, total_reward, role, fee)` — the single
  comprehensive canonical validator covering **all four formats** (official/third-party × no-
  multi-role/multi-role), fee from PRIMARY only, zero-value `irx1` OP_RETURN ignored.

## Canonical coinbase formats (validated)
- Official, no multi-role: `[irx1?] PRIMARY(total)`.
- Third-party fee, no multi-role: `[irx1?] PRIMARY(net) FEE(fee_pkh)`.
- Official, multi-role: `[irx1?] PRIMARY COMPUTE VERIFY SUPPORT`.
- Third-party fee, multi-role: `[irx1?] PRIMARY(net) COMPUTE VERIFY SUPPORT FEE(fee_pkh)`.

## Rounding
Integer atomic units; `fee = floor(primary_gross * fee_bps / 10000)`; miner keeps the
remainder; outputs sum to the allowed reward exactly (no over/underpay).

## Validation rejects (tested)
fee output in official mode · wrong fee amount · fee_pkh mismatch · taxing a role instead of
PRIMARY · hidden value-bearing non-p2pkh output · over-cap (`>200`) · fee>0 without third-party
mode · fee>0 without fee_pkh · mainnet gate/mode on.

## Tests (all passing)
poawx: `validate_fee_terms` (official/over-cap/no-mode/no-pkh), `apply_fee` (floor + remainder
+ zero), `delegation_binds_fee_terms` (round-trip + signature mutation-proof). chain:
`fee_aware_coinbase_payout` (all 4 formats + rejects), `third_party_fee_gate_mainnet_off_and_testnet`.

## Remaining (follow-up; PARTIAL — not faked COMPLETE)
1. **Wallet CLI**: `--third-party-pool` + `--fee-pkh` on `poawx-register`/`--emit-only`,
   allowing `fee_bps 1..200` gated, printing fee terms, with `emit-only == online body` tests.
   Deferred to keep the validated registration path byte-identical (zero regression risk).
2. **Pool registry**: relax `verify_and_store` (currently fail-closed on `fee_bps>0`) to accept
   capped third-party fees + persist `fee_pkh`, gated on third-party mode; reject mismatched
   fee terms / fee_pkh mutation.
3. **Live production enforcement**: build the canonical fee-aware coinbase in the pool and call
   `validate_poawx_coinbase_payout` from `connect_block`/`submit_block_extended` when
   `third_party_fee_active(height)`.
4. Ties to the multi-role production wiring + fairness commitment root (separate follow-ups).

Official pool stays 0% and `fee_bps>0` stays rejected on the live path until the follow-up
lands. Mainnet remains disabled.
