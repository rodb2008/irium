"""P2P networking for Irium blockchain."""

from __future__ import annotations
import os
import asyncio
import time
import random
from typing import Dict, Set, Optional, Callable
from dataclasses import dataclass, field

from .protocol import (
    Message, MessageType,
    HandshakeMessage, PingMessage, PongMessage,
    GetPeersMessage, PeersMessage,
    BlockMessage, TxMessage, DisconnectMessage
)
from .network import PeerDirectory, SeedlistManager


@dataclass
class Peer:
    """Connected peer."""
    reader: asyncio.StreamReader
    writer: asyncio.StreamWriter
    address: str
    agent: str = "unknown"
    height: int = 0
    connected_at: float = field(default_factory=time.time)
    last_ping: float = field(default_factory=time.time)
    
    async def send_message(self, msg: Message) -> None:
        """Send a message to this peer."""
        try:
            data = msg.serialize()
            self.writer.write(data)
            # Add timeout to drain to prevent hanging
            await asyncio.wait_for(self.writer.drain(), timeout=30.0)
        except asyncio.TimeoutError:
            print(f"⚠️  Send timeout to {self.address} (data size: {len(data)} bytes)")
            # Don't raise - connection might still be usable
            pass
        except Exception as e:
            print(f"Error sending message to {self.address}: {e}")
            # Don't raise - let the cleanup task handle dead connections
            pass
    
    async def recv_message(self) -> Optional[Message]:
        """Receive a message from this peer."""
        try:
            # Read header (6 bytes: version, type, length)
            header = await self.reader.readexactly(6)
            if not header:
                return None

            # Parse header to get payload length
            import struct
            version, msg_type, length = struct.unpack("!BBI", header)

            # Read payload if any
            if length > 0:
                payload = await self.reader.readexactly(length)
            else:
                payload = b""

            # Deserialize the COMPLETE message (header + payload)
            full_message = header + payload
            return Message.deserialize(full_message)

        except asyncio.IncompleteReadError:
            return None
        except Exception as e:
            print(f"Error receiving message from {self.address}: {e}")
            return None

    def close(self) -> None:
        """Close connection."""
        try:
            self.writer.close()
        except:
            pass


