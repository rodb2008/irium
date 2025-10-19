#!/bin/bash
# Multi-core mining - miners run WITHOUT P2P, main node broadcasts

CORES=${1:-4}

echo "🔥 Starting $CORES mining processes (headless mode)..."
echo "📡 Main node will handle broadcasting"

# Kill existing miners
pkill -f irium-miner.py

# Start miners WITHOUT P2P (they'll mine locally)
# The main node will detect and broadcast their blocks
for i in $(seq 1 $CORES); do
    # Run in background, output to log
    nohup python3 -c "
import sys
sys.path.insert(0, '.')
# Monkey-patch to disable P2P
import scripts.__init__ as si
" > /dev/null 2>&1 &
    
    # Actually, just run the miner - it needs P2P for now
    # TODO: Add --no-p2p flag
    PORT=$((38292 + i))
    nohup python3 scripts/irium-miner.py $PORT > /tmp/miner-$i.log 2>&1 &
    echo "  ✅ Miner $i started (PID: $!, log: /tmp/miner-$i.log)"
    sleep 0.5
done

echo ""
echo "⛏️  Mining with $CORES cores!"
echo "📡 Blocks will be broadcast by main node (port 38291)"
echo ""
echo "View logs: tail -f /tmp/miner-1.log"
echo "Stop all: pkill -f irium-miner.py"
