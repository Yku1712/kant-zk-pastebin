#!/bin/bash
set -e

# SPARQL Status Script - Check SPARQL server integration status
TASK_CONTEXT_DIR="/mnt/data1/kant/pastebin/scripts/task-contexts"
SPARQL_ENDPOINT="${SPARQL_ENDPOINT:-http://localhost:3030/task-context/query}"

echo "=== SPARQL Server Integration Status ==="
echo ""
echo "Endpoint: $SPARQL_ENDPOINT"
echo ""

# Check if SPARQL server is reachable
echo "📡 SPARQL Server Connectivity:"
if command -v curl &>/dev/null; then
    status=$(curl -s -o /dev/null -w "%{http_code}" "$SPARQL_ENDPOINT" 2>/dev/null || echo "000")
    echo "  HTTP Status: $status"
    if [[ "$status" == "200" ]]; then
        echo "  ✅ Server reachable"
    else
        echo "  ⚠️  Server not reachable (expected if not installed)"
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

# Show context details
echo "📋 Task Context Details:"
for task_dir in $(ls -1 "$TASK_CONTEXT_DIR" 2>/dev/null | head -10); do
    task_path="$TASK_CONTEXT_DIR/$task_dir"
    if [[ -d "$task_path" ]]; then
        components=""
        for f in shmem_chats.json git_history.txt related_tasks.json planner_status.txt recent_commits.txt metadata.json; do
            if [[ -s "$task_path/$f" ]]; then
                components+="$f "
            fi
        done
        echo "  $task_dir: $components"
    fi
done
echo ""

echo "🔧 Scripts Available:"
echo "  sparql-query.sh - Execute SPARQL queries"
echo "  sparql-import.sh - Import task contexts into SPARQL"
echo "  sparql-status.sh - Check SPARQL status"
echo ""

echo "📦 MCP Tools Status:"
echo "  lean-ctx addon shmem@0.1.0: INSTALLED"
echo "  SPARQL MCP tools: PENDING (Phase 4)"
echo ""

echo "✅ SPARQL status check complete"
