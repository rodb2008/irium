#!/bin/bash
# Quick genesis mining progress checker

cd /home/irium/irium

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  ⛏️  GENESIS MINING PROGRESS"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

# Check if process is running
if ps -p 144679 > /dev/null 2>&1; then
    echo "✅ Mining process: RUNNING (PID 144679)"
    ps -p 144679 -o pid,etime,pcpu,rss --no-headers
else
    echo "❌ Mining process: STOPPED"
    echo "   Restart: nohup python3 scripts/mine-genesis.py > genesis-mainnet-mining.log 2>&1 &"
fi

echo ""
echo "📊 Latest progress:"
tail -5 genesis-mainnet-mining.log

echo ""
echo "🔍 Check if complete:"
if grep -q "Found valid genesis block" genesis-mainnet-mining.log 2>/dev/null; then
    echo "🎉 GENESIS FOUND!"
    grep -A 10 "Found valid genesis block" genesis-mainnet-mining.log
else
    echo "⏳ Still mining..."
    current=$(grep 'Nonce:' genesis-mainnet-mining.log | tail -1 | awk '{print $2}' | tr -d ',')
    if [ ! -z "$current" ]; then
        progress=$((current / 42949672))  # Percent of 2^32
        echo "   Progress through current timestamp: ${progress}%"
    fi
fi

echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
