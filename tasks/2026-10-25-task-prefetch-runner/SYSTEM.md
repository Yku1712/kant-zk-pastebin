---
name: task-prefetch-runner
priority: HIGH
depends_on: thunk-server, do-scheduler-modes, federated-p2p-relay
---

# Task Prefetch Runner — Prefetch + Load via lean-ctx + shmem

**Status:** proposed · **Date:** 2026-10-25

## Goal

Create a dedicated task context runner that prefetches and loads all related context (old chats, gits, related tasks) for each task via lean-ctx using shmem (IPLD CAR memory), coordinated by the dasl-planner scheduler.

## Instructions

- Each task gets its own context runner directory under `scripts/task-contexts/`
- Prefetch loads: shmem chats/blocks, git history, related task blocks
- Store task context metadata in shmem for future prefetch reuse
- Integrate with dasl-planner: OODA cycle, planner status, shmem state, IPLD mapping
- Use lean-ctx MCP tools (shmem_blocks_query, shmem_blocks_search, shmem_stats) for all shmem access
- Concurrent prefetching of related tasks
- Task runner main entry point: `scripts/task-runner.sh`

## Discoveries

- shmem service: active (PID 784999), port 8150, 351,750 blocks
- lean-ctx addon shmem@0.1.0 installed with 5 MCP tools
- dasl-planner binary at /mnt/data1/dasl-planning/dasl-planner supports: status, ooda, shmem, ipld commands
- Existing task runner scripts deployed at /mnt/data1/kant/pastebin/scripts/task-*.sh
- 11 task contexts already prefetched and loaded

## Accomplished

- [x] Task runner scripts created (task-context-runner.sh, task-prefetcher.sh, task-scheduler-integration.sh, task-runner.sh)
- [x] Task context directories created for 11 related tasks
- [x] lean-ctx MCP tools verified working (shmem_stats, shmem_blocks_query, shmem_blocks_search)
- [x] Dasl-planner integration verified (status, ooda, shmem)
- [x] Git commit and push to feature/big-merge

## Not Done / In Progress

- [ ] Enhance task-context-runner.sh to load all old chats for each task
- [ ] Enhance task-context-runner.sh to load full git history + branches for each task
- [ ] Enhance task-context-runner.sh to load all related tasks via shmem search
- [ ] Add concurrent parallel prefetching with batch processing
- [ ] Add lean-ctx context storage for task runner (store contexts in lean-ctx memory)
- [ ] Add automatic task detection from git commits
- [ ] Add task runner status reporting dashboard

## Relevant Files

| File | Status | Notes |
|------|--------|-------|
| `scripts/task-context-runner.sh` | done | Load task context via lean-ctx + shmem |
| `scripts/task-prefetcher.sh` | done | Prefetch related tasks |
| `scripts/task-scheduler-integration.sh` | done | Integrate with dasl-planner |
| `scripts/task-runner.sh` | done | Main entry point |
| `scripts/TASK_RUNNER_README.md` | done | Documentation |
| `/mnt/data1/dasl-planning/dasl-planner` | reference | DASL planner binary |
| `tasks/thunk-server/` | reference | Thunk server design |
| `tasks/do-scheduler-modes/` | reference | Scheduler modes |

## Next Actions

1. **Enhance task-context-runner.sh** — load all old chats for each task from shmem
2. **Enhance task-context-runner.sh** — load full git history + branches + remotes
3. **Enhance task-context-runner.sh** — load all related tasks via shmem_blocks_search
4. **Add lean-ctx context storage** — store loaded contexts in lean-ctx memory
5. **Add parallel prefetching** — batch process multiple tasks concurrently
6. **Add task auto-detection** — detect tasks from git commit messages
7. **Add status dashboard** — report task runner health and context counts

## Task Context Components

Each task context includes:

1. **Chats**: Related conversation history from shmem (IPLD CAR)
2. **Git Context**: Git log, branches, commits for the task
3. **Related Tasks**: Other tasks in the same area from shmem search
4. **Planner State**: Current DASL planner status via `dasl-planner status`
5. **Shmem Metadata**: IPLD CAR memory state via `dasl-planner shmem`
6. **IPLD Mapping**: IPLD → shmem mapping via `dasl-planner ipld`

## Integration Points

- **lean-ctx**: Context managed by lean-ctx, MCP tools for shmem access
- **shmem**: IPLD CAR memory for fast task context retrieval (351,750 blocks available)
- **dasl-planner**: Scheduler coordination, OODA cycle, planner status
- **Git**: Old git history loaded for each task context
- **Context Runner**: Dedicated runner for task context isolation
