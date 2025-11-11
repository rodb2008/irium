"""P2P protocol messages for Irium blockchain."""

from __future__ import annotations
from dataclasses import dataclass
from typing import List, Optional
import struct
import json
from enum import IntEnum

# Protocol version
PROTOCOL_VERSION = 1
MAX_MESSAGE_SIZE = 32 * 1024 * 1024  # 32 MB max message size
MAX_BLOCK_SIZE = 4 * 1024 * 1024   # 4 MB max block size

# Message types
class MessageType(IntEnum):
    """P2P message types."""
    HANDSHAKE = 1
    PING = 2
    PONG = 3
    GET_PEERS = 4
    PEERS = 5
    GET_BLOCKS = 6
    BLOCK = 7
    GET_HEADERS = 8
    HEADERS = 9
    TX = 10
    MEMPOOL = 11
    DISCONNECT = 99


@dataclass
class Message:
    """Base P2P message."""
    msg_type: MessageType
    payload: bytes
    
    def serialize(self) -> bytes:
        """Serialize message to bytes."""
        # Format: [version:1][type:1][length:4][payload]
        header = struct.pack('!BBI', PROTOCOL_VERSION, self.msg_type, len(self.payload))
        return header + self.payload
    
    @classmethod
    def deserialize(cls, data: bytes) -> Message:
        """Deserialize message from bytes."""
        if len(data) < 6:
            raise ValueError("Message too short")
        
        version, msg_type, length = struct.unpack('!BBI', data[:6])
        
        if version != PROTOCOL_VERSION:
            raise ValueError(f"Unsupported protocol version: {version}")
        
        if len(data) < 6 + length:
            raise ValueError("Incomplete message")
        
        payload = data[6:6+length]
        return cls(msg_type=MessageType(msg_type), payload=payload)


@dataclass
class HandshakeMessage:
    """Handshake message to establish connection."""
    version: int
    agent: str
    height: int
    timestamp: int
    port: int = 0  # Peer's listening port
    checkpoint_height: Optional[int] = None
    checkpoint_hash: Optional[str] = None
    
    def to_message(self) -> Message:
        """Convert to generic Message."""
        payload_dict = {
            'version': self.version,
            'agent': self.agent,
            'height': self.height,
            'timestamp': self.timestamp,
            'port': self.port,
        }
        if self.checkpoint_height is not None:
            payload_dict['checkpoint_height'] = self.checkpoint_height
        if self.checkpoint_hash is not None:
            payload_dict['checkpoint_hash'] = self.checkpoint_hash
        payload = json.dumps(payload_dict).encode('utf-8')
        return Message(MessageType.HANDSHAKE, payload)
    
    @classmethod
    def from_message(cls, msg: Message) -> HandshakeMessage:
        """Parse from generic Message."""
        data = json.loads(msg.payload.decode('utf-8'))
        return cls(
            version=data['version'],
            agent=data['agent'],
            height=data['height'],
            timestamp=data['timestamp'],
            port=data.get('port', 0),
            checkpoint_height=data.get('checkpoint_height'),
            checkpoint_hash=data.get('checkpoint_hash')
        )

@dataclass
class PingMessage:
    """Ping message for keepalive."""
    nonce: int
    
    def to_message(self) -> Message:
        """Convert to generic Message."""
        payload = struct.pack('!Q', self.nonce)
        return Message(MessageType.PING, payload)
    
    @classmethod
    def from_message(cls, msg: Message) -> PingMessage:
        """Parse from generic Message."""
        nonce, = struct.unpack('!Q', msg.payload)
        return cls(nonce=nonce)


@dataclass
class PongMessage:
    """Pong response to ping."""
    nonce: int
    
    def to_message(self) -> Message:
        """Convert to generic Message."""
        payload = struct.pack('!Q', self.nonce)
        return Message(MessageType.PONG, payload)
    
    @classmethod
    def from_message(cls, msg: Message) -> PongMessage:
        """Parse from generic Message."""
        nonce, = struct.unpack('!Q', msg.payload)
        return cls(nonce=nonce)


@dataclass
class GetPeersMessage:
    """Request peer list."""
    
    def to_message(self) -> Message:
        """Convert to generic Message."""
        return Message(MessageType.GET_PEERS, b'')
    
    @classmethod
    def from_message(cls, msg: Message) -> GetPeersMessage:
        """Parse from generic Message."""
        return cls()


