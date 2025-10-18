#!/usr/bin/env python3
"""Check wallet balance by scanning blockchain."""

import os
import sys
import json

sys.path.insert(0, os.path.join(os.path.dirname(__file__), '..'))

from irium.wallet import Wallet

def scan_blockchain_for_balance(wallet):
    """Scan blockchain files for UTXOs belonging to wallet addresses."""
    addresses = set(wallet.addresses())
    balance = 0
    utxos = []
    
    blocks_dir = os.path.expanduser("~/.irium/blocks")
    if not os.path.exists(blocks_dir):
        return 0, []
    
    # Scan all blocks
    block_files = sorted([f for f in os.listdir(blocks_dir) if f.startswith('block_') and f.endswith('.json')])
    
    for block_file in block_files:
        block_path = os.path.join(blocks_dir, block_file)
        with open(block_path, 'r') as f:
            block = json.load(f)
        
        height = block.get('height', 0)
        
        # For now, just count coinbase rewards
        # (Full UTXO scanning would require parsing transactions)
        print(f"Block {height}: Reward {block.get('reward', 0) / 100000000} IRM")
    
    return balance, utxos

def main():
    # Load wallet
    WALLET_FILE = os.path.expanduser("~/.irium/irium-wallet.json")
    wallet = Wallet()
    
    if os.path.exists(WALLET_FILE):
        with open(WALLET_FILE, 'r') as f:
            data = json.load(f)
        for addr, wif in data.get('keys', {}).items():
            wallet.import_wif(wif)
    
    addresses = list(wallet.addresses())
    if not addresses:
        print("No addresses in wallet")
        return
    
    print(f"Wallet addresses: {len(addresses)}")
    for addr in addresses:
        print(f"  • {addr}")
    
    print(f"\nScanning blockchain...")
    balance, utxos = scan_blockchain_for_balance(wallet)
    
    print(f"\nNote: Full UTXO scanning will be implemented in v1.1.0")
    print(f"For now, check block files to see your mining rewards:")
    print(f"  ls ~/.irium/blocks/")

if __name__ == "__main__":
    main()
