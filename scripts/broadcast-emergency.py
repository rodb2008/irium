#!/usr/bin/env python3
"""Emergency network-wide broadcast message."""
import sys
import os
sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

import asyncio
from irium.protocol import Message, MessageType

async def broadcast_emergency(message_text: str):
    """Send emergency message to all connected peers."""
    from irium.p2p import P2PNode
    
    # Create emergency message type
    emergency_msg = Message(
        msg_type=MessageType.DISCONNECT,  # Reuse for now
        payload={"type": "EMERGENCY", "message": message_text, "version_required": "1.1.8"}
    )
    
    print(f"📢 Broadcasting emergency message to network:")
    print(f"   {message_text}")
    
    # TODO: Connect to peers and broadcast
    # For now, this is a placeholder

if __name__ == "__main__":
    message = sys.argv[1] if len(sys.argv) > 1 else "URGENT: Update to v1.1.8 required!"
    asyncio.run(broadcast_emergency(message))
