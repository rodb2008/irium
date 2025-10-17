# Irium v1.0.4 - Network Launch Release

## ✅ ALL SYSTEMS OPERATIONAL

This is the first fully operational release of Irium!

## Critical Fixes in v1.0.4

✅ **Peer connections working** - Nodes successfully connect and maintain peers
✅ **Blockchain sync operational** - Nodes load and sync to current height
✅ **Handshake protocol fixed** - P2P handshakes complete successfully  
✅ **Self-connection prevention** - Seed node doesn't connect to itself
✅ **Message protocol corrected** - Full messages read before deserialization
✅ **RIPEMD160 compatibility** - Works on all modern systems

## Network Status

- **Mainnet:** LIVE ✅
- **Blocks:** 3+ mined
- **IRM in circulation:** 100+
- **Seed node:** 207.244.247.86:38291
- **Verified working:** Peer connections, mining, wallet creation

## Installation

```bash
git clone https://github.com/iriumlabs/irium.git
cd irium
pip3 install --user pycryptodome qrcode pillow
python3 scripts/irium-node.py
```

## Mining

```bash
python3 scripts/irium-miner.py
```

Earn 50 IRM per block!

## Resources

- Website: https://www.iriumlabs.org
- Whitepaper: https://www.iriumlabs.org/whitepaper.html
- Explorer: http://207.244.247.86:8082

---

**This release is production-ready and recommended for all users.**
