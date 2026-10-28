#!/bin/bash
# Enhanced shmem call wrapper
# Captures full context (PID, cwd, git info, fd info) for each shmem daemon call
# Usage: shmem-call-wrapper.sh <command> [args...]
# Example: shmem-call-wrapper.sh query project/
#          shmem-call-wrapper.sh get project/twitterstorm/scheduler-merge-task

SHMEM_BINARY="/home/mdupont/dasl/ipld-car-ipc-shmem-linux/target/release/letta-ipld-memory"
LOGFILE="/mnt/data1/kant/pastebin/shmem-context-logs/shmem-call-context.log"
CONTEXT_DIR="/mnt/data1/kant/pastebin/shmem-context-logs"

mkdir -p "$CONTEXT_DIR"

# Capture context for this call
TIMESTAMP=$(date -u +"%Y-%m-%dT%H:%M:%SZ")
CALLER_PID=$$
CALLER_CWD=$(pwd)
CALLER_UID=$(id -u)
CALLER_GID=$(id -g)
CALLER_USER=$(whoami)
SHMEM_CMD="$*"

# Git context
GIT_REPO=$(git rev-parse --show-toplevel 2>/dev/null)
GIT_BRANCH=$(git rev-parse --abbrev-ref HEAD 2>/dev/null)
GIT_COMMIT=$(git rev-parse HEAD 2>/dev/null)
GIT_DIRTY_COUNT=$(git status --porcelain 2>/dev/null | wc -l)
GIT_DIRTY_FILES=$(git status --porcelain 2>/dev/null | head -n 10)

# File descriptor count
FD_COUNT=$(ls /proc/$CALLER_PID/fd/ 2>/dev/null | wc -l)

# Disk space in current directory
DISK_SPACE=$(df -h . 2>/dev/null | tail -n 1)

# System load
SYSTEM_LOAD=$(uptime 2>/dev/null)

# Write context entry
{
    echo "========================================="
    echo "CALL_ID: $TIMESTAMP-$CALLER_PID"
    echo "TIMESTAMP: $TIMESTAMP"
    echo "PID: $CALLER_PID"
    echo "UID: $CALLER_UID"
    echo "GID: $CALLER_GID"
    echo "USER: $CALLER_USER"
    echo "CWD: $CALLER_CWD"
    echo "COMMAND: $SHMEM_CMD"
    echo "GIT_REPO: ${GIT_REPO:-N/A}"
    echo "GIT_BRANCH: ${GIT_BRANCH:-N/A}"
    echo "GIT_COMMIT: ${GIT_COMMIT:-N/A}"
    echo "GIT_DIRTY_COUNT: $GIT_DIRTY_COUNT"
    echo "GIT_DIRTY_FILES: ${GIT_DIRTY_FILES:-none}"
    echo "FD_COUNT: $FD_COUNT"
    echo "DISK_SPACE: ${DISK_SPACE:-N/A}"
    echo "SYSTEM_LOAD: ${SYSTEM_LOAD:-N/A}"
    echo "---"
} >> "$LOGFILE"

# Execute the actual shmem command and capture the result
RESULT=$("$SHMEM_BINARY" "$@" 2>&1)
EXIT_CODE=$?

# Log the result summary
{
    echo "EXIT_CODE: $EXIT_CODE"
    echo "RESULT_SUMMARY:"
    # Try to summarize JSON output
    if echo "$RESULT" | python3 -c "import json,sys; json.load(sys.stdin)" 2>/dev/null; then
        RESULT_TYPE=$(echo "$RESULT" | python3 -c "
import json, sys
d = json.load(sys.stdin)
if isinstance(d, list):
    print(f'list_of_{len(d)}_items')
elif isinstance(d, dict):
    blocks = d.get('blocks', [])
    print(f'json_object_blocks_{len(blocks)}')
else:
    print(f'json_other_{len(str(d))}_chars')
" 2>/dev/null)
        echo "  Type: $RESULT_TYPE"
        echo "  Size: $(echo "$RESULT" | wc -c) bytes"
    else
        # Non-JSON or error output
        FIRST_LINE=$(echo "$RESULT" | head -n 1)
        echo "  Type: text_output"
        echo "  Size: $(echo "$RESULT" | wc -c) bytes"
        echo "  First line: ${FIRST_LINE:0:200}"
    fi
    echo "========================================="
    echo ""
} >> "$LOGFILE"

# Also output result to stdout for normal usage
echo "$RESULT"
exit $EXIT_CODE
