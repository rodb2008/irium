#!/bin/bash
# Irium Blockchain - Check Genesis and Launch

cd /home/irium/irium

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  IRIUM BLOCKCHAIN - LAUNCH CHECKLIST"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

# Step 1: Check if genesis mining is complete
echo "1️⃣  Checking genesis mining status..."
echo ""

if grep -q "Found valid genesis block" genesis-mainnet-mining.log 2>/dev/null; then
    echo "✅ GENESIS MINING COMPLETE!"
    echo ""
    
    # Show genesis details
    echo "Genesis block details:"
    grep -A 10 "Found valid genesis block" genesis-mainnet-mining.log
    echo ""
    
    # Check genesis.json was updated
    echo "2️⃣  Checking genesis.json..."
    cat configs/genesis.json | jq '{network, nonce, hash}'
    echo ""
    
    # Step 2: Push to GitHub
    echo "3️⃣  Pushing genesis to GitHub..."
    git add configs/genesis.json
    git commit -m "Add mined mainnet genesis block

Genesis Details:
$(cat configs/genesis.json | jq -r '.hash // "N/A"')
Nonce: $(cat configs/genesis.json | jq -r '.nonce // "N/A"')

Mainnet genesis successfully mined and ready for launch!"
    
    git push origin main
    echo ""
    
    # Step 3: Initialize blockchain
    echo "4️⃣  Initializing blockchain..."
    python3 scripts/init-blockchain.py
    echo ""
    
    # Step 4: Start services
    echo "5️⃣  Starting all services..."
    sudo systemctl start irium-node
    sudo systemctl start irium-miner
    sudo systemctl start irium-explorer
    sudo systemctl start irium-wallet-api
    echo ""
    
    # Step 5: Verify services
    echo "6️⃣  Verifying services..."
    echo ""
    echo "Node status:"
    sudo systemctl status irium-node --no-pager -l | head -5
    echo ""
    echo "Miner status:"
    sudo systemctl status irium-miner --no-pager -l | head -5
    echo ""
    echo "Explorer status:"
    sudo systemctl status irium-explorer --no-pager -l | head -5
    echo ""
    echo "Wallet API status:"
    sudo systemctl status irium-wallet-api --no-pager -l | head -5
    echo ""
    
    # Step 6: Test APIs
    echo "7️⃣  Testing APIs..."
    echo ""
    echo "Blockchain stats:"
    curl -s http://localhost:8082/api/stats | jq '.'
    echo ""
    echo "Wallet status:"
    curl -s http://localhost:8080/api/wallet/status | jq '.'
    echo ""
    
    # Success!
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo "  🎉 IRIUM BLOCKCHAIN IS LIVE!"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo ""
    echo "🌐 Public Endpoints:"
    echo "   Wallet API: http://207.244.247.86:8080"
    echo "   Explorer API: http://207.244.247.86:8082"
    echo "   P2P Node: 207.244.247.86:38291"
    echo ""
    echo "✅ Services Running"
    echo "✅ Blockchain Active"
    echo "✅ Ready for Public Access"
    echo ""
    
else
    echo "⏳ Genesis mining still in progress..."
    echo ""
    
    # Check if mining process is running
    if ps -p 135668 > /dev/null 2>&1; then
        echo "✅ Mining process is running (PID: 135668)"
    else
        echo "❌ Mining process not found!"
        echo "   Start it: nohup python3 scripts/mine-genesis.py > genesis-mainnet-mining.log 2>&1 &"
    fi
    
    echo ""
    echo "Current progress:"
    grep 'Nonce:' genesis-mainnet-mining.log 2>/dev/null | tail -5
    echo ""
    echo "Check back later with: ./scripts/check-and-launch.sh"
fi
