#!/usr/bin/env python3
"""Irium blockchain node with P2P networking."""

import sys
import os
import asyncio
import signal

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from irium.p2p import P2PNode
from irium.chain import ChainParams, ChainState
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
                inputs=[TxInput(prev_txid=bytes(32), prev_index=0xFFFFFFFF, script_sig=b"Genesis")],
                outputs=outputs
            )
            
            # Calculate merkle root
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
            
            # Create chain params and state
            pow_limit = Target(bits=int(genesis_data['bits'], 16))
            self.chain_params = ChainParams(genesis_block=genesis_block, pow_limit=pow_limit)
            self.chain_state = ChainState(params=self.chain_params)
            
            print(f"✅ Blockchain loaded at height {self.chain_state.height}")
            return True
        
        except Exception as e:
            print(f"❌ Failed to load blockchain: {e}")
            import traceback
            traceback.print_exc()
            return False
    
    async def handle_block(self, peer, block_data: bytes):
        """Handle received block from peer."""
        try:
            print(f"📦 Received block from {peer.address}")
            # TODO: Validate and add block to chain
            # For now, just log it
        except Exception as e:
            print(f"❌ Error handling block: {e}")
    
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
            max_peers=8,
            agent="irium-node/1.0",
            chain_height=self.chain_state.height
        )
        
        # Set callbacks
        self.p2p.on_block = self.handle_block
        self.p2p.on_tx = self.handle_tx
        self.p2p.on_peer_connected = self.handle_peer_connected
        
        # Start P2P networking
        await self.p2p.start()
        
        print()
        print("✅ Irium Node started successfully!")
        print(f"🌐 Listening for P2P connections on port {self.port}")
        print(f"📊 Blockchain height: {self.chain_state.height}")
        print(f"👥 Max peers: {self.p2p.max_peers}")
        print()
        
        # Status loop
        while self.running:
            await asyncio.sleep(30)
            
            # Print status
            peer_count = self.p2p.get_peer_count()
            print(f"📊 Status: {peer_count} peers connected, height {self.chain_state.height}")
            
            if peer_count > 0:
                peers_info = self.p2p.get_peers_info()
                for peer_info in peers_info[:3]:  # Show first 3
                    print(f"   • {peer_info['address']} ({peer_info['agent']}, height: {peer_info['height']})")
    
    async def stop(self):
        """Stop the node."""
        print()
        print("🛑 Stopping Irium Node...")
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
    
    signal.signal(signal.SIGINT, signal_handler)
    signal.signal(signal.SIGTERM, signal_handler)
    
    try:
        await node.start()
    except KeyboardInterrupt:
        await node.stop()


if __name__ == "__main__":
    asyncio.run(main())
