#!/bin/bash
echo "=== STARTING IRIUM BLOCKCHAIN ==="

# 1. Initialize blockchain with genesis
echo "1. Initializing blockchain..."
python3 scripts/init-blockchain.py

if [ $? -ne 0 ]; then
    echo "❌ Blockchain initialization failed"
    exit 1
fi

echo ""
echo "2. Starting services..."

# Start node
sudo systemctl start irium-node
echo "✅ Node started"

# Start miner
sudo systemctl start irium-miner
echo "✅ Miner started"

# Start wallet API
sudo systemctl start irium-wallet-api
echo "✅ Wallet API started"

echo ""
echo "3. Checking service status..."
sudo systemctl status irium-node --no-pager -l | head -10
sudo systemctl status irium-miner --no-pager -l | head -10

echo ""
echo "✅ IRIUM BLOCKCHAIN IS RUNNING!"
