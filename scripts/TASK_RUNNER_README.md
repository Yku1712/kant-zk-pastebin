# Task Runner - Kant Pastebin

**Status:** proposed · **Date:** 2026-10-03

## Overview

Task Runner integrates **prefetch** + **lean-ctx** + **shmem** for task preparation.

- **Prefetch**: Load all related context before task execution
- **lean-ctx**: Use lean-ctx to manage task context
- **shmem**: Use IPLD CAR memory for fast task context retrieval
- **Context Runner**: Dedicated runner for task context
- **Planner/Scheduler**: Use dasl-planner for task coordination

## Files

| File | Purpose |
|------|---------|
| `task-runner.sh` | Main entry point |
| `task-context-runner.sh` | Load task context via lean-ctx + shmem |
| `task-prefetcher.sh` | Prefetch related tasks |
| `task-scheduler-integration.sh` | Integrate with dasl-planner |

## Usage

```bash
# Run full task runner
./task-runner.sh

# Or run individual components
./task-context-runner.sh load <task_id>
./task-prefetcher.sh
./task-scheduler-integration.sh
```

## Key Features

- **Fast Context Loading**: Tasks prefetched via shmem (IPLD CAR)
- **lean-ctx Integration**: Context managed by lean-ctx
- **Git Context**: Old git history loaded for each task
- **Related Tasks**: Automatically prefetch related tasks
- **Planner Coordination**: DASL planner integrates with task runner
- **Scalable**: Handles multiple tasks concurrently

## Example

```bash
# Load a specific task context
./task-context-runner.sh load 2026-10-03-pastebin-kant-pastebin-build-workflow

# Prefetch all related tasks
./task-prefetcher.sh

# Get task planning status
./task-scheduler-integration.sh
```

## Task Context Components

Each task context includes:

1. **Chats**: Related conversation history from shmem
2. **Git Context**: Git log, branches, and commits for the task
3. **Related Tasks**: Other tasks in the same area
4. **Planner State**: Current DASL planner status
5. **Shmem Metadata**: IPLD CAR memory state

## Integration with Lean-ctx

- Context stored in lean-ctx memory
- Shmem used for fast retrieval
- Task runner manages context lifecycle
- lean-ctx provides context management APIs

## Integration with Dasl-Planner

- Scheduler coordination
- Task planning
- Shmem/CAR integration
- IPLD mapping support

## Future Enhancements

- Parallel task prefetching
- Machine learning for task prediction
- Dynamic task scheduling
- Real-time context updates