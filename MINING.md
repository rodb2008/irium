# Irium Mining Guide

Complete guide to mining IRM cryptocurrency.

## Mining Rewards
- Current: 50 IRM per block
- Halving: Every 210,000 blocks (~4 years)
- Algorithm: SHA-256d (Bitcoin-compatible)

## Quick Start
```bash
# 1. Create wallet
python3 scripts/irium-wallet-proper.py create

# 2. Start mining
python3 scripts/irium-miner.py
```

## Hardware
- CPU: Any modern processor
- GPU: Bitcoin SHA-256d compatible
- ASIC: Bitcoin ASICs work

## Expected Earnings
- Block reward: 50 IRM
- Block time: 10 minutes
- Difficulty: Adjusts every 2016 blocks

## Wallet and Mining Address

### Important: Miner Loads Wallet at Startup

The miner loads your wallet when it starts. If you create a new wallet address while the miner is running, you need to **restart the miner** to use the new address.

### How It Works

1. **First Time Mining:**
   - If no wallet exists at `~/.irium/irium-wallet.json`, the miner creates one automatically
   - The first address in the wallet is used for mining rewards

2. **Using an Existing Wallet:**
   - The miner loads `~/.irium/irium-wallet.json` at startup
   - Uses the first address in the wallet for mining rewards

3. **Creating a New Address:**
   ```bash
   # Create new address
   python3 scripts/irium-wallet-proper.py new-address
   
   # Restart miner to use the NEW address
   sudo systemctl restart irium-miner.service
   
   # Verify the mining address
   sudo journalctl -u irium-miner.service -n 20 | grep "Mining address"
   ```

### Using a Specific Address for Mining

If you want to mine to a specific address:

```bash
# 1. Stop the miner
sudo systemctl stop irium-miner.service

# 2. Backup old wallet (optional)
cp ~/.irium/irium-wallet.json ~/.irium/irium-wallet.json.backup

# 3. Create new wallet with your desired address
rm ~/.irium/irium-wallet.json
python3 scripts/irium-wallet-proper.py new-address

# 4. Start miner
sudo systemctl start irium-miner.service

# 5. Verify
sudo journalctl -u irium-miner.service -n 20 | grep "Mining address"
```

### Check Current Mining Address

```bash
# See what address the miner is currently using
sudo journalctl -u irium-miner.service | grep "Mining address" | tail -1

# See all addresses in your wallet
python3 scripts/irium-wallet-proper.py list
```

**Remember:** Mining rewards go to the address that was loaded when the miner started, not addresses created afterwards!

## Mining Faster

### Coming in v1.1.0
- Multi-threaded miner (uses all cores)
- Full UTXO balance scanning
- Transaction history
