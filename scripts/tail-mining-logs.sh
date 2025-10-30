#!/bin/bash
set -euo pipefail
# Tail node and miner logs.
# Usage:
#   ./scripts/tail-mining-logs.sh [CORES] [BASE_PORT]
# Defaults: CORES=4, BASE_PORT=38292

CORES=${1:-4}
BASE=${2:-38292}

echo "Tailing node log: /tmp/node.log (if present)"
[ -f /tmp/node.log ] && tail -n 50 -F /tmp/node.log &

echo "Tailing $CORES miner logs starting at port $BASE"
for i in $(seq 0 $((CORES-1))); do
  PORT=$((BASE + i))
  LOG="/tmp/miner-$PORT.log"
  if [ -f "$LOG" ]; then
    echo "  -> $LOG"
    tail -n 50 -F "$LOG" &
  else
    echo "  (missing) $LOG"
  fi
done

wait
