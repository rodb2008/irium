#!/usr/bin/env python3
"""Irium Wallet Integration for External Wallets."""

import sys
import json
import os
import requests
sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
from irium.wallet import Wallet, KeyPair

class IriumWalletIntegration:
    def __init__(self, api_url="https://207.244.247.86/api"):
        self.api_url = api_url
        self.wallet = Wallet()
        self.wallet_file = "irium-wallet.json"
        self.load_wallet()
    
    def load_wallet(self):
        if os.path.exists(self.wallet_file):
            with open(self.wallet_file, 'r') as f:
                data = json.load(f)
            for addr, wif in data.get('keys', {}).items():
                self.wallet.import_wif(wif)
    
    def save_wallet(self):
        data = {'keys': {}, 'addresses': []}
        if os.path.exists(self.wallet_file):
            with open(self.wallet_file, 'r') as f:
                data = json.load(f)
        
        for addr in self.wallet.addresses():
            if addr not in data['keys']:
                # This is a simplified approach - in real implementation, store WIF when creating
                pass
        
        with open(self.wallet_file, 'w') as f:
            json.dump(data, f, indent=2)
    
    def create_wallet(self):
        key = KeyPair.generate()
        wif = key.to_wif()
        address = key.address()
        
        self.wallet.import_wif(wif)
        
        data = {'keys': {}, 'addresses': []}
        if os.path.exists(self.wallet_file):
            with open(self.wallet_file, 'r') as f:
                data = json.load(f)
        
        data['keys'][address] = wif
        data['addresses'].append(address)
        
        with open(self.wallet_file, 'w') as f:
            json.dump(data, f, indent=2)
        
        return {
            "address": address,
            "wif": wif,
            "public_key": key.public_key().hex(),
            "message": "Wallet created successfully"
        }
    
    def get_wallet_status(self):
        return {
            "addresses": list(self.wallet.addresses()),
            "balance": self.wallet.balance(),
            "network": "irium-mainnet",
            "ssl_enabled": True,
            "api_endpoint": self.api_url
        }
    
    def get_network_info(self):
        return {
            "network": "irium-mainnet",
            "ticker": "IRM",
            "block_height": 0,
            "difficulty": "1d00ffff",
            "peers": 1,
            "version": "0.1.0",
            "bootstrap_node": "207.244.247.86:19444",
            "ssl_enabled": True,
            "api_endpoint": self.api_url
        }

def main():
    if len(sys.argv) < 2:
        print("Irium Wallet Integration")
        print("Usage:")
        print("  python3 irium-wallet-integration.py create-wallet")
        print("  python3 irium-wallet-integration.py status")
        print("  python3 irium-wallet-integration.py network-info")
        print("  python3 irium-wallet-integration.py api-test")
        return
    
    integration = IriumWalletIntegration()
    command = sys.argv[1]
    
    if command == "create-wallet":
        result = integration.create_wallet()
        print(json.dumps(result, indent=2))
        
    elif command == "status":
        result = integration.get_wallet_status()
        print(json.dumps(result, indent=2))
        
    elif command == "network-info":
        result = integration.get_network_info()
        print(json.dumps(result, indent=2))
        
    elif command == "api-test":
        try:
            response = requests.get(f"{integration.api_url}/wallet/status", verify=False)
            print("API Test Result:")
            print(json.dumps(response.json(), indent=2))
        except Exception as e:
            print(f"API Test Failed: {e}")
    
    else:
        print(f"Unknown command: {command}")

if __name__ == "__main__":
    main()
