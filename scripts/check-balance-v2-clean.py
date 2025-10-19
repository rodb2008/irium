#!/usr/bin/env python3
"""
Accurate Irium Balance Checker - Shows only YOUR mined blocks
"""
import os
import json

WALLET_FILE = os.path.expanduser("~/.irium/irium-wallet.json")
BLOCKS_DIR = os.path.expanduser("~/.irium/blocks")

def main():
    # Load wallet
    if not os.path.exists(WALLET_FILE):
        print("❌ Wallet not found!")
        return
    
    with open(WALLET_FILE) as f:
        wallet = json.load(f)
    
    my_addresses = wallet.get('addresses', [])
    
    print("=" * 70)
    print("IRIUM WALLET BALANCE")
    print("=" * 70)
    print(f"\nYour wallet addresses: {len(my_addresses)}")
    for i, addr in enumerate(my_addresses, 1):
        print(f"  {i}. {addr}")
    
    # Scan blocks
    my_blocks = []
    total_balance = 0
    
    if os.path.exists(BLOCKS_DIR):
        block_files = [f for f in os.listdir(BLOCKS_DIR) if f.startswith("block_") and f.endswith(".json")]
        
        for block_file in block_files:
            try:
                with open(os.path.join(BLOCKS_DIR, block_file)) as f:
                    block = json.load(f)
                
                miner = block.get('miner_address', '')
                if miner in my_addresses:
                    reward = block.get('reward', 0) / 100000000  # Convert to IRM
                    my_blocks.append({
                        'height': block['height'],
                        'reward': reward,
                        'hash': block['hash']
                    })
                    total_balance += reward
            except:
                pass
    
    # Sort by height
    my_blocks.sort(key=lambda x: x['height'])
    
    print(f"\n{'=' * 70}")
    print(f"BLOCKS YOU MINED: {len(my_blocks)}")
    print("=" * 70)
    
    if my_blocks:
        for block in my_blocks:
            print(f"✅ Block {block['height']:3d}:  {block['reward']:8.2f} IRM  (hash: {block['hash'][:16]}...)")
    else:
        print("No blocks mined yet. Start mining to earn IRM!")
    
    print(f"\n{'=' * 70}")
    print(f"YOUR BALANCE: {total_balance:.2f} IRM")
    print("=" * 70)

if __name__ == "__main__":
    main()
