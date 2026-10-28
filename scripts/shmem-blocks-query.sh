#!/bin/bash
# shmem-blocks-query.sh - Query memory blocks by path prefix via lean-ctx MCP

set -e

PREFIX="${1:-"project/"}"
LIMIT="${2:-"50"}"

echo "🔎 Querying blocks with prefix: $PREFIX (limit: $LIMIT)..."

# Call shmem_blocks_query via shmem-mcp
result=$(echo "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/call\",\"params\":{\"name\":\"shmem_blocks_query\",\"arguments\":{\"prefix\":\"$PREFIX\",\"limit\":$LIMIT}}}" | shmem-mcp 2>/dev/null)

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