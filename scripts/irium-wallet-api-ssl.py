#!/usr/bin/env python3
import json
import os
import sys
from pathlib import Path
from http.server import HTTPServer, BaseHTTPRequestHandler
from urllib.parse import urlparse

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from irium.wallet import Wallet
from irium.tools.genesis_loader import load_locked_genesis

REPO_ROOT = Path(__file__).resolve().parents[1]
BLOCKCHAIN_DIR = Path(os.path.expanduser(os.getenv("IRIUM_BLOCKS_DIR", "~/.irium/blocks")))
_GENESIS_BLOCK, _GENESIS_PAYLOAD = load_locked_genesis(REPO_ROOT)
GENESIS_META = {
    "hash": _GENESIS_PAYLOAD["header"]["hash"],
    "time": _GENESIS_PAYLOAD["header"]["time"],
    "bits": _GENESIS_PAYLOAD["header"]["bits"],
    "transactions": len(_GENESIS_PAYLOAD.get("transactions", [])),
}


WALLET_HOST = os.getenv("IRIUM_WALLET_HOST", "127.0.0.1")
WALLET_PORT = int(os.getenv("IRIUM_WALLET_PORT", "8080"))

class IriumWalletAPI(BaseHTTPRequestHandler):
    def __init__(self, *args, **kwargs):
        self.wallet = Wallet()
        self.wallet_file = Path(os.path.expanduser(os.getenv("IRIUM_WALLET_FILE", "~/.irium/irium-wallet.json")))
        self.load_wallet()
        super().__init__(*args, **kwargs)

    def load_wallet(self) -> None:
        if not self.wallet_file.exists():
            return
        try:
            data = json.loads(self.wallet_file.read_text())
        except json.JSONDecodeError:
            return
        for wif in data.get("keys", {}).values():
            self.wallet.import_wif(wif)

    def persist_wallet(self) -> None:
        payload = {"keys": {addr: self.wallet.get_wif(addr) for addr in self.wallet.addresses()}}
        self.wallet_file.parent.mkdir(parents=True, exist_ok=True)
        self.wallet_file.write_text(json.dumps(payload, indent=2))

    def do_OPTIONS(self):
        self.send_response(200)
        self.send_header('Access-Control-Allow-Origin', '*')
        self.send_header('Access-Control-Allow-Methods', 'GET,POST,OPTIONS')
        self.send_header('Access-Control-Allow-Headers', 'Content-Type')
        self.end_headers()

    def do_GET(self):
        path = urlparse(self.path).path
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
            self.send_error(404, 'Not Found')

    def do_POST(self):
        path = urlparse(self.path).path
        if path == '/api/wallet/new-address':
            self.create_address()
        else:
            self.send_error(404, 'Not Found')

    def send_api_info(self):
        html = """<!DOCTYPE html>
<html>
<head>
    <title>Irium Wallet API</title>
    <style>
        body { font-family: Arial, sans-serif; max-width: 720px; margin: 40px auto; padding: 20px; }
        h1 { color: #333; }
        .endpoint { background: #f5f5f5; padding: 10px; margin: 10px 0; border-radius: 5px; }
        code { color: #0a4; }
    </style>
</head>
<body>
    <div class="logo">
        <img src="/irium-logo-wallet.svg" alt="Irium" style="max-width:180px;" />
    </div>
    <h1>Irium Wallet API</h1>
    <p>Offline-friendly HTTP interface for node-operated wallets. All paths are local to this instance.</p>
    <div class="endpoint"><strong>GET</strong> <code>/api/wallet/status</code> – wallet metadata</div>
    <div class="endpoint"><strong>GET</strong> <code>/api/wallet/addresses</code> – list imported addresses</div>
    <div class="endpoint"><strong>GET</strong> <code>/api/wallet/balance</code> – mined balance summary</div>
    <div class="endpoint"><strong>GET</strong> <code>/api/network/info</code> – genesis + network info</div>
    <div class="endpoint"><strong>POST</strong> <code>/api/wallet/new-address</code> – generate a new address</div>
</body>
</html>"""
        self.send_response(200)
        self.send_header('Content-type', 'text/html')
        self.send_header('Access-Control-Allow-Origin', '*')
        self.end_headers()
        self.wfile.write(html.encode())

    def send_wallet_status(self):
        response = {
            "status": "success",
            "data": {
                "addresses": self._addresses(),
                "wallet_file": str(self.wallet_file),
                "network": "irium-mainnet"
            },
        }
        self.send_json_response(response)

    def send_addresses(self):
        self.send_json_response({
            "status": "success",
            "data": {"addresses": self._addresses()},
        })

    def send_balance(self):
        addresses = set(self._addresses())
        blocks = self._block_records()
        tip_height = blocks[-1]["height"] if blocks else 0
        total = mature = immature = 0
        mined = 0
        for block in blocks:
            miner = block.get("miner_address")
            if miner in addresses:
                reward = block.get("reward", 0)
                total += reward
                confirmations = tip_height - block.get("height", tip_height) + 1
                if confirmations >= 100:
                    mature += reward
                else:
                    immature += reward
                mined += 1
        response = {
            "status": "success",
            "data": {
                "balance": total / 100000000,
                "mature": mature / 100000000,
                "immature": immature / 100000000,
                "currency": "IRM",
                "blocks_mined": mined,
            },
        }
        self.send_json_response(response)

    def send_network_info(self):
        response = {
            "status": "success",
            "data": {
                "network": "irium-mainnet",
                "genesis_hash": GENESIS_META["hash"],
                "genesis_time": GENESIS_META["time"],
                "genesis_bits": GENESIS_META["bits"],
                "genesis_transactions": GENESIS_META["transactions"],
            },
        }
        self.send_json_response(response)

    def create_address(self):
        body = self._read_json_body() or {}
        compressed = bool(body.get("compressed", True))
        address = self.wallet.new_address(compressed=compressed)
        self.persist_wallet()
        response = {
            "status": "success",
            "data": {
                "address": address,
                "wif": self.wallet.get_wif(address),
                "compressed": compressed,
            },
        }
        self.send_json_response(response, status=201)

    def send_logo(self):
        logo_path = REPO_ROOT / 'irium-logo-wallet.svg'
        if logo_path.exists():
            self.send_response(200)
            self.send_header('Content-type', 'image/svg+xml')
            self.send_header('Access-Control-Allow-Origin', '*')
            self.send_header('Cache-Control', 'public, max-age=3600')
            self.end_headers()
            self.wfile.write(logo_path.read_bytes())
        else:
            self.send_error(404, 'Logo not found')

    def send_json_response(self, data, status=200):
        self.send_response(status)
        self.send_header('Content-type', 'application/json')
        self.send_header('Access-Control-Allow-Origin', '*')
        self.end_headers()
        self.wfile.write(json.dumps(data, indent=2).encode())

    def _read_json_body(self):
        length = int(self.headers.get('Content-Length', 0))
        if length == 0:
            return None
        raw = self.rfile.read(length)
        try:
            return json.loads(raw.decode())
        except json.JSONDecodeError:
            return None

    def _addresses(self):
        return list(self.wallet.addresses())

    def _block_records(self):
        records = []
        if BLOCKCHAIN_DIR.exists():
            for path in BLOCKCHAIN_DIR.glob('block_*.json'):
                try:
                    height = int(path.stem.split('_')[1])
                except (IndexError, ValueError):
                    continue
                with path.open() as fh:
                    data = json.load(fh)
                data.setdefault('height', height)
                records.append(data)
        return sorted(records, key=lambda item: item['height'])


def main():
    host = WALLET_HOST
    port = WALLET_PORT
    server = HTTPServer((host, port), IriumWalletAPI)
    print(f"Irium Wallet API running on http://{host}:{port}")
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        server.server_close()


if __name__ == "__main__":
    main()
