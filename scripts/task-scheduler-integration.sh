#!/bin/bash
set -e

# Task Scheduler Integration - Use dasl-planner for task coordination
# Integrates task runner with shmem, lean-ctx, and dasl-planner

TASK_RUNNER_HOME="/mnt/data1/kant/pastebin/scripts"
PLANNER_BIN="/mnt/data1/dasl-planning/dasl-planner"

echo "🧠 Getting DASL planner task status..."

if [[ -f "$PLANNER_BIN" ]]; then
    echo "  Getting planner status..."
    "$PLANNER_BIN" status 2>&1 | head -100

    echo ""
    echo "🚀 Running OODA cycle for task planning..."
    "$PLANNER_BIN" ooda 2>&1 | head -150

    echo ""
    echo "📋 Getting shmem planner state..."
    "$PLANNER_BIN" shmem 2>&1

    echo ""
    echo "📊 Getting IPLD → shmem mapping..."
    "$PLANNER_BIN" ipld 2>&1

    echo ""
    echo "✅ Task scheduler integration complete"
else
    echo "⚠️  Dasl-planner binary not found: $PLANNER_BIN"
fi
