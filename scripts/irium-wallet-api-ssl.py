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
        self.wallet_file = "irium-wallet.json"
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
        
        if path == '/api/wallet/status':
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
    
    def do_HEAD(self):
        parsed_path = urlparse(self.path)
        path = parsed_path.path
        
        if path == '/irium-logo-wallet.svg':
            self.send_logo_headers()
        else:
            self.send_error(404, "Not Found")
    
    def send_logo_headers(self):
        logo_path = os.path.join(os.path.dirname(__file__), '..', 'irium-logo-wallet.svg')
        if os.path.exists(logo_path):
            self.send_response(200)
            self.send_header('Content-type', 'image/svg+xml')
            self.send_header('Access-Control-Allow-Origin', '*')
            self.send_header('Cache-Control', 'public, max-age=3600')
            self.end_headers()
        else:
            self.send_error(404, "Logo not found")
    
    def send_logo(self):
        logo_path = os.path.join(os.path.dirname(__file__), '..', 'irium-logo-wallet.svg')
        if os.path.exists(logo_path):
            self.send_response(200)
            self.send_header('Content-type', 'image/svg+xml')
            self.send_header('Access-Control-Allow-Origin', '*')
            self.send_header('Cache-Control', 'public, max-age=3600')
            self.end_headers()
            with open(logo_path, 'r') as f:
                self.wfile.write(f.read().encode('utf-8'))
        else:
            self.send_error(404, "Logo not found")
    
    def send_wallet_status(self):
        addresses = list(self.wallet.addresses())
        balance = self.wallet.balance()
        status = {
            "status": "success",
            "data": {
                "addresses": addresses,
                "balance": balance,
                "network": "irium-mainnet",
                "ssl_enabled": True
            }
        }
        self.send_json_response(status)
    
    def send_addresses(self):
        addresses = list(self.wallet.addresses())
        response = {"status": "success", "data": {"addresses": addresses}}
        self.send_json_response(response)
    
    def send_balance(self):
        balance = self.wallet.balance()
        response = {"status": "success", "data": {"balance": balance, "currency": "IRM"}}
        self.send_json_response(response)
    
    def send_network_info(self):
        info = {
            "status": "success",
            "data": {
                "network": "irium-mainnet",
                "ticker": "IRM",
                "ssl_enabled": True,
                "endpoint": "https://207.244.247.86/api",
                "logo_url": "http://207.244.247.86:8080/irium-logo-wallet.svg"
            }
        }
        self.send_json_response(info)
    
    def send_json_response(self, data):
        self.send_response(200)
        self.send_header('Content-type', 'application/json')
        self.send_header('Access-Control-Allow-Origin', '*')
        self.end_headers()
        self.wfile.write(json.dumps(data).encode('utf-8'))

def run_server(port=8080):
    # Change from 127.0.0.1 to 0.0.0.0 to listen on all interfaces
    server = HTTPServer(('0.0.0.0', port), IriumWalletAPI)
    print(f"Wallet API running on http://0.0.0.0:{port}")
    print("SSL available at https://207.244.247.86/api")
    print("Logo available at http://207.244.247.86:8080/irium-logo-wallet.svg")
    server.serve_forever()

if __name__ == "__main__":
    run_server()
