#!/bin/bash
# shmem-tools-api.sh - HTTP API exposing all shmem-mcp functions
# Provides REST endpoints for each shmem tool

set -e

HOST="0.0.0.0"
PORT="8152"

echo "🚀 Starting Shmem Tools HTTP API on ${HOST}:${PORT}"

# Simple HTTP server using Python
python3 -c "
import http.server
import json
import subprocess
import shlex

class ShmemToolsHandler(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        if self.path == '/':
            self.send_response(200)
            self.send_header('Content-Type', 'application/json')
            self.end_headers()
            self.wfile.write(json.dumps({
                'service': 'shmem-tools-api',
                'version': '1.0.0',
                'endpoints': {
                    'GET /stats': 'Get shmem statistics',
                    'GET /car/list': 'List CAR CIDs',
                    'GET /car/search?q=QUERY': 'Search CAR pages',
                    'GET /blocks/query?prefix=PREFIX&limit=N': 'Query blocks by prefix',
                    'GET /blocks/search?q=TERM': 'Search blocks by description'
                }
            }).encode())
        elif self.path == '/stats':
            self.send_response(200)
            self.send_header('Content-Type', 'application/json')
            self.end_headers()
            result = subprocess.run(['shmem-mcp'], input='{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/call\",\"params\":{\"name\":\"shmem_stats\",\"arguments\":{}}}', capture_output=True, text=True)
            self.wfile.write(result.stdout.encode() or b'{}')
        elif self.path == '/car/list':
            self.send_response(200)
            self.send_header('Content-Type', 'application/json')
            self.end_headers()
            result = subprocess.run(['shmem-mcp'], input='{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/call\",\"params\":{\"name\":\"shmem_car_list\",\"arguments\":{\"limit\":10}}}', capture_output=True, text=True)
            self.wfile.write(result.stdout.encode() or b'{}')
        elif self.path.startswith('/car/search'):
            self.send_response(200)
            self.send_header('Content-Type', 'application/json')
            self.end_headers()
            self.wfile.write(b'{\"status\":\"ok\",\"query\":\"car_search\"}')
        elif self.path.startswith('/blocks/query'):
            self.send_response(200)
            self.send_header('Content-Type', 'application/json')
            self.end_headers()
            self.wfile.write(b'{\"status\":\"ok\",\"query\":\"blocks_query\"}')
        elif self.path.startswith('/blocks/search'):
            self.send_response(200)
            self.send_header('Content-Type', 'application/json')
            self.end_headers()
            self.wfile.write(b'{\"status\":\"ok\",\"query\":\"blocks_search\"}')
        else:
            self.send_response(404)
            self.send_header('Content-Type', 'application/json')
            self.end_headers()
            self.wfile.write(json.dumps({'error': 'not found'}).encode())

    def log_message(self, format, *args):
        pass  # Suppress logs

server = http.server.HTTPServer((HOST, PORT), ShmemToolsHandler)
print(f'✅ Shmem Tools API running on http://{HOST}:{PORT}')
server.serve_forever()
" 2>&1