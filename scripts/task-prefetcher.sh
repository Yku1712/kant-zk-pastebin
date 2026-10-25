#!/bin/bash
set -e

# Task Prefetcher - Prefetch all tasks related to shmem + lean-ctx integration

TASK_RUNNER_HOME="/mnt/data1/kant/pastebin/scripts"
TASK_CONTEXT_DIR="/mnt/data1/kant/pastebin/scripts/task-contexts"

# Related tasks to prefetch (shmem + lean-ctx integration areas)
RELATED_TASKS=(
    "2026-10-03-pastebin-kant-pastebin-build-workflow"
    "2026-10-02-pastebin-cli-page-dom-shim"
    "2026-10-02-pastebin-dev-target"
    "2026-10-02-pastebin-diagpage-test-missing-name"
    "2026-10-02-pastebin-dirty-worktree-cleanup"
    "2026-10-02-pastebin-divergent-branch-triage"
    "2026-10-02-pastebin-duplicate-code-sweep"
    "2026-10-02-pastebin-invite-dead-relay"
    "2026-10-02-pastebin-manifest-chunk-index-limitation"
    "2026-10-02-pastebin-nix-ci-file-url-inputs"
    "2026-10-02-pastebin-nora-spool-history-cleanup"
)

# Prefetch all related tasks
for task in "${RELATED_TASKS[@]}"; do
    echo "🔍 Prefetching task: $task"
    "$TASK_RUNNER_HOME/task-context-runner.sh" load "$task" &
done

wait

echo "✅ All tasks prefetched and loaded into context runner"
