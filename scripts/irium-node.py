#!/usr/bin/env python3
"""Irium blockchain node with P2P networking."""

import sys
import os
import asyncio
import signal

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from irium.p2p import P2PNode
from irium.chain import ChainParams, ChainState
from irium.update_checker import UpdateChecker, display_update_notification
from irium.block import Block, BlockHeader
from irium.tx import Transaction, TxInput, TxOutput
from irium.pow import Target
import json


class IriumNode:
    """Full Irium blockchain node."""
    
    def __init__(self, port: int = 38291):
        self.port = port
        self.running = True
        
        # Initialize blockchain
        self.chain_params = None
        self.chain_state = None
        
        # P2P node
        self.p2p = None
    
    def load_blockchain(self):
        """Load blockchain from genesis."""
        print("📋 Loading blockchain...")
        
        # Load genesis
        genesis_file = os.path.join(os.path.dirname(__file__), '..', 'configs', 'genesis.json')
        
        if not os.path.exists(genesis_file):
            print("❌ Genesis file not found")
            return False
        
        try:
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
                inputs=[TxInput(prev_txid=bytes(32), prev_index=0xFFFFFFFF, script_sig=b"Irium Genesis Block - SHA256d PoW")],
                outputs=outputs
            )
            
            # Calculate merkle root
            temp_block = Block(
                header=BlockHeader(
                    version=1,
                    prev_hash=bytes(32),
                    merkle_root=bytes(32),
                    time=genesis_data.get('time', genesis_data['timestamp']),
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
                time=genesis_data.get('time', genesis_data['timestamp']),
                bits=int(genesis_data['bits'], 16),
                nonce=genesis_data.get('nonce', 0)
            )
            
            genesis_block = Block(header=genesis_header, transactions=[coinbase_tx])
            
            # Create chain params and state
            pow_limit = Target(bits=int(genesis_data['bits'], 16))
            self.chain_params = ChainParams(genesis_block=genesis_block, pow_limit=pow_limit)
            self.chain_state = ChainState(params=self.chain_params)
            
            # Load mined blocks from disk
            print("  Scanning for mined blocks...")
            blocks_dir = os.path.expanduser("~/.irium/blocks")
            print(f"  Blocks directory: {blocks_dir}")
            if os.path.exists(blocks_dir):
                print(f"  Found blocks directory with files: {os.listdir(blocks_dir)}")
                block_files = sorted([f for f in os.listdir(blocks_dir) if f.endswith(".json")])
                for block_file in block_files:
                    try:
                        with open(os.path.join(blocks_dir, block_file)) as bf:
                            block_data = json.load(bf)
                            # Migration: Add version field if missing
                            if "version" not in block_data:
                                block_data["version"] = 1
                            print(f"  Updated height to {block_data['height']}")
                        if block_data["height"] > self.chain_state.height:
                            self.chain_state.height = block_data["height"] + 1
                    except Exception as be:
                        pass

            print(f"✅ Blockchain loaded height: {self.chain_state.height - 1}")
            return True
        
        except Exception as e:
            print(f"❌ Failed to load blockchain: {e}")
            import traceback
            traceback.print_exc()
            return False
    
    async def handle_block(self, peer, block_data: bytes):
        """Handle received block from peer with fork prevention."""
        try:
            block_json = json.loads(block_data.decode("utf-8"))
            height = block_json.get("height", 0)
            block_hash = block_json.get("hash", "unknown")
            prev_hash = block_json.get("prev_hash", "")
            
            # Validate hash format
            if "test" in block_hash.lower() or len(block_hash) != 64:
                return
            try:
                bytes.fromhex(block_hash)
            except ValueError:
                return
            
            # FORK PREVENTION: Accept blocks that fill gaps or extend chain
            blocks_dir = os.path.expanduser("~/.irium/blocks")
            
            # Skip blocks we already have
            block_file = os.path.join(blocks_dir, f"block_{height}.json")
            if os.path.exists(block_file):
                return  # Already have this block
            
            # Validate it extends our current chain
            if self.chain_state.height > 0:
                tip_file = os.path.join(blocks_dir, f"block_{self.chain_state.height}.json")
                if os.path.exists(tip_file):
                    with open(tip_file) as f:
                        tip = json.load(f)
                    if prev_hash != tip["hash"]:
                        print(f"   ❌ FORK REJECTED: Block {height}")
                        return
            
            print(f"📦 Received block {height} from {peer.address}")

            # VALIDATE PROOF-OF-WORK (v1.1.1 fix)
            from irium.block import BlockHeader
            from irium.pow import Target
            
            block_hash = block_json.get('hash', '')
            
            # Reconstruct block header and verify PoW
            try:
                header = BlockHeader(
                    version=1,
                    prev_hash=bytes.fromhex(prev_hash),
                    merkle_root=bytes.fromhex(block_json['merkle_root']),
                    time=block_json['time'],
                    bits=int(block_json['bits'], 16),
                    nonce=block_json['nonce']
                )
                
                # Calculate and verify hash
                calculated_hash = header.hash().hex()
                if calculated_hash != block_hash:
                    print(f"   ❌ REJECTED: Block {height} has invalid hash (doesn't match header)")
                    print(f"      Claimed:    {block_hash[:32]}...")
                    print(f"      Calculated: {calculated_hash[:32]}...")
                    return
                
                # Verify hash meets difficulty target
                target = Target(header.bits)
                hash_int = int.from_bytes(header.hash(), 'big')
                if hash_int > target.to_target():
                    print(f"   ❌ REJECTED: Block {height} doesn't meet difficulty target")
                    print(f"      Hash: {hash_int}")
                    print(f"      Target: {target.to_target()}")
                    return
                    
            except Exception as e:
                print(f"   ❌ REJECTED: Block {height} validation error: {e}")
                return
            os.makedirs(blocks_dir, exist_ok=True)
            block_file = os.path.join(blocks_dir, f"block_{height}.json")
            with open(block_file, "w") as f:
                json.dump(block_json, f, indent=2)
            print(f"   💾 Saved block {height}")
            self.chain_state.height = height + 1
            self.p2p.chain_height = height
            peer.height = height
            print(f"   ✅ Height now {height}")
        except Exception as e:
            print(f"Error: {e}")

    async def handle_tx(self, peer, tx_data: bytes):
        """Handle received transaction from peer."""
        try:
            print(f"💸 Received transaction from {peer.address}")
            # TODO: Validate and add to mempool
            # For now, just log it
        except Exception as e:
            print(f"❌ Error handling transaction: {e}")
    
    async def handle_peer_connected(self, peer):
        """Handle new peer connection."""
        print(f"👋 New peer: {peer.address} ({peer.agent}, height: {peer.height})")
    
    async def start(self):
        """Start the node."""
        print("🚀 Starting Irium Node...")
        print(f"📡 Network: irium-mainnet")
        print(f"🔗 Port: {self.port}")
        print()
        
        # Load blockchain
        if not self.load_blockchain():
            print("❌ Failed to start node - blockchain initialization failed")
            return
        
        print()
        
        # Create P2P node
        self.p2p = P2PNode(
            port=self.port,
            max_peers=8000,
            agent="irium-node/1.0",
            chain_height=self.chain_state.height
        )
        
        # Set callbacks
        self.p2p.on_block = self.handle_block
        self.p2p.on_tx = self.handle_tx
        self.p2p.on_peer_connected = self.handle_peer_connected
        
        # Start P2P networking
        await self.p2p.start()

        # Periodic block rescan (every 5 seconds)
        async def rescan_blocks():
            """Periodically rescan blocks directory for new blocks."""
            while True:
                await asyncio.sleep(5)  # Check every 5 seconds
                
                blocks_dir = os.path.expanduser("~/.irium/blocks")
                if os.path.exists(blocks_dir):
                    block_files = sorted([f for f in os.listdir(blocks_dir) if f.endswith(".json")])
                    if block_files:
                        max_height = 0
                        for block_file in block_files:
                            try:
                                with open(os.path.join(blocks_dir, block_file)) as bf:
                                    block_data = json.load(bf)
                                    if block_data["height"] > max_height:
                                        max_height = block_data["height"]
                            except Exception:
                                pass
                        
                        if max_height > self.chain_state.height:
                            old_height = self.chain_state.height
                            self.chain_state.height = max_height + 1
                            self.p2p.chain_height = max_height
                            print(f"📊 Detected new blocks! Updated height: {old_height} -> {max_height}")
                            
                            # Broadcast newly detected blocks to peers
                            for h in range(old_height + 1, max_height + 1):
                                block_file = os.path.join(blocks_dir, f"block_{h}.json")
                                if os.path.exists(block_file):
                                    try:
                                        with open(block_file, 'rb') as bf:
                                            block_data = bf.read()
                                        peer_count = len(self.p2p.peers)
                                        print(f"  📡 Broadcasting block {h} to {peer_count} peers...")
                                        await self.p2p.broadcast_block(block_data)
                                    except Exception as e:
                                        print(f"  ⚠️  Broadcast error for block {h}: {e}")
        
        # Start rescan task
        asyncio.create_task(rescan_blocks())
        
        print()
        print("✅ Irium Node started successfully!")
        
        # Check for updates
        import irium
        checker = UpdateChecker(irium.__version__)
        if checker.should_check_now():
            update_info = checker.check_for_updates()
            if update_info:
                display_update_notification(update_info)
            checker.save_check_time()
        print(f"🌐 Listening for P2P connections on port {self.port}")
        print(f"📊 Blockchain: height: {self.chain_state.height - 1}")
        print(f"👥 Max peers: {self.p2p.max_peers}")
        print()
        
        # Status loop
        print("🔄 Entering main status loop...")
        while self.running:
            print("💓 Heartbeat - node is running")
            await asyncio.sleep(5)
            
            # Print status
            peer_count = self.p2p.get_peer_count()
            print(f"📊 Status: {peer_count} peers connected, height: {self.chain_state.height - 1}")
            
            if peer_count > 0:
                peers_info = self.p2p.get_peers_info()
                for peer_info in peers_info[:3]:  # Show first 3
                    print(f"   • {peer_info['address']} ({peer_info['agent']}, height: {peer_info['height']})")
    
    async def stop(self):
        """Stop the node."""
        import traceback, inspect
        print()
        print("🛑 Stopping Irium Node...")
        print("🔍 CALLER STACK:")
        for frame in inspect.stack()[1:5]:
            print(f"  {frame.filename}:{frame.lineno} in {frame.function}")
        self.running = False
        
        if self.p2p:
            await self.p2p.stop()
        
        print("✅ Node stopped")


async def main():
    """Main entry point."""
    # Parse port from command line
    port = 38291
    if len(sys.argv) > 1:
        try:
            port = int(sys.argv[1])
        except ValueError:
            print(f"Invalid port: {sys.argv[1]}")
            sys.exit(1)
    
    node = IriumNode(port=port)
    
    # Handle shutdown signals
    def signal_handler(signum, frame):
        asyncio.create_task(node.stop())
    
    # signal.signal(signal.SIGINT, signal_handler)  # TEMP DISABLED
    # signal.signal(signal.SIGTERM, signal_handler)  # TEMP DISABLED
    
    try:
        print("🔵 Calling node.start()...")
        await node.start()
        print("🔴 WARNING: node.start() RETURNED! (should run forever)")
        print("🔴 This means the while self.running loop exited")
        await node.stop()
    except KeyboardInterrupt:
        await node.stop()
    except Exception as e:
        print(f"🔴 EXCEPTION in main: {e}")
        import traceback
        traceback.print_exc()
        await node.stop()


if __name__ == "__main__":
    asyncio.run(main())
