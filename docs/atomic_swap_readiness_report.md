# Irium Atomic Swap Readiness Report

Date: 2026-03-09
Codebase audited: Rust node/wallet in `src/` and `src/bin/`

## 1. Current transaction / locking model summary

### Transaction format
- `src/tx.rs`
  - `Transaction { version, inputs, outputs, locktime }`
  - `TxInput { prev_txid, prev_index, script_sig, sequence }`
  - `TxOutput { value, script_pubkey }`
- Encoding is compact custom binary (count + 1-byte script lengths), not Bitcoin varint script container format.

### Consensus validation model
- `src/chain.rs::validate_transaction_internal`
  - All non-coinbase spends must reference an existing UTXO.
  - Signature verification is mandatory via `verify_transaction_signature`.
- `src/chain.rs::verify_transaction_signature`
  - Hard-coded P2PKH verification only.
  - Requires output `script_pubkey` to match exactly `OP_DUP OP_HASH160 PUSH20 <pkh> OP_EQUALVERIFY OP_CHECKSIG`.
  - Requires `script_sig` to be exactly DER+SIGHASH + pubkey push pair.
  - No script interpreter / VM execution.

### Wallet + RPC spend construction
- `src/bin/iriumd.rs::wallet_send` and `src/bin/irium-wallet.rs`
  - Construct only P2PKH outputs.
  - Sign only P2PKH UTXOs.
  - Coin selection assumes wallet-owned P2PKH outputs.

### Mempool policy
- `src/mempool.rs`
  - Fee-per-byte policy and eviction only.
  - No script standardness beyond consensus checks performed before mempool admission.

## 2. Primitive support status

### Hashlock support (preimage hash check)
- **Not supported** in consensus spend rules.
- SHA-256 exists for PoW/signature digesting, but not as spend-condition predicate.

### Timelock support
- Transaction has `locktime` and inputs have `sequence` fields (`src/tx.rs`), but:
- **No consensus enforcement** of CLTV/CSV-like rules in `src/chain.rs`.
- Therefore practical timelock spending rules are **not supported**.

### Conditional spending (IF/ELSE)
- **Not supported**.
- No script VM, no branching opcodes, no redeem-script evaluation path.

### P2SH / redeem-script equivalent
- **Not supported**.
- Output scripts are validated as strict P2PKH template in signature verification path.

## 3. Can atomic swaps be done today without consensus changes?

**No.**

Reason: current consensus only accepts P2PKH signature spends and has no native hashlock + timelock dual-path encumbrance.

## 4. Minimum required consensus additions

Minimum safe addition (without introducing a general VM):

1. New output encumbrance type: `HTLCv1`
   - Fields: `hash_alg`, `secret_hash32`, `claim_pubkey_hash20`, `refund_pubkey_hash20`, `timeout_height`.
2. New unlock witness variants for spends of `HTLCv1` UTXO:
   - Claim witness: `sig + pubkey + preimage`
   - Refund witness: `sig + pubkey`
3. Consensus validation logic for two spend paths:
   - Claim: `sha256(preimage) == secret_hash` and signer matches claim key.
   - Refund: `current_height >= timeout_height` and signer matches refund key.
4. Strict canonical encoding and size bounds for HTLC fields/witness.
5. Mempool policy rules allowing standard HTLC funding/spend forms.

## 5. Recommended implementation path

### Path A (no consensus change)
- Not viable for trustless Bitcoin-style atomic swaps.
- At best possible only with trusted intermediaries / custodial off-chain coordination.

### Path B (minimal consensus change)
- Add **narrow HTLCv1 primitive only** (no general script engine).
- Gate with explicit network upgrade flag/activation height.
- Keep existing P2PKH flow unchanged.
- Add wallet/RPC support (`createhtlc`, `claimhtlc`, `refundhtlc`, `decodehtlc`, `inspecthtlc`) only after consensus path is test-complete.

## 6. Risk assessment

### Consensus risk
- High if activated without a clear upgrade mechanism and compatibility window.
- Mitigation: explicit activation height + node version enforcement + exhaustive tests.

### Mempool risk
- Medium: policy/standardness mismatch can cause relay friction.
- Mitigation: policy rules mirror consensus HTLC acceptance.

### Wallet UX risk
- Medium-high: accidental spending/confusion for HTLC UTXOs.
- Mitigation: isolate HTLC UTXOs from normal coin selection unless explicitly opted in.

### Backward compatibility
- Medium: old nodes cannot validate new encumbrance after activation.
- Mitigation: staged activation and release coordination.

## 7. Audit conclusion

- Atomic swaps are **not currently possible** on Irium mainnet semantics without consensus changes.
- Safest route is Path B minimal `HTLCv1` under staged activation.
- No safe "always-on immediate" implementation is recommended without first introducing and validating upgrade mechanics.
