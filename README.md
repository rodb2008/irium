# Irium (IRM)

**Settlement-first SHA-256d proof-of-work blockchain for trustless commerce.**

[![Build](https://github.com/iriumlabs/irium/actions/workflows/release.yml/badge.svg)](https://github.com/iriumlabs/irium/actions/workflows/release.yml)
[![Latest Release](https://img.shields.io/github/v/release/iriumlabs/irium)](https://github.com/iriumlabs/irium/releases/latest)
[![License: MIT](https://img.shields.io/badge/License-MIT-lightgrey)](LICENSE)
[![Mainnet](https://img.shields.io/badge/Mainnet-Live-brightgreen)](https://www.iriumlabs.org)

---

## What is Irium

Irium is a proof-of-work blockchain built for trustless escrow and proof-based commerce. Instead of smart contracts, it uses a deterministic settlement layer: buyer and seller lock funds on-chain, an attestor submits a cryptographic proof of delivery, and the chain enforces release or refund automatically. No lawyers, no chargebacks, no intermediaries.

SHA-256d consensus. No premine. No admin keys. ~24,500,000 IRM total supply (21M from mining + 3.5M genesis CLTV vesting). Mainnet live since January 5, 2026.

---

## Current State

| Feature | Status |
|---------|--------|
| Chain and mining | Live on mainnet |
| Settlement layer | Live |
| Marketplace | Live |
| Reputation system | Live |
| Proof ecosystem | Live |
| Merchant tools | Live |
| AuxPoW merged mining | Activating at block 26,347 |
| WebSocket streaming API | Live |
| BIP32/BIP39 key derivation | Live |
| Multisig (2-of-2, 2-of-3) | Live |
| Confidential agreements | Live |
| Desktop / web / mobile wallet | In development |

---

## Quick Install

**Option 1 — Pre-built binary (Linux/macOS)**

```bash
curl -fsSL https://raw.githubusercontent.com/iriumlabs/irium/main/install.sh | bash
```

Installs `iriumd`, `irium-wallet`, `irium-miner`, and `irium-miner-gpu` to `/usr/local/bin`.

**Option 2 — Docker**

```bash
cp .env.example .env   # fill in your wallet address
docker-compose up -d
```

**Option 3 — Build from source**

```bash
git clone https://github.com/iriumlabs/irium.git && cd irium
cargo build --release
```

GPU miner:

```bash
cargo build --release --features gpu --bin irium-miner-gpu
```

---

## Run a Node

```bash
iriumd                              # start the node (syncs automatically)
curl http://127.0.0.1:38300/status  # confirm it is running
irium-wallet new-address            # generate your first address
irium-wallet balance                # check balance once synced
```

The node connects to the two official seed nodes and begins syncing. No configuration needed for a basic setup. Default P2P port: 38291. Default RPC port: 38300.

---

## Settle a Trade

The minimum commands for a complete buyer–seller trade:

```bash
# Seller: create an offer
irium-wallet offer-create \
  --seller <YOUR_ADDRESS> \
  --amount 1.0 \
  --description "Software licence delivery" \
  --policy-template software_delivery

# Buyer: list open offers and take one
irium-wallet offer-list --status open
irium-wallet offer-take --offer <OFFER_ID> --buyer <BUYER_ADDRESS>

# Attestor or seller: submit delivery proof
irium-wallet agreement-proof-submit --proof proof.json

# Check release eligibility (true after 6-block finality)
irium-wallet agreement-release-eligibility <AGREEMENT_HASH>
```

Full walkthrough: [QUICKSTART.md](QUICKSTART.md) | API reference: [docs/API.md](docs/API.md)

---

## Documentation

| Document | What it covers |
|----------|---------------|
| [QUICKSTART.md](QUICKSTART.md) | Zero-to-settlement walkthrough for new users |
| [docs/WHITEPAPER.md](docs/WHITEPAPER.md) | Full protocol specification — all 18 layers |
| [docs/WALLET-CLI.md](docs/WALLET-CLI.md) | Complete wallet command reference |
| [docs/API.md](docs/API.md) | REST API reference for all endpoints |
| [docs/WEBSOCKET.md](docs/WEBSOCKET.md) | WebSocket and SSE streaming event API |
| [docs/SETTLEMENT-DEV.md](docs/SETTLEMENT-DEV.md) | Settlement layer developer guide |
| [docs/SETTLEMENT-EXAMPLE.md](docs/SETTLEMENT-EXAMPLE.md) | Worked agreement examples |
| [docs/KEY-DERIVATION.md](docs/KEY-DERIVATION.md) | Custom and BIP32/BIP39 key derivation |
| [docs/MULTISIG.md](docs/MULTISIG.md) | 2-of-2 and 2-of-3 multisig guide |
| [docs/ATTESTOR-GUIDE.md](docs/ATTESTOR-GUIDE.md) | Attestor bonding and responsibilities |
| [docs/MERGED-MINING.md](docs/MERGED-MINING.md) | AuxPoW merged mining setup |
| [docs/POOL-OPERATOR.md](docs/POOL-OPERATOR.md) | Stratum pool operator guide |
| [docs/POOL_STRATUM.md](docs/POOL_STRATUM.md) | Pool mining for miners |
| [docs/DOCKER.md](docs/DOCKER.md) | Docker deployment guide |
| [docs/SEED-NODE.md](docs/SEED-NODE.md) | Running a public seed node |
| [docs/DEVELOPER-QUICKSTART.md](docs/DEVELOPER-QUICKSTART.md) | Dev environment setup |
| [docs/LISTING-APPLICATION.md](docs/LISTING-APPLICATION.md) | Exchange listing application template |

---

## Community

| | |
|-|--|
| Telegram | [t.me/iriumlabs](https://t.me/iriumlabs) |
| Bitcointalk | [ANN thread](https://bitcointalk.org/index.php?topic=5572239.0) |
| GitHub Issues | [github.com/iriumlabs/irium/issues](https://github.com/iriumlabs/irium/issues) |

---

## License

MIT
