# Irium v1.0.7 - Stability & Documentation 📚

## What's New

✅ **P2P Connection Stability** - Improved peer connection reliability
✅ **Wallet/Mining Documentation** - Clear guide on wallet address behavior  
✅ **Better Error Handling** - Graceful handling of network errors
✅ **Mining Guide Updates** - Step-by-step wallet management instructions

## P2P Improvements

- **Ping interval**: Increased from 60s to 120s (less aggressive)
- **Peer timeout**: Increased from 180s to 300s (more tolerant)
- **Error handling**: Better handling of connection errors
- **Result**: Significantly more stable peer connections!

## Documentation Updates

- **MINING.md**: Complete guide on wallet and mining address behavior
- **README.md**: Important notes about wallet/miner interaction  
- **QUICKSTART.md**: Enhanced wallet management section

## Key Points for Users

🔑 **The miner loads your wallet when it starts**
- If you create a new address, restart the miner to use it
- First address in wallet is used for mining rewards
- Check mining address: `sudo journalctl -u irium-miner.service | grep "Mining address"`

## Network Status

- **Current Height**: 3 blocks (mining block 4)
- **Active Peers**: 2-3 stable connections
- **Miner Status**: ✅ Stable and hashing
- **P2P Network**: ✅ Much improved stability

## Upgrade Instructions

```bash
cd irium-bootstrap-v1.0.6  # or your install directory
git pull origin main
sudo systemctl restart irium-miner.service
sudo systemctl restart irium-node.service
```

Or download fresh:
```bash
wget https://github.com/iriumlabs/irium/releases/download/v1.0.7/irium-bootstrap-v1.0.7.tar.gz
tar -xzf irium-bootstrap-v1.0.7.tar.gz
cd irium-bootstrap-v1.0.7
./install.sh
```

---

**🎊 Irium is stable and ready for mining!**

Join the network: https://www.iriumlabs.org
