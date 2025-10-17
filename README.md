# Irium Blockchain (IRM)

**A next-generation proof-of-work blockchain designed for true decentralization**

[![Network](https://img.shields.io/badge/network-mainnet-green.svg)](https://github.com/iriumlabs/irium)
[![Status](https://img.shields.io/badge/status-live-brightgreen.svg)](http://207.244.247.86:8082/api/stats)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

---

## What is Irium?

Irium is a **decentralized cryptocurrency** built from the ground up with a focus on solving real problems in blockchain networks. Using proven **SHA-256d proof-of-work** (same as Bitcoin), Irium introduces **8 unique innovations** that make it more resilient, accessible, and fair.

### Why Irium?

- **True Decentralization**: Zero-DNS bootstrap means no single point of failure
- **Ultra-Low Fees**: 0.0001 IRM per transaction (1,000x cheaper than Bitcoin)
- **Fair Launch**: No ICO, no premine (only transparent 3.5% founder vesting with timelocks)
- **Energy Efficient**: SHA-256d can leverage existing Bitcoin mining infrastructure
- **Mobile-First**: Built-in light client support (SPV + NiPoPoW)
- **Incentivized Network**: Relay rewards encourage node operators

---

## Technical Specifications

| Parameter | Value | Description |
|-----------|-------|-------------|
| **Ticker** | IRM | Official symbol |
| **Algorithm** | SHA-256d | Proof-of-Work (Bitcoin-compatible) |
| **Max Supply** | 100,000,000 IRM | Hard cap, never changes |
| **Genesis Vesting** | 3,500,000 IRM | Founder allocation (3.5%, timelocked 1y/2y/3y) |
| **Mineable Supply** | 96,500,000 IRM | Available to public miners |
| **Block Time** | 600 seconds | 10 minutes per block (like Bitcoin) |
| **Initial Reward** | 50 IRM | First 210,000 blocks |
| **Halving** | Every 210,000 blocks | Approximately every 4 years |
| **Difficulty Retarget** | Every 2016 blocks | Approximately every 14 days |
| **Coinbase Maturity** | 100 blocks | Mining rewards mature after 100 blocks |
| **Min Transaction Fee** | 0.0001 IRM | 10,000 satoshis (ultra-low) |
| **P2P Port** | 38291 | Default peer-to-peer network port |

---

## 8 Unique Innovations

### 1. Zero-DNS Bootstrap
No reliance on DNS servers. The network uses signed IP multiaddr lists (seedlist.txt) and checkpoint anchors (anchors.json) for bootstrapping. This eliminates DNS as a single point of failure and censorship vector.

### 2. Self-Healing Peer Discovery
The network "remembers" reliable peers through an uptime proof system. Peers build reputation scores based on availability, valid blocks shared, and successful connections. The network naturally gravitates toward stable, honest nodes.

### 3. Genesis Vesting with CLTV  
The 3.5M IRM founder allocation is locked in 3 on-chain UTXOs with OP_CHECKLOCKTIMEVERIFY:
- 1M IRM unlocks after 52,560 blocks (~1 year)
- 1.25M IRM unlocks after 105,120 blocks (~2 years)
- 1.25M IRM unlocks after 157,680 blocks (~3 years)

This is transparent, consensus-enforced, and irreversible.

### 4. Per-Transaction Relay Rewards
Nodes that relay transactions earn up to 10% of the transaction fee. The first relay gets 50%, second gets 30%, third gets 20%. This incentivizes running nodes and ensures healthy network propagation.

### 5. Sybil-Resistant Handshake
Peers must complete a small proof-of-work challenge during connection to prevent botnet attacks and Sybil attacks. The difficulty is calibrated to be trivial for legitimate nodes but prohibitive for mass bot connections.

### 6. Anchor-File Consensus  
Signed checkpoint headers provide an audit layer that protects new nodes from eclipse attacks. Even if an attacker controls all your peer connections, the anchors ensure you're on the real chain.

### 7. Light Client First (SPV + NiPoPoW)
Full SPV support with Non-Interactive Proofs of Proof-of-Work enables mobile wallets, IoT devices, and low-resource nodes to participate without downloading the full blockchain.

### 8. On-chain Metadata Commitments
The coinbase transaction can include hash pointers to off-chain data, enabling timestamping, notarization, and proof-of-existence applications.

---

## Getting Started

### 1. Run a Full Node

```bash
# Clone repository
git clone https://github.com/iriumlabs/irium.git
cd irium

# Install Python dependencies
pip3 install qrcode[pil]

# Start node (syncs with network)
python3 scripts/irium-node.py
```

Your node will:
- Connect to seed nodes
- Download the blockchain
- Validate all blocks
- Participate in P2P network

### 2. Create a Wallet

```bash
# Create new wallet
python3 scripts/irium-wallet-proper.py create
```

This generates:
- New IRM address (starts with Q or P)
- Private key (WIF format)
- Saved to `~/.irium/irium-wallet.json`

**Important:** Back up your wallet file! If you lose it, you lose your coins.

### 3. Start Mining

```bash
# Start mining to your wallet
python3 scripts/irium-miner.py
```

Your miner will:
- Create block templates
- Solve SHA-256d proof-of-work
- Earn 50 IRM per block (current reward)
- Broadcast blocks to network

**Hardware Requirements:**
- Any CPU can mine
- GPU mining: Use existing Bitcoin SHA-256d miners
- ASIC mining: Compatible with Bitcoin ASICs

### 4. Send & Receive IRM

```bash
# Check balance
python3 scripts/irium-wallet-proper.py balance

# Send IRM
python3 scripts/irium-wallet-proper.py send <address> <amount>

# Generate QR code
python3 scripts/irium-qrcode.py address <your_address>
```

---

## Network Information

### Live Mainnet

**Genesis Block:**
- Hash: `cbdd1b...000000`
- Mined: October 16, 2025
- Vesting: 3.5M IRM timelocked

**Current Status:**
- Network: LIVE ✅
- Services: Operational ✅
- P2P Peers: Growing 🌱

### Public Services

**Explorer API:**
```bash
# Get blockchain stats
curl http://207.244.247.86:8082/api/stats

# Get latest blocks
curl http://207.244.247.86:8082/api/latest?count=10
```

**Wallet API:**
```bash
# Get network info
curl http://207.244.247.86:8080/api/network/info

# Get wallet status
curl http://207.244.247.86:8080/api/wallet/status
```

---

## Documentation

- **[QUICKSTART.md](QUICKSTART.md)** - Get started in 5 minutes
- **[MINING.md](MINING.md)** - Complete mining guide
- **[WALLET.md](WALLET.md)** - Wallet management guide
- **[API_REFERENCE.md](API_REFERENCE.md)** - API documentation
- **[WHITEPAPER.md](WHITEPAPER.md)** - Technical whitepaper
- **[CONTRIBUTING.md](CONTRIBUTING.md)** - How to contribute

---

## How Irium Works

### Blockchain Basics

Irium uses a **UTXO (Unspent Transaction Output)** model, similar to Bitcoin. Every transaction consumes previous outputs and creates new ones. The blockchain is a chain of blocks, each containing transactions validated by proof-of-work.

### Mining Process

1. Miner creates block template with pending transactions
2. Adds coinbase transaction (block reward + fees)
3. Calculates merkle root of all transactions
4. Iterates nonce to find hash < difficulty target
5. Broadcasts valid block to network
6. Other nodes validate and accept block
7. Miner earns reward (50 IRM currently)

### Difficulty Adjustment

Every 2016 blocks (~14 days), difficulty adjusts based on actual vs target block time:
- If blocks are too fast: Difficulty increases
- If blocks are too slow: Difficulty decreases
- Target: 10 minutes per block

### Transaction Fees

Irium has **ultra-low fees** (0.0001 IRM minimum):
- Bitcoin: ~0.001 BTC ($30-50)
- Irium: ~0.0001 IRM (fraction of a cent)

Fees are distributed:
- 90% to miner
- 10% to relay nodes (up to 3 relays)

---

## Security

### Consensus Security
- ✅ SHA-256d proof-of-work (battle-tested)
- ✅ 51% attack resistant (requires majority hashpower)
- ✅ Merkle tree validation
- ✅ UTXO model prevents double-spends

### Network Security
- ✅ Sybil-resistant handshake
- ✅ Peer reputation system
- ✅ Eclipse attack protection (anchors)
- ✅ DoS protection (message limits)

### Wallet Security
- ✅ Standard key derivation
- ✅ WIF private key format
- ✅ Local wallet storage only
- ✅ No custodial services

---

## Join the Network

### As a Node Operator
Run a node to support the network:
```bash
python3 scripts/irium-node.py
```

Benefits:
- Help secure the network
- Earn relay rewards
- Support decentralization

### As a Miner
Mine IRM and earn rewards:
```bash
python3 scripts/irium-miner.py
```

Current Reward: **50 IRM per block**

### As a User
Use Irium for payments:
- Ultra-low fees (0.0001 IRM)
- Fast confirmations (10 min)
- Secure transactions

---

## Community & Support

- **GitHub:** https://github.com/iriumlabs/irium
- **Explorer:** http://207.244.247.86:8082
- **Email:** info@iriumlabs.org

---

## Community & Support

- **Discussions**: https://github.com/iriumlabs/irium/discussions - Ask questions, share ideas, connect with the community
- **Issues**: https://github.com/iriumlabs/irium/issues - Report bugs or request features
- **Email**: info@iriumlabs.org - Direct contact for security issues or partnerships

## License

MIT License - Free and open source

---

**Built with dedication to true decentralization**  
**Irium Labs © 2025**

## ⚠️ Important: Install Dependencies

After downloading, you MUST install dependencies:

```bash
pip3 install --user pycryptodome qrcode pillow
```

This is required for wallet creation and blockchain operations.


## 💡 Important Notes

### Wallet and Mining

- The miner loads your wallet **at startup**
- If you create a new address, **restart the miner** to use it:
  ```bash
  sudo systemctl restart irium-miner.service
  ```
- Check your mining address:
  ```bash
  sudo journalctl -u irium-miner.service | grep "Mining address" | tail -1
  ```

