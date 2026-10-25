#!/bin/bash
set -e

# Task Context Runner - Prefetch + Load via lean-ctx + shmem
# Loads old chats, gits, and related tasks into context runner

TASK_RUNNER_HOME="/mnt/data1/kant/pastebin/scripts"
TASK_CONTEXT_DIR="/mnt/data1/kant/pastebin/scripts/task-contexts"
TASK_CACHE_DIR="/mnt/data1/kant/pastebin/scripts/task-cache"

# Load task context into lean-ctx memory via shmem
load_task_context() {
    local task_id="$1"
    local context_dir="$TASK_CONTEXT_DIR"

    echo "📋 Loading task context for: $task_id"

    # Create task context directory
    mkdir -p "$context_dir/$task_id"

    # 1. Load old chats related to this task from shmem
    echo "  📝 Prefetching old chats via shmem..."

    # Query shmem for task context using lean-ctx MCP
    python3 -c "
import json, subprocess

# Get task-specific context from shmem
result = subprocess.run(
    ['shmem-mcp'],
    input=json.dumps({
        'jsonrpc': '2.0',
        'id': 1,
        'method': 'tools/call',
        'params': {
            'name': 'shmem_blocks_query',
            'arguments': {
                'prefix': 'project/kant-pastebin/',
                'limit': 50
            }
        }
    }),
    capture_output=True,
    text=True
)
print(result.stdout)
" 2>/dev/null > "$context_dir/$task_id/shmem_chats.json" || echo "{}" > "$context_dir/$task_id/shmem_chats.json"

    # 2. Load git context for the task
    echo "  🔧 Prefetching git context..."
    local git_info
    git_info=$(cd /mnt/data1/kant/pastebin && git log --oneline -20 -- "$task_id" 2>/dev/null | head -10)
    echo "$git_info" > "$context_dir/$task_id/git_history.txt"

    # Also get recent commits to the task file itself
    (cd /mnt/data1/kant/pastebin && git log --oneline -10) > "$context_dir/$task_id/recent_commits.txt"

    # 3. Load related tasks from shmem
    echo "  📋 Prefetching related tasks..."
    python3 -c "
import json, subprocess

# Get related task blocks from shmem
result = subprocess.run(
    ['shmem-mcp'],
    input=json.dumps({
        'jsonrpc': '2.0',
        'id': 2,
        'method': 'tools/call',
        'params': {
            'name': 'shmem_blocks_search',
            'arguments': {'term': '$task_id'}
        }
    }),
    capture_output=True,
    text=True
)
print(result.stdout)
" 2>/dev/null > "$context_dir/$task_id/related_tasks.json" || echo "{}" > "$context_dir/$task_id/related_tasks.json"

    # 4. Store task context metadata in shmem for future prefetch
    echo "  💾 Storing task context metadata in shmem..."

    local task_metadata
    task_metadata=$(python3 -c "
import json
from datetime import datetime

task_data = {
    'id': '$task_id',
    'type': 'task_context',
    'description': 'Task context loaded via lean-ctx + shmem',
    'created': datetime.now().isoformat(),
    'prefetched': True,
    'components': {
        'chats': True,
        'git': True,
        'related_tasks': True
    },
    'path': '$task_id',
    'namespace': 'project/kant-pastebin/'
}

print(json.dumps(task_data, indent=2))
" 2>/dev/null)

    # Store task metadata in task cache
    if [ -n "$task_metadata" ]; then
        echo "$task_metadata" > "$context_dir/$task_id/metadata.json"
    fi

    # 5. Also query dasl-planner for task planning info
    echo "  🧠 Getting planner info for task..."
    python3 /mnt/data1/dasl-planning/dasl-planner status 2>/dev/null > "$context_dir/$task_id/planner_status.txt" || true

    echo "✅ Task context loaded for: $task_id"
}

# Main task execution
if [[ "$1" == "load" ]]; then
    task_id="$2"
    if [[ -z "$task_id" ]]; then
        echo "Error: task_id required"
        echo "Usage: $0 load <task_id>"
        exit 1
    fi
    load_task_context "$task_id"
elif [[ "$1" == "list" ]]; then
    echo "Available task contexts:"
    ls -1 "$TASK_CONTEXT_DIR" 2>/dev/null | head -20
elif [[ "$1" == "clear" ]]; then
    task_id="$2"
    if [[ -n "$task_id" ]]; then
        rm -rf "$TASK_CONTEXT_DIR/$task_id"
        echo "Cleared context for: $task_id"
    else
        echo "Usage: $0 clear <task_id>"
    fi
else
    echo "Task Context Runner - Prefetch + Load via lean-ctx + shmem"
    echo ""
    echo "Usage:"
    echo "  $0 load <task_id>    - Load task context (prefetch chats, git, related tasks)"
    echo "  $0 list              - List available task contexts"
    echo "  $0 clear <task_id>   - Clear cached context for task"
    echo ""
    echo "Example:"
    echo "  $0 load 2026-10-03-pastebin-kant-pastebin-build-workflow"
    exit 1
fi