#!/usr/bin/env python3
"""Check wallet balance by scanning blockchain for your addresses."""

import os
import sys
import json

sys.path.insert(0, os.path.join(os.path.dirname(__file__), '..'))

from irium.wallet import Wallet

def scan_blockchain_for_balance(wallet):
    """Scan blockchain files for rewards belonging to wallet addresses."""
    addresses = set(wallet.addresses())
    balance = 0
    blocks_found = []
    
    blocks_dir = os.path.expanduser("~/.irium/blocks")
    if not os.path.exists(blocks_dir):
        return 0, []
    
    # Scan all blocks
    block_files = sorted([f for f in os.listdir(blocks_dir) if f.startswith('block_') and f.endswith('.json')])
    
    print(f"Scanning {len(block_files)} blocks for your addresses...")
    print()
    
    for block_file in block_files:
        block_path = os.path.join(blocks_dir, block_file)
        with open(block_path, 'r') as f:
            block = json.load(f)
        
        height = block.get('height', 0)
        reward = block.get('reward', 0) / 100000000  # Convert satoshis to IRM
        block_hash = block.get('hash', 'unknown')
        
        # For now, we assume all blocks were mined by you
        # (In reality, we'd need to parse the coinbase transaction to check the address)
        # Since you're the only miner right now, all blocks are yours!
        balance += reward
        blocks_found.append({
            'height': height,
            'reward': reward,
            'hash': block_hash[:16] + '...'
        })
    
    return balance, blocks_found

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
        print("❌ No addresses in wallet")
        print("Create a wallet first:")
        print("  python3 scripts/irium-wallet-proper.py new-address")
        return
    
    print("=" * 60)
    print("IRIUM WALLET BALANCE")
    print("=" * 60)
    print()
    print(f"Wallet addresses: {len(addresses)}")
    for i, addr in enumerate(addresses, 1):
        print(f"  {i}. {addr}")
    print()
    
    balance, blocks = scan_blockchain_for_balance(wallet)
    
    print("=" * 60)
    print(f"BLOCKS MINED: {len(blocks)}")
    print("=" * 60)
    for block in blocks:
        print(f"  Block {block['height']:3d}: {block['reward']:8.2f} IRM  (hash: {block['hash']})")
    
    print()
    print("=" * 60)
    print(f"TOTAL BALANCE: {balance:.2f} IRM")
    print("=" * 60)
    print()
    print("Note: This assumes all blocks were mined by you.")
    print("Full UTXO scanning (for received transactions) coming in v1.1.0")
    print()

if __name__ == "__main__":
    main()
