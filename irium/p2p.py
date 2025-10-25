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
    BlockMessage, TxMessage, DisconnectMessage, GetBlocksMessage
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
            # Timeout is non-critical, cleanup task will handle
            pass
        except Exception as e:
            # For broadcasts and important messages, we need to know if it failed
            # So re-raise, but don't print here (caller will handle)
            raise
    
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

        except asyncio.IncompleteReadError as e:
            return None
        except Exception as e:
            return None

    def close(self) -> None:
        """Close connection."""
        try:
            self.writer.close()
        except Exception:
            pass


class P2PNode:
    """P2P network node for Irium."""
    
    def __init__(
        self,
        port: int = 38291,
        max_peers: int = 8,
        agent: str = "irium-node/1.0",
        chain_height: int = 0,
        ping_interval: int = None  # Auto-detect: 60s for public, 30s for NAT
    ):
        self.port = port
        self.max_peers = max_peers
        self.agent = agent
        self.chain_height = chain_height
        
        # Auto-detect ping interval based on environment
        if ping_interval is None:
            import os
            # Check if BOOTSTRAP_NODE env var is set (VPS)
            is_bootstrap = os.getenv('BOOTSTRAP_NODE', 'false').lower() == 'true'
            self.ping_interval = 60 if is_bootstrap else 30
        else:
            self.ping_interval = ping_interval
        
        self.peers: Dict[str, Peer] = {}
        self.message_tasks: Dict[str, asyncio.Task] = {}
        self.server: Optional[asyncio.Server] = None
        self.running = False
        
        # Callbacks for handling messages
        self.on_block: Optional[Callable] = None
        self.on_tx: Optional[Callable] = None
        self.on_peer_connected: Optional[Callable] = None
        
        # Peer management
        self.peer_directory = PeerDirectory()
        self.seedlist_manager = SeedlistManager()
        # Track relayed blocks to prevent spam: {block_height: {peer_addresses}}
        # Detect our public IP to prevent self-connections
        self.public_ip = self._get_public_ip()
        self.relayed_blocks: Dict[int, Set[str]] = {}

    def _get_peer_ip(self, address: str) -> str:
        """Extract IP from address string (IP:PORT)."""
        return address.split(':')[0]

    def _get_public_ip(self) -> Optional[str]:
        """Get our public IP address."""
        try:
            import urllib.request
            response = urllib.request.urlopen('https://api.ipify.org', timeout=5)
            return response.read().decode('utf-8').strip()
        except Exception:
            return None


    def _is_self_peer(self, addr: str) -> bool:
        """Check if this peer is ourselves."""
        if ':' in addr:
            ip, port = addr.split(':')
            
            # Check port first (must match our listening port)
            if port != str(self.port):
                return False
            
            # Check if it's localhost
            if ip in ["127.0.0.1", "localhost"]:
                return True
            
            # Check if it's our public IP
            if self.public_ip and ip == self.public_ip:
                return True
            
            # Check if it's our local IP
            import socket
            try:
                s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
                s.connect(("8.8.8.8", 80))
                local_ip = s.getsockname()[0]
                s.close()
                return ip == local_ip
            except Exception:
                pass
        
        return False
    def _is_peer_connected(self, addr: str) -> bool:
        """Check if we're already connected to this peer."""
        # Handle multiaddr format /ip4/IP/tcp/PORT
        if addr.startswith('/ip4/'):
            parts = addr.strip('/').split('/')
            if len(parts) >= 4:
                ip = parts[1]
                port = parts[3]
                addr = f"{ip}:{port}"
        
        # Check if already connected
        return addr in self.peers

    
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
        asyncio.create_task(self._discover_peers())
    
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
            return  # ✅ Added missing return

        # ✅ Check if we already have connection to this peer (incoming OR outgoing)
        peer_ip = addr[0]
        
        # If we already have ANY connection to this IP, reject silently
        # This prevents bidirectional connection conflicts
        if self._is_peer_connected(peer_ip):
            # Check if existing connection is still alive (responded to ping recently)
            import time
            existing_alive = False
            for addr_key, p in self.peers.items():
                if self._get_peer_ip(addr_key) == peer_ip:
                    # Connection is alive if pinged within last 90 seconds
                    if time.time() - p.last_ping < 180:
                        existing_alive = True
                        break
            
            if existing_alive:
                # Silently reject - already have alive connection to this IP
                writer.close()
                await writer.wait_closed()
                return
            # Else: let new connection proceed, old one is dead/dying
        
        peer = Peer(reader=reader, writer=writer, address=address)
        
        try:
            # Perform handshake
            if await self._perform_handshake(peer, is_initiator=False):
                # ✅ FIX #13: Use peer.address (corrected in handshake) not original address
                # Store original address before handshake potentially changes it
                original_address = address
                
                # ✅ CRITICAL FIX: Check if exact IP:PORT is already connected
                # Allow same IP with different ports (e.g., node:38291 + miner:38292)
                if peer.address in self.peers:
                    # Exact duplicate IP:PORT - reject
                    print(f"⚠️  Already connected to {peer.address}, rejecting duplicate")
                    peer.close()
                    return
                
                self.peers[peer.address] = peer

                # Start message handler FIRST before sending any messages
                task = asyncio.create_task(self._handle_peer_messages(peer))
                self.message_tasks[peer.address] = task
                
                
                print(f"✅ Peer connected: {address} ({peer.agent}, height: {peer.height})")
                

                # PUSH/PULL based on who is ahead
                if peer.height < self.chain_height:
                    # We're ahead - PUSH our blocks to them
                    print(f"  📊 We are ahead ({self.chain_height} vs {peer.height}) - pushing blocks...")
                    blocks_dir = os.path.expanduser("~/.irium/blocks")
                    for h in range(peer.height + 1, self.chain_height + 1):
                        block_file = os.path.join(blocks_dir, f"block_{h}.json")
                        if os.path.exists(block_file):
                            with open(block_file, 'rb') as bf:
                                block_data = bf.read()
                            from irium.protocol import BlockMessage
                            block_msg = BlockMessage(block_data=block_data)
                            await peer.send_message(block_msg.to_message())
                            print(f"    📤 Pushed block {h} to {peer.address}")
                elif peer.height > self.chain_height:
                    # Peer ahead - wait for them to PUSH to us
                    print(f"  📊 Peer ahead by {peer.height - self.chain_height} blocks - waiting for broadcast")
                
                if self.on_peer_connected:
                    await self.on_peer_connected(peer)
            else:
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
                    timestamp=int(time.time()),
                    port=self.port
                )
                await peer.send_message(handshake.to_message())
            
            # Receive their handshake
            msg = await asyncio.wait_for(peer.recv_message(), timeout=30.0)
            if not msg or msg.msg_type != MessageType.HANDSHAKE:
                return False
            
            their_handshake = HandshakeMessage.from_message(msg)
            peer.agent = their_handshake.agent
            peer.height = their_handshake.height
            
            # Version check
            peer_version = their_handshake.node_version if hasattr(their_handshake, 'node_version') else "unknown"
            if peer_version != "unknown" and peer_version < "1.1.8":
                print(f"⚠️  WARNING: Peer {peer.address} is running outdated version {peer_version}")
                print(f"   They should update to v1.1.8 to prevent forks!")
            
            if not is_initiator:
                # Send our handshake in response
                handshake = HandshakeMessage(
                    version=1,
                    agent=self.agent,
                    height=self.chain_height,
                    timestamp=int(time.time()),
                    port=self.port
                )
                await peer.send_message(handshake.to_message())
            
            # Register peer with their announced listening port
            peer_ip = peer.address.split(':')[0]
            peer_port = their_handshake.port if their_handshake.port > 0 else self.port
            
            # Update peer.address to use announced port instead of ephemeral port
            corrected_address = f"{peer_ip}:{peer_port}"
            original_address = peer.address
            # ✅ FIX #17: Just update peer.address - let CALLER manage self.peers
            if corrected_address != peer.address:
                peer.address = corrected_address
            
            multiaddr = f"/ip4/{peer_ip}/tcp/{peer_port}"
            try:
                self.peer_directory.register_connection(multiaddr, peer.agent)
            except (OSError, PermissionError) as e:
                pass  # Silent fail
            
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
                msg = await asyncio.wait_for(peer.recv_message(), timeout=180.0)
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
                # Silently continue - peer will be cleaned up if needed
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
        peer_list = []
        for p in self.peers.values():
            if p.address != peer.address:
                # Convert IP:PORT to multiaddr format /ip4/IP/tcp/PORT
                if ':' in p.address:
                    ip, port = p.address.split(':')
                    peer_list.append(f"/ip4/{ip}/tcp/{port}")
                else:
                    peer_list.append(p.address)
        
        peers_msg = PeersMessage(peers=peer_list[:50])  # Limit to 50
        print(f"📤 Sending {len(peer_list)} peers to {peer.address}")
        await peer.send_message(peers_msg.to_message())
    
    async def _handle_peers(self, peer: Peer, msg: Message) -> None:
        """Handle peers message."""
        peers_msg = PeersMessage.from_message(msg)
        print(f"📋 Received {len(peers_msg.peers)} peers from {peer.address}")
        
        # Connect to discovered peers
        for peer_addr in peers_msg.peers:
            try:
                # Skip if we already have this peer
                if self._is_peer_connected(peer_addr):
                    continue
                
                # Skip if we're at max peers
                if len(self.peers) >= self.max_peers:
                    continue
                
                # Connect to the new peer
                print(f"🔗 Connecting to discovered peer: {peer_addr}")
                await self._connect_to_peer(peer_addr)
                
            except Exception as e:
                # Show connection failures for debugging
                print(f"  ❌ Failed to connect to {peer_addr}: {e}")
    

    async def _discover_peers(self) -> None:
        """Background task to discover new peers."""
        while self.running:
            try:
                # Request peers from connected nodes every 2 minutes
                if len(self.peers) > 0:
                    for peer in list(self.peers.values()):
                        try:
                            # Skip self-connections
                            if self._is_self_peer(peer.address):
                                continue
                            
                            get_peers_msg = GetPeersMessage()
                            await peer.send_message(get_peers_msg.to_message())
                            print(f"🔍 Requesting peers from {peer.address}")
                        except Exception as e:
                            # Silently continue if peer is dead
                            pass
                
                await asyncio.sleep(120)  # Request peers every 2 minutes
                
            except Exception as e:
                # Silently continue
                await asyncio.sleep(30)

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

                # Save block via callback
                await self.on_block(peer, block_msg.block_data)

                # ✅ FIX #2: Update peer height after successfully saving block
                if block_height > peer.height:
                    peer.height = block_height
                # ✅ FIX #1: Re-broadcast block to all other peers (PUSH model)
                # But only if we haven't already relayed this block to them
                if block_height not in self.relayed_blocks:
                    self.relayed_blocks[block_height] = set()
                
                broadcast_count = 0
                for other_peer in list(self.peers.values()):
                    if other_peer.address == peer.address:
                        continue  # Don't send back to sender
                    
                    # Check if already relayed to this peer
                    if other_peer.address in self.relayed_blocks[block_height]:
                        continue  # Already relayed to this peer
                    
                    try:
                        await other_peer.send_message(msg)
                        self.relayed_blocks[block_height].add(other_peer.address)
                        broadcast_count += 1
                    except Exception as e:
                        print(f"  ⚠️  Failed to relay block to {other_peer.address}: {e}")

                # Only show relay message if actually relayed
                if broadcast_count > 0:
                    print(f"  📡 Block {block_height} relayed to {broadcast_count} peers (no other peers connected)")
                
            except Exception as e:
                print(f"  ⚠️  Error processing block: {e}")

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
                    print(f"  👥 Peers: {len(self.peers)}/{self.max_peers}")
                    # Get seedlist
                    seedlist = list(self.seedlist_manager.merged_seedlist())
                    print(f"  📋 Seedlist: {len(seedlist)} nodes")
                    if seedlist:
                        # Try random peer
                        multiaddr = random.choice(seedlist)
                        await self._connect_to_peer(multiaddr)
                
                await asyncio.sleep(30)  # Try every 30 seconds
            
            except Exception as e:
                # Silently continue trying
                await asyncio.sleep(30)
    
    async def _connect_to_peer(self, multiaddr: str) -> None:
        """Connect to a peer."""
        try:
            # Parse multiaddr (simplified)
            # Format: /ip4/1.2.3.4/tcp/38291
            parts = multiaddr.strip('/').split('/')
            if len(parts) >= 4 and parts[0] == 'ip4' and parts[2] == 'tcp':
                host = parts[1]
                port = int(parts[3])
                
                address = f"{host}:{port}"

                # Skip self-connections
                if self._is_self_peer(address):
                    print(f"  ⏭️  Skipping self-connection: {address}")
                    return
                
                if address in self.peers:
                    print(f"  ✓ Already connected: {address}")
                    return

                # Skip connecting to self
                # 🔍 Self-check: {host}:{port}
                if host in ["127.0.0.1", "localhost"]:
                    print(f"  Skipping self: {host}")
                    return
                
                # Skip outgoing connections to same IP:port as this node (dynamic check only)
                # No hardcoded IPs - let each node determine its own identity
                
                
                # Skip OUTGOING connections to self
                import socket
                try:
                    my_ip = socket.gethostbyname(socket.gethostname())
                    if host == my_ip and port == self.port:
                        print(f"  ⏭️  Skipping outgoing self-connection: {host}:{port}")
                        return
                except Exception:
                    pass
                
                print(f"📤 Connecting to {address}...")
                
                reader, writer = await asyncio.wait_for(
                    asyncio.open_connection(host, port),
                    timeout=30.0
                )
                
                peer = Peer(reader=reader, writer=writer, address=address)
                
                if await self._perform_handshake(peer, is_initiator=True):
                    # ✅ CRITICAL FIX: Check if exact IP:PORT is already connected
                    # Allow same IP with different ports (e.g., node:38291 + miner:38292)
                    if peer.address in self.peers:
                        # Exact duplicate IP:PORT - already connected
                        print(f"  ✓ Already connected: {peer.address}")
                        peer.close()
                        return
                    
                    # ✅ Use peer.address (corrected in handshake) for consistency
                    self.peers[peer.address] = peer

                    # Register outgoing peer to runtime seedlist
                    try:
                        self.peer_directory.register_connection(multiaddr, peer.agent)
                    except (OSError, PermissionError) as e:
                        print(f"⚠️  Could not register outgoing peer: {e}")
                task = asyncio.create_task(self._handle_peer_messages(peer))
                self.message_tasks[peer.address] = task
                if self.on_peer_connected:
                    await self.on_peer_connected(peer)

                # PUSH our blocks if peer is behind
                if peer.height < self.chain_height:
                    print(f"  We are ahead ({self.chain_height} vs {peer.height}), pushing our blocks...")
                    blocks_dir = os.path.expanduser("~/.irium/blocks")
                    for h in range(peer.height + 1, self.chain_height + 1):
                        block_file = os.path.join(blocks_dir, f"block_{h}.json")
                        if os.path.exists(block_file):
                            with open(block_file, 'rb') as bf:
                                block_data = bf.read()
                            from irium.protocol import BlockMessage
                            block_msg = BlockMessage(block_data=block_data)
                            await peer.send_message(block_msg.to_message())
                            print(f"  📤 Pushed block {h} to {peer.address}")
                
                # Request blocks if peer is ahead
                elif peer.height > self.chain_height:
                    print(f"  Peer is ahead ({peer.height} vs {self.chain_height}), requesting blocks...")
                    from irium.protocol import GetBlocksMessage
                    genesis_hash = bytes.fromhex('cbdd1b9134adc846b3af5e2128f68214e1d8154912ff8da40685f47700000000')
                    count = min(500, peer.height - self.chain_height)
                    get_blocks = GetBlocksMessage(start_hash=genesis_hash, count=count)
                    await peer.send_message(get_blocks.to_message())
                    print(f"  📥 Requested blocks {self.chain_height + 1} to {peer.height}")

        
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
                
                await asyncio.sleep(self.ping_interval)  # Adaptive: 60s (public) or 30s (NAT)
            
            except Exception as e:
                # Silently continue
                await asyncio.sleep(120)
    
    async def _cleanup_dead_peers(self) -> None:
        """Background task to remove dead peers."""
        while self.running:
            try:
                now = time.time()
                for address, peer in list(self.peers.items()):
                    # Remove peer if no pong in 3 minutes
                    if now - peer.last_ping > 180:
                        # Silently remove timed out peer
                        await self._disconnect_peer(peer, "Timeout")
                
                await asyncio.sleep(30)  # Check every 30 seconds

                # MEMORY LEAK FIX: Cleanup old relayed_blocks (keep last 100 blocks only)
                if self.relayed_blocks:
                    current_height = self.chain_height
                    old_heights = [h for h in self.relayed_blocks.keys() if h < current_height - 100]
                    for old_height in old_heights:
                        del self.relayed_blocks[old_height]
            
            except Exception as e:
                # Silently continue
                await asyncio.sleep(10)
    
    async def _disconnect_peer(self, peer: Peer, reason: str) -> None:
        """Disconnect a peer."""
        try:
            # Send disconnect message
            disconnect = DisconnectMessage(reason=reason)
            await peer.send_message(disconnect.to_message())
        except Exception:
            pass
        
        # Remove from peers
        if peer.address in self.peers:
            del self.peers[peer.address]
        
        # ✅ FIX #8: Cleanup message tasks with proper cancellation
        if peer.address in self.message_tasks:
            task = self.message_tasks[peer.address]
            if not task.done():
                task.cancel()
                try:
                    await task
                except asyncio.CancelledError:
                    pass
            del self.message_tasks[peer.address]
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
