#!/bin/bash
# Multi-core mining - miners run WITHOUT P2P, main node broadcasts

CORES=${1:-8}

echo "🔥 Starting $CORES mining processes (no P2P)..."
echo "📡 Main node (port 38291) will handle all P2P and broadcasting"
echo ""

# Kill existing miners
pkill -f irium-simple-miner.py

# Start simple miners (no P2P)
for i in $(seq 1 $CORES); do
    nohup python3 -u scripts/irium-simple-miner.py > /tmp/miner-$i.log 2>&1 &
    echo "  ✅ Miner $i started (PID: $!, log: /tmp/miner-$i.log)"
    sleep 0.5
done

echo ""
echo "⛏️  Mining with $CORES cores!"
echo "📡 Blocks will be detected and broadcast by main node"
echo ""
echo "View logs: tail -f /tmp/miner-1.log"
echo "Stop all: pkill -f irium-simple-miner.py"

# Keep script running to prevent systemd from killing miners
while true; do
    sleep 60
    # Check if miners are still running
    if ! pgrep -f irium-simple-miner.py > /dev/null; then
        echo "⚠️  All miners stopped, exiting..."
        break
    fi
done
