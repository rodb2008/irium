#!/usr/bin/env python3
"""Broadcast Irium transactions to the network."""

import sys
import os
import asyncio
sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from irium.tx import Transaction
from irium.network import PeerDirectory

async def broadcast_transaction(tx_hex):
    """Broadcast a transaction to the Irium network."""
    try:
        # Deserialize transaction
        tx_bytes = bytes.fromhex(tx_hex)
        
        print(f"📡 Broadcasting transaction to network...")
        print(f"Transaction size: {len(tx_bytes)} bytes")
        print(f"Transaction hex: {tx_hex[:64]}...")
        print()
        
        # Load peer directory
        peer_dir = PeerDirectory()
        
        # TODO: Implement actual peer-to-peer broadcasting
        # For now, we'll create a placeholder
        print("⚠️ Note: Peer-to-peer broadcasting not yet fully implemented")
        print("Transaction is ready and serialized")
        print("Needs libp2p/gossip protocol integration")
        print()
        print("✅ Transaction prepared for broadcast")
        print(f"✅ Transaction ID: {Transaction.txid(tx_bytes).hex() if hasattr(Transaction, 'txid') else 'N/A'}")
        
        return True
    except Exception as e:
        print(f"❌ Error broadcasting transaction: {e}")
        return False

def main():
    if len(sys.argv) < 2:
        print("Irium Transaction Broadcaster")
        print("Usage: python3 broadcast-transaction.py <transaction_hex>")
        sys.exit(1)
    
    tx_hex = sys.argv[1]
    asyncio.run(broadcast_transaction(tx_hex))

if __name__ == "__main__":
    main()
