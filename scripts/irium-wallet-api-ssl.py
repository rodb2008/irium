#!/usr/bin/env python3
import sys
import json
import os
from http.server import HTTPServer, BaseHTTPRequestHandler
from urllib.parse import urlparse

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from irium.wallet import Wallet, KeyPair

class IriumWalletAPI(BaseHTTPRequestHandler):
    def __init__(self, *args, **kwargs):
        self.wallet = Wallet()
        self.wallet_file = os.path.expanduser(os.getenv("IRIUM_WALLET_FILE", "~/.irium/irium-wallet.json"))
        self.load_wallet()
        super().__init__(*args, **kwargs)

    def load_wallet(self):
        if os.path.exists(self.wallet_file):
            with open(self.wallet_file, 'r') as f:
                data = json.load(f)
                for addr, wif in data.get('keys', {}).items():
                    self.wallet.import_wif(wif)

    def do_GET(self):
        parsed_path = urlparse(self.path)
        path = parsed_path.path

        if path == '/':
            self.send_api_info()
        elif path == '/api/wallet/status':
            self.send_wallet_status()
        elif path == '/api/wallet/addresses':
            self.send_addresses()
        elif path == '/api/wallet/balance':
            self.send_balance()
        elif path == '/api/network/info':
            self.send_network_info()
        elif path == '/irium-logo-wallet.svg':
            self.send_logo()
        else:
            self.send_error(404, "Not Found")

    def send_api_info(self):
        """Send API information page."""
        html = """<!DOCTYPE html>
<html>
<head>
    <title>Irium Wallet API</title>
    <style>
        body { font-family: Arial, sans-serif; max-width: 800px; margin: 50px auto; padding: 20px; }
        h1 { color: #333; }
        .endpoint { background: #f5f5f5; padding: 10px; margin: 10px 0; border-radius: 5px; }
        .endpoint code { color: #0066cc; }
        .logo { text-align: center; margin: 20px 0; }
        .logo img { max-width: 200px; }
    </style>
</head>
<body>
    <div class="logo">
        <img src="/irium-logo-wallet.svg" alt="Irium Logo">
    </div>
    <h1>Irium Wallet API</h1>
    <p>Welcome to the Irium blockchain wallet API server.</p>
    
    <h2>Available Endpoints:</h2>
    
    <div class="endpoint">
        <strong>GET</strong> <code>/api/wallet/status</code>
        <p>Get wallet status and addresses</p>
    </div>
    
    <div class="endpoint">
        <strong>GET</strong> <code>/api/wallet/addresses</code>
        <p>List all wallet addresses</p>
    </div>
    
    <div class="endpoint">
        <strong>GET</strong> <code>/api/wallet/balance</code>
        <p>Get total wallet balance</p>
    </div>
    
    <div class="endpoint">
        <strong>GET</strong> <code>/api/network/info</code>
        <p>Get network information</p>
    </div>
    
    <div class="endpoint">
        <strong>GET</strong> <code>/irium-logo-wallet.svg</code>
        <p>Irium official logo (SVG)</p>
    </div>
    
    <h2>Network Status:</h2>
    <p><strong>Network:</strong> Mainnet (LIVE)</p>
    <p><strong>Genesis:</strong> 0000000040e3eb5ed9db5cc8df56dd6db9c6f3009ca7e9114fb52400e0136fb6</p>
    
    <h2>Links:</h2>
    <ul>
        <li><a href="https://github.com/iriumlabs/irium">GitHub Repository</a></li>
        <li><a href="/api/stats">API Documentation</a></li>
    </ul>
</body>
</html>"""
        self.send_response(200)
        self.send_header('Content-type', 'text/html')
        self.send_header('Access-Control-Allow-Origin', '*')
        self.end_headers()
        self.wfile.write(html.encode())

    def send_wallet_status(self):
        addresses = list(self.wallet._keys.keys())
        response = {
            "status": "success",
            "data": {
                "addresses": addresses,
                "balance": "see /api/wallet/balance for calculated balance",
                "network": "irium-mainnet",
                "ssl_enabled": True
            }
        }
        self.send_json_response(response)

    def send_addresses(self):
        addresses = list(self.wallet._keys.keys())
        response = {
            "status": "success",
            "data": {
                "addresses": addresses
            }
        }
        self.send_json_response(response)

    def send_balance(self):
        import os
        import json
        
        # Calculate balance from blockchain
        blocks_dir = os.path.expanduser("~/.irium/blocks")
        total_balance = 0
        mature_balance = 0
        immature_balance = 0
        
        if os.path.exists(blocks_dir):
            blocks = {}
            for f in os.listdir(blocks_dir):
                if f.startswith('block_') and f.endswith('.json'):
                    height = int(f.replace('block_', '').replace('.json', ''))
                    with open(f"{blocks_dir}/{f}") as file:
                        blocks[height] = json.load(file)
            
            current_height = max(blocks.keys()) if blocks else 0
            addresses = list(self.wallet._keys.keys())
            
            for height in blocks:
                block = blocks[height]
                if block.get('miner_address') in addresses:
                    reward = block.get('reward', 0)
                    total_balance += reward
                    
                    confirmations = current_height - height + 1
                    if confirmations >= 100:
                        mature_balance += reward
                    else:
                        immature_balance += reward
        
        response = {
            "status": "success",
            "data": {
                "balance": total_balance / 100000000,
                "mature": mature_balance / 100000000,
                "immature": immature_balance / 100000000,
                "currency": "IRM",
                "blocks_mined": len([b for b in blocks.values() if b.get('miner_address') in addresses]) if 'blocks' in locals() else 0
            }
        }
        self.send_json_response(response)

    def send_network_info(self):
        response = {
            "status": "success",
            "data": {
                "network": "irium-mainnet",
                "ticker": "IRM",
                "ssl_enabled": True,
                "endpoint": "/wallet",
                "logo_url": "/irium-logo-wallet.svg"
            }
        }
        self.send_json_response(response)

    def send_logo(self):
        logo_path = os.path.join(os.path.dirname(os.path.dirname(__file__)), 'irium-logo-wallet.svg')
        if os.path.exists(logo_path):
            self.send_response(200)
            self.send_header('Content-type', 'image/svg+xml')
            self.send_header('Access-Control-Allow-Origin', '*')
            self.send_header('Cache-Control', 'public, max-age=3600')
            self.end_headers()
            with open(logo_path, 'rb') as f:
                self.wfile.write(f.read())
        else:
            self.send_error(404, "Logo not found")

    def send_json_response(self, data):
        self.send_response(200)
        self.send_header('Content-type', 'application/json')
        self.send_header('Access-Control-Allow-Origin', '*')
        self.end_headers()
        self.wfile.write(json.dumps(data, indent=2).encode())

if __name__ == "__main__":
    server = HTTPServer(('0.0.0.0', 8080), IriumWalletAPI)
    print("Irium Wallet API running on http://0.0.0.0:8080")
    server.serve_forever()
