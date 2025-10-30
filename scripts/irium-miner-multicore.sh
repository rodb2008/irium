#!/bin/bash
PYTHONPATH=${PYTHONPATH:-$PWD}
export PYTHONPATH
set -euo pipefail
# Multi-core mining using full P2P miner. Requires IRIUM_WALLET_FILE to be set.

CORES=${1:-4}
BASE_PORT=${BASE_PORT:-38292}

if [ -z "${IRIUM_WALLET_FILE:-}" ]; then
  echo "❌ IRIUM_WALLET_FILE is not set. Example:"
  echo "   export IRIUM_WALLET_FILE=/path/to/your/irium-wallet.json"
  echo "   ./scripts/irium-miner-multicore.sh 4"
  exit 1
fi

echo "🔥 Starting $CORES full miners with P2P"
echo "💼 Wallet: $IRIUM_WALLET_FILE"
echo "📡 Miner P2P base port: $BASE_PORT (one port per worker)"
echo ""

# Stop any previous full miners launched by this script
pkill -f 'scripts/irium-miner.py' || true

# Spawn CORES full miners, each with its own P2P port
for i in $(seq 0 $((CORES-1))); do
  PORT=$((BASE_PORT + i))
  LOG="/tmp/miner-$PORT.log"
  nohup env IRIUM_WALLET_FILE="$IRIUM_WALLET_FILE" \
    python3 -u scripts/irium-miner.py "$PORT" > "$LOG" 2>&1 &
  echo "  ✅ Miner[$i] on port $PORT (PID $!) -> $LOG"
  sleep 0.5
done

echo ""
echo "⛏️  Mining with $CORES workers."
echo "ℹ️  Ensure a node is running (e.g., 'nohup python3 -u scripts/irium-node.py 38291 &')"
echo "📄 View logs: tail -f /tmp/miner-$BASE_PORT.log"
echo "🛑 Stop all: pkill -f 'scripts/irium-miner.py'"
