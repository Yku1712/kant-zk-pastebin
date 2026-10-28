#!/bin/bash
# Enhanced shmem call context collector
# Captures full context for each daemon call: PID, cwd, git info, fd info, etc.

LOGFILE="/mnt/data1/kant/pastebin/shmem-context-logs/shmem-call-context.log"
CONTEXT_DIR="/mnt/data1/kant/pastebin/shmem-context-logs"

# Ensure log directory exists
mkdir -p "$CONTEXT_DIR"

# Function to capture full context for a shmem command
capture_context() {
    local cmd="$1"
    local timestamp=$(date -u +"%Y-%m-%dT%H:%M:%SZ")
    local my_pid=$$
    
    echo "=========================================" >> "$LOGFILE"
    echo "TIMESTAMP: $timestamp" >> "$LOGFILE"
    echo "COLLECTOR_PID: $my_pid" >> "$LOGFILE"
    echo "CALLING_COMMAND: $cmd" >> "$LOGFILE"
    echo "CALLING_CWD: $(pwd)" >> "$LOGFILE"
    echo "COLLECTOR_UID: $(id -u)" >> "$LOGFILE"
    echo "COLLECTOR_GID: $(id -g)" >> "$LOGFILE"
    echo "" >> "$LOGFILE"
    
    # Git context (if in a git repo)
    if git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
        echo "GIT_REPO: $(git rev-parse --show-toplevel)" >> "$LOGFILE"
        echo "GIT_BRANCH: $(git rev-parse --abbrev-ref HEAD)" >> "$LOGFILE"
        echo "GIT_COMMIT: $(git rev-parse HEAD)" >> "$LOGFILE"
        echo "GIT_DIRTY:" >> "$LOGFILE"
        git status --porcelain 2>/dev/null | while IFS= read -r line; do
            echo "  $line" >> "$LOGFILE"
        done
        echo "" >> "$LOGFILE"
    else
        echo "GIT_REPO: not in a git repository" >> "$LOGFILE"
        echo "" >> "$LOGFILE"
    fi
    
    # Environment variables (relevant ones only)
    echo "ENV_CONTEXT:" >> "$LOGFILE"
    echo "  HOME: $HOME" >> "$LOGFILE"
    echo "  PWD: $PWD" >> "$LOGFILE"
    echo "  USER: $USER" >> "$LOGFILE"
    echo "  LOGNAME: $LOGNAME" >> "$LOGFILE"
    echo "" >> "$LOGFILE"
    
    # File descriptors for current process
    echo "FILE_DESCRIPTORS:" >> "$LOGFILE"
    ls -la /proc/$my_pid/fd/ 2>/dev/null | head -n 20 >> "$LOGFILE"
    echo "" >> "$LOGFILE"
    
    # Disk usage info
    echo "DISK_USAGE:" >> "$LOGFILE"
    df -h . 2>/dev/null >> "$LOGFILE"
    echo "" >> "$LOGFILE"
    
    # System load
    echo "SYSTEM_LOAD:" >> "$LOGFILE"
    uptime >> "$LOGFILE"
    echo "" >> "$LOGFILE"
    
    echo "END_OF_CONTEXT" >> "$LOGFILE"
    echo "" >> "$LOGFILE"
}

# Execute the context capture for the current call
capture_context "$@"

# Return exit code 0 (this is just context logging, not the actual call)
exit 0
