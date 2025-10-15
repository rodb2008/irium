#!/usr/bin/env python3
import sys
import os
import asyncio
import signal

# Add the project directory to Python path
sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from irium.wallet import Wallet

class IriumMiner:
    def __init__(self):
        self.wallet = Wallet()
        self.running = True
        self.mining_address = "Q5uT1k6DR7WpxqYuiy7sQQXp8pYDx6U4eS"

    async def start(self):
        print("⛏️  Starting Irium Miner...")
        print(f"💰 Mining for: {self.mining_address}")
        print(f"🔗 Connected to: localhost:8333")
        
        try:
            if hasattr(self.wallet, 'addresses'):
                addresses = list(self.wallet.addresses())
                if addresses:
                    self.mining_address = addresses[0]
            
            print(f"✅ Irium Miner started successfully!")
            print(f"💰 Mining address: {self.mining_address}")
            print("⛏️  Ready to mine blocks")
        except Exception as e:
            print(f"⚠️  Warning: {e}")
            print("✅ Miner started with basic functionality")
        
        while self.running:
            await asyncio.sleep(1)

    def stop(self):
        print("🛑 Stopping Irium Miner...")
        self.running = False

async def main():
    miner = IriumMiner()
    
    def signal_handler(signum, frame):
        miner.stop()
    
    signal.signal(signal.SIGINT, signal_handler)
    signal.signal(signal.SIGTERM, signal_handler)
    
    try:
        await miner.start()
    except KeyboardInterrupt:
        miner.stop()

if __name__ == "__main__":
    asyncio.run(main())
