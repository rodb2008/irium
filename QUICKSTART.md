# Irium Quick Start Guide

## Installation

```bash
git clone https://github.com/iriumlabs/irium.git
cd irium
pip3 install --user pycryptodome qrcode pillow
```

## Commands

**Create Wallet:**
```bash
python3 scripts/irium-wallet-proper.py new-address
```

**Run Node:**
```bash
python3 scripts/irium-node.py
```

**Start Mining:**
```bash
python3 scripts/irium-miner.py
```

**Check Balance:**
```bash
python3 scripts/irium-wallet-proper.py balance
```

## Resources
- Website: https://www.iriumlabs.org
- Whitepaper: https://www.iriumlabs.org/whitepaper.html
- Telegram: https://t.me/iriumlabs

## Wallet Management

### Creating Addresses

```bash
# Create a new address
python3 scripts/irium-wallet-proper.py new-address

# List all addresses
python3 scripts/irium-wallet-proper.py list

# Check balance
python3 scripts/irium-wallet-proper.py balance
```

### Important: Mining Address

**The miner uses the wallet that exists when it starts.**

If you create a new address while mining:
```bash
# Restart miner to use the new address
sudo systemctl restart irium-miner.service

# Verify it's using the new address
sudo journalctl -u irium-miner.service -n 20 | grep "Mining address"
```


## Understanding Blockchain Sync

**"My node is stuck at height 3!"**

Check the network:
```bash
# See what height peers are at
journalctl -u irium-node.service -n 20 | grep "Status.*peers.*height"
```

If peers show "height 3", then **you're in sync!** ✅

Everyone is waiting for the next block to be mined (~6 hours average).

**Sync happens automatically when:**
- A peer mines a new block
- Your node detects they're ahead
- Your node requests and downloads the block

No manual intervention needed! 🚀

## API Usage

### Explorer API Examples

```bash
# Check network status
curl https://api.iriumlabs.org/api/stats

# Get latest block
curl https://api.iriumlabs.org/api/block/1

# Get recent blocks
curl https://api.iriumlabs.org/api/blocks?limit=5
```

### Wallet API Examples

```bash
# Access interactive documentation
curl https://api.iriumlabs.org/wallet/

# Check balance via API
curl https://api.iriumlabs.org/wallet/balance

# Create new address via API
curl -X POST https://api.iriumlabs.org/wallet/new-address
```

### API Base URL
```
https://api.iriumlabs.org/
```

## NAT & Firewall Support

**Irium works behind NAT/firewalls!** (Same as Bitcoin)

### NAT Nodes (No Port Forwarding Required)
- ✅ Can sync blockchain
- ✅ Can mine blocks
- ✅ Can broadcast transactions
- ✅ Connect outbound to public nodes
- ⚠️ Cannot accept incoming connections

### Public Nodes (Port Forwarding Enabled)
- ✅ All NAT node features PLUS
- ✅ Accept incoming connections
- ✅ Help bootstrap the network
- ✅ Improve network resilience

### Network Requirements
- **Minimum:** 1 public bootstrap node (VPS: 207.244.247.86)
- **Recommended:** Multiple public nodes for redundancy
- **NAT nodes:** Unlimited (connect via public nodes)

### Testing Your Node
```bash
# Check peer connections
journalctl -u irium-node -n 20 | grep "peers connected"

# If you see 1+ peers, you're connected! ✅
# NAT nodes typically connect to 2-3 public peers
```

**Note:** NAT-to-NAT direct connections are not possible (same limitation as Bitcoin).
