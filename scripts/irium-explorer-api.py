#!/usr/bin/env python3
"""Irium Blockchain Explorer API."""

import sys
import os
import json
from pathlib import Path
from http.server import HTTPServer, BaseHTTPRequestHandler
from urllib.parse import urlparse, parse_qs

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from irium.rate_limiter import RateLimiter
from irium.tools.genesis_loader import load_locked_genesis

# Initialize rate limiter
rate_limiter = RateLimiter(requests_per_minute=120)  # 120 requests per minute

BLOCKCHAIN_DIR = Path(os.path.expanduser(os.getenv("IRIUM_BLOCKS_DIR", "~/.irium/blocks")))
MEMPOOL_DIR = Path(os.path.expanduser(os.getenv("IRIUM_MEMPOOL_DIR", "~/.irium/mempool")))
REPO_ROOT = Path(__file__).resolve().parents[1]


EXPLORER_HOST = os.getenv("IRIUM_EXPLORER_HOST", "127.0.0.1")
EXPLORER_PORT = int(os.getenv("IRIUM_EXPLORER_PORT", "8082"))

def _genesis_block_dict():
    """Return a JSON-serialisable dict representing the locked genesis block."""
    block, payload = load_locked_genesis(REPO_ROOT)
    header = payload["header"]
    txs = payload.get("transactions", [])
    return {
        "height": 0,
        "hash": header["hash"],
        "prev_hash": header["prev_hash"],
        "merkle_root": header["merkle_root"],
        "time": header["time"],
        "bits": hex(int(header["bits"], 16)),
        "nonce": header["nonce"],
        "transactions": len(txs),
        "tx_hex": txs,
        "reward": 0,
        "miner_address": "GENESIS",
    }


GENESIS_BLOCK = _genesis_block_dict()


def _load_block_files():
    if not BLOCKCHAIN_DIR.exists():
        return []
    blocks = []
    for entry in BLOCKCHAIN_DIR.iterdir():
        if not entry.name.startswith("block_") or entry.suffix != ".json":
            continue
        try:
            height = int(entry.stem.split("_")[1])
        except (IndexError, ValueError):
            continue
        with entry.open() as fh:
            data = json.load(fh)
        data.setdefault("height", height)
        blocks.append(data)
    return blocks


def _all_blocks_desc():
    blocks = _load_block_files()
    if not any(block.get("height") == 0 for block in blocks):
        blocks.append(GENESIS_BLOCK.copy())
    return sorted(blocks, key=lambda item: item["height"], reverse=True)


