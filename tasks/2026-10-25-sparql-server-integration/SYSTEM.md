---
name: sparql-server-integration
priority: HIGH
depends_on: task-runner-self-improvement, task-prefetch-runner
---

# SPARQL Server Integration — Task Runner Next Tool

**Status:** proposed · **Date:** 2026-10-25

## Goal

Integrate a SPARQL server into the task runner system, enabling SPARQL queries over task context data, old chats, git history, and related tasks loaded via lean-ctx and shmem.

## Instructions

- Set up SPARQL server endpoint for task context data
- Integrate SPARQL server with task context runner
- Enable SPARQL queries over loaded task contexts
- Prepare SPARQL server for lean-ctx MCP tool integration
- Add SPARQL query capabilities to task runner scripts

## Discoveries

- Task runner currently supports lean-ctx MCP tools: shmem_stats, shmem_car_search, shmem_car_list, shmem_blocks_query, shmem_blocks_search
- Dasl-planner supports SPARQL-related commands
- Shmem service active with 351,750 blocks
- lean-ctx addon shmem@0.1.0 installed

## Accomplished

- [x] Task runner created with shmem + lean-ctx integration
- [x] Self-improvement task created for task runner
- [x] Task runner scripts ready for new tool integration

## Not Done / In Progress

- [ ] Set up SPARQL server endpoint
- [ ] Load task context data into SPARQL format
- [ ] Integrate SPARQL server with task context runner
- [ ] Add SPARQL query capabilities to task runner scripts
- [ ] Enable SPARQL MCP tools in lean-ctx

## Relevant Files

| File | Status | Notes |
|------|--------|-------|
| `tasks/2026-10-25-task-runner-self-improvement/SYSTEM.md` | done | Self-improvement task |
| `tasks/2026-10-25-task-prefetch-runner/SYSTEM.md` | done | Main task prefetch runner |
| `scripts/task-context-runner.sh` | done | Ready for SPARQL integration |
| `scripts/task-runner.sh` | done | Ready for SPARQL integration |

## Next Actions

1. Set up SPARQL server endpoint
2. Load task context data into SPARQL format
3. Integrate SPARQL server with task context runner
4. Add SPARQL query capabilities to task runner scripts
5. Enable SPARQL MCP tools in lean-ctx
6. Test SPARQL server integration

## Task Runner SPARQL Integration Components

1. **SPARQL Server Endpoint** — SPARQL server for querying task context
2. **Task Context Loading** — Load task context data into SPARQL
3. **Task Context Runner Integration** — Integrate SPARQL with task context runner
4. **SPARQL Query Capabilities** — Add SPARQL queries to task runner scripts
5. **Lean-ctx MCP Tools** — Add SPARQL MCP tools to lean-ctx

## Integration Points

- **SPARQL Server** — SPARQL server endpoint for querying task context data
- **Task Context Data** — Old chats, git history, related tasks loaded via lean-ctx + shmem
- **Lean-ctx** — Add SPARQL MCP tools to lean-ctx integration
- **Shmem** — Load shmem data into SPARQL queries
- **Dasl-planner** — Use dasl-planner to coordinate SPARQL server integration

## SPARQL Integration Goals

1. **Query Interface**: SPARQL queries over task context data
2. **Integration**: Seamless integration with task context runner
3. **Performance**: Fast SPARQL query response times
4. **Scalability**: Support for large task context datasets
5. **Maintainability**: Easy maintenance and updates for SPARQL server integration
