# Irium Blockchain (IRM)

[![Release](https://img.shields.io/badge/release-v1.0-blue)](https://github.com/iriumlabs/irium/releases/tag/v1.0)
[![Status](https://img.shields.io/badge/status-stable-brightgreen)](#)
[![Network](https://img.shields.io/badge/network-LIVE-brightgreen)](#)
[![License](https://img.shields.io/badge/license-MIT-green)](LICENSE)

Irium is a purpose‑built proof‑of‑work blockchain for the IRM asset. The network is engineered for production use only—no bundled testnet—and is designed to:

- Bootstrap without DNS dependence.
- Persist peers and checkpoints via signed, machine‑readable artifacts.
- Enforce transparent founder vesting and a fixed supply cap at the consensus layer.

A next‑generation proof‑of‑work blockchain designed for true decentralization.

- Release: v1.0 – Production Release (stable mining + P2P sync)
- Release Notes: https://github.com/iriumlabs/irium/releases/tag/v1.0

## Table of Contents
- [Network Specifications](#network-specifications)
- [Key Features](#key-features)
- [Repository Layout](#repository-layout)
- [Getting Started](#getting-started)
- [Bootstrapping Without DNS](#bootstrapping-without-dns)
- [Automatic Peer Persistence](#automatic-peer-persistence)
- [Genesis Configuration](#genesis-configuration)
- [Consensus & Monetary Policy](#consensus--monetary-policy)
- [P2P & Security](#p2p--security)
- [Light Clients](#light-clients)
- [Wallet Integration](#wallet-integration)
- [Mining Toolkit](#mining-toolkit)
- [Licensing](#licensing)

## Network Specifications

| Parameter              | Value                                   |
|------------------------|-----------------------------------------|
| Ticker                 | IRM                                     |
| Consensus              | Proof‑of‑Work (SHA‑256d)                |
| Target Block Time      | 600 seconds                             |
| Initial Block Subsidy  | 50 IRM                                  |
| Halving Interval       | 210,000 blocks (config‑driven)          |
| Coinbase Maturity      | 100 blocks                              |
| Difficulty Retarget    | Every 2,016 blocks                      |
| Maximum Supply         | 100,000,000 IRM                         |
| Founder Allocation     | 3,500,000 IRM (CLTV‑locked timelock)    |
| Mining Allocation      | ~96,500,000 IRM (PoW issuance)          |

The exact monetary schedule and consensus parameters are defined in `configs/consensus.json` and the locked genesis payload in `configs/genesis-locked.json`.

## Key Features

- **Zero‑DNS bootstrap** – nodes join the network using signed seedlists and anchors instead of DNS seeds.
- **Self‑healing peer discovery** – peers are persisted to disk and promoted into a runtime seedlist.
- **Transparent founder vesting** – 3,500,000 IRM are timelocked on‑chain; the remaining supply is mined via PoW.
- **Low, predictable fees** – protocol is tuned for small transaction fees suitable for everyday use.
- **SPV‑ready from genesis** – light clients can validate headers and anchors without storing full blocks.
- **Relay‑friendly design** – the protocol leaves room for optional fee‑sharing schemes that reward high‑quality relays.

## Repository Layout

High‑level structure at `/home/irium`:

```text
configs/              # Consensus + genesis configuration (JSON)
bootstrap/            # Signed seedlists, anchors, trust roots
irium/                # Python package: blocks, chain state, P2P, wallet, tools
scripts/              # Operational entrypoints (node, miner, wallet APIs, etc.)
docs/                 # Architecture and whitepaper
state/                # Runtime data produced by nodes (peers.json, etc.)
LICENSE               # Project license
README.md             # This document
IRIUM_NETWORK_TRACKER.md  # Human‑readable mainnet status tracker
```

Runtime artifacts under `~/.irium/` (blocks, wallet files, logs) are never committed to the repository.

## Getting Started

### 1. Environment

- Python 3.11 or newer.
- Recommended: a virtual environment.

```bash
git clone https://github.com/iriumlabs/irium.git
cd irium
python3 -m venv .venv
. .venv/bin/activate
pip install -r requirements.txt
export PYTHONPATH="$PWD"
```

### 2. Verify Genesis and Anchors

Before running a node, verify the locked genesis header and anchors shipped with the release:

```bash
python3 scripts/verify_genesis.py
```

You should see matching derived and file header hashes. Anchors and checkpoints are defined in `bootstrap/anchors.json`.

### 3. Start a Node

To run a local mainnet node directly from the repository:

```bash
export PYTHONPATH="$PWD"
python3 scripts/irium-node.py 38291
```

On production systems, use the packaged systemd unit (see `irium/QUICKSTART.md`) so the node restarts automatically and logs to `journalctl`.

### 4. Create a Wallet

Use the reference wallet CLI to create and manage keys:

```bash
export PYTHONPATH="$PWD"
python3 scripts/irium-wallet-proper.py create-wallet
python3 scripts/irium-wallet-proper.py new-address
python3 scripts/irium-wallet-proper.py show-wallet
```

Wallet state is stored in `~/.irium/irium-wallet.json`. Back this file up securely.

### 5. Start Mining

Once the node is in sync and a wallet address exists, you can start mining:

```bash
export PYTHONPATH="$PWD"
export IRIUM_WALLET_FILE="$HOME/.irium/irium-wallet.json"

nohup python3 -u scripts/irium-node.py 38291 > /tmp/node-38291.log 2>&1 &
python3 -u scripts/irium-miner.py 38292
```

For multicore mining and more operational details, see `irium/QUICKSTART.md`.

## Bootstrapping Without DNS

Irium avoids DNS seeds. Instead, nodes rely on signed bootstrap artifacts:

- `bootstrap/seedlist.txt` – authoritative set of initial peers, signed.
- `bootstrap/seedlist.runtime` – node‑maintained cache of additional peers discovered at runtime.
- `bootstrap/anchors.json` – signed anchor checkpoints used for eclipse protection.

The `scripts/irium-zero.sh` helper can be used to fetch and validate bootstrap data on a fresh system:

```bash
./scripts/irium-zero.sh
```

At a high level it:

1. Locates or downloads `seedlist.txt` and `anchors.json`.
2. Verifies signatures against trusted keys.
3. Produces libp2p‑compatible multiaddresses for dialing peers.
4. Prints the latest checkpoints that new nodes must validate while syncing.

This design removes DNS as a bootstrap dependency; artifacts can be mirrored via GitHub Releases, IPFS, or other distribution channels.

## Automatic Peer Persistence

When the node observes healthy peers, it updates two data sets:

- `state/peers.json` – a local peer book with basic metadata.
- `bootstrap/seedlist.runtime` – a rolling cache of observed multiaddresses.

Tools that consume seed material should read both the signed `seedlist.txt` and the runtime `seedlist.runtime`. This allows nodes to continue discovering peers even if the original mirrors go offline, without mutating the signed release seedlist.

## Genesis Configuration

Genesis is specified in machine‑readable JSON and locked into the repository:

- `configs/genesis.json` – human‑friendly description of the genesis layout.
- `configs/genesis-locked.json` – canonical, locked genesis header and payload used by nodes and miners.
- `bootstrap/anchors.json` – an anchored copy of the genesis hash, time, and merkle root.

The locked genesis defines:

- A single CLTV‑protected founder allocation of 3,500,000 IRM.
- No other premine outputs.
- A merkle root that encodes the text commitment visible in the coinbase.

Nodes verify that the derived header from the payload matches the locked hash before accepting any chain as valid.

## Consensus & Monetary Policy

Consensus rules are centralized in `irium/` and parameterised by `configs/consensus.json`:

- SHA‑256d proof‑of‑work with a 600‑second target block time.
- Difficulty retarget every 2,016 blocks with bounded adjustment.
- Initial block subsidy of 50 IRM, halving every 210,000 blocks.
- Coinbase outputs mature after 100 confirmations.
- A hard cap of 100,000,000 IRM enforced via cumulative subsidy checks.

The `ChainState` implementation:

- Re‑computes merkle roots and verifies header linkage.
- Validates PoW targets against the configured limit.
- Tracks cumulative subsidy and fees, rejecting coinbases that exceed the permitted reward.
- Maintains a UTXO set to enforce value conservation and reject double spends.

## P2P & Security

The P2P stack (see `irium/p2p.py`) focuses on robustness and eclipse resistance:

- **Zero‑DNS bootstrap** via signed seedlists and anchors.
- **Self‑healing peer discovery** by persisting observed peers into `state/peers.json` and `bootstrap/seedlist.runtime`.
- **Anchor‑based checkpoints** using `bootstrap/anchors.json` and `AnchorManager`/`EclipseProtection` to reject chains that diverge from signed anchors.

The node and miner both expose configuration through environment variables such as:

- `IRIUM_BLOCKS_DIR` – directory for `block_*.json` files.
- `IRIUM_WALLET_FILE` – wallet JSON path.
- `IRIUM_MEMPOOL_DIR` – mempool storage directory.
- `IRIUM_EXPLORER_HOST` / `IRIUM_EXPLORER_PORT` – explorer API bind address.

Defaults are safe for localhost operation and can be overridden per deployment.

## Light Clients

From genesis, Irium is designed to support light clients:

- Header verification and PoW validation are available as library calls.
- Anchors provide compact checkpoints for NiPoPoW/SPV clients.
- Proof‑of‑inclusion logic can be layered on top of the existing merkle and block primitives.

Wallet and explorer tooling can use these primitives to build SPV clients without requiring a full node on end‑user machines.

## Wallet Integration

The reference wallet (`irium.wallet.Wallet` and `scripts/irium-wallet-proper.py`) provides:

- Deterministic key management and WIF import/export.
- Balance tracking over UTXOs controlled by the wallet.
- Transaction construction and signing using standard P2PKH scripts.

Example:

```python
from irium.wallet import Wallet

wallet = Wallet()
address = wallet.import_wif("…your WIF here…")
print("Address:", address)
print("Balance (sats):", wallet.balance())
```

The wallet CLI wraps these primitives with commands for creating wallets, generating addresses, checking balances, sending transactions, and monitoring activity.

## Mining Toolkit

The reference miner (`scripts/irium-miner.py`) runs a P2P‑enabled mining loop:

- Loads consensus parameters, genesis, and anchors.
- Discovers peers and broadcasts solved blocks.
- Assembles block templates with a coinbase paying to your configured wallet.
- Iterates nonces and updates timestamps when the 32‑bit nonce space is exhausted.

Rewards are paid to the first wallet address in `~/.irium/irium-wallet.json` (or the path provided via `IRIUM_WALLET_FILE`). Mined blocks are stored in `block_*.json` files under the configured `IRIUM_BLOCKS_DIR`.

## Licensing

Irium is released under the MIT License. See `LICENSE` for details.

