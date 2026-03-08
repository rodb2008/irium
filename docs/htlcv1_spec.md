# Irium HTLCv1 Specification

## Scope

HTLCv1 is a **minimal native output encumbrance** for atomic-swap style contracts.
It is not a general script VM and does not add smart contracts.

HTLCv1 is consensus-gated by `htlcv1_activation_height`.

- `None`: disabled.
- `Some(h)`: active at spend/validation heights `>= h`.

Mainnet default is disabled.

## Output Encoding

HTLCv1 uses `script_pubkey` bytes with fixed size `83`:

- byte `0`: tag = `0xc0`
- byte `1`: version = `0x01`
- byte `2`: hash algorithm = `0x01` (SHA-256 only)
- bytes `3..35`: `expected_hash` (32 bytes)
- bytes `35..55`: `recipient_pkh` (20 bytes, HASH160 pubkey)
- bytes `55..75`: `refund_pkh` (20 bytes, HASH160 pubkey)
- bytes `75..83`: `timeout_height` (`u64`, little-endian)

Reference implementation:
- `src/tx.rs::encode_htlcv1_script`
- `src/tx.rs::parse_htlcv1_script`

## Witness Encoding

Witness data is carried in input `script_sig` as tagged binary payload.

### Claim witness

- byte `0`: witness type = `0x01`
- byte `1`: signature length `sig_len`
- bytes following: DER signature + sighash byte
- next byte: pubkey length `pk_len`
- next `pk_len` bytes: SEC1 pubkey (33 or 65 bytes)
- next byte: preimage length `pre_len`
- next `pre_len` bytes: preimage

Reference:
- `src/tx.rs::encode_htlcv1_claim_witness`
- `src/tx.rs::parse_input_witness`

### Refund witness

- byte `0`: witness type = `0x02`
- byte `1`: signature length `sig_len`
- next `sig_len` bytes: DER signature + sighash byte
- next byte: pubkey length `pk_len`
- next `pk_len` bytes: SEC1 pubkey (33 or 65 bytes)

Reference:
- `src/tx.rs::encode_htlcv1_refund_witness`
- `src/tx.rs::parse_input_witness`

## Consensus Rules

Reference implementation:
- `src/chain.rs::validate_output`
- `src/chain.rs::verify_transaction_signature`

### Funding output (HTLCv1 script_pubkey)

- Before activation: rejected.
- After activation: accepted only if fixed-format parser succeeds.

### Claim spend

For UTXO encumbered by HTLCv1, claim spend is valid iff:

1. witness type is claim
2. `SHA256(preimage) == expected_hash`
3. `HASH160(pubkey) == recipient_pkh`
4. signature verifies against existing tx sighash rules using HTLC script as scriptCode

### Refund spend

For UTXO encumbered by HTLCv1, refund spend is valid iff:

1. witness type is refund
2. `spend_height >= timeout_height`
3. `HASH160(pubkey) == refund_pkh`
4. signature verifies against existing tx sighash rules using HTLC script as scriptCode

Malformed witness, wrong path, wrong pubkey hash, wrong preimage, or invalid signature fails.

## Mempool Policy

Mempool admission uses the same fee/signature/consensus checks as transaction validation path.
Therefore HTLCv1 is implicitly rejected before activation and accepted after activation only when consensus-valid.

## RPC Surface

Implemented in `src/bin/iriumd.rs`:

- `POST /rpc/createhtlc`
- `POST /rpc/decodehtlc`
- `POST /rpc/claimhtlc`
- `POST /rpc/refundhtlc`
- `GET /rpc/inspecthtlc?txid=<hex>&vout=<n>`

All endpoints require existing RPC auth/rate-limit flow.

## Safety Notes

- HTLCv1 is disabled by default on mainnet.
- Use block-height timeout only in v1.
- Use longer timeout on chain A and shorter timeout on chain B in cross-chain swaps.
- Wait confirmations before reveal/claim across chains.
