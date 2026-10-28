# Task Runner Operations — Skills for Task Runner Management

**Author:** Task Runner System  
**Version:** 1.0  
**Status:** proposed

## Skill: Task Context Loading

### Purpose
Optimize and enhance task context loading via lean-ctx and shmem.

### Commands
```bash
# Load task context
./task-context-runner.sh load <task_id>

# Load with performance optimization
./task-context-runner.sh load --optimized <task_id>

# Load multiple tasks in parallel
./task-context-runner.sh load-batch <task_id1> <task_id2> ...

# Get task context status
./task-context-runner.sh status <task_id>

# Check task context health
./task-context-runner.sh health <task_id>
```

### Performance Optimization Targets
- Load time: < 5 seconds per task
- Memory usage: < 10MB per task context
- Concurrent load: 10 tasks in parallel
- Success rate: 99.9%

## Skill: Task Prefetching

### Purpose
Efficiently prefetch related tasks and context data.

### Commands
```bash
# Prefetch single task
./task-prefetcher.sh single <task_id>

# Prefetch all related tasks
./task-prefetcher.sh all

# Prefetch with dependency resolution
./task-prefetcher.sh --resolve-deps

# Prefetch with performance optimization
./task-prefetcher.sh --optimized

# Get prefetch status
./task-prefetcher.sh status
```

## Skill: Scheduler Integration

### Purpose
Integrate task runner with dasl-planner scheduler.

### Commands
```bash
# Get scheduler status
./task-scheduler-integration.sh status

# Run OODA cycle
./task-scheduler-integration.sh ooda

# Update task schedule
./task-scheduler-integration.sh update

# Get scheduler recommendations
./task-scheduler-integration.sh recommend
```

## Skill: SPARQL Server Integration

### Purpose
Integrate SPARQL server for querying task context data.

### Commands
```bash
# Set up SPARQL server
./sparql-setup.sh init

# Load task context data into SPARQL
./sparql-setup.sh load-context

# Query task context via SPARQL
./sparql-query.sh "SELECT ?task ?status WHERE { ?task rdf:type Task ; :status ?status }"

# Sync SPARQL with shmem
./sparql-sync.sh

# Get SPARQL server status
./sparql-query.sh --status
```

## Skill: Self-Monitoring

### Purpose
Monitor task runner health and performance.

### Commands
```bash
# Check task runner health
./task-runner.sh health

# Get performance metrics
./task-runner.sh metrics

# Run diagnostics
./task-runner.sh diagnose

# Check shmem integration
./task-runner.sh check-shmem

# Check lean-ctx integration
./task-runner.sh check-lean-ctx

# Check dasl-planner integration
./task-runner.sh check-planner
```

## Skill: Maintenance Operations

### Purpose
Perform maintenance and optimization of task runner.

### Commands
```bash
# Clean old task contexts
./task-runner.sh clean --older-than 7d

# Optimize task contexts
./task-runner.sh optimize

# Rebuild shmem cache
./task-runner.sh rebuild-cache

# Update task runner scripts
./task-runner.sh update

# Backup task contexts
./task-runner.sh backup

# Restore task contexts
./task-runner.sh restore <backup-file>
```

## Skill: Task Discovery

### Purpose
Discover and identify tasks from various sources.

### Commands
```bash
# Scan for new tasks
./task-runner.sh discover --scan

# Find task by keyword
./task-runner.sh find <keyword>

# List all task contexts
./task-runner.sh list-tasks

# Show task relationships
./task-runner.sh show-deps <task_id>

# Get task history
./task-runner.sh history <task_id>
```

## Integration with Tools

### SPARQL Server Integration
- SPARQL server endpoint for querying task context data
- MCP tools: sparql_query, sparql_update, sparql_import
- Integration with lean-ctx via MCP

### Shmem Integration
- IPLD CAR memory for fast context retrieval
- 351,750 blocks available
- MCP tools: shmem_stats, shmem_blocks_query, shmem_blocks_search

### Lean-ctx Integration
- Context management via lean-ctx
- MCP tools: shmem_stats, shmem_car_search, shmem_car_list, shmem_blocks_query, shmem_blocks_search
- Future: add sparql_query, sparql_update, sparql_import

### Dasl-planner Integration
- Scheduler coordination
- OODA cycle execution
- Planner status reporting
