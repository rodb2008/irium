# Mining Setup Guide

## How to Start Mining with Your Address

### Quick Start (Recommended)

```bash
# 1) Create a wallet address (save the private key)
./target/release/irium-wallet new-address

# 2) Start mining with that address
export IRIUM_MINER_ADDRESS=<YOUR_IRIUM_ADDRESS>
./target/release/irium-miner --threads 4 --verbose
```

### Verify the Mining Address

```bash
# systemd service logs
journalctl -u irium-miner.service -f --no-pager
```
Look for the `Using miner address:` line when the miner starts.

### Miner Address Is Read at Startup

If you change your payout address, restart the miner:
```bash
sudo systemctl restart irium-miner.service
```

### Managing Multiple Addresses

The miner uses a single address at a time. Switch by updating `IRIUM_MINER_ADDRESS` (or `IRIUM_MINER_PKH`) and restarting the miner.

### Starting Fresh with a New Address

```bash
# 1) Generate a new address
./target/release/irium-wallet new-address

# 2) Export it and restart the miner
export IRIUM_MINER_ADDRESS=<NEW_ADDRESS>
./target/release/irium-miner --threads 4 --verbose
```

### Backup Your Keys

**CRITICAL: Save the printed private key.** The Rust wallet CLI does not store a wallet file for you.

### Troubleshooting

**"Miner is using a different address"**

The miner was already running. Restart it after updating `IRIUM_MINER_ADDRESS`.

**"How do I check my mining rewards?"**

```bash
./target/release/irium-wallet balance <YOUR_IRIUM_ADDRESS>
```
Rewards are spendable after 100 block confirmations (coinbase maturity).

### Mining Tips

- Save your private key and address offline.
- Keep your system updated (`git pull`, rebuild).
- Monitor miner logs for template errors or RPC failures.

### Need Help?

- GitHub Issues: https://github.com/iriumlabs/irium/issues
- Discussions: https://github.com/iriumlabs/irium/discussions
