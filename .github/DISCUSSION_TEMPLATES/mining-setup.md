# ⛏️ Mining Setup & Tips

Share your mining setup, hashrate, and tips for optimizing Irium mining!

## Post Your Setup

**Example:**
- **Hardware**: AMD Ryzen 9 5950X (16-core)
- **Hashrate**: ~850 kH/s
- **OS**: Ubuntu 22.04
- **Optimizations**: Using all cores, CPU governor set to performance

## Mining Tips

1. **CPU Governor**: Set to "performance" mode for better hashrate
   ```bash
   sudo cpupower frequency-set -g performance
   ```

2. **Monitor Mining**: Check your mining progress
   ```bash
   # In mining terminal, you'll see:
   # Block mined! Hash: [hash] Height: [height]
   ```

3. **Check Rewards**: View your mining address balance
   ```bash
   python3 scripts/irium-wallet-proper.py balance
   ```

## Current Block Reward

- **Initial**: 50 IRM per block
- **Block Time**: 600 seconds (10 minutes target)
- **Halving**: Every 210,000 blocks

Share your experiences below! 👇
