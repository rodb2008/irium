#!/usr/bin/env python3
"""Monitor Irium blockchain for incoming transactions."""

import sys
import os
import asyncio
import json
sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from irium.wallet import Wallet

WALLET_FILE = "irium-wallet.json"

async def monitor_transactions():
    """Monitor blockchain for transactions to wallet addresses."""
    print("🔍 Starting Irium Transaction Monitor...")
    print()
    
    # Load wallet
    if not os.path.exists(WALLET_FILE):
        print("❌ No wallet file found. Create a wallet first.")
        return
    
    with open(WALLET_FILE, 'r') as f:
        data = json.load(f)
    
    wallet = Wallet()
    for addr, wif in data.get('keys', {}).items():
        wallet.import_wif(wif)
    
    addresses = list(wallet.addresses())
    print(f"👀 Monitoring {len(addresses)} addresses:")
    for addr in addresses:
        print(f"  - {addr}")
    print()
    
    print("🔄 Checking for new transactions...")
    print()
    
    # TODO: Implement actual blockchain monitoring
    # For now, show current balance
    balance = wallet.balance()
    print(f"💰 Current balance: {balance / 100000000} IRM ({balance} satoshis)")
    print()
    print("⚠️ Note: Real-time monitoring not yet implemented")
    print("Needs blockchain scanning and UTXO tracking")
    print()
    print("Current status:")
    print("  - Can check balance ✅")
    print("  - Can list addresses ✅")
    print("  - Real-time monitoring ⏳")
    
    # Keep running and check periodically
    while True:
        await asyncio.sleep(30)
        balance = wallet.balance()
        print(f"[{asyncio.get_event_loop().time():.0f}] Balance: {balance / 100000000} IRM")

def main():
    print("Irium Transaction Monitor")
    print("Press Ctrl+C to stop")
    print()
    
    try:
        asyncio.run(monitor_transactions())
    except KeyboardInterrupt:
        print("\n👋 Stopping monitor...")

if __name__ == "__main__":
    main()
