# Atomic Swaps on Irium (HTLCv1)

## Status

Irium now includes a **minimal HTLCv1 primitive** behind activation gating.

- Default mainnet behavior: HTLCv1 disabled (`htlcv1_activation_height = None`).
- Non-HTLC and legacy P2PKH behavior remains unchanged.

## What was implemented

1. Consensus types and encodings for HTLCv1 outputs/witnesses.
2. Activation-gated consensus validation for funding/claim/refund paths.
3. Activation-aware mempool admission via existing transaction validation path.
4. RPC endpoints for HTLC funding, decode, claim, refund, and inspection.
5. Unit tests for serialization + consensus behavior + legacy regression.

Reference spec: `docs/htlcv1_spec.md`.

## HTLCv1 contract model

Funding output stores:
- `expected_hash` (`SHA256` preimage hash)
- `recipient_pkh`
- `refund_pkh`
- `timeout_height` (absolute block height)

Two spend paths:

- Claim path:
  - recipient signature valid
  - `SHA256(preimage) == expected_hash`

- Refund path:
  - current chain height `>= timeout_height`
  - refund signature valid

No general script VM is introduced.

## Activation behavior

Activation is controlled by chain params (`ChainParams::htlcv1_activation_height`).

- before activation: HTLC outputs/spends are rejected
- at/after activation: HTLC outputs/spends are validated under HTLCv1 rules

`iriumd` reads activation from env:

- `IRIUM_HTLCV1_ACTIVATION_HEIGHT=<height>`

If unset, HTLCv1 remains disabled.

## RPC usage

### Create funding tx

`POST /rpc/createhtlc`

Body:

```json
{
  "amount": "1.00000000",
  "recipient_address": "<base58>",
  "refund_address": "<base58>",
  "secret_hash_hex": "<64-hex>",
  "timeout_height": 200000,
  "fee_per_byte": 1,
  "broadcast": true
}
```

### Decode HTLC output

`POST /rpc/decodehtlc`

Body:

```json
{
  "raw_tx_hex": "<hex>",
  "vout": 0
}
```

### Claim HTLC

`POST /rpc/claimhtlc`

Body:

```json
{
  "funding_txid": "<txid>",
  "vout": 0,
  "destination_address": "<base58>",
  "secret_hex": "<preimage-hex>",
  "fee_per_byte": 1,
  "broadcast": true
}
```

### Refund HTLC

`POST /rpc/refundhtlc`

Body:

```json
{
  "funding_txid": "<txid>",
  "vout": 0,
  "destination_address": "<base58>",
  "fee_per_byte": 1,
  "broadcast": true
}
```

### Inspect HTLC state

`GET /rpc/inspecthtlc?txid=<txid>&vout=<n>`

## Operational guidance for cross-chain swaps

- Use timeout asymmetry:
  - Chain A timeout (T1) longer
  - Chain B timeout (T2) shorter
  - enforce `T2 < T1`
- Wait adequate confirmations before revealing secret on the opposite chain.
- Do not reuse swap secrets across unrelated swaps.

## Known limitations

- No routing layer
- No orderbook/coordinator
- No GUI orchestration
- No cross-chain automation

HTLCv1 provides only the on-chain primitive for safe future swap tooling.
