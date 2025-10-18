#!/bin/bash
# Simple multi-core mining - runs multiple miner instances on different ports

CORES=${1:-4}  # Default 4 cores, or specify: ./irium-miner-multicore.sh 8

echo "🔥 Starting $CORES mining processes..."

# Kill any existing miners
pkill -f irium-miner.py

# Start multiple miners in background with different P2P ports
PIDS=""
for i in $(seq 1 $CORES); do
    PORT=$((38292 + i))
    nohup python3 scripts/irium-miner.py $PORT > /tmp/miner-$PORT.log 2>&1 &
    PIDS="$PIDS $!"
    echo "✅ Miner $i started on port $PORT (PID: $!, log: /tmp/miner-$PORT.log)"
    sleep 1
done

echo ""
echo "⛏️  Mining with $CORES cores!"
echo "View logs: tail -f /tmp/miner-38293.log"
echo "Stop all: pkill -f irium-miner.py"
echo ""
