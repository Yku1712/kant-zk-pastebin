#!/bin/bash
# shmem-car-search.sh - Search stored CAR pages via lean-ctx MCP

set -e

QUERY="${1:-""}"

if [ -z "$QUERY" ]; then
    echo "Usage: $0 <search_query>"
    echo "Example: $0 scheduler"
    exit 1
fi

echo "🔍 Searching CAR pages for: $QUERY..."

# Call shmem_car_search via shmem-mcp
result=$(echo "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/call\",\"params\":{\"name\":\"shmem_car_search\",\"arguments\":{\"query\":\"$QUERY\"}}}" | shmem-mcp 2>/dev/null)

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