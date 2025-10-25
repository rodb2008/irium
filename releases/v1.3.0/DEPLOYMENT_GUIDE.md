# Irium v1.3.0 Deployment Guide

## ⚠️ CRITICAL: This is a Hard Fork

All nodes must upgrade to v1.3.0 to remain on the network.

---

## Quick Deployment (5 minutes)

### Step 1: Stop Services
```bash
sudo systemctl stop irium-node.service
sudo systemctl stop irium-miner.service
```

### Step 2: Backup Wallet
```bash
cp ~/.irium/irium-wallet.json ~/.irium/irium-wallet.json.backup-$(date +%s)
```

### Step 3: Update Code
```bash
cd /home/irium/irium
git fetch origin
git checkout v1.3.0
```

### Step 4: Clear Chainstate
```bash
rm -rf ~/.irium/chainstate/*
```
*Note: Blockchain will rebuild from block files automatically*

### Step 5: Restart Services
```bash
sudo systemctl start irium-node.service
sleep 5
sudo systemctl start irium-miner.service
```

### Step 6: Verify
```bash
# Check node status
sudo systemctl status irium-node.service

# Monitor logs
sudo journalctl -u irium-node.service -f

# Check blockchain height
curl -s http://localhost:8082/api/stats | python3 -m json.tool
```

---

## Verification Checklist

- [ ] Node service running
- [ ] Miner service running (if applicable)
- [ ] No errors in logs
- [ ] Blockchain height matches network
- [ ] Peers connected
- [ ] Wallet accessible

---

## Rollback (if needed)

```bash
cd /home/irium/irium
git checkout v1.2.0
sudo systemctl restart irium-node.service irium-miner.service
```

---

## Support

- GitHub Issues: https://github.com/iriumlabs/irium/issues
- Release Notes: releases/v1.3.0/RELEASE_NOTES.md

---

**Deployment Time:** ~5 minutes  
**Downtime:** ~30 seconds  
**Data Loss:** None (blocks preserved)
