# 🚨 EMERGENCY: Network Fork - Immediate Action Required

## CRITICAL SITUATION
The Irium network forked at block 19. The network is currently deadlocked.

## For ALL Network Participants

### STOP ALL SERVICES IMMEDIATELY
```bash
sudo systemctl stop irium-node irium-miner
```

### DELETE FORKED BLOCKS
```bash
cd ~/.irium/blocks/
rm -f block_2*.json block_3*.json
rm -f *.fork *.orphan
ls block_*.json  # Should only show blocks 2-19
```

### UPDATE CODE
```bash
cd ~/irium
git pull origin main
```

### RESTART
```bash
sudo systemctl start irium-node
sudo systemctl start irium-miner
```

### VERIFY
```bash
sudo journalctl -u irium-node -n 10 | grep "height"
```

**MUST show: height 19**

## What Happens Next

1. All nodes reset to height 19 (last valid block)
2. Miners compete to mine block 20 from the CORRECT chain tip
3. Fork prevention ensures only ONE valid block 20
4. Network rebuilds from block 20 onwards

## DO NOT PROCEED until you see "height 19"

Contact bootstrap node: 207.244.247.86:38291
