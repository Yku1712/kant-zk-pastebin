#!/bin/bash
set -e

# SPARQL Query Script - Execute SPARQL queries against the running SPARQL server

SPARQL_ENDPOINT="${SPARQL_ENDPOINT:-http://127.0.0.1:8088/api/sparql}"

show_help() {
    echo "SPARQL Query Script"
    echo ""
    echo "Usage:"
    echo "  $0 <query>                  - Execute SPARQL query via HTTP API"
    echo "  $0 --query <query>          - Execute SPARQL query"
    echo "  $0 --test                   - Run test query"
    echo "  $0 --list-blocks            - List blocks as SPARQL results"
    echo "  $0 --task-stats             - Get task statistics as SPARQL results"
    echo ""
    echo "Examples:"
    echo "  $0 'SELECT * WHERE { ?s ?p ?o } LIMIT 5'"
    echo "  $0 --test"
}

if [[ "$1" == "--help" ]]; then
    show_help
    exit 0
elif [[ "$1" == "--test" ]]; then
    echo "🧪 Running test SPARQL query..."
    query="SELECT * WHERE { ?s ?p ?o } LIMIT 5"
    echo "Query: $query"
    echo ""
    result=$(curl -s -X POST "$SPARQL_ENDPOINT" \
        -H "Content-Type: application/json" \
        -d "{\"query\":\"$query\"}" 2>/dev/null)
    echo "$result" | python3 -m json.tool 2>/dev/null || echo "$result"
    echo ""
    echo "✅ Test query complete"
elif [[ "$1" == "--list-blocks" ]]; then
    echo "📋 Listing blocks via SPARQL..."
    query="SELECT ?path ?description ?size WHERE { ?s <http://dasl.org/letta/memory#path> ?path . ?s <http://dasl.org/letta/memory#description> ?description . ?s <http://dasl.org/letta/memory#size> ?size . }"
    echo "Query: SELECT paths, descriptions, sizes"
    result=$(curl -s -X POST "$SPARQL_ENDPOINT" \
        -H "Content-Type: application/json" \
        -d "{\"query\":\"$query\"}" 2>/dev/null)
    echo "$result" | python3 -c "
import sys, json
data = json.load(sys.stdin)
if data.get('success'):
    rows = data.get('rows', [])
    print(f'  Total blocks: {len(rows)}')
    for row in rows[:10]:
        print(f'  {row[0]} ({row[1]}) - {row[2]}')
else:
    print(f'Error: {data.get(\"error\", \"unknown\")}')" 2>/dev/null || echo "$result"
elif [[ "$1" == "--task-stats" ]]; then
    echo "📊 Task Statistics via SPARQL..."
    query="SELECT ?task ?type ?created WHERE { ?task <http://task-runner.kant/type> ?type . ?task <http://task-runner.kant/created> ?created . }"
    echo "Query: Task metadata"
    result=$(curl -s -X POST "$SPARQL_ENDPOINT" \
        -H "Content-Type: application/json" \
        -d "{\"query\":\"$query\"}" 2>/dev/null)
    echo "$result" | python3 -m json.tool 2>/dev/null || echo "$result"
elif [[ -n "$1" ]]; then
    echo "🔍 Executing SPARQL query..."
    echo "Query: $1"
    echo ""
    result=$(curl -s -X POST "$SPARQL_ENDPOINT" \
        -H "Content-Type: application/json" \
        -d "{\"query\":\"$1\"}" 2>/dev/null)
    echo "$result" | python3 -m json.tool 2>/dev/null || echo "$result"
else
    show_help
    exit 1
fi
