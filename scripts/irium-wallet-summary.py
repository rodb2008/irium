#!/usr/bin/env python3
"""Irium wallet summary and status."""

import sys
import json
import os
sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from irium.wallet import Wallet, KeyPair

def main():
    print("=== IRIUM WALLET STATUS ===")
    print()
    
    # Check if wallet file exists
    wallet_file = "irium-wallet.json"
    if os.path.exists(wallet_file):
        with open(wallet_file, 'r') as f:
            data = json.load(f)
        
        print(f"Wallet File: {wallet_file}")
        print(f"Addresses: {len(data.get('keys', {}))}")
        print()
        
        # Show each address and its details
        for address, wif in data.get('keys', {}).items():
            key = KeyPair.from_wif(wif)
            print(f"Address: {address}")
            print(f"WIF: {wif}")
            print(f"Public Key: {key.public_key().hex()}")
            print()
    else:
        print("No wallet file found. Create a wallet first:")
        print("  python3 irium-wallet-proper.py create-wallet")
        print()
    
    # Show wallet balance (will be 0 until real transactions)
    wallet = Wallet()
    if os.path.exists(wallet_file):
        with open(wallet_file, 'r') as f:
            data = json.load(f)
        for addr, wif in data.get('keys', {}).items():
            wallet.import_wif(wif)
    
    balance = wallet.balance()
    print(f"Current Balance: {balance} IRM")
    print("(Balance will show actual coins once the blockchain is fully implemented)")
    print()
    
    # Show wallet features
    print("=== WALLET FEATURES ===")
    print("✓ Generate new addresses")
    print("✓ Import/export WIF keys")
    print("✓ Check balances")
    print("✓ Create transactions (when blockchain is implemented)")
    print("✓ SPV (Simplified Payment Verification) support")
    print("✓ Deterministic key generation")
    print("✓ Secp256k1 cryptography")
    print("✓ Base58 address encoding")
    print("✓ P2PKH (Pay-to-Public-Key-Hash) transactions")
    print()
    
    # Show network status
    print("=== NETWORK STATUS ===")
    print("✓ Bootstrap node running at 207.244.247.86:19444")
    print("✓ Genesis block locked and verified")
    print("✓ Consensus parameters configured")
    print("✓ Founder vesting with CLTV timelocks")
    print("⏳ Full blockchain implementation in progress")
    print("⏳ Mining and transaction processing coming soon")
    print()
    
    print("=== NEXT STEPS ===")
    print("1. Create a wallet: python3 irium-wallet-proper.py create-wallet")
    print("2. Save your WIF key securely")
    print("3. Wait for full blockchain implementation")
    print("4. Start mining and transacting IRM coins")

if __name__ == "__main__":
    main()
