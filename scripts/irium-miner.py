#!/usr/bin/env python3
"""Working Irium miner with actual PoW mining."""

import sys
import os
import asyncio
import signal
import json
import time

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from irium.wallet import Wallet
from irium.chain import ChainParams, ChainState
from irium.block import Block, BlockHeader
from irium.tx import Transaction, TxInput, TxOutput
from irium.pow import Target

WALLET_FILE = os.path.expanduser("~/.irium/irium-wallet.json")
MEMPOOL_FILE = os.path.expanduser("~/.irium/mempool/pending.json")
BLOCKCHAIN_DIR = os.path.expanduser("~/.irium/blocks")

class IriumMiner:
    def __init__(self):
        self.wallet = self.load_wallet()
        self.mining_address = self.get_mining_address()
        self.chain_params = None
        self.chain_state = None
        self.running = True
        self.blocks_mined = 0
        
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
        """Get address for mining rewards."""
        addresses = list(self.wallet.addresses())
        if addresses:
            return addresses[0]
        # Generate new address if none exists
        from irium.wallet import KeyPair
        key = KeyPair.generate()
        wif = key.to_wif()
        self.wallet.import_wif(wif)
        return key.address()
    
    def load_mempool(self):
        """Load pending transactions from mempool."""
        if os.path.exists(MEMPOOL_FILE):
            with open(MEMPOOL_FILE, 'r') as f:
                mempool = json.load(f)
            return [bytes.fromhex(tx['hex']) for tx in mempool]
        return []
    
    def clear_mempool(self):
        """Clear mempool after mining a block."""
        if os.path.exists(MEMPOOL_FILE):
            os.remove(MEMPOOL_FILE)
    
    def create_coinbase_transaction(self, height, reward):
        """Create coinbase transaction for mining reward."""
        # Coinbase input
        coinbase_input = TxInput(
            prev_txid=bytes(32),
            prev_index=0xFFFFFFFF,
            script_sig=f"Block {height}".encode()
        )
        
        # Coinbase output (mining reward)
        # Simple P2PKH-like script (not fully validated, just for testing)
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
    
    def mine_block(self, height, prev_hash, transactions, target):
        """Mine a new block."""
        print(f"⛏️  Mining block {height}...")
        print(f"  Transactions: {len(transactions)}")
        print(f"  Prev hash: {prev_hash.hex()[:16]}...")
        
        # Create temporary block to calculate merkle root
        temp_block = Block(
            header=BlockHeader(
                version=1,
                prev_hash=prev_hash,
                merkle_root=bytes(32),
                time=int(time.time()),
                bits=target.bits,
                nonce=0
            ),
            transactions=transactions
        )
        
        merkle_root = temp_block.merkle_root()[::-1]
        
        # Mine for valid nonce
        nonce = 0
        start_time = time.time()
        
        while self.running:
            header = BlockHeader(
                version=1,
                prev_hash=prev_hash,
                merkle_root=merkle_root,
                time=int(time.time()),
                bits=target.bits,
                nonce=nonce
            )
            
            header_hash = header.hash()[::-1]
            header_hash_int = int.from_bytes(header_hash, "big")
            
            if header_hash_int < target.to_target():
                elapsed = time.time() - start_time
                hashrate = nonce / elapsed if elapsed > 0 else 0
                
                print(f"\n✅ Block {height} mined!")
                print(f"  Hash: {header.hash().hex()}")
                print(f"  Nonce: {nonce}")
                print(f"  Time: {elapsed:.2f}s")
                print(f"  Hashrate: {hashrate:.2f} H/s")
                
                return Block(header=header, transactions=transactions)
            
            nonce += 1
            
            if nonce % 10000 == 0:
                elapsed = time.time() - start_time
                hashrate = nonce / elapsed if elapsed > 0 else 0
                print(f"  Nonce: {nonce:,} | Hashrate: {hashrate:.2f} H/s", end='\r')
        
        return None

    async def start(self):
        """Start mining."""
        print("⛏️  Starting Irium Miner...")
        print(f"💰 Mining address: {self.mining_address}")
        print()
        
        # Initialize blockchain
        print("📋 Initializing blockchain...")
        
        # Load genesis
        genesis_file = os.path.join(os.path.dirname(__file__), '..', 'configs', 'genesis.json')
        with open(genesis_file, 'r') as f:
            genesis_data = json.load(f)
        
        # Create genesis block
        allocations = genesis_data.get('allocations', [])
        outputs = []
        for alloc in allocations:
            script_pubkey = bytes.fromhex(alloc['script_pubkey'])
            amount = alloc['amount_sats']
            outputs.append(TxOutput(value=amount, script_pubkey=script_pubkey))
        
        coinbase_tx = Transaction(
            version=1,
            inputs=[TxInput(prev_txid=bytes(32), prev_index=0xFFFFFFFF, script_sig=b"Genesis")],
            outputs=outputs
        )
        
        temp_block = Block(
            header=BlockHeader(
                version=1,
                prev_hash=bytes(32),
                merkle_root=bytes(32),
                time=genesis_data['timestamp'],
                bits=int(genesis_data['bits'], 16),
                nonce=genesis_data.get('nonce', 0)
            ),
            transactions=[coinbase_tx]
        )
        
        merkle_root = temp_block.merkle_root()[::-1]
        
        genesis_header = BlockHeader(
            version=1,
            prev_hash=bytes(32),
            merkle_root=merkle_root,
            time=genesis_data['timestamp'],
            bits=int(genesis_data['bits'], 16),
            nonce=genesis_data.get('nonce', 0)
        )
        
        genesis_block = Block(header=genesis_header, transactions=[coinbase_tx])
        
        pow_limit = Target(bits=int(genesis_data['bits'], 16))
        self.chain_params = ChainParams(genesis_block=genesis_block, pow_limit=pow_limit)
        self.chain_state = ChainState(params=self.chain_params)
        
        print(f"✅ Blockchain initialized at height {self.chain_state.height}")
        print()
        
        # Mining loop
        while self.running:
            try:
                # Get current height and tip
                height = self.chain_state.height + 1
                tip_block = self.chain_state.chain[-1]  # Last block in chain
                prev_hash = tip_block.header.hash()
                
                # Calculate reward (50 IRM initially, halving every 210000 blocks)
                reward = 5000000000  # 50 IRM in satoshis
                halvings = (height - 1) // 210000
                reward = reward >> halvings
                
                # Create coinbase transaction
                coinbase_tx = self.create_coinbase_transaction(height, reward)
                
                # Get pending transactions from mempool
                mempool_txs = self.load_mempool()
                
                # Build transaction list
                transactions = [coinbase_tx]
                # TODO: Validate and add mempool transactions
                
                # Get target
                target = self.chain_params.pow_limit
                
                # Mine block
                block = self.mine_block(height, prev_hash, transactions, target)
                
                if block:
                    self.blocks_mined += 1
                    print(f"💰 Reward: {reward / 100000000} IRM")
                    print()
                    
                    # Clear mempool
                    if mempool_txs:
                        self.clear_mempool()
                        print(f"📝 Cleared {len(mempool_txs)} transactions from mempool")
                    
                    # Save block
                    os.makedirs(BLOCKCHAIN_DIR, exist_ok=True)
                    block_file = os.path.join(BLOCKCHAIN_DIR, f"block_{height}.json")
                    with open(block_file, 'w') as f:
                        json.dump({
                            'height': height,
                            'hash': block.header.hash().hex(),
                            'prev_hash': prev_hash.hex(),
                            'merkle_root': block.header.merkle_root.hex(),
                            'time': block.header.time,
                            'bits': hex(block.header.bits),
                            'nonce': block.header.nonce,
                            'transactions': len(transactions),
                            'reward': reward
                        }, f, indent=2)
                    
                    print(f"💾 Saved block to {block_file}")
                    print()
                    
                    # Add block to chain (simplified - just append)
                    self.chain_state.chain.append(block)
                    self.chain_state.height += 1
                    
                    print(f"📊 Chain height: {self.chain_state.height}")
                    print(f"📊 Total blocks mined: {self.blocks_mined}")
                    print()
                
                await asyncio.sleep(1)
                
            except Exception as e:
                print(f"❌ Mining error: {e}")
                import traceback
                traceback.print_exc()
                await asyncio.sleep(5)

    def stop(self):
        """Stop mining."""
        print("\n🛑 Stopping Irium Miner...")
        self.running = False

async def main():
    miner = IriumMiner()
    
    def signal_handler(signum, frame):
        miner.stop()
    
    signal.signal(signal.SIGINT, signal_handler)
    signal.signal(signal.SIGTERM, signal_handler)
    
    try:
        await miner.start()
    except KeyboardInterrupt:
        miner.stop()

if __name__ == "__main__":
    asyncio.run(main())
