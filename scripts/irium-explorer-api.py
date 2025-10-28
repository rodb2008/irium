#!/usr/bin/env python3
"""Irium Blockchain Explorer API."""

import sys
import os
import json
from http.server import HTTPServer, BaseHTTPRequestHandler
from urllib.parse import urlparse, parse_qs

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from irium.rate_limiter import RateLimiter

# Initialize rate limiter
rate_limiter = RateLimiter(requests_per_minute=120)  # 120 requests per minute

BLOCKCHAIN_DIR = os.path.expanduser("~/.irium/blocks")
MEMPOOL_DIR = os.path.expanduser("~/.irium/mempool")


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
            if not os.path.exists(BLOCKCHAIN_DIR):
                self.send_json_response({'blocks': [], 'total': 0})
                return
            
            # Get all block files
            block_files = sorted(
                [f for f in os.listdir(BLOCKCHAIN_DIR) if f.startswith('block_')],
                key=lambda x: int(x.split('_')[1].split('.')[0]),
                reverse=True
            )
            
            # Pagination
            page = int(query.get('page', [1])[0])
            per_page = int(query.get('per_page', [10])[0])
            start = (page - 1) * per_page
            end = start + per_page
            
            blocks = []
            for block_file in block_files[start:end]:
                with open(os.path.join(BLOCKCHAIN_DIR, block_file), 'r') as f:
                    blocks.append(json.load(f))
            
            self.send_json_response({
                'blocks': blocks,
                'total': len(block_files),
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
            
            # Search for block
            for block_file in os.listdir(BLOCKCHAIN_DIR):
                if not block_file.startswith('block_'):
                    continue
                
                with open(os.path.join(BLOCKCHAIN_DIR, block_file), 'r') as f:
                    block_data = json.load(f)
                    if block_data['hash'] == block_hash:
                        self.send_json_response({'block': block_data})
                        return
            
            self.send_json_response({'error': 'Block not found'}, 404)
        
        except Exception as e:
            self.send_json_response({'error': str(e)}, 500)
    
    def get_block_by_height(self, height):
        """Get block by height."""
        try:
            block_file = os.path.join(BLOCKCHAIN_DIR, f'block_{height}.json')
            
            if not os.path.exists(block_file):
                self.send_json_response({'error': 'Block not found'}, 404)
                return
            
            with open(block_file, 'r') as f:
                block_data = json.load(f)
            
            self.send_json_response({'block': block_data})
        
        except Exception as e:
            self.send_json_response({'error': str(e)}, 500)
    
    def get_latest_blocks(self, query):
        """Get latest N blocks."""
        try:
            count = int(query.get('count', [10])[0])
            
            if not os.path.exists(BLOCKCHAIN_DIR):
                self.send_json_response({'blocks': []})
                return
            
            block_files = sorted(
                [f for f in os.listdir(BLOCKCHAIN_DIR) if f.startswith('block_')],
                key=lambda x: int(x.split('_')[1].split('.')[0]),
                reverse=True
            )
            
            blocks = []
            for block_file in block_files[:count]:
                with open(os.path.join(BLOCKCHAIN_DIR, block_file), 'r') as f:
                    blocks.append(json.load(f))
            
            self.send_json_response({'blocks': blocks})
        
        except Exception as e:
            self.send_json_response({'error': str(e)}, 500)
    
    def get_stats(self):
        """Get blockchain statistics."""
        try:
            if not os.path.exists(BLOCKCHAIN_DIR):
                self.send_json_response({
                    'height': 0,
                    'total_blocks': 0,
                    'total_supply': 0
                })
                return
            
            block_files = [f for f in os.listdir(BLOCKCHAIN_DIR) if f.startswith('block_') and f.endswith('.json') and 'backup' not in f]
            
            if not block_files:
                self.send_json_response({
                    'height': 0,
                    'total_blocks': 0,
                    'total_supply': 0
                })
                return
            
            # Get latest block
            latest_file = sorted(block_files, key=lambda x: int(x.split('_')[1].split('.')[0]))[-1]
            
            with open(os.path.join(BLOCKCHAIN_DIR, latest_file), 'r') as f:
                latest_block = json.load(f)
            
            # Calculate total supply (simplified)
            total_supply = 0
            for block_file in block_files:
                with open(os.path.join(BLOCKCHAIN_DIR, block_file), 'r') as f:
                    block_data = json.load(f)
                    total_supply += block_data.get('reward', 0)
            
            self.send_json_response({
                'height': latest_block['height'],
                'total_blocks': len(block_files),
                'total_supply': total_supply,
                'supply_irm': total_supply / 100000000,
                'latest_block': latest_block['hash'],
                'latest_block_time': latest_block.get('time', 0)
            })
        
        except Exception as e:
            self.send_json_response({'error': str(e)}, 500)
    
    def get_mempool(self):
        """Get mempool transactions."""
        try:
            mempool_file = os.path.join(MEMPOOL_DIR, 'pending.json')
            
            if not os.path.exists(mempool_file):
                self.send_json_response({'transactions': [], 'count': 0})
                return
            
            with open(mempool_file, 'r') as f:
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


def main():
    port = 8082
    if len(sys.argv) > 1:
        port = int(sys.argv[1])
    
    server = HTTPServer(('0.0.0.0', port), ExplorerAPI)
    print(f"🔗 Irium Blockchain Explorer API")
    print(f"📡 Listening on http://0.0.0.0:{port}")
    print(f"📊 API Documentation: http://localhost:{port}/")
    print()
    print("Endpoints:")
    print(f"  • http://localhost:{port}/api/stats")
    print(f"  • http://localhost:{port}/api/blocks")
    print(f"  • http://localhost:{port}/api/latest?count=10")
    print(f"  • http://localhost:{port}/api/mempool")
    print()
    
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        print("\n👋 Shutting down...")
        server.shutdown()


if __name__ == '__main__':
    main()
