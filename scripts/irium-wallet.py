#!/usr/bin/env python3
"""Simple Irium wallet CLI for users."""

import sys
import json
import os
sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from irium.wallet import Wallet, KeyPair

def main():
    if len(sys.argv) < 2:
        print("Irium Wallet CLI")
        print("Usage:")
        print("  python3 irium-wallet.py new-address")
        print("  python3 irium-wallet.py import-wif <WIF>")
        print("  python3 irium-wallet.py balance")
        print("  python3 irium-wallet.py addresses")
        print("  python3 irium-wallet.py generate-key")
        return

    command = sys.argv[1]
    
    if command == "new-address":
        wallet = Wallet()
        address = wallet.new_address()
        print(f"New address: {address}")
        
    elif command == "import-wif":
        if len(sys.argv) < 3:
            print("Error: WIF required")
            return
        wif = sys.argv[2]
        try:
            wallet = Wallet()
            address = wallet.import_wif(wif)
            print(f"Imported address: {address}")
        except Exception as e:
            print(f"Error importing WIF: {e}")
            
    elif command == "balance":
        wallet = Wallet()
        balance = wallet.balance()
        print(f"Balance: {balance} IRM")
        
    elif command == "addresses":
        wallet = Wallet()
        addresses = list(wallet.addresses())
        print(f"Addresses: {addresses}")
        
    elif command == "generate-key":
        key = KeyPair.generate()
        wif = key.to_wif()
        address = key.address()
        print(f"Private Key (WIF): {wif}")
        print(f"Address: {address}")
        print(f"Public Key: {key.public_key().hex()}")
        
    else:
        print(f"Unknown command: {command}")

if __name__ == "__main__":
    main()
