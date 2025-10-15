#!/usr/bin/env python3
"""Initialize Irium blockchain with genesis."""

import sys
import os
sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from irium.chain import ChainParams, ChainState
from irium.pow import Target
from irium.block import Block, BlockHeader
from irium.tx import Transaction, TxInput, TxOutput
import json

def load_genesis_block():
    """Load genesis block from config."""
    genesis_file = os.path.join(os.path.dirname(__file__), '..', 'configs', 'genesis.json')
    
    with open(genesis_file, 'r') as f:
        genesis_data = json.load(f)
    
    allocations = genesis_data.get('allocations', [])
    
    outputs = []
    for alloc in allocations:
        script_pubkey = bytes.fromhex(alloc['script_pubkey'])
        amount = alloc['amount_sats']
        outputs.append(TxOutput(value=amount, script_pubkey=script_pubkey))
    
    coinbase_input = TxInput(
        prev_txid=bytes(32),
        prev_index=0xFFFFFFFF,
        script_sig=b"Irium Genesis Block"
    )
    
    coinbase_tx = Transaction(
        version=1,
        inputs=[coinbase_input],
        outputs=outputs
    )
    
    # Create temporary block to calculate merkle root
    temp_block = Block(
        header=BlockHeader(
            version=1,
            prev_hash=bytes(32),
            merkle_root=bytes(32),
            time=genesis_data['timestamp'],
            bits=int(genesis_data['bits'], 16),
            nonce=genesis_data['nonce']
        ),
        transactions=[coinbase_tx]
    )
    
    # Calculate merkle root and REVERSE it (as validation expects)
    correct_merkle_root = temp_block.merkle_root()[::-1]
    
    # Create final header with correct reversed merkle root
    header = BlockHeader(
        version=1,
        prev_hash=bytes(32),
        merkle_root=correct_merkle_root,
        time=genesis_data['timestamp'],
        bits=int(genesis_data['bits'], 16),
        nonce=genesis_data['nonce']
    )
    
    genesis_block = Block(
        header=header,
        transactions=[coinbase_tx]
    )
    
    return genesis_block

def init_blockchain():
    """Initialize blockchain with genesis block."""
    print("🚀 Initializing Irium Blockchain...")
    print()
    
    # Load genesis block
    print("1. Loading genesis block...")
    genesis_block = load_genesis_block()
    print(f"  ✅ Genesis hash: {genesis_block.header.hash().hex()}")
    print(f"  ✅ Merkle root: {genesis_block.header.merkle_root.hex()}")
    print()
    
    # Create PoW limit
    print("2. Creating PoW limit...")
    pow_limit = Target(bits=0x1d00ffff)
    print(f"  ✅ PoW limit created")
    print(f"  ✅ Difficulty: {pow_limit.difficulty()}")
    print()
    
    # Create ChainParams
    print("3. Creating ChainParams...")
    chain_params = ChainParams(
        genesis_block=genesis_block,
        pow_limit=pow_limit
    )
    print("  ✅ ChainParams created")
    print()
    
    # Create ChainState
    print("4. Creating ChainState...")
    chain_state = ChainState(params=chain_params)
    print(f"  ✅ ChainState created")
    print(f"  Height: {chain_state.height}")
    print(f"  Total work: {chain_state.total_work}")
    print(f"  Issued: {chain_state.issued / 100000000} IRM")
    print()
    
    return chain_params, chain_state

if __name__ == "__main__":
    params, state = init_blockchain()
    print("✅ Blockchain initialized successfully!")
    print()
    print("Ready for mining!")
