---
name: sparql-gui-integration
priority: HIGH
depends_on: sparql-server-integration, task-prefetch-runner
---

# SPARQL GUI Integration — Task Runner + SPARQL Server + Web UI

**Status:** proposed · **Date:** 2026-10-25

## Goal

Integrate the SPARQL GUI project (`/mnt/data1/time-2026/06-june/07/sparql-gui/`) with the task runner system, enabling:
- SPARQL queries over task context data via REST API
- Web UI for SPARQL query execution
- Task context data loaded into SPARQL via task runner
- Dasl-planner integration for task coordination

## Instructions

- Use task runner to prefetch and load sparql-gui task context
- Build sparql-gui binaries via Nix
- Run SPARQL server (letta-ipld-memory web_server command)
- Integrate SPARQL queries with task context data
- Add SPARQL MCP tools to lean-ctx

## Discoveries

- sparql-gui project at `/mnt/data1/time-2026/06-june/07/sparql-gui/` uses ipld-car-shmem server
- SPARQL endpoint: `/api/sparql` via axum web server
- Web UI: `web/sparql-gui.html`
- CAR store opens at `/mnt/data1/dasl-cache`
- Binaries available via Nix: `ipld-car-shmem-server`, `ipld-car-shmem-client`, `letta-ipld-memory`, `test-tag-42`
- Uses sophia_sparql for SPARQL engine over CAR blocks

## Accomplished

- [x] Task runner with shmem + lean-ctx integration
- [x] SPARQL integration scripts created (query, import, status)
- [x] 12 task contexts loaded
- [x] Dasl-planner integration working

## Not Done / In Progress

- [ ] Build sparql-gui binaries
- [ ] Run SPARQL web server
- [ ] Load task contexts into SPARQL via sparql-gui
- [ ] Test SPARQL queries over task context data
- [ ] Add SPARQL MCP tools to lean-ctx
- [ ] Integrate with task runner scripts

## Relevant Files

| File | Status | Notes |
|------|--------|-------|
| `/mnt/data1/time-2026/06-june/07/sparql-gui/` | reference | SPARQL GUI project |
| `scripts/sparql-query.sh` | done | SPARQL query script |
| `scripts/sparql-import.sh` | done | SPARQL import script |
| `scripts/sparql-status.sh` | done | SPARQL status script |
| `scripts/task-context-runner.sh` | done | Task context loading |

## Next Actions

1. Build sparql-gui project via Nix
2. Run SPARQL web server (`letta-ipld-memory web_server`)
3. Load task context data into SPARQL via sparql-gui
4. Test SPARQL queries over task context
5. Add SPARQL MCP tools to lean-ctx
6. Integrate with task runner

## SPARQL GUI Integration Components

1. **SPARQL Server** — `letta-ipld-memory web_server` command
2. **Web UI** — `web/sparql-gui.html` served by axum
3. **REST API** — `/api/sparql` for SPARQL queries
4. **CAR Store** — Opens CAR blocks from shmem at `/mnt/data1/dasl-cache`
5. **SPARQL Engine** — sophia_sparql over CAR blocks
6. **Task Context Data** — Loaded via task runner into SPARQL

## Integration Points

- **Task Runner** — Prefetches and loads task context
- **Shmem** — CAR blocks at `/mnt/data1/dasl-cache`
- **SPARQL GUI** — Runs SPARQL server over CAR blocks
- **Lean-ctx** — Add SPARQL MCP tools
- **Dasl-planner** — Coordinate task execution