class P2PNode:
    """P2P network node for Irium."""
    
    def __init__(
        self,
        port: int = 38291,
        max_peers: int = 8,
        agent: str = "irium-node/1.0",
        chain_height: int = 0
    ):
        self.port = port
        self.max_peers = max_peers
        self.agent = agent
        self.chain_height = chain_height
        
        self.peers: Dict[str, Peer] = {}
        self.server: Optional[asyncio.Server] = None
        self.running = False
        
        # Callbacks for handling messages
        self.on_block: Optional[Callable] = None
        self.on_tx: Optional[Callable] = None
        self.on_peer_connected: Optional[Callable] = None
        
        # Peer management
        self.peer_directory = PeerDirectory()
        self.seedlist_manager = SeedlistManager()
    
    async def start(self) -> None:
        """Start the P2P node."""
        print(f"🚀 Starting P2P Node on port {self.port}")
        self.running = True
        
        # Start server
        self.server = await asyncio.start_server(
            self._handle_incoming_connection,
            '0.0.0.0',
            self.port
        )
        
        print(f"✅ P2P Node listening on port {self.port}")
        
        # Start background tasks
        asyncio.create_task(self._connect_to_peers())
        asyncio.create_task(self._ping_peers())
        asyncio.create_task(self._cleanup_dead_peers())
        asyncio.create_task(self._periodic_sync_check())
    
    async def _periodic_sync_check(self) -> None:
        """Periodically check if connected peers are ahead and request blocks."""
        while self.running:
            await asyncio.sleep(60)  # Check every 60 seconds
            
            for peer in list(self.peers.values()):
                if peer.height > self.chain_height:
                    print(f"🔄 Periodic check: Peer {peer.address} is ahead ({peer.height} vs {self.chain_height}), requesting blocks...")
                    try:
                        from irium.protocol import GetBlocksMessage
                        genesis_hash = bytes.fromhex('cbdd1b9134adc846b3af5e2128f68214e1d8154912ff8da40685f47700000000')
                        count = min(500, peer.height - self.chain_height)
                        get_blocks = GetBlocksMessage(start_hash=genesis_hash, count=count)
                        await peer.send_message(get_blocks.to_message())
                        print(f"  📥 Requested {count} blocks from {peer.address}")
                    except Exception as e:
                        print(f"  ⚠️  Error requesting blocks: {e}")

    async def stop(self) -> None:
        """Stop the P2P node."""
        print("🛑 Stopping P2P Node...")
        self.running = False
        
        # Close all peer connections
        for peer in list(self.peers.values()):
            await self._disconnect_peer(peer, "Node shutting down")
        
        # Close server
        if self.server:
            self.server.close()
            await self.server.wait_closed()
        
        print("✅ P2P Node stopped")
    
    async def _handle_incoming_connection(
        self,
        reader: asyncio.StreamReader,
        writer: asyncio.StreamWriter
    ) -> None:
        """Handle incoming peer connection."""
        addr = writer.get_extra_info('peername')
        address = f"{addr[0]}:{addr[1]}"
        
        print(f"📥 Incoming connection from {address}")
        
        if len(self.peers) >= self.max_peers:
            print(f"⚠️  Max peers reached, rejecting {address}")
            writer.close()
            await writer.wait_closed()
            return
        
        peer = Peer(reader=reader, writer=writer, address=address)
        
        try:
            # Perform handshake
            if await self._perform_handshake(peer, is_initiator=False):
                self.peers[address] = peer
                # Send immediate ping
                await asyncio.sleep(0.1)
                import random
                from irium.protocol import PingMessage
                nonce = random.randint(0, 2**64 - 1)
                ping = PingMessage(nonce=nonce)
                await peer.send_message(ping.to_message())
                print(f"✅ Peer connected: {address} ({peer.agent}, height: {peer.height})")
                

                # Request blocks if peer is ahead
                if peer.height > self.chain_height:
                    print(f"  Peer is ahead ({peer.height} vs {self.chain_height}), requesting blocks...")
                    # Request blocks from our height to their height
                    from irium.protocol import GetBlocksMessage
                    # Request blocks we're missing
                    # For now, use genesis hash as start (TODO: use actual last block hash)
                    genesis_hash = bytes.fromhex('cbdd1b9134adc846b3af5e2128f68214e1d8154912ff8da40685f47700000000')
                    count = min(500, peer.height - self.chain_height)  # Request up to 500 blocks
                    get_blocks = GetBlocksMessage(start_hash=genesis_hash, count=count)
                    await peer.send_message(get_blocks.to_message())
                    print(f"  📥 Requested blocks {self.chain_height + 1} to {peer.height}")

                if self.on_peer_connected:
                    await self.on_peer_connected(peer)
                
                # Handle messages from this peer
                asyncio.create_task(self._handle_peer_messages(peer))
            else:
                print(f"❌ Handshake failed with {address}")
                peer.close()
        
        except Exception as e:
            print(f"❌ Error handling connection from {address}: {e}")
            if address in self.peers:
                del self.peers[address]
            peer.close()
    
    async def _perform_handshake(self, peer: Peer, is_initiator: bool) -> bool:
        """Perform handshake with peer."""
        try:
            if is_initiator:
                # Send our handshake first
                handshake = HandshakeMessage(
                    version=1,
                    agent=self.agent,
                    height=self.chain_height,
                    timestamp=int(time.time())
                )
                await peer.send_message(handshake.to_message())
            
            # Receive their handshake
            msg = await asyncio.wait_for(peer.recv_message(), timeout=10.0)
            if not msg or msg.msg_type != MessageType.HANDSHAKE:
                return False
            
            their_handshake = HandshakeMessage.from_message(msg)
            peer.agent = their_handshake.agent
            peer.height = their_handshake.height
            
            if not is_initiator:
                # Send our handshake in response
                handshake = HandshakeMessage(
                    version=1,
                    agent=self.agent,
                    height=self.chain_height,
                    timestamp=int(time.time())
                )
                await peer.send_message(handshake.to_message())
            
            # Register peer
            multiaddr = f"/ip4/{peer.address.split(':')[0]}/tcp/{self.port}"
            self.peer_directory.register_connection(multiaddr, peer.agent)
            
            return True
        
        except asyncio.TimeoutError:
            print(f"⚠️  Handshake timeout with {peer.address}")
            return False
        except Exception as e:
            print(f"⚠️  Handshake error with {peer.address}: {e}")
            return False
    
    async def _handle_peer_messages(self, peer: Peer) -> None:
        """Handle messages from a connected peer."""
        while self.running and peer.address in self.peers:
            try:
                msg = await peer.recv_message()
                if not msg:
                    break
                
                # Handle different message types
                if msg.msg_type == MessageType.PING:
                    await self._handle_ping(peer, msg)
                elif msg.msg_type == MessageType.PONG:
                    await self._handle_pong(peer, msg)
                elif msg.msg_type == MessageType.GET_PEERS:
                    await self._handle_get_peers(peer)
                elif msg.msg_type == MessageType.PEERS:
                    await self._handle_peers(peer, msg)
                elif msg.msg_type == MessageType.GET_BLOCKS:
                    await self._handle_get_blocks(peer, msg)
                elif msg.msg_type == MessageType.BLOCK:
                    await self._handle_block(peer, msg)
                elif msg.msg_type == MessageType.TX:
                    await self._handle_tx(peer, msg)
                elif msg.msg_type == MessageType.DISCONNECT:
                    break
            
            except Exception as e:
                print(f"❌ Error handling message from {peer.address}: {e}")
                continue
        
        # Clean up
        await self._disconnect_peer(peer, "Connection closed")
    
    async def _handle_ping(self, peer: Peer, msg: Message) -> None:
        """Handle ping message."""
        ping = PingMessage.from_message(msg)
        pong = PongMessage(nonce=ping.nonce)
        await peer.send_message(pong.to_message())
    
    async def _handle_pong(self, peer: Peer, msg: Message) -> None:
        """Handle pong message."""
        peer.last_ping = time.time()
    
    async def _handle_get_peers(self, peer: Peer) -> None:
        """Handle get peers request."""
        # Send list of known peers
        peer_list = [p.address for p in self.peers.values() if p.address != peer.address]
        peers_msg = PeersMessage(peers=peer_list[:50])  # Limit to 50
        await peer.send_message(peers_msg.to_message())
    
    async def _handle_peers(self, peer: Peer, msg: Message) -> None:
        """Handle peers message."""
        peers_msg = PeersMessage.from_message(msg)
        print(f"📋 Received {len(peers_msg.peers)} peers from {peer.address}")
        # Could connect to these peers
    

    async def _handle_get_blocks(self, peer: Peer, msg: Message) -> None:
        """Handle GET_BLOCKS request - send requested blocks to peer."""
        try:
            from irium.protocol import GetBlocksMessage
            get_blocks_msg = GetBlocksMessage.from_message(msg)

            print(f"  📤 Peer {peer.address} requested {get_blocks_msg.count} blocks starting from hash {get_blocks_msg.start_hash.hex()[:16]}...")

            # Send blocks the peer needs
            blocks_dir = os.path.expanduser("~/.irium/blocks")
            if os.path.exists(blocks_dir):
                # Find all blocks we have
                available_blocks = []
                for f in os.listdir(blocks_dir):
                    if f.startswith("block_") and f.endswith(".json"):
                        h = int(f.replace("block_", "").replace(".json", ""))
                        available_blocks.append(h)
                available_blocks.sort()
                
                # Send blocks higher than peer's current height
                start_height = peer.height + 1
                blocks_to_send = [h for h in available_blocks if h >= start_height][:get_blocks_msg.count]
                
                print(f"  📤 Sending {len(blocks_to_send)} blocks to {peer.address} (peer at {peer.height}, we have {available_blocks})")
                
                for height in blocks_to_send:
                    block_file = os.path.join(blocks_dir, f"block_{height}.json")
                    if os.path.exists(block_file):
                        with open(block_file, 'rb') as f:
                            block_data = f.read()

                        from irium.protocol import BlockMessage
                        block_msg = BlockMessage(block_data=block_data)
                        await peer.send_message(block_msg.to_message())
                        print(f"  📤 Sent block {height} to {peer.address}")
                    else:
                        print(f"  ⚠️  Block {height} not found on disk")
                        break
        except Exception as e:
            print(f"  ❌ Error handling GET_BLOCKS: {e}")
            import traceback
            traceback.print_exc()
    async def _handle_block(self, peer: Peer, msg: Message) -> None:
        """Handle block message."""
        if self.on_block:
            block_msg = BlockMessage.from_message(msg)
            
            # Try to extract block height to update peer height
            try:
                import json
                block_data = json.loads(block_msg.block_data.decode('utf-8'))
                block_height = block_data.get('height', 0)
                
                # Only update peer height if we successfully save the block
                # Don't auto-request more blocks here - causes infinite loop if block is invalid
                # Peer height will be updated by the callback if block is valid
            except:
                pass
            
            await self.on_block(peer, block_msg.block_data)
    
    async def _handle_tx(self, peer: Peer, msg: Message) -> None:
        """Handle transaction message."""
        if self.on_tx:
            tx_msg = TxMessage.from_message(msg)
            await self.on_tx(peer, tx_msg.tx_data)
    
    async def _connect_to_peers(self) -> None:
        """Background task to connect to peers from seedlist."""
        print("🔄 Peer connection task started")
        while self.running:
            try:
                if len(self.peers) < self.max_peers:
                    print(f"  Current peers: {len(self.peers)}/{self.max_peers}")
                    # Get seedlist
                    seedlist = list(self.seedlist_manager.merged_seedlist())
                    print(f"  Seedlist has {len(seedlist)} entries: {seedlist}")
                    if seedlist:
                        # Try random peer
                        multiaddr = random.choice(seedlist)
                        await self._connect_to_peer(multiaddr)
                
                await asyncio.sleep(30)  # Try every 30 seconds
            
            except Exception as e:
                print(f"❌ Error in peer connection task: {e}")
                await asyncio.sleep(30)
    
    async def _connect_to_peer(self, multiaddr: str) -> None:
        """Connect to a peer."""
        print(f"🔍 _connect_to_peer called with: {multiaddr}")
        """Connect to a peer."""
        try:
            # Parse multiaddr (simplified)
            # Format: /ip4/1.2.3.4/tcp/38291
            parts = multiaddr.strip('/').split('/')
            if len(parts) >= 4 and parts[0] == 'ip4' and parts[2] == 'tcp':
                host = parts[1]
                port = int(parts[3])
                
                address = f"{host}:{port}"
                
                if address in self.peers:
                    print(f"  Already connected to {address}")
                    return

                # Skip connecting to self
                print(f"  Checking if {host}:{port} is self")
                if host in ["127.0.0.1", "localhost"]:
                    print(f"  Skipping self: {host}")
                    return
                
                # Skip VPS IP on same port
                if host == "207.244.247.86" and port == self.port:
                    print(f"  Skipping self: {host}:{port} (VPS on my port)")
                    return
                
                # Skip if same IP and same port
                import socket
                try:
                    my_ip = socket.gethostbyname(socket.gethostname())
                    if host == my_ip and port == self.port:
                        print(f"  Skipping self: {host}:{port}")
                        return
                except:
                    pass
                
                print(f"  Passed self-check, will connect to {address}")
                print(f"📤 Connecting to {address}...")
                
                reader, writer = await asyncio.wait_for(
                    asyncio.open_connection(host, port),
                    timeout=10.0
                )
                
                peer = Peer(reader=reader, writer=writer, address=address)
                
                if await self._perform_handshake(peer, is_initiator=True):
                    self.peers[address] = peer
                    print(f"✅ Connected to peer: {address} ({peer.agent})")
                    

                # Request blocks if peer is ahead
                if peer.height > self.chain_height:
                    print(f"  Peer is ahead ({peer.height} vs {self.chain_height}), requesting blocks...")
                    # Request blocks from our height to their height
                    from irium.protocol import GetBlocksMessage
                    # Request blocks we're missing
                    # For now, use genesis hash as start (TODO: use actual last block hash)
                    genesis_hash = bytes.fromhex('cbdd1b9134adc846b3af5e2128f68214e1d8154912ff8da40685f47700000000')
                    count = min(500, peer.height - self.chain_height)  # Request up to 500 blocks
                    get_blocks = GetBlocksMessage(start_hash=genesis_hash, count=count)
                    await peer.send_message(get_blocks.to_message())
                    print(f"  📥 Requested blocks {self.chain_height + 1} to {peer.height}")

                    if self.on_peer_connected:
                        await self.on_peer_connected(peer)
                    
                    # Handle messages from this peer
                    task = asyncio.create_task(self._handle_peer_messages(peer))
                    self.message_tasks[address] = task
                else:
                    peer.close()
        
        except asyncio.TimeoutError:
            print(f"⚠️  Connection timeout: {multiaddr}")
        except Exception as e:
            print(f"❌ Failed to connect to {multiaddr}: {e}")
    
    async def _ping_peers(self) -> None:
        """Background task to ping peers."""
        while self.running:
            try:
                for peer in list(self.peers.values()):
                    nonce = random.randint(0, 2**64 - 1)
                    ping = PingMessage(nonce=nonce)
                    await peer.send_message(ping.to_message())
                
                await asyncio.sleep(120)  # Ping every 120 seconds
            
            except Exception as e:
                print(f"❌ Error in ping task: {e}")
                await asyncio.sleep(120)
    
    async def _cleanup_dead_peers(self) -> None:
        """Background task to remove dead peers."""
        while self.running:
            try:
                now = time.time()
                for address, peer in list(self.peers.items()):
                    # Remove peer if no pong in 3 minutes
                    if now - peer.last_ping > 300:
                        print(f"⚠️  Peer {address} timed out")
                        await self._disconnect_peer(peer, "Timeout")
                
                await asyncio.sleep(30)  # Check every 30 seconds
            
            except Exception as e:
                print(f"❌ Error in cleanup task: {e}")
                await asyncio.sleep(30)
    
    async def _disconnect_peer(self, peer: Peer, reason: str) -> None:
        """Disconnect a peer."""
        try:
            # Send disconnect message
            disconnect = DisconnectMessage(reason=reason)
            await peer.send_message(disconnect.to_message())
        except:
            pass
        
        # Remove from peers
        if peer.address in self.peers:
            del self.peers[peer.address]
            print(f"👋 Disconnected from {peer.address}: {reason}")
        
        peer.close()
    
    async def broadcast_block(self, block_data: bytes) -> None:
        """Broadcast a block to all peers."""
        block_msg = BlockMessage(block_data=block_data)
        msg = block_msg.to_message()
        
        for peer in list(self.peers.values()):
            try:
                await peer.send_message(msg)
            except Exception as e:
                print(f"❌ Failed to broadcast block to {peer.address}: {e}")
    
    async def broadcast_tx(self, tx_data: bytes) -> None:
        """Broadcast a transaction to all peers."""
        tx_msg = TxMessage(tx_data=tx_data)
        msg = tx_msg.to_message()
        
        for peer in list(self.peers.values()):
            try:
                await peer.send_message(msg)
            except Exception as e:
                print(f"❌ Failed to broadcast tx to {peer.address}: {e}")
    
    def get_peer_count(self) -> int:
        """Get number of connected peers."""
        return len(self.peers)
    
    def get_peers_info(self) -> list:
        """Get information about connected peers."""
        return [
            {
                'address': peer.address,
                'agent': peer.agent,
                'height': peer.height,
                'connected': int(time.time() - peer.connected_at)
            }
            for peer in self.peers.values()
        ]
