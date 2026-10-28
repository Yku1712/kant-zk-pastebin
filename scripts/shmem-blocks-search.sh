#!/bin/bash
# shmem-blocks-search.sh - Search memory blocks by description via lean-ctx MCP

set -e

TERM="${1:-"task"}"

echo "🔍 Searching blocks by description for: $TERM..."

# Call shmem_blocks_search via shmem-mcp
result=$(echo "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/call\",\"params\":{\"name\":\"shmem_blocks_search\",\"arguments\":{\"term\":\"$TERM\"}}}" | shmem-mcp 2>/dev/null)

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