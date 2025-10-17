#!/bin/bash
echo "🚀 Installing Irium Blockchain..."
echo ""

# Check Python
if ! command -v python3 &> /dev/null; then
    echo "❌ Python 3 is required. Please install Python 3.8 or higher."
    exit 1
fi

echo "✅ Python 3 found"

# Install dependencies
echo ""
echo "📦 Installing dependencies..."
pip3 install qrcode[pil] 2>/dev/null || pip3 install qrcode pillow

echo ""
echo "✅ Installation complete!"
echo ""
echo "📋 Available commands:"
echo ""
echo "  Create Wallet:"
echo "    python3 scripts/irium-wallet-proper.py new-address"
echo ""
echo "  Run Node:"
echo "    python3 scripts/irium-node.py"
echo ""
echo "  Start Mining:"
echo "    python3 scripts/irium-miner.py"
echo ""
echo "  Check Balance:"
echo "    python3 scripts/irium-wallet-proper.py balance"
echo ""
echo "🎉 Ready to use Irium!"