class ExplorerAPI(BaseHTTPRequestHandler):
    """Blockchain Explorer REST API."""
    
    def do_GET(self):
        # Rate limiting
        client_ip = self.client_address[0]
        if not rate_limiter.is_allowed(client_ip):
            self.send_response(429)
            self.send_header('Content-type', 'application/json')
            self.send_header('Retry-After', '60')
            self.end_headers()
            self.wfile.write(b'{"error": "Rate limit exceeded. Try again later."}')
            return
        
        """Handle GET requests."""
        parsed_path = urlparse(self.path)
        path = parsed_path.path
        query = parse_qs(parsed_path.query)
        
        # CORS headers
        
        if path == '/api/blocks':
            self.get_blocks(query)
        elif path == '/api/block':
            self.get_block(query)
        elif path.startswith('/api/block/'):
            height = path.split('/')[-1]
            self.get_block_by_height(height)
        elif path == '/api/stats':
            self.get_stats()
        elif path == '/api/mempool':
            self.get_mempool()
        elif path == '/api/latest':
            self.get_latest_blocks(query)
        elif path == '/':
            self.serve_index()
        else:
            self.send_error(404, "Not Found")
    
    def send_json_response(self, data, status=200):
        """Send JSON response."""
        self.send_response(status)
        self.send_header('Content-type', 'application/json')
        self.send_header('Access-Control-Allow-Origin', '*')
        self.end_headers()
        self.wfile.write(json.dumps(data, indent=2).encode())
    
    def get_blocks(self, query):
        """Get list of blocks."""
        try:
            blocks = _all_blocks_desc()
            total = len(blocks)
            page = max(1, int(query.get('page', [1])[0]))
            per_page = max(1, int(query.get('per_page', [10])[0]))
            start = (page - 1) * per_page
            end = start + per_page

            self.send_json_response({
                'blocks': blocks[start:end],
                'total': total,
                'page': page,
                'per_page': per_page
            })
        
        except Exception as e:
            self.send_json_response({'error': str(e)}, 500)
    
    def get_block(self, query):
        """Get block by hash."""
        try:
            block_hash = query.get('hash', [None])[0]
            if not block_hash:
                self.send_json_response({'error': 'Hash parameter required'}, 400)
                return

            for block in _all_blocks_desc():
                if block.get('hash') == block_hash:
                    self.send_json_response({'block': block})
                    return

            self.send_json_response({'error': 'Block not found'}, 404)
        
        except Exception as e:
            self.send_json_response({'error': str(e)}, 500)
    
    def get_block_by_height(self, height):
        """Get block by height."""
        try:
            height_int = int(height)
        except ValueError:
            self.send_json_response({'error': 'Invalid height'}, 400)
            return

        try:
            block = self._block_for_height(height_int)
            if block is None:
                self.send_json_response({'error': 'Block not found'}, 404)
                return

            self.send_json_response({'block': block})
        
        except Exception as e:
            self.send_json_response({'error': str(e)}, 500)
    
    def get_latest_blocks(self, query):
        """Get latest N blocks."""
        try:
            count = int(query.get('count', [10])[0])
            blocks = _all_blocks_desc()
            self.send_json_response({'blocks': blocks[:count]})
        
        except Exception as e:
            self.send_json_response({'error': str(e)}, 500)
    
    def get_stats(self):
        """Get blockchain statistics."""
        try:
            blocks = _all_blocks_desc()
            latest_block = blocks[0] if blocks else GENESIS_BLOCK
            total_supply = sum(block.get('reward', 0) for block in blocks if block.get('height', 0) > 0)

            self.send_json_response({
                'height': latest_block.get('height', 0),
                'total_blocks': len(blocks),
                'total_supply': total_supply,
                'supply_irm': total_supply / 100000000,
                'latest_block': latest_block.get('hash'),
                'latest_block_time': latest_block.get('time', 0),
                'genesis_hash': GENESIS_BLOCK['hash']
            })
        
        except Exception as e:
            self.send_json_response({'error': str(e)}, 500)
    
    def get_mempool(self):
        """Get mempool transactions."""
        try:
            mempool_file = MEMPOOL_DIR / 'pending.json'
            
            if not mempool_file.exists():
                self.send_json_response({'transactions': [], 'count': 0})
                return
            
            with mempool_file.open() as f:
                mempool = json.load(f)
            
            self.send_json_response({
                'transactions': mempool,
                'count': len(mempool)
            })
        
        except Exception as e:
            self.send_json_response({'error': str(e)}, 500)
    
    def serve_index(self):
        """Serve index page with API documentation."""
        html = """
        <!DOCTYPE html>
        <html>
        <head>
            <title>Irium Blockchain Explorer API</title>
            <style>
                body { font-family: Arial, sans-serif; margin: 40px; background: #f5f5f5; }
                h1 { color: #333; }
                .endpoint { background: white; padding: 15px; margin: 10px 0; border-radius: 5px; }
                .method { color: #0066cc; font-weight: bold; }
                code { background: #eee; padding: 2px 5px; border-radius: 3px; }
            </style>
        </head>
        <body>
            <h1>🔗 Irium Blockchain Explorer API</h1>
            <p>RESTful API for exploring the Irium blockchain</p>
            
            <h2>Endpoints:</h2>
            
            <div class="endpoint">
                <p><span class="method">GET</span> <code>/api/blocks?page=1&per_page=10</code></p>
                <p>Get paginated list of blocks</p>
            </div>
            
            <div class="endpoint">
                <p><span class="method">GET</span> <code>/api/block?hash=BLOCK_HASH</code></p>
                <p>Get block by hash</p>
            </div>
            
            <div class="endpoint">
                <p><span class="method">GET</span> <code>/api/block/HEIGHT</code></p>
                <p>Get block by height (e.g., /api/block/5)</p>
            </div>
            
            <div class="endpoint">
                <p><span class="method">GET</span> <code>/api/latest?count=10</code></p>
                <p>Get latest N blocks</p>
            </div>
            
            <div class="endpoint">
                <p><span class="method">GET</span> <code>/api/stats</code></p>
                <p>Get blockchain statistics</p>
            </div>
            
            <div class="endpoint">
                <p><span class="method">GET</span> <code>/api/mempool</code></p>
                <p>Get pending transactions in mempool</p>
            </div>
            
            <h2>Example Usage:</h2>
            <pre>curl http://localhost:8082/api/stats
curl http://localhost:8082/api/latest?count=5
curl http://localhost:8082/api/block/2</pre>
        </body>
        </html>
        """
        
        self.send_response(200)
        self.send_header('Content-type', 'text/html')
        self.end_headers()
        self.wfile.write(html.encode())

    def _block_for_height(self, height: int):
        if height == 0:
            return GENESIS_BLOCK.copy()
        candidate = BLOCKCHAIN_DIR / f'block_{height}.json'
        if not candidate.exists():
            return None
        with candidate.open() as fh:
            data = json.load(fh)
        data.setdefault("height", height)
        return data


def main():
    host = EXPLORER_HOST
    port = EXPLORER_PORT
    if len(sys.argv) > 1:
        port = int(sys.argv[1])

    server = HTTPServer((host, port), ExplorerAPI)
    print("🔗 Irium Blockchain Explorer API")
    print(f"📡 Listening on http://{host}:{port}")
    print(f"📊 API Documentation: http://{host}:{port}/")
    print()
    print("Endpoints:")
    print(f"  • http://{host}:{port}/api/stats")
    print(f"  • http://{host}:{port}/api/blocks")
    print(f"  • http://{host}:{port}/api/latest?count=10")
    print(f"  • http://{host}:{port}/api/mempool")
    print()

    try:
        server.serve_forever()
    except KeyboardInterrupt:
        print("\n👋 Shutting down...")
        server.shutdown()


if __name__ == '__main__':
    main()
