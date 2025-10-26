#!/usr/bin/env python3
"""Proper balance checker - matches coinbase addresses to wallet."""

import os
import sys
import json

sys.path.insert(0, os.path.join(os.path.dirname(__file__), '..'))

from irium.wallet import Wallet

def scan_blockchain_for_balance(wallet):
    """Scan blockchain and match miner addresses to wallet."""
    addresses = set(wallet.addresses())
    balance = 0
    blocks_mined = []
    blocks_other = []
    
    blocks_dir = os.path.expanduser("~/.irium/blocks")
    if not os.path.exists(blocks_dir):
        return 0, [], []
    
    # Scan all blocks
    block_files = sorted([f for f in os.listdir(blocks_dir) if f.startswith('block_') and f.endswith('.json')])
    
    print(f"Scanning {len(block_files)} blocks...")
    print(f"Your wallet has {len(addresses)} address(es)")
    print()
    
    for block_file in block_files:
        block_path = os.path.join(blocks_dir, block_file)
        
        try:
            with open(block_path, 'r') as f:
                block_data = json.load(f)
            
            height = block_data.get('height', 0)
            reward = block_data.get('reward', 0) / 100000000  # Convert to IRM
            block_hash = block_data.get('hash', 'unknown')
            miner_addr = block_data.get('miner_address', None)
            
            block_info = {
                'height': height,
                'reward': reward,
                'hash': block_hash[:16] + '...',
                'miner': miner_addr or 'Unknown'
            }
            
            # Check if this block was mined by you
            if miner_addr and miner_addr in addresses:
                balance += reward
                blocks_mined.append(block_info)
            else:
                blocks_other.append(block_info)
                
        except Exception as e:
            print(f"Error reading {block_file}: {e}")
            continue
    
    return balance, blocks_mined, blocks_other

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
    
    print("=" * 70)
    print("IRIUM WALLET BALANCE - ACCURATE")
    print("=" * 70)
    print()
    print(f"Your wallet addresses: {len(addresses)}")
    for i, addr in enumerate(addresses[:5], 1):
        print(f"  {i}. {addr}")
    if len(addresses) > 5:
        print(f"  ... and {len(addresses) - 5} more")
    print()
    
    balance, blocks_yours, blocks_others = scan_blockchain_for_balance(wallet)
    
    if blocks_yours:
        print("=" * 70)
        print(f"BLOCKS YOU MINED: {len(blocks_yours)}")
        print("=" * 70)
        for block in blocks_yours:
            print(f"✅ Block {block['height']:3d}: {block['reward']:8.2f} IRM  (hash: {block['hash']})")
    
    if blocks_others:
        print()
        print("=" * 70)
        print(f"BLOCKS MINED BY OTHERS: {len(blocks_others)}")
        print("=" * 70)
        for block in blocks_others[:5]:  # Show max 5
            miner = block['miner'][:20] + '...' if len(block['miner']) > 20 else block['miner']
            print(f"  Block {block['height']:3d}: {block['reward']:8.2f} IRM  (miner: {miner})")
        if len(blocks_others) > 5:
            print(f"  ... and {len(blocks_others) - 5} more blocks by others")
    
    print()
    print("=" * 70)
    print(f"YOUR BALANCE: {balance:.2f} IRM")
    print("=" * 70)
    print()
    
    if balance == 0 and blocks_others:
        print("⚠️  Note: Old blocks (2-10) don't have miner_address field yet.")
        print("   Only NEW blocks mined after this update will show accurate balance.")
        print()
        print("   To verify old blocks, check timestamps:")
        print("   Blocks created AFTER you started mining = yours")
        print()

if __name__ == "__main__":
    main()
