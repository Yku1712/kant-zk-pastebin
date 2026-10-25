#!/bin/bash
set -e

# SPARQL Query Script - Query task context data via SPARQL server
# Integrates with lean-ctx + shmem + task runner

SPARQL_ENDPOINT="${SPARQL_ENDPOINT:-http://localhost:3030/task-context/query}"
TASK_CONTEXT_DIR="/mnt/data1/kant/pastebin/scripts/task-contexts"

show_help() {
    echo "SPARQL Query Script"
    echo ""
    echo "Usage:"
    echo "  $0 <query>                - Execute SPARQL query"
    echo "  $0 --status               - Check SPARQL server status"
    echo "  $0 --setup                - Setup SPARQL server environment"
    echo "  $0 --import-tasks         - Import task contexts into SPARQL"
    echo ""
    echo "Examples:"
    echo "  $0 'SELECT ?task WHERE { ?task rdf:type Task }'"
    echo "  $0 --status"
}

if [[ "$1" == "--status" ]]; then
    echo "📊 Checking SPARQL server status..."
    if command -v curl &>/dev/null; then
        curl -s -o /dev/null -w "%{http_code}" "$SPARQL_ENDPOINT" 2>/dev/null || echo "000"
        echo ""
    else
        echo "curl not available"
    fi
elif [[ "$1" == "--setup" ]]; then
    echo "🔧 Setting up SPARQL server environment..."
    echo "SPARQL_ENDPOINT=$SPARQL_ENDPOINT"
    echo "TASK_CONTEXT_DIR=$TASK_CONTEXT_DIR"
    echo "To install Apache Jena Fuseki:"
    echo "  wget https://archive.apache.org/dist/jena/binaries/apache-jena-fuseki-5.x.x.tar.gz"
    echo "  tar -xzf apache-jena-fuseki-*.tar.gz"
    echo "  cd apache-jena-fuseki-*"
    echo "  ./fuseki-server --mem /task-context"
elif [[ "$1" == "--import-tasks" ]]; then
    echo "📥 Importing task contexts into SPARQL..."
    
    # Create SPARQL data from task context JSONs
    python3 << PYEOF
import json, os, glob
from datetime import datetime

task_context_dir = os.environ.get('TASK_CONTEXT_DIR', '/mnt/data1/kant/pastebin/scripts/task-contexts')

triples = []

for task_dir in sorted(os.listdir(task_context_dir)):
    task_path = os.path.join(task_context_dir, task_dir)
    if not os.path.isdir(task_path):
        continue
    
    task_uri = f"<http://task-runner.kant/context/{task_dir}>"
    
    # Read metadata
    metadata_path = os.path.join(task_path, 'metadata.json')
    if os.path.exists(metadata_path):
        with open(metadata_path) as f:
            metadata = json.load(f)
        
        triples.append(f"{task_uri} <http://task-runner.kant/id> \"{metadata.get('id', task_dir)}\" .")
        triples.append(f"{task_uri} <http://task-runner.kant/type> \"{metadata.get('type', 'task_context')}\" .")
        triples.append(f"{task_uri} <http://task-runner.kant/created> \"{metadata.get('created', 'unknown')}\" .")
        triples.append(f"{task_uri} <http://task-runner.kant/prefetched> \"{metadata.get('prefetched', False)}\" .")
        triples.append(f"{task_uri} <http://task-runner.kant/path> \"{metadata.get('path', task_dir)}\" .")
    
    # Read shmem chats
    chats_path = os.path.join(task_path, 'shmem_chats.json')
    if os.path.exists(chats_path):
        with open(chats_path) as f:
            chats_data = f.read().strip()
        if chats_data:
            triples.append(f"{task_uri} <http://task-runner.kant/hasChats> \"true\" .")
    
    # Read git history
    git_path = os.path.join(task_path, 'git_history.txt')
    if os.path.exists(git_path):
        with open(git_path) as f:
            git_content = f.read().strip()
        if git_content:
            triples.append(f"{task_uri} <http://task-runner.kant/hasGitHistory> \"true\" .")
    
    # Read related tasks
    related_path = os.path.join(task_path, 'related_tasks.json')
    if os.path.exists(related_path):
        with open(related_path) as f:
            related_data = f.read().strip()
        if related_data:
            triples.append(f"{task_uri} <http://task-runner.kant/hasRelatedTasks> \"true\" .")
    
    # Read planner status
    planner_path = os.path.join(task_path, 'planner_status.txt')
    if os.path.exists(planner_path):
        with open(planner_path) as f:
            planner_content = f.read().strip()
        if planner_content:
            triples.append(f"{task_uri} <http://task-runner.kant/hasPlannerStatus> \"true\" .")

# Output Turtle format
print("@prefix task: <http://task-runner.kant/context/> .")
print("@prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .")
print("@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .")
print()
for triple in triples:
    print(triple)
PYEOF
elif [[ -n "$1" ]]; then
    echo "🔍 Executing SPARQL query..."
    echo "Query: $1"
    echo ""
    echo "SPARQL server not yet installed. Run with --setup to configure."
    echo "Endpoint: $SPARQL_ENDPOINT"
else
    show_help
    exit 1
fi
