# Irium v1.0.3 - Critical Blockchain Sync Fix

## Critical Fixes

✅ **Node now loads mined blocks from disk** - Height displays correctly
✅ **Blockchain sync working** - Nodes will sync to current height
✅ **Peer connections enabled** - Network can now grow

## Download

https://github.com/iriumlabs/irium/releases/download/v1.0.3/irium-bootstrap-v1.0.3.tar.gz

## Update Instructions

git clone https://github.com/iriumlabs/irium.git
cd irium
pip3 install --user pycryptodome qrcode pillow
python3 scripts/irium-node.py

Node will now correctly show height 3 and sync with the network!
