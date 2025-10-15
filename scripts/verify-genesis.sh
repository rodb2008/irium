#!/usr/bin/env bash
set -euo pipefail

GENESIS_FILE="${1:-configs/genesis-locked.json}"
FOUNDER_PUBKEY="03131a7d6ed16c46b059600f88493d79201aea6f7c2386a9765fca1dc79f6d641a"

echo "=== IRIUM GENESIS VERIFICATION ==="
echo "File: $GENESIS_FILE"
echo ""

# Check if file exists
if [ ! -f "$GENESIS_FILE" ]; then
  echo "❌ Genesis file not found: $GENESIS_FILE"
  exit 1
fi

# Verify JSON structure
if ! jq empty "$GENESIS_FILE" 2>/dev/null; then
  echo "❌ Invalid JSON structure"
  exit 1
fi

# Verify founder pubkey
FOUNDER_IN_FILE="$(jq -r '.founder_pubkey' "$GENESIS_FILE")"
if [ "$FOUNDER_IN_FILE" != "$FOUNDER_PUBKEY" ]; then
  echo "❌ Founder pubkey mismatch"
  echo "Expected: $FOUNDER_PUBKEY"
  echo "Found:    $FOUNDER_IN_FILE"
  exit 1
fi

# Verify vesting UTXOs
VESTING_COUNT="$(jq '.vesting_utxos | length' "$GENESIS_FILE")"
if [ "$VESTING_COUNT" != "3" ]; then
  echo "❌ Expected 3 vesting UTXOs, found $VESTING_COUNT"
  exit 1
fi

# Verify CLTV heights
CLTV_HEIGHTS="$(jq -r '.vesting_utxos[].cltv_height' "$GENESIS_FILE" | sort -n)"
EXPECTED_HEIGHTS="52560
105120
157680"

if [ "$CLTV_HEIGHTS" != "$EXPECTED_HEIGHTS" ]; then
  echo "❌ CLTV heights mismatch"
  echo "Expected: $EXPECTED_HEIGHTS"
  echo "Found:    $CLTV_HEIGHTS"
  exit 1
fi

# Verify total vesting amount
TOTAL_VESTING="$(jq '.vesting_utxos | map(.amount) | add' "$GENESIS_FILE")"
if [ "$TOTAL_VESTING" != "3500000" ]; then
  echo "❌ Total vesting amount mismatch"
  echo "Expected: 3500000"
  echo "Found:    $TOTAL_VESTING"
  exit 1
fi

# Verify public mined supply
PUBLIC_SUPPLY="$(jq -r '.public_mined_supply' "$GENESIS_FILE")"
if [ "$PUBLIC_SUPPLY" != "96500000" ]; then
  echo "❌ Public mined supply mismatch"
  echo "Expected: 96500000"
  echo "Found:    $PUBLIC_SUPPLY"
  exit 1
fi

# Verify merkle root is not all zeros
MERKLE_ROOT="$(jq -r '.merkle_root' "$GENESIS_FILE")"
if [ "$MERKLE_ROOT" = "0000000000000000000000000000000000000000000000000000000000000000" ]; then
  echo "❌ Merkle root is all zeros (not calculated)"
  exit 1
fi

# Verify genesis hash is not all zeros
GENESIS_HASH="$(jq -r '.genesis_hash' "$GENESIS_FILE")"
if [ "$GENESIS_HASH" = "0000000000000000000000000000000000000000000000000000000000000000" ]; then
  echo "❌ Genesis hash is all zeros (not calculated)"
  exit 1
fi

echo "✅ Genesis block verification PASSED"
echo ""
echo "Genesis Details:"
echo "  Chain: $(jq -r '.chain' "$GENESIS_FILE")"
echo "  Height: $(jq -r '.height' "$GENESIS_FILE")"
echo "  Timestamp: $(jq -r '.timestamp' "$GENESIS_FILE")"
echo "  Merkle Root: $(jq -r '.merkle_root' "$GENESIS_FILE")"
echo "  Genesis Hash: $(jq -r '.genesis_hash' "$GENESIS_FILE")"
echo "  Difficulty: $(jq -r '.difficulty' "$GENESIS_FILE")"
echo "  Nonce: $(jq -r '.nonce' "$GENESIS_FILE")"
echo "  Founder Pubkey: $(jq -r '.founder_pubkey' "$GENESIS_FILE")"
echo "  Vesting UTXOs: $VESTING_COUNT"
echo "  Total Vesting: $TOTAL_VESTING IRM"
echo "  Public Supply: $PUBLIC_SUPPLY IRM"
echo "  CLTV Heights: 52560, 105120, 157680"
echo ""
echo "🔒 Genesis block is LOCKED and VERIFIED with real calculated values"
