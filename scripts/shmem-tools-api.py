#!/usr/bin/env python3
"""Shmem Tools HTTP API - Exposes all shmem-mcp functions via HTTP"""
import http.server
import json
import subprocess
import os
import sys

PORT = int(os.environ.get('PORT', 8153))

class ShmemToolsHandler(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        path = self.path.split('?')[0]  # Remove query string for path matching
        
        if path == '/':
            self.send_response(200)
            self.send_header('Content-Type', 'application/json')
            self.end_headers()
            response = {
                'service': 'shmem-tools-api',
                'version': '1.0.0',
                'endpoints': {
                    'GET /stats': 'Get shmem statistics',
                    'GET /car/list': 'List CAR CIDs',
                    'GET /car/search?q=QUERY': 'Search CAR pages',
                    'GET /blocks/query?prefix=PREFIX&limit=N': 'Query blocks by prefix',
                    'GET /blocks/search?q=TERM': 'Search blocks by description'
                }
            }
            self.wfile.write(json.dumps(response).encode())
            return
            
        elif path == '/stats':
            self.send_response(200)
            self.send_header('Content-Type', 'application/json')
            self.end_headers()
            try:
                result = subprocess.run(
                    ['/home/mdupont/.nix-profile/bin/shmem-mcp'],
                    input='{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"shmem_stats","arguments":{}}}',
                    capture_output=True, text=True
                )
                self.wfile.write(result.stdout.encode() or b'{}')
            except Exception as e:
                self.wfile.write(json.dumps({"error": str(e)}).encode())
            return
            
        elif path == '/car/list':
            self.send_response(200)
            self.send_header('Content-Type', 'application/json')
            self.end_headers()
            try:
                result = subprocess.run(
                    ['/home/mdupont/.nix-profile/bin/shmem-mcp'],
                    input='{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"shmem_car_list","arguments":{"limit":10}}}',
                    capture_output=True, text=True
                )
                self.wfile.write(result.stdout.encode() or b'{}')
            except Exception as e:
                self.wfile.write(json.dumps({"error": str(e)}).encode())
            return
            
        elif path == '/car/search':
            from urllib.parse import urlparse, parse_qs
            qs = parse_qs(urlparse(self.path).query)
            q = qs.get('q', [''])[0]
            self.send_response(200)
            self.send_header('Content-Type', 'application/json')
            self.end_headers()
            try:
                result = subprocess.run(
                    ['/home/mdupont/.nix-profile/bin/shmem-mcp'],
                    input=f'{{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{{"name":"shmem_car_search","arguments":{{"query":"{q}"}}}}}}',
                    capture_output=True, text=True
                )
                self.wfile.write(result.stdout.encode() or b'{}')
            except Exception as e:
                self.wfile.write(json.dumps({"error": str(e)}).encode())
            return
            
        elif path == '/blocks/query':
            from urllib.parse import urlparse, parse_qs
            qs = parse_qs(urlparse(self.path).query)
            prefix = qs.get('prefix', ['project/'])[0]
            limit = qs.get('limit', ['50'])[0]
            self.send_response(200)
            self.send_header('Content-Type', 'application/json')
            self.end_headers()
            try:
                result = subprocess.run(
                    ['/home/mdupont/.nix-profile/bin/shmem-mcp'],
                    input=f'{{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{{"name":"shmem_blocks_query","arguments":{{"prefix":"{prefix}","limit":{limit}}}}}}}',
                    capture_output=True, text=True
                )
                self.wfile.write(result.stdout.encode() or b'{}')
            except Exception as e:
                self.wfile.write(json.dumps({"error": str(e)}).encode())
            return
            
        elif path == '/blocks/search':
            from urllib.parse import urlparse, parse_qs
            qs = parse_qs(urlparse(self.path).query)
            term = qs.get('q', ['task'])[0]
            self.send_response(200)
            self.send_header('Content-Type', 'application/json')
            self.end_headers()
            try:
                result = subprocess.run(
                    ['/home/mdupont/.nix-profile/bin/shmem-mcp'],
                    input=f'{{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{{"name":"shmem_blocks_search","arguments":{{"term":"{term}"}}}}}}',
                    capture_output=True, text=True
                )
                self.wfile.write(result.stdout.encode() or b'{}')
            except Exception as e:
                self.wfile.write(json.dumps({"error": str(e)}).encode())
            return
            
        else:
            self.send_response(404)
            self.send_header('Content-Type', 'application/json')
            self.end_headers()
            self.wfile.write(json.dumps({'error': 'not found'}).encode())
            return
    
    def log_message(self, *args):
        pass  # Suppress logs

def main():
    server_address = ('0.0.0.0', PORT)
    httpd = http.server.HTTPServer(server_address, ShmemToolsHandler)
    print(f'✅ Shmem Tools API running on http://0.0.0.0:{PORT}', flush=True)
    try:
        httpd.serve_forever()
    except KeyboardInterrupt:
        pass
    httpd.server_close()

if __name__ == '__main__':
    main()
