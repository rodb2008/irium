# Irium Blockchain (IRM)

> Next-generation proof-of-work blockchain with 8 unique innovations

## Status: LIVE on Mainnet

- Explorer API: http://207.244.247.86:8082
- Wallet API: http://207.244.247.86:8080  
- P2P Network: 207.244.247.86:38291

## Quick Start

```bash
git clone https://github.com/iriumlabs/irium.git
cd irium
pip3 install qrcode[pil]
python3 scripts/irium-wallet-proper.py create
python3 scripts/irium-node.py
```

## Specifications

- Ticker: IRM
- Algorithm: SHA-256d PoW
- Max Supply: 100,000,000 IRM
- Block Time: 600 seconds
- Block Reward: 50 IRM (halves every 210k blocks)
- Transaction Fees: 0.0001 IRM

## 8 Unique Innovations

1. Zero-DNS Bootstrap
2. Self-Healing Peer Discovery  
3. Genesis Vesting (CLTV)
4. Per-Tx Relay Rewards
5. Sybil-Resistant Handshake
6. Anchor-File Consensus
7. Light Client (SPV + NiPoPoW)
8. On-chain Metadata

## Genesis Block

Hash: cbdd1b9134adc846b3af5e2128f68214e1d8154912ff8da40685f47700000000
Nonce: 1,110,943,221
Network: Mainnet LIVE

## License

MIT License
