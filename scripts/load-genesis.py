#!/usr/bin/env python3
"""Load and create Irium genesis block."""

import sys
import os
import json
sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from irium.block import Block, BlockHeader
from irium.tx import Transaction, TxInput, TxOutput
from irium.pow import Target

def load_genesis_block():
    """Load genesis block from config."""
    genesis_file = os.path.join(os.path.dirname(__file__), '..', 'configs', 'genesis.json')
    
    with open(genesis_file, 'r') as f:
        genesis_data = json.load(f)
    
    print("📋 Genesis Configuration:")
    print(f"  Network: {genesis_data['network']}")
    print(f"  Timestamp: {genesis_data['timestamp']}")
    print(f"  Bits: {genesis_data['bits']}")
    print(f"  Nonce: {genesis_data['nonce']}")
    print()
    
    # Create genesis coinbase transaction
    allocations = genesis_data.get('allocations', [])
    
    # Create outputs from allocations
    outputs = []
    total_vesting = 0
    for alloc in allocations:
        script_pubkey = bytes.fromhex(alloc['script_pubkey'])
        amount = alloc['amount_sats']
        outputs.append(TxOutput(value=amount, script_pubkey=script_pubkey))
        total_vesting += amount
        print(f"  Allocation: {alloc['label']}")
        print(f"    Amount: {amount / 100000000} IRM")
    
    print()
    
    # Create coinbase input
    coinbase_input = TxInput(
        prev_txid=bytes(32),
        prev_index=0xFFFFFFFF,
        script_sig=b"Irium Genesis Block - SHA256d PoW"
    )
    
    # Create coinbase transaction
    coinbase_tx = Transaction(
        version=1,
        inputs=[coinbase_input],
        outputs=outputs
    )
    
    # Calculate merkle root
    merkle_root = coinbase_tx.txid()[::-1]  # Reverse for merkle root
    
    # Create genesis block header with CORRECT parameter names
    header = BlockHeader(
        version=1,
        prev_hash=bytes(32),  # All zeros for genesis
        merkle_root=merkle_root,
        time=genesis_data['timestamp'],
        bits=int(genesis_data['bits'], 16),
        nonce=genesis_data['nonce']
    )
    
    # Create genesis block
    genesis_block = Block(
        header=header,
        transactions=[coinbase_tx]
    )
    
    print("✅ Genesis block created:")
    print(f"  Block hash: {header.hash().hex()}")
    print(f"  Merkle root: {merkle_root.hex()}")
    print(f"  Transactions: {len(genesis_block.transactions)}")
    print(f"  Total vesting: {total_vesting / 100000000} IRM")
    print()
    
    return genesis_block

if __name__ == "__main__":
    genesis = load_genesis_block()
    print("Genesis block loaded successfully!")
