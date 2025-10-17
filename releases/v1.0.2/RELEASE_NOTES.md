# Irium v1.0.2 Release Notes

## Critical Fix

**Fixed RIPEMD160 compatibility for Ubuntu 22.04+ and modern Python versions**

This release fixes wallet creation on systems where RIPEMD160 is not available in the default OpenSSL/Python configuration.

## Changes

- ✅ Added PyCryptodome fallback for RIPEMD160 hashing
- ✅ Updated install.sh to install pycryptodome automatically
- ✅ Updated QUICKSTART.md with correct dependencies
- ✅ All wallet commands now work on all systems

## Installation

```bash
git clone https://github.com/iriumlabs/irium.git
cd irium
pip3 install --user pycryptodome qrcode pillow
python3 scripts/irium-wallet-proper.py new-address
```

## Verified Commands

- Create Wallet: `python3 scripts/irium-wallet-proper.py new-address` ✅
- Run Node: `python3 scripts/irium-node.py` ✅
- Start Mining: `python3 scripts/irium-miner.py` ✅

## Download

[irium-bootstrap-v1.0.2.tar.gz](https://github.com/iriumlabs/irium/releases/download/v1.0.2/irium-bootstrap-v1.0.2.tar.gz)

## Genesis Block

Same as v1.0.1 - no consensus changes, only client compatibility fix.

Genesis Hash: `cbdd1b9134adc846b3af5e2128f68214e1d8154912ff8da40685f47700000000`
