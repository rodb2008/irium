#!/usr/bin/env python3
import http.server
import socketserver
import os

PORT = 8082

class LogoHandler(http.server.SimpleHTTPRequestHandler):
    def do_GET(self):
        if self.path == '/irium-logo-wallet.svg':
            self.send_response(200)
            self.send_header('Content-type', 'image/svg+xml')
            self.send_header('Access-Control-Allow-Origin', '*')
            self.end_headers()
            
            logo_path = 'irium-logo-wallet.svg'
            if os.path.exists(logo_path):
                with open(logo_path, 'r') as f:
                    self.wfile.write(f.read().encode('utf-8'))
            else:
                self.wfile.write(b'<svg><text>Logo not found</text></svg>')
        else:
            self.send_error(404, "Not Found")

if __name__ == "__main__":
    with socketserver.TCPServer(("0.0.0.0", PORT), LogoHandler) as httpd:
        print(f"Test logo server running on port {PORT}")
        print(f"Logo URL: http://207.244.247.86:{PORT}/irium-logo-wallet.svg")
        httpd.serve_forever()
