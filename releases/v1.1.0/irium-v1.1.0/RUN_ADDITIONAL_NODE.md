# Running Additional Irium Nodes

To improve network decentralization, run nodes on multiple servers.

## Quick Setup on New Server

1. Install dependencies:
```bash
sudo apt update
sudo apt install python3 python3-pip git
```

2. Clone Irium:
```bash
cd ~
git clone https://github.com/iriumlabs/irium.git
cd irium
```

3. Run node (non-bootstrap):
```bash
# Remove BOOTSTRAP_NODE flag
python3 scripts/irium-node.py
```

4. Optional: Run miner:
```bash
python3 scripts/irium-miner.py
```

## The node will automatically:
- Connect to existing network via seedlist
- Discover other peers
- Sync blockchain
- Propagate blocks
- Add itself to runtime seedlist

## Network Benefits:
- Each additional node increases resilience
- Blockchain copies distributed
- No single point of failure
- Network survives even if multiple nodes go down
