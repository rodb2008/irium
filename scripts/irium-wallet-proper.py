#!/usr/bin/env python3
"""Proper Irium wallet CLI with persistent storage."""

import sys
import json
import os
sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from irium.wallet import Wallet, KeyPair

WALLET_FILE = "irium-wallet.json"

def load_wallet():
    """Load wallet from file or create new one."""
    if os.path.exists(WALLET_FILE):
        with open(WALLET_FILE, 'r') as f:
            data = json.load(f)
        wallet = Wallet()
        for addr, wif in data.get('keys', {}).items():
            wallet.import_wif(wif)
        return wallet, data
    return Wallet(), {'keys': {}, 'addresses': []}

def save_wallet(wallet, data):
    """Save wallet to file - FIXED VERSION."""
    data['addresses'] = list(wallet.addresses())
    with open(WALLET_FILE, 'w') as f:
        json.dump(data, f, indent=2)

def main():
    if len(sys.argv) < 2:
        print("Irium Wallet CLI - Complete Implementation")
        print("Usage:")
        print("  python3 irium-wallet-proper.py new-address")
        print("  python3 irium-wallet-proper.py import-wif <WIF>")
        print("  python3 irium-wallet-proper.py balance")
        print("  python3 irium-wallet-proper.py addresses")
        print("  python3 irium-wallet-proper.py generate-key")
        print("  python3 irium-wallet-proper.py create-wallet")
        print("  python3 irium-wallet-proper.py show-wallet")
        print("  python3 irium-wallet-proper.py show-keys")
        print("  python3 irium-wallet-proper.py send <address> <amount>")
        return

    command = sys.argv[1]

    if command == "new-address":
        wallet, data = load_wallet()
        key = KeyPair.generate()
        wif = key.to_wif()
        address = key.address()
        wallet.import_wif(wif)
        data['keys'][address] = wif
        save_wallet(wallet, data)
        print(f"New address: {address}")
        print(f"WIF: {wif}")
        print(f"Public Key: {key.public_key().hex()}")

    elif command == "import-wif":
        if len(sys.argv) < 3:
            print("Error: WIF required")
            return
        wif = sys.argv[2]
        try:
            wallet, data = load_wallet()
            address = wallet.import_wif(wif)
            data['keys'][address] = wif
            save_wallet(wallet, data)
            print(f"Imported address: {address}")
            print(f"WIF: {wif}")
            key = KeyPair.from_wif(wif)
            print(f"Public Key: {key.public_key().hex()}")
        except Exception as e:
            print(f"Error importing WIF: {e}")

    elif command == "balance":
        wallet, data = load_wallet()
        balance = wallet.balance()
        print(f"Balance: {balance / 100000000} IRM ({balance} satoshis)")

    elif command == "addresses":
        wallet, data = load_wallet()
        addresses = list(wallet.addresses())
        print(f"Addresses: {addresses}")

    elif command == "generate-key":
        key = KeyPair.generate()
        wif = key.to_wif()
        address = key.address()
        print(f"Private Key (WIF): {wif}")
        print(f"Address: {address}")
        print(f"Public Key: {key.public_key().hex()}")

    elif command == "create-wallet":
        wallet, data = load_wallet()
        key = KeyPair.generate()
        wif = key.to_wif()
        address = key.address()
        wallet.import_wif(wif)
        data['keys'][address] = wif
        save_wallet(wallet, data)
        print(f"Wallet created!")
        print(f"Address: {address}")
        print(f"WIF: {wif}")
        print(f"Public Key: {key.public_key().hex()}")
        print("Save this WIF to backup your wallet!")

    elif command == "show-wallet":
        wallet, data = load_wallet()
        addresses = list(wallet.addresses())
        balance = wallet.balance()
        print(f"Wallet Status:")
        print(f"  Addresses: {len(addresses)}")
        print(f"  Balance: {balance / 100000000} IRM ({balance} satoshis)")
        for addr in addresses:
            print(f"    {addr}")

    elif command == "show-keys":
        wallet, data = load_wallet()
        print(f"Wallet Keys:")
        for address, wif in data.get('keys', {}).items():
            key = KeyPair.from_wif(wif)
            print(f"  Address: {address}")
            print(f"  WIF: {wif}")
            print(f"  Public Key: {key.public_key().hex()}")
            print()

    elif command == "send":
        if len(sys.argv) < 4:
            print("Error: Usage: send <address> <amount_in_irm>")
            print("Example: send Q5uT1k6DR7WpxqYuiy7sQQXp8pYDx6U4eS 1.5")
            return
        
        to_address = sys.argv[2]
        amount_irm = float(sys.argv[3])
        amount_sats = int(amount_irm * 100000000)  # Convert IRM to satoshis
        
        wallet, data = load_wallet()
        
        try:
            # Check balance
            balance = wallet.balance()
            if balance < amount_sats:
                print(f"Error: Insufficient balance")
                print(f"  You have: {balance / 100000000} IRM")
                print(f"  You need: {amount_irm} IRM")
                return
            
            # Create transaction
            payments = [(to_address, amount_sats)]
            fee = 10000  # 0.0001 IRM fee
            tx = wallet.create_transaction(payments, fee=fee)
            
            print(f"✅ Transaction created successfully!")
            print(f"  To: {to_address}")
            print(f"  Amount: {amount_irm} IRM ({amount_sats} satoshis)")
            print(f"  Fee: {fee / 100000000} IRM ({fee} satoshis)")
            print(f"  Transaction ID: {tx.txid().hex()}")
            print()
            print("⚠️ IMPORTANT:")
            print("  Transaction created but NOT broadcast yet")
            print("  Broadcasting functionality needs to be implemented")
            print("  The transaction is ready to be sent to the network")
            
        except Exception as e:
            print(f"Error creating transaction: {e}")

    else:
        print(f"Unknown command: {command}")

if __name__ == "__main__":
    main()
