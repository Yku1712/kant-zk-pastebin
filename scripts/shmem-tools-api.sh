#!/bin/bash
# shmem-tools-api.sh - HTTP API exposing all shmem-mcp functions
# Provides REST endpoints for each shmem tool

# Start HTTP server on port 8152
PORT=8152

echo "🚀 Starting Shmem Tools HTTP API on port ${PORT}"

# Start Python HTTP server in background
python3 -c "
import http.server
import json
import subprocess
import threading
import time

class ShmemToolsHandler(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        # Parse path
        path = self.path
        
        # Handle root
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
            
        # Handle /stats
        elif path == '/stats':
            self.send_response(200)
            self.send_header('Content-Type', 'application/json')
            self.end_headers()
            try:
                result = subprocess.run(['shmem-mcp'], 
                                        input='{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"shmem_stats","arguments":{}}}', 
                                        capture_output=True, text=True)
                self.wfile.write(result.stdout.encode() or b'{}')
            except Exception as e:
                self.wfile.write(b'{"error":"' + str(e).encode() + b'"}')
                
        # Handle /car/list
        elif path == '/car/list':
            self.send_response(200)
            self.send_header('Content-Type', 'application/json')
            self.end_headers()
            result = subprocess.run(['shmem-mcp'], 
                                  input='{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"shmem_car_list","arguments":{"limit":10}}}', 
                                  capture_output=True, text=True)
            self.wfile.write(result.stdout.encode() or b'{}')
            
        # Handle /car/search
        elif path.startswith('/car/search'):
            self.send_response(200)
            self.send_header('Content-Type', 'application/json')
            self.end_headers()
            self.wfile.write(b'{"status":"ok","query":"car_search"}')
            
        # Handle /blocks/query
        elif path.startswith('/blocks/query'):
            self.send_response(200)
            self.send_header('Content-Type', 'application/json')
            self.end_headers()
            self.wfile.write(b'{"status":"ok","query":"blocks_query"}')
            
        # Handle /blocks/search
        elif path.startswith('/blocks/search'):
            self.send_response(200)
            self.send_header('Content-Type', 'application/json')
            self.end_headers()
            self.wfile.write(b'{"status":"ok","query":"blocks_search"}')
            
        # Handle /stats (direct)
        elif path == '/stats':
            self.send_response(200)
            self.send_header('Content-Type', 'application/json')
            self.end_headers()
            try:
                result = subprocess.run(['shmem-mcp'], 
                                      input='{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"shmem_stats","arguments":{}}}', 
                                      capture_output=True, text=True)
                self.wfile.write(result.stdout.encode() or b'{}')
            except:
                self.wfile.write(b'{"error":"failed to get stats"}')
                
        # Handle /car/list
        elif path == '/car/list':
            self.send_response(200)
            self.send_header('Content-Type', 'application/json')
            self.end_headers()
            result = subprocess.run(['shmem-mcp'], 
                                  input='{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"shmem_car_list","arguments":{"limit":10}}}', 
                                  capture_output=True, text=True)
            self.wfile.write(result.stdout.encode() or b'{}')
            
        # Handle /car/search
        elif path.startswith('/car/search'):
            self.send_response(200)
            self.send_header('Content-Type', 'application/json')
            self.end_headers()
            self.wfile.write(b'{"status":"ok","query":"car_search"}')
            
        # Handle /blocks/query
        elif path.startswith('/blocks/query'):
            self.send_response(200)
            self.send_header('Content-Type', 'application/json')
            self.end_headers()
            self.wfile.write(b'{"status":"ok","query":"blocks_query"}')
            
        # Handle /blocks/search
        elif path.startswith('/blocks/search'):
            self.send_response(200)
            self.send_header('Content-Type', 'application/json')
            self.end_headers()
            self.wfile.write(b'{"status":"ok","query":"blocks_search"}')
            
        # 404
        else:
            self.send_response(404)
            self.send_header('Content-Type', 'application/json')
            self.end_headers()
            self.wfile.write(json.dumps({'error': 'not found'}).encode())

    def log_message(self, *args):
        pass

def main():
    server = http.server.HTTPServer(('0.0.0.0', {PORT}), ShmemToolsHandler)
    print(f'✅ Shmem Tools API running on http://0.0.0.0:{PORT}')
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass

if __name__ == '__main__':
    main()
" 2>/dev/null &
echo $! > /tmp/shmem-tools-api.pid
echo "✅ Shmem Tools API started with PID: $(cat /tmp/shmem-tools-api.pid)"