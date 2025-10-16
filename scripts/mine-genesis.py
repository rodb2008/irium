#!/usr/bin/env python3
"""Mine a valid genesis block for Irium."""

import sys
import os
sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from irium.block import Block, BlockHeader
from irium.tx import Transaction, TxInput, TxOutput
from irium.pow import Target
import json
import time

def mine_genesis():
    """Mine genesis block with valid PoW."""
    print("⛏️  Mining Irium Genesis Block...")
    print()
    
    genesis_file = os.path.join(os.path.dirname(__file__), '..', 'configs', 'genesis.json')
    with open(genesis_file, 'r') as f:
        genesis_data = json.load(f)
    
    allocations = genesis_data.get('allocations', [])
    outputs = []
    total_vesting = 0
    
    for alloc in allocations:
        script_pubkey = bytes.fromhex(alloc['script_pubkey'])
        amount = alloc['amount_sats']
        outputs.append(TxOutput(value=amount, script_pubkey=script_pubkey))
        total_vesting += amount
    
    coinbase_input = TxInput(
        prev_txid=bytes(32),
        prev_index=0xFFFFFFFF,
        script_sig=b"Irium Genesis Block - SHA256d PoW"
    )
    
    coinbase_tx = Transaction(
        version=1,
        inputs=[coinbase_input],
        outputs=outputs
    )
    
    temp_block = Block(
        header=BlockHeader(
            version=1,
            prev_hash=bytes(32),
            merkle_root=bytes(32),
            time=genesis_data['timestamp'],
            bits=int(genesis_data['bits'], 16),
            nonce=0
        ),
        transactions=[coinbase_tx]
    )
    
    correct_merkle = temp_block.merkle_root()[::-1]
    
    target = Target(bits=int(genesis_data['bits'], 16))
    target_value = target.to_target()
    
    print(f"Genesis Configuration:")
    print(f"  Timestamp: {genesis_data['timestamp']}")
    print(f"  Bits: {hex(int(genesis_data['bits'], 16))}")
    print(f"  Target: {target_value}")
    print(f"  Merkle root: {correct_merkle.hex()}")
    print(f"  Total vesting: {total_vesting / 100000000} IRM")
    print()
    
    print("⛏️  Mining for valid nonce...")
    nonce = 0
    current_time = genesis_data['timestamp']
    start_time = time.time()
    hashes_done = 0
    
    while True:
        header = BlockHeader(
            version=1,
            prev_hash=bytes(32),
            merkle_root=correct_merkle,
            time=current_time,
            bits=int(genesis_data['bits'], 16),
            nonce=nonce
        )
        
        header_hash = header.hash()[::-1]
        header_hash_int = int.from_bytes(header_hash, "big")
        
        hashes_done += 1
        
        if header_hash_int < target_value:
            elapsed = time.time() - start_time
            hashrate = hashes_done / elapsed if elapsed > 0 else 0
            
            print(f"✅ Found valid genesis block!")
            print()
            print(f"Genesis Block Details:")
            print(f"  Nonce: {nonce}")
            print(f"  Time: {current_time}")
            print(f"  Hash: {header.hash().hex()}")
            print(f"  Merkle root: {correct_merkle.hex()}")
            print(f"  Bits: {hex(int(genesis_data['bits'], 16))}")
            print()
            print(f"Mining Stats:")
            print(f"  Hashes: {hashes_done:,}")
            print(f"  Time: {elapsed:.2f} seconds")
            print(f"  Hashrate: {hashrate:.2f} H/s")
            print()
            
            genesis_data['nonce'] = nonce
            genesis_data['time'] = current_time
            genesis_data['hash'] = header.hash().hex()
            genesis_data['merkle_root'] = correct_merkle.hex()
            
            with open(genesis_file, 'w') as f:
                json.dump(genesis_data, f, indent=2)
            
            print(f"✅ Updated {genesis_file} with new genesis")
            
            return header, coinbase_tx
        
        nonce += 1
        
        # When nonce reaches 2^32, increment timestamp and reset nonce
        if nonce >= 4294967295:
            current_time += 1
            nonce = 0
            print(f"  Timestamp incremented to {current_time}, resetting nonce")
        
        if hashes_done % 100000 == 0:
            elapsed = time.time() - start_time
            hashrate = hashes_done / elapsed if elapsed > 0 else 0
            print(f"  Nonce: {nonce:,} | Time: {current_time} | Hashrate: {hashrate:.2f} H/s")

if __name__ == "__main__":
    mine_genesis()
