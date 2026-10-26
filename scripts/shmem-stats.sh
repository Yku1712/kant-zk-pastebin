#!/bin/bash
# shmem-stats.sh - Get IPLD CAR shmem statistics via lean-ctx MCP

set -e

echo "📊 Getting shmem statistics via lean-ctx MCP..."

# Call shmem_stats via shmem-mcp
result=$(echo '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"shmem_stats","arguments":{}}}' | shmem-mcp 2>/dev/null)

if [ $? -eq 0 ]; then
    echo "$result" | python3 -c "
import sys, json
try:
    data = json.load(sys.stdin)
    if data.get('isError'):
        print('Error: ' + data.get('error', 'unknown'))
    else:
        content = data.get('result', {}).get('content', [{}])[0].get('text', '')
        print(content)
except:
    print(data)
"
else
    echo "⚠️  shmem-mcp not available"
fi