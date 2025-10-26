#!/usr/bin/env python3
"""Manage Irium blockchain state and transactions."""

import sys
import os
import json
sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from irium.chain import ChainState, ChainParams, Block
from irium.tx import Transaction
from irium.block import BlockHeader

BLOCKCHAIN_DIR = os.path.expanduser("~/.irium")
MEMPOOL_FILE = os.path.join(BLOCKCHAIN_DIR, "mempool", "pending.json")

class BlockchainManager:
    def __init__(self):
        self.blockchain_dir = BLOCKCHAIN_DIR
        self.mempool_file = MEMPOOL_FILE
        
    def add_to_mempool(self, tx_hex):
        """Add transaction to mempool."""
        try:
            # Load existing mempool
            mempool = []
            if os.path.exists(self.mempool_file):
                with open(self.mempool_file, 'r') as f:
                    mempool = json.load(f)
            
            # Add new transaction
            tx_data = {
                'hex': tx_hex,
                'size': len(tx_hex) // 2,
                'timestamp': int(os.path.getmtime(self.mempool_file)) if os.path.exists(self.mempool_file) else 0
            }
            mempool.append(tx_data)
            
            # Save mempool
            os.makedirs(os.path.dirname(self.mempool_file), exist_ok=True)
            with open(self.mempool_file, 'w') as f:
                json.dump(mempool, f, indent=2)
            
            print(f"✅ Transaction added to mempool")
            print(f"Mempool size: {len(mempool)} transactions")
            return True
            
        except Exception as e:
            print(f"❌ Error adding to mempool: {e}")
            return False
    
    def get_mempool(self):
        """Get all pending transactions."""
        try:
            if os.path.exists(self.mempool_file):
                with open(self.mempool_file, 'r') as f:
                    return json.load(f)
            return []
        except Exception as e:
            print(f"Error reading mempool: {e}")
            return []
    
    def clear_mempool(self):
        """Clear mempool (after mining)."""
        if os.path.exists(self.mempool_file):
            os.remove(self.mempool_file)
            print("✅ Mempool cleared")

def main():
    if len(sys.argv) < 2:
        print("Irium Blockchain Manager")
        print("Usage:")
        print("  python3 blockchain-manager.py add-tx <tx_hex>")
        print("  python3 blockchain-manager.py show-mempool")
        print("  python3 blockchain-manager.py clear-mempool")
        return
    
    command = sys.argv[1]
    manager = BlockchainManager()
    
    if command == "add-tx":
        if len(sys.argv) < 3:
            print("Error: Transaction hex required")
            return
        tx_hex = sys.argv[2]
        manager.add_to_mempool(tx_hex)
    
    elif command == "show-mempool":
        mempool = manager.get_mempool()
        print(f"Mempool: {len(mempool)} transactions")
        for i, tx in enumerate(mempool):
            print(f"  {i+1}. Size: {tx['size']} bytes, Hex: {tx['hex'][:32]}...")
    
    elif command == "clear-mempool":
        manager.clear_mempool()
    
    else:
        print(f"Unknown command: {command}")

if __name__ == "__main__":
    main()
