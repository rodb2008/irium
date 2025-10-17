#!/bin/bash
echo "🚀 Installing Irium Blockchain..."
echo ""
if ! command -v python3 &> /dev/null; then
    echo "❌ Python 3 required"
    exit 1
fi
echo "✅ Python 3 found"
echo "📦 Installing dependencies..."
pip3 install --user pycryptodome qrcode pillow 2>/dev/null || \
sudo apt install -y python3-pycryptodome python3-qrcode python3-pil 2>/dev/null
echo "✅ Installation complete!"
echo ""
echo "Commands:"
echo "  python3 scripts/irium-wallet-proper.py new-address"
echo "  python3 scripts/irium-node.py"
echo "  python3 scripts/irium-miner.py"
