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

# Load mining address from wallet
from irium.wallet import Wallet
import json

if not os.path.exists(WALLET_FILE):
    print("❌ Wallet not found! Create one first:")
    print("   python3 scripts/irium-wallet-proper.py create")
    exit(1)

# Load wallet from JSON
with open(WALLET_FILE, 'r') as f:
    wallet_data = json.load(f)
    
if not wallet_data.get('addresses'):
    print("❌ No addresses in wallet! Create one:")
    print("   python3 scripts/irium-wallet-proper.py create")
    exit(1)

mining_address = wallet_data['addresses'][0]
print(f"💰 Mining to: {mining_address}")

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

    # Calculate reward
    reward = 5000000000  # 50 IRM
    halvings = (height - 1) // 210000
    reward = reward >> halvings

    # Create coinbase transaction and calculate merkle root
    coinbase_tx = Transaction(
        version=1,
        inputs=[TxInput(prev_txid=bytes(32), prev_index=0xFFFFFFFF, script_sig=f"Block {height}".encode())],
        outputs=[TxOutput(value=reward, script_pubkey=bytes(32))]  # Simplified
    )
    
    temp_block = Block(
        header=BlockHeader(version=1, prev_hash=prev_hash, merkle_root=bytes(32), time=int(time.time()), bits=0x1d00ffff, nonce=0),
        transactions=[coinbase_tx]
    )
    merkle_root = temp_block.merkle_root()[::-1]

    # Mine
    target_bits = 0x1d00ffff
    target_value = (target_bits & 0xFFFFFF) * 2**(8 * ((target_bits >> 24) - 3))

    nonce = 0
    start = time.time()
    while True:
        header_data = (1).to_bytes(4, 'little') + prev_hash + merkle_root + int(time.time()).to_bytes(4, 'little') + target_bits.to_bytes(4, 'little') + nonce.to_bytes(4, 'little')
        import hashlib
        h = hashlib.sha256(hashlib.sha256(header_data).digest()).digest()
        if int.from_bytes(h[::-1], 'big') < target_value:
            print(f"✅ Found block {height}! Hash: {h[::-1].hex()}")
            
            # Check if another miner already saved this block
            block_file = f"{BLOCKCHAIN_DIR}/block_{height}.json"
            if os.path.exists(block_file):
                print(f"⚠️  Block {height} already exists (another miner was faster)")
                print(f"   Moving to next block...")
                break

            block_data = {
                'height': height,
                'hash': h[::-1].hex(),
                'prev_hash': prev_hash.hex(),
                'merkle_root': merkle_root.hex(),
                'time': int(time.time()),
                'bits': hex(target_bits),
                'nonce': nonce,
                'transactions': 1,
                'reward': reward,
                'miner_address': mining_address
            }
            with open(block_file, 'w') as f:
                json.dump(block_data, f, indent=2)
            print(f"💾 Saved to disk")
            break
        nonce += 1
        if nonce % 100000 == 0:
            print(f"  Nonce: {nonce:,}", end='\r')

    time.sleep(1)
