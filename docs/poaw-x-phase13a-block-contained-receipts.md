# PoAW-X Phase 13-A: Block-Contained Receipt Data

## Summary

Phase 13-A adds deterministic PoAW-X receipt data to the block wire format
and JSON storage so every node can validate PoAW-X receipts from block-contained
data rather than relying on local RPC pending-receipt state.

This is the P-1 prerequisite for Phase 13-B (P-2 consensus enforcement).

## Design Chosen

**Wire-format extension with magic-byte sentinel.**

Receipt data is appended after all transactions using an 8-byte magic prefix.
The magic is detected before `decode_full_tx_at` is called, so the transaction
parse loop never touches receipt bytes. Legacy blocks (no magic) parse exactly
as before.

## Wire Format

```
[80-byte header]
[transactions...]
[POAWX_RECEIPT_SECTION_MAGIC: 8 bytes]   b"POAWXR\x01\x00"
[count: u8]
[receipt_0: 166 bytes]
...
[receipt_N: 166 bytes]
```

The receipt section is only appended when `poawx_receipts.is_some() && !receipts.is_empty()`.

### Per-Receipt Wire Layout (166 bytes, all little-endian)

| Field            | Size (bytes) | Type        |
|------------------|-------------|-------------|
| height           | 8           | u64 LE      |
| lane             | 1           | u8 (ASCII)  |
| worker_pkh       | 20          | [u8; 20]    |
| worker_pubkey    | 33          | [u8; 33]    |
| worker_sig       | 64          | [u8; 64]    |
| solution         | 8           | [u8; 8]     |
| commitment_nonce | 32          | [u8; 32]    |
| **Total**        | **166**     |             |

## Storage Format (JSON)

`blocks/<height>.json` gains an optional `"poawx_receipts"` array:

```json
{
  "poawx_receipts": [
    {
      "height": 12345,
      "lane": "A",
      "worker_pkh": "<hex 40 chars>",
      "worker_pubkey": "<hex 66 chars>",
      "worker_sig": "<hex 128 chars>",
      "solution": "<hex 16 chars>",
      "commitment_nonce": "<hex 64 chars>"
    }
  ]
}
```

The field is omitted (`skip_serializing_if = "Option::is_none"`) for blocks without receipts.

## Compatibility Rules

| Context          | Behaviour                                                   |
|------------------|-------------------------------------------------------------|
| Mainnet          | Unchanged — `poawx_receipts: None` always, no magic bytes  |
| Pre-activation   | Unchanged — receipts absent, wire bytes identical to before |
| Post-activation  | `submit_block_extended` attaches receipts from request      |
| Legacy readers   | `deserialize` / `deserialize_for_height` skip magic section |
| P2P relay        | `broadcast_block` uses `block.serialize_for_height()` — receipts included automatically |

## Fields Included / Excluded

**Included** (minimum to recompute `irx1_root`):
- `height`, `lane`, `worker_pkh`, `worker_pubkey`, `worker_sig`, `solution`, `commitment_nonce`

**Excluded** (not needed for root recomputation, reduce block size):
- Worker IP, submission timestamp, internal request IDs

## Security / Privacy Notes

- No PII included in the receipt section.
- Worker public key hash (`worker_pkh`) is already public (in the coinbase).
- `worker_pubkey` and `worker_sig` are cryptographic material, not identifying.
- Receipt section magic (`POAWXR\x01\x00`) version byte allows future format migration.

## Files Changed

| File                          | Change                                                         |
|-------------------------------|----------------------------------------------------------------|
| `src/poawx.rs`                | Added `POAWX_RECEIPT_SECTION_MAGIC`, `PoawxBlockReceipt` struct + impl, Phase 13-A tests |
| `src/block.rs`                | Added `poawx_receipts` field to `Block`; updated `serialize`, `serialize_for_height`, `deserialize`, `deserialize_for_height`; Phase 13-A tests |
| `src/storage.rs`              | Added `JsonPoawxReceipt`, updated `JsonBlock`, `write_block_json_sync` |
| `src/chain.rs`                | Updated 3 Block literals with `poawx_receipts: None` |
| `src/bin/iriumd.rs`           | Added `pending_receipt_to_block_receipt` helper; updated `submit_block_extended` to attach receipts; updated broadcast to use `block.serialize_for_height()`; updated chain loading to parse `poawx_receipts` from JSON; updated Block literals |
| `src/bin/irium-genesis.rs`    | Updated Block literal |
| `src/bin/irium-miner.rs`      | Updated 4 Block literals |
| `src/bin/irium-miner-gpu.rs`  | Updated 1 Block literal |

## Test Results

```
cargo build --release  →  Finished (0 errors, 3 pre-existing warnings)
cargo test             →  557 lib + 293 lib (debug) + 236 iriumd + 26 + 16 + 5 = all pass
```

Phase 13-A tests:

| Test | File | Covers |
|------|------|--------|
| `phase13a_receipt_serialize_deserialize_roundtrip` | poawx.rs | PoawxBlockReceipt wire roundtrip |
| `phase13a_receipt_wire_size_is_166` | poawx.rs | WIRE_SIZE constant |
| `phase13a_receipt_truncated_deserialize_fails` | poawx.rs | Malformed receipt rejected |
| `phase13a_receipt_section_magic_length` | poawx.rs | Magic byte array length |
| `phase13a_block_with_receipts_serialize_deserialize_roundtrip` | block.rs | Full block wire roundtrip with receipts |
| `phase13a_block_without_receipts_no_magic_appended` | block.rs | Legacy wire bytes unchanged |
| `phase13a_block_empty_receipts_vec_no_magic` | block.rs | Empty Some([]) = no magic |
| `phase13a_truncated_receipt_section_rejected` | block.rs | Truncated receipt section → error |
| `phase13a_receipts_count_byte_correct` | block.rs | Count byte + total length for 3 receipts |

## Remaining Work (Phase 13-B / P-2)

- `connect_block` in `chain.rs` to verify `irx1_root` is recomputable from `block.poawx_receipts`
- Reject blocks missing receipt data after activation when coinbase contains an `irx1` commitment
- Full P-2 consensus enforcement gate
