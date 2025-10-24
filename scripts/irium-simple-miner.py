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

# Load wallet
if not os.path.exists(WALLET_FILE):
    print("❌ Wallet not found! Create one first:")
    print("   python3 scripts/irium-wallet-proper.py create")
    exit(1)

with open(WALLET_FILE, 'r') as f:
    wallet_data = json.load(f)

if not wallet_data.get('addresses'):
    print("❌ No addresses in wallet!")
    exit(1)

mining_address = wallet_data['addresses'][0]
print(f"💰 Mining to: {mining_address}")

def get_current_height():
    """Get current blockchain height from disk."""
    if not os.path.exists(BLOCKCHAIN_DIR):
        os.makedirs(BLOCKCHAIN_DIR)
    blocks = [int(f.replace('block_', '').replace('.json', ''))
              for f in os.listdir(BLOCKCHAIN_DIR) if f.startswith('block_') and f.endswith('.json')]
    return max(blocks) + 1 if blocks else 1

while True:
    # Get current height
    height = get_current_height()

    # Load prev block
    if height > 1:
        with open(f"{BLOCKCHAIN_DIR}/block_{height-1}.json") as f:
            prev_block = json.load(f)
        prev_hash = bytes.fromhex(prev_block['hash'])
    else:
        prev_hash = bytes.fromhex('cbdd1b9134adc846b3af5e2128f68214e1d8154912ff8da40685f47700000000')

    print(f"⛏️  Mining block {height}...")

    # Create coinbase
    halvings = (height - 1) // 210000
    reward = 5000000000 >> halvings
    
    coinbase = Transaction(
        version=1,
        inputs=[TxInput(prev_txid=bytes(32), prev_index=0xFFFFFFFF, script_sig=f"Block {height}".encode())],
        outputs=[TxOutput(amount=reward, script_pubkey=mining_address.encode())],
        locktime=0
    )

    # Build block
    header = BlockHeader(
        version=1,
        prev_hash=prev_hash,
        merkle_root=coinbase.hash(),
        timestamp=int(time.time()),
        bits=0x1d00ffff,
        nonce=0
    )

    # Mine with periodic height checks
    nonce_counter = 0
    check_interval = 10000  # Check for new blocks every 10k nonces
    
    while True:
        h = header.hash()
        if int.from_bytes(h, 'big') < Target(header.bits).value:
            print(f"✅ Found block {height}! Hash: {h[::-1].hex()}")
            
            block_file = f"{BLOCKCHAIN_DIR}/block_{height}.json"
            if os.path.exists(block_file):
                print(f"⚠️  Block {height} already exists (another miner was faster)")
                break
            
            # Save block
            with open(block_file, 'w') as f:
                json.dump({
                    'height': height,
                    'hash': h[::-1].hex(),
                    'prev_hash': prev_hash[::-1].hex(),
                    'merkle_root': header.merkle_root[::-1].hex(),
                    'timestamp': header.timestamp,
                    'bits': header.bits,
                    'nonce': header.nonce,
                    'transactions': [coinbase.to_dict()],
                    'miner_address': mining_address
                }, f, indent=2)
            print(f"💾 Saved to {block_file}")
            break
        
        header.nonce += 1
        nonce_counter += 1
        
        # Check if another miner found this block (every 10k nonces)
        if nonce_counter % check_interval == 0:
            current_height = get_current_height()
            if current_height > height:
                print(f"⚠️  Block {height} found by another miner! Moving to {current_height}")
                break
