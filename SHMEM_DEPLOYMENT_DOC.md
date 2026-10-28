# Shmem Service Deployment Documentation

## Overview
This document describes the complete deployment of the ipld-car-shmem service and shmem-mcp MCP server for the Letta shared memory infrastructure.

## Service Architecture

```
┌─────────────────────────────────────────────────────────────┐
│  ipld-car-shmem.service (systemd)                          │
│  - Runs: ipld-car-shmem-server                              │
│  - Port: 8150 (HTTP)                                        │
│  - Socket: @ipld_car_shmem (Unix abstract socket)          │
│  - User: ipld-data                                          │
│  - Working Dir: /mnt/data1/dasl-cache                       │
│  - Capacity: 38.6 GB                                        │
└─────────────────────────────────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────┐
│  shmem-mcp (MCP Server via lean-ctx addon)                 │
│  - Transport: stdio                                         │
│  - Tools: shmem_stats, shmem_car_search, shmem_car_list,   │
│           shmem_blocks_query, shmem_blocks_search          │
│  - Read-only access to CAR shmem ring buffer               │
└─────────────────────────────────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────┐
│  lean-ctx Integration                                       │
│  - Addon: shmem@0.1.0                                       │
│  - Tool Profile: power (78 tools)                          │
│  - Gateway: enabled                                         │
└─────────────────────────────────────────────────────────────┘
```

## Service Configuration

### Systemd Service
- **File**: `/etc/systemd/system/ipld-car-shmem.service`
- **Override**: `/etc/systemd/system/ipld-car-shmem.service.d/override.conf`
- **WantedBy**: `system-manager.target`
- **Environment**: 
  - `IPLD_CAR_CACHE_PATH=/mnt/data1/dasl-cache`
  - `IPLD_CAR_CAPACITY=38654705664`
  - `IPLD_CAR_HTTP_PORT=8150`
  - `IPLD_CAR_SOCKET_NAME=@ipld_car_shmem`

### Nix Integration
- **Flake**: `/mnt/data1/kant/pastebin/flake.nix`
- **System Config**: `systemConfigs.kant-pastebin-only`
- **System Manager**: Uses numtide/system-manager for service deployment

### Lean-ctx Addon
- **Manifest**: `examples/shmem-addon/lean-ctx-addon.toml`
- **Installed via**: `lean-ctx addon add examples/shmem-addon/lean-ctx-addon.toml`
- **Gateway**: Registered in `~/.config/lean-ctx/config.toml`

## Verified Operations

### Health Checks
- ✅ HTTP Health: `GET http://127.0.0.1:8150/health` → `{"status":"ok","blocks":351750}`
- ✅ Socket: `ss -xlp | grep @ipld_car_shmem` → Listening
- ✅ Service: `systemctl is-active ipld-car-shmem.service` → active

### MCP Tools (shmem-mcp)
- ✅ `shmem_stats` → Returns block count, capacity, used space
- ✅ `shmem_car_search` → Searches CAR pages by query
- ✅ `shmem_car_list` → Lists stored CAR CIDs
- ✅ `shmem_blocks_query` → Queries memory blocks by path prefix
- ✅ `shmem_blocks_search` → Full-text search of block descriptions

### Core Shmem Operations
- ✅ `query project/` → 10 blocks returned
- ✅ `get project/twitterstorm/scheduler-merge-task` → Task retrieved
- ✅ `stats` → 351,750 blocks, 641 MB used
- ✅ Error handling: Graceful `not_found` for missing paths

## Context Capture System

### Wrapper Script
- **File**: `/mnt/data1/kant/pastebin/shmem-context-logs/shmem-call-wrapper.sh`
- **Log**: `/mnt/data1/kant/pastebin/shmem-context-logs/shmem-call-context.log`

### Captured Context Per Call
- PID, UID, GID, User
- Current Working Directory (CWD)
- Git Repository, Branch, Commit, Dirty files
- Command executed and arguments
- Exit code, Result summary (type, size)
- File descriptor count, Disk space, System load