@dataclass
class PeersMessage:
    """Response with peer list."""
    peers: List[str]  # List of multiaddrs

    def to_message(self) -> Message:
        """Convert to generic Message."""
        payload = json.dumps({'peers': self.peers}).encode('utf-8')
        return Message(MessageType.PEERS, payload)

    @classmethod
    def from_message(cls, msg: Message) -> PeersMessage:
        """Parse from generic Message."""
        data = json.loads(msg.payload.decode('utf-8'))
        return cls(peers=data['peers'])

@dataclass
class GetBlocksMessage:
    """Request blocks starting from a hash."""
    start_hash: bytes
    count: int
    
    def to_message(self) -> Message:
        """Convert to generic Message."""
        payload = struct.pack('!I', self.count) + self.start_hash
        return Message(MessageType.GET_BLOCKS, payload)
    
    @classmethod
    def from_message(cls, msg: Message) -> GetBlocksMessage:
        """Parse from generic Message."""
        count, = struct.unpack('!I', msg.payload[:4])
        start_hash = msg.payload[4:]
        return cls(start_hash=start_hash, count=count)


@dataclass
class BlockMessage:
    """Block data message."""
    block_data: bytes  # Serialized block
    
    def to_message(self) -> Message:
        """Convert to generic Message."""
        return Message(MessageType.BLOCK, self.block_data)
    
    @classmethod
    def from_message(cls, msg: Message) -> BlockMessage:
        """Parse from generic Message."""
        return cls(block_data=msg.payload)


@dataclass
class TxMessage:
    """Transaction message."""
    tx_data: bytes  # Serialized transaction
    
    def to_message(self) -> Message:
        """Convert to generic Message."""
        return Message(MessageType.TX, self.tx_data)
    
    @classmethod
    def from_message(cls, msg: Message) -> TxMessage:
        """Parse from generic Message."""
        return cls(tx_data=msg.payload)




@dataclass
class GetHeadersMessage:
    """Request block headers (for SPV clients)."""
    start_hash: bytes
    count: int

    def to_message(self) -> Message:
        """Convert to generic Message."""
        payload = struct.pack('!I', self.count) + self.start_hash
        return Message(MessageType.GET_HEADERS, payload)

    @classmethod
    def from_message(cls, msg: Message) -> GetHeadersMessage:
        """Parse from generic Message."""
        count, = struct.unpack('!I', msg.payload[:4])
        start_hash = msg.payload[4:]
        return cls(start_hash=start_hash, count=count)


@dataclass
class HeadersMessage:
    """Response with block headers."""
    headers: bytes  # Serialized block headers

    def to_message(self) -> Message:
        """Convert to generic Message."""
        return Message(MessageType.HEADERS, self.headers)

    @classmethod
    def from_message(cls, msg: Message) -> HeadersMessage:
        """Parse from generic Message."""
        return cls(headers=msg.payload)


@dataclass
class MempoolMessage:
    """Mempool transaction list."""
    tx_hashes: List[bytes]  # List of transaction hashes in mempool

    def to_message(self) -> Message:
        """Convert to generic Message."""
        payload = json.dumps({
            'tx_hashes': [h.hex() for h in self.tx_hashes]
        }).encode('utf-8')
        return Message(MessageType.MEMPOOL, payload)

    @classmethod
    def from_message(cls, msg: Message) -> MempoolMessage:
        """Parse from generic Message."""
        data = json.loads(msg.payload.decode('utf-8'))
        tx_hashes = [bytes.fromhex(h) for h in data['tx_hashes']]
        return cls(tx_hashes=tx_hashes)


@dataclass
class DisconnectMessage:
    """Disconnect notification."""
    reason: str
    
    def to_message(self) -> Message:
        """Convert to generic Message."""
        payload = self.reason.encode('utf-8')
        return Message(MessageType.DISCONNECT, payload)
    
    @classmethod
    def from_message(cls, msg: Message) -> DisconnectMessage:
        """Parse from generic Message."""
        reason = msg.payload.decode('utf-8')
        return cls(reason=reason)


# Uptime proof messages
@dataclass
class UptimeChallenge:
    """Challenge for uptime proof."""
    challenge: bytes
    
    def to_message(self) -> Message:
        """Convert to generic Message."""
        return Message(MessageType.PING, self.challenge)  # Reuse PING
    
    @classmethod
    def from_message(cls, msg: Message) -> UptimeChallenge:
        """Parse from generic Message."""
        return cls(challenge=msg.payload)


@dataclass
class UptimeResponse:
    """Response to uptime challenge."""
    response: bytes
    
    def to_message(self) -> Message:
        """Convert to generic Message."""
        return Message(MessageType.PONG, self.response)  # Reuse PONG
    
    @classmethod
    def from_message(cls, msg: Message) -> UptimeResponse:
        """Parse from generic Message."""
        return cls(response=msg.payload)
