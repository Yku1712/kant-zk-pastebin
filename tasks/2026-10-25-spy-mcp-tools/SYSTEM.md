---
name: spy-mcp-tools
priority: HIGH
depends_on: sparql-gui-server, sparql-gui-integration
---

# SPY MCP Tools — Surface Shmem Functions to SPARQL GUI

**Status:** proposed · **Date:** 2026-10-25

## Goal

Surface all functions of the shmem tool (via shmem-mcp) to the SPARQL GUI, making them accessible as lean-ctx tools that can be called from the task runner and SPARQL queries.

## Instructions

- Expose all 5 shmem-mcp tools as lean-ctx tools accessible via SPARQL GUI
- Create scripts for each shmem function that can be called from task runner
- Integrate with task context runner to load shmem tool functions
- Update task runner scripts to include shmem tool commands
- Add SPARQL queries that call these tools via lean-ctx

## Discoveries

- shmem-mcp provides 5 tools: shmem_stats, shmem_car_search, shmem_car_list, shmem_blocks_query, shmem_blocks_search
- lean-ctx addon shmem@0.1.0 is installed and working
- SPARQL GUI server is running on port 8088
- Task runner has 14 task contexts loaded
- shmem service is active (PID 784999) with 351,750 blocks

## Accomplished

- [x] Task runner with shmem + lean-ctx integration
- [x] SPARQL GUI server running
- [x] 14 task contexts loaded
- [x] Dasl-planner integration verified

## Not Done / In Progress

- [ ] Create scripts for each shmem-mcp tool
- [ ] Integrate shmem tools with task runner
- [ ] Expose shmem tools via lean-ctx to SPARQL GUI
- [ ] Test SPARQL queries that call shmem functions
- [ ] Add SPARQL commands for shmem_stats, shmem_car_search, etc.
- [ ] Update task runner documentation

## Relevant Files

| File | Status | Notes |
|------|--------|-------|
| `scripts/sparql-query.sh` | done | SPARQL query execution |
| `scripts/sparql-import.sh` | done | SPARQL import script |
| `scripts/sparql-status.sh` | done | SPARQL status script |
| `scripts/task-context-runner.sh` | done | Task context loading |
| `scripts/task-prefetcher.sh` | done | Prefetch related tasks |
| `scripts/task-scheduler-integration.sh` | done | Dasl-planner integration |
| `scripts/task-runner.sh` | done | Main entry point |

## Next Actions

1. Create scripts for each shmem-mcp tool:
   - `shmem-stats.sh` - Get shmem statistics
   - `shmem-car-search.sh` - Search CAR pages
   - `shmem-car-list.sh` - List CAR CIDs
   - `shmem-blocks-query.sh` - Query blocks by prefix
   - `shmem-blocks-search.sh` - Search blocks by description

2. Update `task-context-runner.sh` to load shmem tool contexts
3. Update `task-runner.sh` to include shmem tool commands
4. Test SPARQL queries that call shmem functions via lean-ctx
5. Update documentation

## Shmem Tools to Surface

| Tool | Description | Shmem Command |
|------|-------------|---------------|
| shmem_stats | Server stats: slots, used, CIDs, blocks, uptime | stats |
| shmem_car_search | Search stored CAR pages by query | search (CAR) |
| shmem_car_list | List stored CAR CIDs (truncated to limit) | list |
| shmem_blocks_query | Query memory blocks by path prefix with pagination | query |
| shmem_blocks_search | Full-text search memory blocks by description | search |

## Integration Points

- **Task Runner** — Load shmem tool contexts and expose as commands
- **Lean-ctx** — Provide MCP tools for shmem functions
- **SPARQL GUI** — Execute SPARQL queries that call shmem functions
- **Dasl-planner** — Coordinate task execution
- **Shmem Service** — IPLD CAR memory backend

## Self-Improvement Goals

1. **Completeness**: All 5 shmem-mcp tools exposed
2. **Usability**: Simple command interface for each tool
3. **Integration**: Seamless SPARQL query integration
4. **Performance**: Fast shmem tool execution
5. **Maintainability**: Easy to add new shmem tools