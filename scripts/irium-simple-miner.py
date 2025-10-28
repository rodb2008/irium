#!/usr/bin/env python3
"""Simple Irium miner - no P2P, just mining for multicore setups."""

import sys
import os
import json
import time
import hashlib

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from irium.wallet import Wallet
from irium.chain import ChainParams, ChainState
from irium.block import Block, BlockHeader
from irium.tx import Transaction, TxInput, TxOutput
from irium.pow import Target

WALLET_FILE = os.path.expanduser("~/.irium/irium-wallet.json")
BLOCKCHAIN_DIR = os.path.expanduser("~/.irium/blocks")

class SimpleIriumMiner:
    def __init__(self):
        self.wallet = self.load_wallet()
        self.mining_address = self.get_mining_address()
        
    def load_wallet(self):
        """Load wallet from file."""
        wallet = Wallet()
        if os.path.exists(WALLET_FILE):
            with open(WALLET_FILE, 'r') as f:
                data = json.load(f)
            for addr, wif in data.get('keys', {}).items():
                wallet.import_wif(wif)
        return wallet

    def get_mining_address(self):
        """Get mining address."""
        addresses = list(self.wallet.addresses())
        if addresses:
            return addresses[0]
        print("❌ ERROR: No wallet found! Create wallet first.")
        sys.exit(1)

    def get_latest_block(self):
        """Get the latest block info."""
        if not os.path.exists(BLOCKCHAIN_DIR):
            return None
            
        block_files = [f for f in os.listdir(BLOCKCHAIN_DIR) if f.startswith('block_') and f.endswith('.json') and 'backup' not in f]
        if not block_files:
            return None
            
        latest_file = sorted(block_files, key=lambda x: int(x.split('_')[1].split('.')[0]))[-1]
        
        with open(os.path.join(BLOCKCHAIN_DIR, latest_file), 'r') as f:
            return json.load(f)

    def create_coinbase_transaction(self, height, reward):
        """Create coinbase transaction."""
        coinbase_input = TxInput(
            prev_txid=bytes(32),
            prev_index=0xFFFFFFFF,
            script_sig=f"Block {height}".encode()
        )

        script_pubkey = bytes.fromhex(f"76a914{self.mining_address[1:21].encode().hex()}88ac")

        coinbase_output = TxOutput(
            value=reward,
            script_pubkey=script_pubkey
        )

        return Transaction(
            version=1,
            inputs=[coinbase_input],
            outputs=[coinbase_output]
        )

    def mine(self):
        """Mine blocks continuously."""
        print(f"⛏️  Simple Miner Started")
        print(f"💰 Mining address: {self.mining_address}")
        print(f"📁 Blocks directory: {BLOCKCHAIN_DIR}")
        print()

        while True:
            try:
                # Get current chain state
                latest_block = self.get_latest_block()
                if latest_block:
                    height = latest_block['height'] + 1
                    prev_hash = bytes.fromhex(latest_block['hash'])
                else:
                    # Genesis mining
                    height = 1
                    genesis_file = os.path.join(os.path.dirname(__file__), '..', 'configs', 'genesis.json')
                    with open(genesis_file, 'r') as f:
                        genesis_data = json.load(f)
                    prev_hash = bytes.fromhex(genesis_data['hash'])

                # Create block
                reward = 5000000000  # 50 IRM
                coinbase_tx = self.create_coinbase_transaction(height, reward)
                
                block = Block(
                    header=BlockHeader(
                        version=1,
                        prev_hash=prev_hash,
                        merkle_root=coinbase_tx.txid(),
                        time=int(time.time()),
                        bits=0x1d00ffff,
                        nonce=0
                    ),
                    transactions=[coinbase_tx]
                )

                # Mine the block
                target = Target(bits=0x1d00ffff)
                start_time = time.time()
                hashes = 0
                
                print(f"⛏️  Mining block {height}...")
                
                while True:
                    block.header.nonce += 1
                    hashes += 1
                    
                    if block.header.hash_int() <= target.to_int():
                        # Block found!
                        elapsed = time.time() - start_time
                        hashrate = hashes / elapsed if elapsed > 0 else 0
                        
                        print(f"🎉 Block {height} found!")
                        print(f"   Hash: {block.header.hash().hex()}")
                        print(f"   Nonce: {block.header.nonce:,}")
                        print(f"   Hashrate: {hashrate:,.0f} H/s")
                        print(f"   Time: {elapsed:.1f}s")
                        
                        # Save block (main node will detect and broadcast)
                        os.makedirs(BLOCKCHAIN_DIR, exist_ok=True)
                        block_file = os.path.join(BLOCKCHAIN_DIR, f"block_{height}.json")
                        
                        block_data = {
                            'height': height,
                            'hash': block.header.hash().hex(),
                            'prev_hash': prev_hash.hex(),
                            'merkle_root': block.header.merkle_root.hex(),
                            'time': block.header.time,
                            'bits': hex(block.header.bits),
                            'nonce': block.header.nonce,
                            'transactions': 1,
                            'reward': reward,
                            'miner_address': self.mining_address
                        }
                        
                        with open(block_file, 'w') as f:
                            json.dump(block_data, f, indent=4)
                        
                        print(f"💾 Block saved: {block_file}")
                        print()
                        break
                    
                    # Progress update every 100k hashes
                    if hashes % 100000 == 0:
                        elapsed = time.time() - start_time
                        hashrate = hashes / elapsed if elapsed > 0 else 0
                        print(f"   Nonce: {block.header.nonce:,} | Hashrate: {hashrate:,.0f} H/s")

            except KeyboardInterrupt:
                print("\n🛑 Mining stopped")
                break
            except Exception as e:
                print(f"❌ Mining error: {e}")
                time.sleep(5)

if __name__ == "__main__":
    miner = SimpleIriumMiner()
    miner.mine()