### Log Entries: 293+ context entries captured

## Live Monitoring

### Monitor Daemon
- **Script**: `/mnt/data1/kant/pastebin/shmem-context-logs/shmem-monitor.sh`
- **PID**: 708380 (running)
- **Interval**: 10 seconds
- **Log**: `/mnt/data1/kant/pastebin/shmem-logs/monitor-log.txt`

### Monitored Metrics
- Service status (active/inactive)
- Socket connections
- HTTP health (200 OK)
- Block count
- Memory usage

## Deployment Commands

```bash
# Start service
sudo systemctl start ipld-car-shmem.service

# Enable service
sudo systemctl enable ipld-car-shmem.service

# Restart service
sudo systemctl restart ipld-car-shmem.service

# Check status
systemctl status ipld-car-shmem.service

# View logs
journalctl -u ipld-car-shmem.service -f

# Test MCP tools
shmem-mcp <<< '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"shmem_stats","arguments":{}}}'

# Test core operations
/home/mdupont/dasl/ipld-car-ipc-shmem-linux/target/release/letta-ipld-memory query project/
/home/mdupont/dasl/ipld-car-ipc-shmem-linux/target/release/letta-ipld-memory get project/twitterstorm/scheduler-merge-task
/home/mdupont/dasl/ipld-car-ipc-shmem-linux/target/release/letta-ipld-memory stats

# Test with context capture
/mnt/data1/kant/pastebin/shmem-context-logs/shmem-call-wrapper.sh query project/
```

## Git Deployment

### Files Changed
- `/mnt/data1/kant/pastebin/shmem-context-logs/shmem-call-wrapper.sh`
- `/mnt/data1/kant/pastebin/shmem-context-logs/shmem-context-collector.sh`
- `/mnt/data1/kant/pastebin/shmem-context-logs/shmem-monitor.sh`
- `/mnt/data1/kant/pastebin/shmem-logs/shmem-call-context.log`
- `/mnt/data1/kant/pastebin/SERVICE_STATUS_AND_SHMEM_VERIFICATION_REPORT.txt`
- `/mnt/data1/kant/pastebin/SHMEM_QUERY_AND_RESULTS_ANALYSIS.md`
- `/mnt/data1/kant/pastebin/shmem-logs/QA_REPORT.txt`
- `/mnt/data1/kant/pastebin/SHMEM_DEPLOYMENT_DOC.md`

### Git Commit
```bash
cd /mnt/data1/kant/pastebin
git add shmem-context-logs/ shmem-logs/ SERVICE_STATUS_AND_SHMEM_VERIFICATION_REPORT.txt SHMEM_QUERY_AND_RESULTS_ANALYSIS.md QA_REPORT.txt SHMEM_DEPLOYMENT_DOC.md
git commit -m "Deploy shmem service with full context capture, monitoring, and MCP integration"
git push origin feature/big-merge
```

## Verification Checklist

- [x] Service running via systemd
- [x] Service enabled with system-manager.target
- [x] HTTP API responding on port 8150
- [x] Unix socket @ipld_car_shmem listening
- [x] MCP server (shmem-mcp) installed and functional
- [x] 5 MCP tools available: stats, car_search, car_list, blocks_query, blocks_search
- [x] Lean-ctx addon installed (shmem@0.1.0)
- [x] Context capture wrapper deployed
- [x] Live monitor running (10s interval)
- [x] All core operations verified (query, get, stats, search)
- [x] Error handling verified
- [x] Git commit and push completed
- [x] Documentation created

## Status Summary

| Component | Status |
|-----------|--------|
| ipld-car-shmem.service | ✅ Active & Enabled |
| shmem-mcp (MCP server) | ✅ Installed & Working |
| lean-ctx addon (shmem) | ✅ Installed |
| Context capture | ✅ 293+ entries |
| Live monitoring | ✅ Running (PID 708380) |
| Nix/Flake integration | ✅ system-manager.target |
| Git deployment | ✅ Committed & Pushed |

