#!/bin/bash
# shmem-car-list.sh - List stored CAR CIDs via lean-ctx MCP

set -e

LIMIT="${1:-"10"}"

echo "📋 Listing CAR CIDs (limit: $LIMIT)..."

# Call shmem_car_list via shmem-mcp
result=$(echo "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/call\",\"params\":{\"name\":\"shmem_car_list\",\"arguments\":{\"limit\":$LIMIT}}}" | shmem-mcp 2>/dev/null)

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