---
name: sparql-gui-server
priority: HIGH
depends_on: sparql-gui-integration
---

# SPARQL GUI Server — Task Runner Integration

**Status:** proposed · **Date:** 2026-10-25

## Goal

Start the SPARQL GUI web server via task runner integration with the letta-ipld-memory binary. This will:
- Start the web-based SPARQL query tool
- Open HTTP server with rich query UI and pipeline builder
- Integrate with task runner for task context data
- Enable SPARQL queries over CAR block store

## Instructions

- Build letta-ipld-memory binary via Nix flake
- Start SPARQL server with `letta-ipld-memory serve_web`
- Verify /api/sparql endpoint
- Test SPARQL query interface
- Integrate with task runner scripts for task context queries

## Discoveries

- letta-ipld-memory binary available at `/mnt/data1/time-2026/06-june/07/sparql-gui/target/release/letta-memory`
- `ServeWeb` command binds to `127.0.0.1:8088` by default
- Web UI served at root `/`
- Uses IPLD CAR shmem at `/mnt/data1/dasl-cache`
- sophia_sparql engine over CAR blocks
- REST API `/api/sparql` for SPARQL queries

## Accomplished

- [x] Task runner with shmem + lean-ctx integration
- [x] SPARQL integration scripts created
- [x] 13 task contexts loaded
- [x] Dasl-planner integration verified

## Not Done / In Progress

- [ ] Build letta-ipld-memory binary
- [ ] Start SPARQL web server
- [ ] Verify /api/sparql endpoint
- [ ] Test SPARQL queries
- [ ] Integrate with task runner scripts

## Relevant Files

| File | Status | Notes |
|------|--------|-------|
| `/mnt/data1/time-2026/06-june/07/sparql-gui/` | reference | SPARQL GUI project |
| `scripts/sparql-status.sh` | done | SPARQL status script |
| `scripts/task-context-runner.sh` | done | Task context loading |
| `scripts/task-runner.sh` | done | Main task runner entry point |

## Next Actions

1. Build letta-ipld-memory binary via Nix
2. Start SPARQL web server (`letta-ipld-memory serve_web`)
3. Verify /api/sparql endpoint responds
4. Test SPARQL queries over task context data
5. Integrate with task runner scripts
6. Add SPARQL MCP tools to lean-ctx

## SPARQL GUI Server Integration Components

1. **Web Server** — `letta-ipld-memory serve_web` command
2. **HTTP Endpoint** — `/api/sparql` for SPARQL queries
3. **Web UI** — Rich query UI with pipeline builder
4. **CAR Store** — Opens CAR blocks from shmem at `/mnt/data1/dasl-cache`
5. **SPARQL Engine** — sophia_sparql over CAR blocks
6. **Task Context Data** — Loaded via task runner into SPARQL

## Integration Points

- **Task Runner** — Prefetches and loads task context
- **Shmem** — CAR blocks at `/mnt/data1/dasl-cache`
- **SPARQL GUI** — Runs SPARQL server over CAR blocks
- **Lean-ctx** — Add SPARQL MCP tools
- **Dasl-planner** — Coordinate task execution

## Self-Improvement Goals

1. **Performance**: Fast SPARQL query response times
2. **Reliability**: 99.9% uptime with health monitoring
3. **Integration**: Seamless integration with task runner
4. **Scalability**: Support for large task context datasets
5. **Maintainability**: Easy maintenance and updates
