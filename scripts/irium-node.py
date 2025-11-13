#!/usr/bin/env python3
"""Irium blockchain node with P2P networking."""

import sys
import os
import asyncio
import signal
from pathlib import Path

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from irium.p2p import P2PNode
from irium.chain import ChainParams, ChainState
from irium.update_checker import UpdateChecker, display_update_notification
from irium.block import BlockHeader
from irium.pow import Target
from irium.tools.genesis_loader import load_locked_genesis
from irium.anchors import AnchorManager, EclipseProtection, AnchorVerificationError
import json
import argparse

BASE_NODE_PORT = 38291
SYSTEM_NODE_PORT_FILE = Path.home() / ".irium" / "system-node-port"


def _resolve_default_port() -> int:
    """Pick default P2P port, allowing system services to override it."""
    candidate = BASE_NODE_PORT
    if os.environ.get("INVOCATION_ID") and SYSTEM_NODE_PORT_FILE.exists():
        try:
            configured = int(SYSTEM_NODE_PORT_FILE.read_text().strip())
            if 1024 <= configured <= 65535:
                candidate = configured
            else:
                print(f"⚠️  Ignoring invalid system-node-port value {configured}; using {BASE_NODE_PORT}")
        except ValueError:
            print("⚠️  Could not parse ~/.irium/system-node-port; using default 38291")
    return candidate


parser = argparse.ArgumentParser()
parser.add_argument(
    "--port",
    type=int,
    default=_resolve_default_port(),
    help=(
        "P2P port (defaults to 38291; systemd services can override via ~/.irium/system-node-port)"
    ),
)
args, _ = parser.parse_known_args()



class IriumNode:
    """Full Irium blockchain node."""
    
    def __init__(self, port: int = 38291):
        self.port = port
        self.running = True
        self.repo_root = Path(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
        
        # Initialize blockchain
        self.chain_params = None
        self.chain_state = None
        self.anchor_manager = None
        self.eclipse_protection = None
        
        # P2P node
        self.p2p = None
    
    def load_blockchain(self):
        """Load blockchain from genesis."""
        print("📋 Loading blockchain...")
        
        try:
            genesis_block, locked_payload = load_locked_genesis(self.repo_root)
            header = locked_payload["header"]

            derived_hash = genesis_block.header.hash().hex()
            if derived_hash.lower() != header["hash"].lower():
                print("❌ Locked genesis hash mismatch")
                print(f"   Expected: {header['hash']}")
                print(f"   Derived : {derived_hash}")
                return False

            print(f"  🔐 Locked genesis hash: {header['hash']}")

            # Create chain params and state
            pow_limit = Target(bits=int(header["bits"], 16))
            self.chain_params = ChainParams(genesis_block=genesis_block, pow_limit=pow_limit)
            self.chain_state = ChainState(params=self.chain_params)
            
            anchor_default = self.repo_root / "bootstrap/anchors.json"
            anchors_env = os.getenv("IRIUM_ANCHORS_FILE", str(anchor_default))
            try:
                self.anchor_manager = AnchorManager(anchors_env)
                self.eclipse_protection = EclipseProtection(self.anchor_manager)
                if self.anchor_manager.payload_digest:
                    print(f"  📌 Anchors digest: {self.anchor_manager.payload_digest[:16]}...")
            except AnchorVerificationError as exc:
                print(f"❌ Anchor verification failed: {exc}")
                return False
            
            # Load mined blocks from disk
            print("  Scanning for mined blocks...")
            blocks_dir = os.path.expanduser(os.getenv("IRIUM_BLOCKS_DIR","~/.irium/blocks"))
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
                        self.chain_state.height = max(self.chain_state.height, block_data["height"] + 1)
                    except Exception as be:
                        pass

            actual_height = self.chain_state.height - 1
            print(f"✅ Blockchain loaded height: {actual_height}")
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
            
            if self.anchor_manager and not self.anchor_manager.verify_block_against_anchors(height, block_hash):
                print(f"   ❌ REJECTED: Block {height} mismatches signed anchor")
                return

            # FORK PREVENTION: Accept blocks that fill gaps or extend chain
            blocks_dir = os.path.expanduser(os.getenv("IRIUM_BLOCKS_DIR","~/.irium/blocks"))
            
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
            chain_height=self.chain_state.height - 1,
            anchor_manager=self.anchor_manager
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
                
                blocks_dir = os.path.expanduser(os.getenv("IRIUM_BLOCKS_DIR","~/.irium/blocks"))
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
    port = args.port
#    if len(sys.argv) > 1:
#        try:
#            port = int(sys.argv[1])
#        except ValueError:
#            print(f"Invalid port: {sys.argv[1]}")
#            sys.exit(1)
    
    node = IriumNode(port=args.port)
    
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
