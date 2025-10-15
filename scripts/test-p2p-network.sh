#!/bin/bash
# Test P2P network with multiple nodes

echo "=== IRIUM P2P NETWORK TEST ==="
echo ""
echo "This will test P2P connectivity between:"
echo "  • Node 1: Port 38291"
echo "  • Node 2: Port 38293"
echo "  • Node 3: Port 38294"
echo ""
echo "Note: Since mainnet genesis is not mined, nodes will fail"
echo "to initialize blockchain but P2P layer will still work"
echo ""

cd /home/irium/irium

# Start node 1
echo "Starting Node 1 (port 38291)..."
timeout 20 python3 scripts/irium-node.py 38291 2>&1 | grep -E "Starting|P2P|Listening|peer|Error" &
NODE1_PID=$!

sleep 3

# Start node 2
echo "Starting Node 2 (port 38293)..."
timeout 20 python3 scripts/irium-node.py 38293 2>&1 | grep -E "Starting|P2P|Listening|peer|Error" &
NODE2_PID=$!

sleep 3

# Start node 3
echo "Starting Node 3 (port 38294)..."
timeout 20 python3 scripts/irium-node.py 38294 2>&1 | grep -E "Starting|P2P|Listening|peer|Error" &
NODE3_PID=$!

echo ""
echo "All nodes starting... waiting 15 seconds for connections..."
sleep 15

echo ""
echo "Stopping all nodes..."
kill $NODE1_PID $NODE2_PID $NODE3_PID 2>/dev/null
wait 2>/dev/null

echo ""
echo "=== TEST COMPLETE ==="
echo ""
echo "Expected results:"
echo "  ✅ Each node starts P2P server"
echo "  ✅ Nodes attempt to connect to seedlist"
echo "  ✅ Blockchain loading fails (no valid genesis)"
echo ""
echo "For full testing, we need to:"
echo "  1. Mine mainnet genesis first"
echo "  2. Or create separate testnet configuration"
