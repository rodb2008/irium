#!/usr/bin/env python3
import sys

"""Irium miner with P2P block broadcasting."""

import sys
import os
import asyncio
import signal
import json
import time
from pathlib import Path

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from irium.wallet import Wallet
from irium.update_checker import UpdateChecker, display_update_notification
from irium.chain import ChainParams, ChainState
from irium.block import Block, BlockHeader
from irium.tx import Transaction, TxInput, TxOutput
from irium.pow import Target
from irium.p2p import P2PNode
from irium.tools.genesis_loader import load_locked_genesis
from irium.anchors import AnchorManager, AnchorVerificationError

WALLET_FILE = os.path.expanduser(os.getenv("IRIUM_WALLET_FILE", "~/.irium/irium-wallet.json"))
MEMPOOL_FILE = os.path.expanduser(os.getenv("IRIUM_MEMPOOL_FILE", "~/.irium/mempool/pending.json"))
BLOCKCHAIN_DIR = os.path.expanduser(os.getenv("IRIUM_BLOCKS_DIR", "~/.irium/blocks"))


class IriumMiner:
    def __init__(self, p2p_port: int = 38292):
        self.wallet = self.load_wallet()
        self.mining_address = self.get_mining_address()
        self.chain_params = None
        self.chain_state = None
        self.running = True
        self.blocks_mined = 0
        self.p2p_port = p2p_port
        self.p2p = None
        self.repo_root = Path(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
        self.anchor_manager = None

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

        # No wallet found - require user to create one
        print("❌ ERROR: No wallet found!")
        print("You must create a wallet BEFORE mining:")
        print("  1. python3 scripts/irium-wallet-proper.py create")
        print("  2. python3 scripts/irium-wallet-proper.py new-address")
        print("  3. python3 scripts/irium-wallet-proper.py balance")
        print("⚠️  IMPORTANT: Backup your wallet keys!")
        print("  Wallet location: ~/.irium/wallet.dat")
        print("")
        import sys
        

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

    async def mine_block(self, height, prev_hash, transactions, target):
        """Mine a new block with nonce overflow protection."""
        print(f"⛏️  Mining block {height}...")
        print(f"  Transactions: {len(transactions)}")
        print(f"  Prev hash: {prev_hash.hex()[:16]}...")

        nonce = 0
        start_time = time.time()
        block_time = int(time.time())

        while self.running:
            # Recalculate merkle root with current timestamp
            temp_block = Block(
                header=BlockHeader(
                    version=1,
                    prev_hash=prev_hash,
                    merkle_root=bytes(32),
                    time=block_time,
                    bits=target.bits,
                    nonce=0
                ),
                transactions=transactions
            )
            merkle_root = temp_block.merkle_root()[::-1]

            header = BlockHeader(
                version=1,
                prev_hash=prev_hash,
                merkle_root=merkle_root,
                time=block_time,
                bits=target.bits,
                nonce=nonce
            )

            header_hash = header.hash()
            header_hash_int = int.from_bytes(header_hash, "big")

            if header_hash_int < target.to_target():
                elapsed = time.time() - start_time
                hashrate = nonce / elapsed if elapsed > 0 else 0

                print(f"✅ Block {height} mined!")
                print(f"  Hash: {header.hash().hex()}")
                print(f"  Nonce: {nonce}")
                print(f"  Time: {elapsed:.2f}s")
                print(f"  Hashrate: {hashrate:.2f} H/s")

                return Block(header=header, transactions=transactions)

            nonce += 1

            # NONCE OVERFLOW FIX: Reset nonce and update timestamp when exhausted
            if nonce > 0xFFFFFFFF:
                print(f"  🔄 Nonce space exhausted (4.29B attempts), updating timestamp...")
                nonce = 0
                block_time = int(time.time())
                start_time = time.time()  # Reset timer

            if nonce % 100 == 0:
                elapsed = time.time() - start_time
                hashrate = nonce / elapsed if elapsed > 0 else 0
                print(f"  Nonce: {nonce:,} | Hashrate: {hashrate:.2f} H/s", end='\r')
                await asyncio.sleep(0)  # Yield to other tasks

        return None

    async def handle_peer_block(self, peer, block_data: bytes):
        """Handle block with fork prevention."""
        try:
            import json
            block_json = json.loads(block_data.decode('utf-8'))
            height = block_json.get('height', 0)
            block_hash = block_json.get('hash', 'unknown')
            prev_hash = block_json.get('prev_hash', '')

            # Validate format
            if "test" in block_hash.lower() or len(block_hash) != 64:
                return
            try:
                bytes.fromhex(block_hash)
            except ValueError:
                return

            if self.anchor_manager and not self.anchor_manager.verify_block_against_anchors(height, block_hash):
                print(f"  ❌ Anchor mismatch for block {height}")
                return

            # FORK PREVENTION: Only accept next block
            if height != self.chain_state.height + 1:
                return

            # Validate it extends our chain
            if self.chain_state.height > 0:
                tip_file = os.path.join(BLOCKCHAIN_DIR, f"block_{self.chain_state.height}.json")
                if os.path.exists(tip_file):
                    with open(tip_file) as f:
                        tip = json.load(f)
                    if prev_hash != tip['hash']:
                        print(f"  ❌ FORK REJECTED: Block {height}")
                        return

            print(f"📦 Received block {height}")
            os.makedirs(BLOCKCHAIN_DIR, exist_ok=True)
            with open(os.path.join(BLOCKCHAIN_DIR, f"block_{height}.json"), 'w') as f:
                json.dump(block_json, f, indent=2)
            self.chain_state.height = max(self.chain_state.height, height + 1)
            self.p2p.chain_height = height
            print(f"  ✅ Updated to height {height}")
        except Exception as e:
            print(f"  ❌ Error: {e}")
            import traceback
            traceback.print_exc()

    async def handle_peer_tx(self, peer, tx_data: bytes):
        """Handle transaction from peer."""
        try:
            print(f"💸 Received transaction from {peer.address}")
            # TODO: Add to mempool
        except Exception as e:
            print(f"❌ Error handling peer tx: {e}")

    async def start(self):
        """Start mining with P2P."""
        print("⛏️  Starting Irium Miner with P2P...")
        print(f"💰 Mining address: {self.mining_address}")
        print(f"🔗 P2P port: {self.p2p_port}")
        print()

        # Initialize blockchain
        print("📋 Initializing blockchain...")

        genesis_block, locked_payload = load_locked_genesis(self.repo_root)
        header = locked_payload["header"]
        print(f"  🔐 Locked genesis hash: {header['hash']}")

        pow_limit = Target(bits=int(header['bits'], 16))
        self.chain_params = ChainParams(genesis_block=genesis_block, pow_limit=pow_limit)
        self.chain_state = ChainState(params=self.chain_params)
        
        anchor_default = self.repo_root / "bootstrap/anchors.json"
        anchors_env = os.getenv("IRIUM_ANCHORS_FILE", str(anchor_default))
        try:
            self.anchor_manager = AnchorManager(anchors_env)
            if self.anchor_manager.payload_digest:
                print(f"  📌 Anchors digest: {self.anchor_manager.payload_digest[:16]}...")
        except AnchorVerificationError as exc:
            print(f"❌ Anchor verification failed: {exc}")
            return

        # Scan for existing mined blocks
        blocks_dir = BLOCKCHAIN_DIR
        if os.path.exists(blocks_dir):
            block_files = os.listdir(blocks_dir)
            for block_file in block_files:
                if block_file.startswith("block_") and block_file.endswith(".json"):
                    try:
                        height = int(block_file.replace("block_", "").replace(".json", ""))
                        if height > self.chain_state.height:
                            self.chain_state.height = max(self.chain_state.height, height + 1)
                    except ValueError:
                        pass

        print(f"✅ Blockchain initialized at height {self.chain_state.height}")

        # Start P2P
        print(f"🌐 Starting P2P networking on port {self.p2p_port}...")
        self.p2p = P2PNode(
            port=self.p2p_port,
            max_peers=8000,
            agent="irium-miner/1.0",
            chain_height=self.chain_state.height - 1,
            anchor_manager=self.anchor_manager
        )

        self.p2p.on_block = self.handle_peer_block
        self.p2p.on_tx = self.handle_peer_tx

        await self.p2p.start()
        print(f"✅ P2P node started")
        print()

        # Mining loop
        while self.running:
            try:
                height = self.chain_state.height

                # Get prev_hash from the actual tip block file
                if height == 1:
                    # Genesis
                    tip_block = self.chain_state.chain[-1]
                    prev_hash = tip_block.header.hash()
                else:
                    # Load from disk
                    prev_block_file = os.path.join(BLOCKCHAIN_DIR, f"block_{height-1}.json")
                    with open(prev_block_file, 'r') as f:
                        prev_block = json.load(f)
                    prev_hash = bytes.fromhex(prev_block['hash'])

                reward = 5000000000  # 50 IRM
                halvings = (height - 1) // 210000
                reward = reward >> halvings

                coinbase_tx = self.create_coinbase_transaction(height, reward)
                mempool_txs = self.load_mempool()
                transactions = [coinbase_tx]

                target = self.chain_params.pow_limit

                block = await self.mine_block(height, prev_hash, transactions, target)

                if block:
                    self.blocks_mined += 1
                    print(f"💰 Reward: {reward / 100000000} IRM")

                    # Broadcast block to P2P network
                    print(f"📡 Broadcasting block to {self.p2p.get_peer_count()} peers...")
                    block_json = json.dumps({
                        'height': height,
                        'hash': block.header.hash().hex(),
                        'prev_hash': prev_hash.hex(),
                        'merkle_root': block.header.merkle_root.hex(),
                        'time': block.header.time,
                        'bits': hex(block.header.bits),
                        'nonce': block.header.nonce,
                        'transactions': len(transactions),
                        'reward': reward,
                        'miner_address': self.mining_address
                    })
                    await self.p2p.broadcast_block(block_json.encode('utf-8'))
                    print(f"✅ Block broadcast complete")
                    print()

                    if mempool_txs:
                        self.clear_mempool()

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
                            'reward': reward,
                            'miner_address': self.mining_address
                        }, f, indent=2)

                    print(f"💾 Saved block to {block_file}")

                    self.chain_state.chain.append(block)
                    self.chain_state.height += 1

                    print(f"📊 Chain height: {self.chain_state.height}")
                    print(f"📊 Total blocks mined: {self.blocks_mined}")
                    print(f"👥 Connected peers: {self.p2p.get_peer_count()}")
                    print()

                await asyncio.sleep(1)

            except Exception as e:
                print(f"❌ Mining error: {e}")
                import traceback
                traceback.print_exc()
                await asyncio.sleep(5)

    async def stop(self):
        """Stop mining."""
        print("🛑 Stopping Irium Miner...")
        self.running = False

        if self.p2p:
            await self.p2p.stop()

        print("✅ Miner stopped")


async def main():
    # Parse port from command line
    port = 38292  # Different from node (38291)
    if len(sys.argv) > 1:
        try:
            port = int(sys.argv[1])
        except ValueError:
            print(f"Invalid port: {sys.argv[1]}")
        

    miner = IriumMiner(p2p_port=port)

    def signal_handler(signum, frame):
        asyncio.create_task(miner.stop())

    signal.signal(signal.SIGINT, signal_handler)
    signal.signal(signal.SIGTERM, signal_handler)

    try:
        await miner.start()
    except KeyboardInterrupt:
        await miner.stop()


if __name__ == "__main__":
    asyncio.run(main())
