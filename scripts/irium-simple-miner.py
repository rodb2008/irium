#!/usr/bin/env python3
"""Simple miner without P2P - just mines and saves blocks."""
import sys, os, json, time
sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from irium.wallet import Wallet
from irium.chain import ChainParams, ChainState
from irium.block import Block, BlockHeader
from irium.tx import Transaction, TxInput, TxOutput
from irium.pow import Target

BLOCKCHAIN_DIR = os.path.expanduser("~/.irium/blocks")
WALLET_FILE = os.path.expanduser("~/.irium/irium-wallet.json")

mining_address = "Q8Ni6TJ6Y77vvtMZ1E474kn2jYNawjvaLa"

while True:
    # Get current height
    blocks = [int(f.replace('block_', '').replace('.json', '')) 
              for f in os.listdir(BLOCKCHAIN_DIR) if f.startswith('block_') and f.endswith('.json')]
    height = max(blocks) + 1 if blocks else 1
    
    # Load prev block
    if height > 1:
        with open(f"{BLOCKCHAIN_DIR}/block_{height-1}.json") as f:
            prev_block = json.load(f)
        prev_hash = bytes.fromhex(prev_block['hash'])
    else:
        prev_hash = bytes.fromhex('cbdd1b9134adc846b3af5e2128f68214e1d8154912ff8da40685f47700000000')
    
    print(f"⛏️  Mining block {height}...")
    print(f"  Prev: {prev_hash.hex()[:16]}...")
    
    # Mine
    target_bits = 0x1d00ffff
    target_value = (target_bits & 0xFFFFFF) * 2**(8 * ((target_bits >> 24) - 3))
    
    nonce = 0
    start = time.time()
    while True:
        header_data = (1).to_bytes(4, 'little') + prev_hash + bytes(32) + int(time.time()).to_bytes(4, 'little') + target_bits.to_bytes(4, 'little') + nonce.to_bytes(4, 'little')
        import hashlib
        h = hashlib.sha256(hashlib.sha256(header_data).digest()).digest()
        if int.from_bytes(h[::-1], 'big') < target_value:
            print(f"✅ Found block {height}! Hash: {h[::-1].hex()}")
            block_data = {'height': height, 'hash': h[::-1].hex(), 'prev_hash': prev_hash.hex(), 
                         'time': int(time.time()), 'nonce': nonce, 'miner_address': mining_address}
            with open(f"{BLOCKCHAIN_DIR}/block_{height}.json", 'w') as f:
                json.dump(block_data, f, indent=2)
            print(f"💾 Saved to disk")
            break
        nonce += 1
        if nonce % 100000 == 0:
            print(f"  Nonce: {nonce:,}", end='\r')
    
    time.sleep(1)
