# Irium (IRM)

**Settlement-first SHA-256d proof-of-work blockchain for trustless commerce.**

[![Build](https://github.com/iriumlabs/irium/actions/workflows/release.yml/badge.svg)](https://github.com/iriumlabs/irium/actions/workflows/release.yml)
[![Latest Release](https://img.shields.io/github/v/release/iriumlabs/irium)](https://github.com/iriumlabs/irium/releases/latest)
[![License: MIT](https://img.shields.io/badge/License-MIT-lightgrey)](LICENSE)
[![Mainnet](https://img.shields.io/badge/Mainnet-Live-brightgreen)](https://www.iriumlabs.org)

---

## What is Irium

Irium is a proof-of-work blockchain built for trustless escrow and proof-based commerce. Instead of smart contracts, it uses a deterministic settlement layer: buyer and seller lock funds on-chain, an attestor submits a cryptographic proof of delivery, and the chain enforces release or refund automatically. No lawyers, no chargebacks, no intermediaries.

SHA-256d consensus. No premine. No admin keys. 100,000,000 IRM total supply (96.5M from mining + 3.5M genesis CLTV vesting). Mainnet live since January 5, 2026.

---

## At a Glance

| Parameter | Value |
|-----------|-------|
| Current node version | `v1.9.18` (released) · `v1.9.19` queued on `testing-codes-before-merging` |
| Consensus algorithm | SHA-256d proof of work |
| Block target interval | 600 s (10 min) |
| Difficulty adjustment | LWMA (60-block window; LWMA v2 — 30-block window — wired but inactive until rolled forward) |
| Block reward | 50 IRM during the Early Miner Era; halves every 210,000 blocks |
| AuxPoW merged mining | Activates at block 26,347 (still pending; current tip ≈ 22k) |
| Total supply cap | 100,000,000 IRM (96.5M mineable + 3.5M genesis CLTV vest) |
| Address prefix | `Q` (single-sig P2PKH, version byte 0x39) · `P` (multisig, version byte 0x28) |
| Default P2P port | 38291 |
| Default RPC / explorer port | 38300 |
| `/status` lightweight port | 8080 (loopback only by default) |
| Official pool — CPU/GPU | `stratum+tcp://pool.iriumlabs.org:3335` |
| Official pool — ASIC | `stratum+tcp://pool.iriumlabs.org:3333` |
| Public pool stats proxy | `http://pool.iriumlabs.org:3337/stats` |

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
| Rich list endpoint (`/rpc/richlist?limit=N`) | Live (since v1.9.17) |
| Self-advertised P2P external endpoint (CGNAT escape) | Live on `testing-codes-before-merging`, scheduled for v1.9.19 |
| AuxPoW merged mining | Activating at block 26,347 |
| WebSocket streaming API | Live |
| BIP32/BIP39 key derivation | Live |
| Multisig (2-of-2, 2-of-3) | Live |
| Confidential agreements | Live |
| Desktop / web / mobile wallet | Desktop wallet (`irium-core`) shipping; web / mobile in development |

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
irium-wallet list-addresses         # see your addresses
irium-wallet balance <YOUR_ADDRESS> # check balance once synced
```

The node connects to the Irium network automatically. On first run it uses the signed seed list bundled with the software. After the first connection it caches peer addresses locally and never needs the seed nodes again. No configuration needed for a basic setup. Default P2P port: 38291. Default RPC port: 38300.

### Mining options

| Profile | Command / pool |
|---------|----------------|
| Solo CPU | `irium-miner --address Q<your-address>` |
| Solo GPU | `irium-miner-gpu --address Q<your-address>` (build with `--features gpu`) |
| Pool — CPU/GPU | Point any Stratum v1 miner at `stratum+tcp://pool.iriumlabs.org:3335`, worker `Q<your-address>` |
| Pool — ASIC | Point any SHA-256 ASIC at `stratum+tcp://pool.iriumlabs.org:3333`, worker `Q<your-address>` |

Public pool stats live at `http://pool.iriumlabs.org:3337/stats` (CORS-enabled JSON with active miners, accepted shares, blocks found, and a rolling-window hashrate estimate per profile).

---

## Running a Public Node

If your node has a public IP address, you can help new users bootstrap by
advertising your address. Two complementary mechanisms exist:

**Coinbase peer-discovery embed** — your miner stamps the listen address
into every coinbase transaction so new nodes scanning the chain discover
you without DNS or a central seed server:

```bash
export IRIUM_ADVERTISE_ADDR=<your-public-ip>:38291
```

**P2P handshake external endpoint** — your node tells each peer the IP
they should record as your dialable address. This is the CGNAT escape
hatch: without it, peers infer your address from the TCP source IP, which
under carrier-grade NAT (RFC 6598, `100.64.0.0/10`) is the carrier's NAT
address rather than your real public IP. Set this when you know your real
external IPv4:

```bash
export IRIUM_EXTERNAL_ENDPOINT=<your-public-ip>:38291
```

iriumd validates the value (rejects RFC1918 private, RFC6598 CGNAT,
loopback, link-local, multicast, documentation, and IPv6) before
advertising it. Old peers without the field simply fall back to the
TCP-source-IP behavior, so this is fully backwards compatible.

To add your node as a seed for a running node without restarting:

```bash
curl -X POST http://localhost:38300/admin/add-seed \
  -H 'Authorization: Bearer <token>' \
  -H 'Content-Type: application/json' \
  -d '{"addr": "<your-public-ip>:38291"}'
```

### Environment variables (P2P networking)

| Variable | Purpose |
|----------|---------|
| `IRIUM_P2P_BIND` | Listen address/port for incoming P2P (default `0.0.0.0:38291`) |
| `IRIUM_EXTERNAL_ENDPOINT` | Self-advertised public `ip:port` carried in the handshake. **The CGNAT escape — set this when your TCP source IP is a carrier-NAT address.** Validated server-side: loopback, RFC1918, RFC6598 100.64/10, link-local, broadcast, multicast, documentation, and IPv6 are all rejected. |
| `IRIUM_NODE_PUBLIC_IP` | Self-IP filter used only by `local_ip_set()` so iriumd doesn't try to dial itself. Not advertised — set this if you operate multiple addresses behind the same node and want to avoid self-dial loops. |

### Behind CGNAT?

If your ISP gives you a 100.64.0.0/10 address rather than a real public IP,
peers that observe your TCP source IP record an unroutable address and gossip
it onwards. Symptoms: `0 inbound` peers indefinitely, even with port
forwarding configured on the local router.

Fixes, in order of impact:

1. Ask your ISP for a public IPv4 (many offer it free on request) — then set `IRIUM_EXTERNAL_ENDPOINT=<that-public-ip>:38291`.
2. Run a small relay on a VPS and forward port 38291 to your home node.
3. Accept outbound-only operation: iriumd still syncs and submits transactions, you just don't accept inbound connections.

The desktop wallet (`irium-core`) auto-detects your public IPv4 via an
external IP-echo service before launching iriumd, and only sets
`IRIUM_EXTERNAL_ENDPOINT` when the result validates as globally routable.
Manual CLI users on cloud servers should set the env var explicitly.

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
