#!/usr/bin/env python3
"""Simple miner without P2P - just mines and saves blocks."""
import sys, os, json, time, signal
sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from irium.wallet import Wallet
from irium.chain import ChainParams, ChainState
from irium.block import Block, BlockHeader
from irium.tx import Transaction, TxInput, TxOutput
from irium.pow import Target

BLOCKCHAIN_DIR = os.path.expanduser("~/.irium/blocks")
WALLET_FILE = os.path.expanduser("~/.irium/irium-wallet.json")

# Global flag for graceful shutdown
shutdown_requested = False

def signal_handler(signum, frame):
    """Handle shutdown signals gracefully."""
    global shutdown_requested
    print(f"\n🛑 Shutdown signal received (signal {signum}). Finishing current work...")
    shutdown_requested = True

# Register signal handlers
signal.signal(signal.SIGTERM, signal_handler)
signal.signal(signal.SIGINT, signal_handler)

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

while not shutdown_requested:
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
        outputs=[TxOutput(value=reward, script_pubkey=mining_address.encode())],
        locktime=0
    )

    # Build block
    header = BlockHeader(
        version=1,
        prev_hash=prev_hash,
        merkle_root=coinbase.txid(),
        time=int(time.time()),
        bits=0x1d00ffff,
        nonce=0
    )

    # Mine with TIME-BASED height checks and PROGRESS output
    last_check_time = time.time()
    last_progress_time = time.time()
    check_interval = 1.0  # Check for new blocks every 1 second
    progress_interval = 10.0  # Show progress every 10 seconds
    start_time = time.time()
    nonce_at_last_progress = 0

    while not shutdown_requested:
        h = header.hash()
        if int.from_bytes(h, 'big') < Target(header.bits).to_target():
            print(f"✅ Found block {height}! Hash: {h[::-1].hex()}")

            block_file = f"{BLOCKCHAIN_DIR}/block_{height}.json"
            if os.path.exists(block_file):
                print(f"⚠️  Block {height} already exists (another miner was faster)")
                break

            # Save block
            with open(block_file, 'w') as f:
                json.dump({
                    'height': height,
                    'hash': h.hex(),
                    'prev_hash': prev_hash.hex(),
                    'merkle_root': header.merkle_root.hex(),
                    'time': header.time,
                    'bits': hex(header.bits),
                    'nonce': header.nonce,
                    'transactions': [{'txid': coinbase.txid().hex(), 'data': coinbase.serialize().hex()}],
                    'miner_address': mining_address
                }, f, indent=2)
            print(f"💾 Saved to {block_file}")
            break

        header.nonce += 1

        # NONCE OVERFLOW FIX: Reset nonce and update timestamp when exhausted
        if header.nonce > 0xFFFFFFFF:  # Exceeded 4-byte limit (2^32 - 1)
            print(f"  🔄 Nonce space exhausted (4.29B attempts), updating timestamp...")
            header.nonce = 0
            header.time = int(time.time())
            start_time = time.time()  # Reset timer for new search space
            nonce_at_last_progress = 0

        current_time = time.time()

        # Show progress every 10 seconds
        if current_time - last_progress_time >= progress_interval:
            elapsed = current_time - start_time
            nonces_tried = header.nonce - nonce_at_last_progress
            hashrate = nonces_tried / progress_interval if progress_interval > 0 else 0
            print(f"  📊 Block {height} | Nonce: {header.nonce:,} | Hashrate: {hashrate:,.0f} H/s | Time: {int(elapsed)}s")
            last_progress_time = current_time
            nonce_at_last_progress = header.nonce

        # TIME-BASED check for new blocks (every 1 second)
        if current_time - last_check_time >= check_interval:
            last_check_time = current_time
            current_height = get_current_height()
            if current_height > height:
                print(f"⚠️  Block {height} found by another miner! Moving to {current_height}")
                break

print("✅ Miner stopped gracefully.")
