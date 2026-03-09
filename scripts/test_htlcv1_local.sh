#!/usr/bin/env bash
set -euo pipefail

RPC_URL="${RPC_URL:-http://127.0.0.1:38300}"
RECIPIENT_ADDR="${RECIPIENT_ADDR:-}"
REFUND_ADDR="${REFUND_ADDR:-}"
TIMEOUT_HEIGHT="${TIMEOUT_HEIGHT:-200}"

if [[ -z "$RECIPIENT_ADDR" || -z "$REFUND_ADDR" ]]; then
  echo "Set RECIPIENT_ADDR and REFUND_ADDR before running"
  exit 1
fi

SECRET_HEX=$(openssl rand -hex 32)
SECRET_HASH_HEX=$(printf "%s" "$SECRET_HEX" | xxd -r -p | sha256sum | awk '{print $1}')

echo "SECRET_HEX=$SECRET_HEX"
echo "SECRET_HASH_HEX=$SECRET_HASH_HEX"

echo "[1] createhtlc"
CREATE_JSON=$(curl -sS -X POST "$RPC_URL/rpc/createhtlc" \
  -H 'Content-Type: application/json' \
  -d "{\"amount\":\"1.00000000\",\"recipient_address\":\"$RECIPIENT_ADDR\",\"refund_address\":\"$REFUND_ADDR\",\"secret_hash_hex\":\"$SECRET_HASH_HEX\",\"timeout_height\":$TIMEOUT_HEIGHT,\"fee_per_byte\":1,\"broadcast\":true}")

echo "$CREATE_JSON"
FUNDING_TXID=$(echo "$CREATE_JSON" | sed -n 's/.*"txid":"\([^"]*\)".*/\1/p')
if [[ -z "$FUNDING_TXID" ]]; then
  echo "createhtlc failed (activation off or wallet/utxo issue)"
  exit 1
fi

echo "Funding txid: $FUNDING_TXID"
echo "Mine/confirm funding tx, then press Enter"
read -r _

echo "[2] inspecthtlc"
curl -sS "$RPC_URL/rpc/inspecthtlc?txid=$FUNDING_TXID&vout=0"
echo

echo "[3] claimhtlc valid"
curl -sS -X POST "$RPC_URL/rpc/claimhtlc" \
  -H 'Content-Type: application/json' \
  -d "{\"funding_txid\":\"$FUNDING_TXID\",\"vout\":0,\"destination_address\":\"$RECIPIENT_ADDR\",\"secret_hex\":\"$SECRET_HEX\",\"fee_per_byte\":1,\"broadcast\":false}"
echo

echo "[4] claimhtlc wrong secret (expect HTTP 400)"
set +e
curl -sS -X POST "$RPC_URL/rpc/claimhtlc" \
  -H 'Content-Type: application/json' \
  -d "{\"funding_txid\":\"$FUNDING_TXID\",\"vout\":0,\"destination_address\":\"$RECIPIENT_ADDR\",\"secret_hex\":\"deadbeef\",\"fee_per_byte\":1,\"broadcast\":false}"
set -e
echo

echo "[5] refundhtlc (expect fail before timeout, success after timeout)"
set +e
curl -sS -X POST "$RPC_URL/rpc/refundhtlc" \
  -H 'Content-Type: application/json' \
  -d "{\"funding_txid\":\"$FUNDING_TXID\",\"vout\":0,\"destination_address\":\"$REFUND_ADDR\",\"fee_per_byte\":1,\"broadcast\":false}"
set -e
echo
