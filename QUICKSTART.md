# Irium Blockchain - Quick Start Guide

Get up and running with Irium in 5 minutes!

## Prerequisites
- Linux (Ubuntu 20.04+)
- Python 3.10+
- 2GB RAM, 10GB disk

## Installation
```bash
git clone https://github.com/iriumlabs/irium.git
cd irium
pip3 install qrcode[pil]
```

## Create Wallet
```bash
python3 scripts/irium-wallet-proper.py create
```

## Run Node
```bash
python3 scripts/irium-node.py
```

## Start Mining
```bash
python3 scripts/irium-miner.py
```

See full documentation in repo for details.
