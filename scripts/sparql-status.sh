#!/bin/bash
set -e

# SPARQL Status Script - Check SPARQL server integration status
TASK_CONTEXT_DIR="/mnt/data1/kant/pastebin/scripts/task-contexts"
SPARQL_ENDPOINT="${SPARQL_ENDPOINT:-http://127.0.0.1:8088/api/sparql}"
SPARQL_UI="${SPARQL_UI:-http://127.0.0.1:8088}"

echo "=== SPARQL Server Integration Status ==="
echo ""
echo "Endpoint: $SPARQL_ENDPOINT"
echo "Web UI: $SPARQL_UI"
echo ""

# Check if SPARQL server is reachable
echo "📡 SPARQL Server Connectivity:"
if command -v curl &>/dev/null; then
    status=$(curl -s -o /dev/null -w "%{http_code}" "$SPARQL_UI" 2>/dev/null || echo "000")
    echo "  HTTP Status: $status"
    if [[ "$status" == "200" ]]; then
        echo "  ✅ Server reachable"
    else
        echo "  ⚠️  Server not reachable"
    fi
else
    echo "  curl not available"
fi
echo ""

# Count task contexts
echo "📊 Task Contexts:"
task_count=$(ls -1 "$TASK_CONTEXT_DIR" 2>/dev/null | wc -l)
echo "  Total task contexts: $task_count"
echo ""

# Test SPARQL query
if [[ "$status" == "200" ]]; then
    echo "🧪 SPARQL Query Test:"
    echo "  Running: SELECT * WHERE { ?s ?p ?o } LIMIT 5"
    result=$(curl -s -X POST "$SPARQL_ENDPOINT" \
        -H "Content-Type: application/json" \
        -d '{"query":"SELECT * WHERE { ?s ?p ?o } LIMIT 5"}' 2>/dev/null)
    echo "$result" | python3 -c "
import sys, json
data = json.load(sys.stdin)
if data.get('success'):
    rows = data.get('rows', [])
    print(f'  ✅ Query executed successfully')
    print(f'  Result rows: {len(rows)}')
    print(f'  Variables: {data.get(\"variables\", [])}')
else:
    print(f'  ⚠️  Query failed: {data.get(\"error\", \"unknown\")}')" 2>/dev/null || echo "  ⚠️  Query test failed"
fi
echo ""

# Show task context details
echo "📋 Task Context Details:"
for task_dir in $(ls -1 "$TASK_CONTEXT_DIR" 2>/dev/null | head -15); do
    task_path="$TASK_CONTEXT_DIR/$task_dir"
    if [[ -d "$task_path" ]]; then
        components=""
        for f in shmem_chats.json git_history.txt related_tasks.json planner_status.txt recent_commits.txt metadata.json; do
            if [[ -s "$task_path/$f" ]]; then
                components+="$f "
            fi
        done
        size=$(du -sh "$task_path" 2>/dev/null | cut -f1)
        echo "  $task_dir ($size): $components"
    fi
done
echo ""

echo "🔧 Scripts Available:"
echo "  sparql-query.sh - Execute SPARQL queries"
echo "  sparql-import.sh - Import task contexts into SPARQL"
echo "  sparql-status.sh - Check SPARQL status"
echo "  task-context-runner.sh - Load task context"
echo "  task-prefetcher.sh - Prefetch related tasks"
echo "  task-scheduler-integration.sh - Dasl-planner integration"
echo "  task-runner.sh - Main entry point"
echo ""

echo "📦 MCP Tools Status:"
echo "  lean-ctx addon shmem@0.1.0: INSTALLED"
echo "  - shmem_stats"
echo "  - shmem_car_search"
echo "  - shmem_car_list"
echo "  - shmem_blocks_query"
echo "  - shmem_blocks_search"
echo "  SPARQL MCP tools: PENDING (Phase 4)"
echo ""

echo "✅ SPARQL status check complete"
