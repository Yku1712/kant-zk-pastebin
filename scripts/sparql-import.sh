#!/bin/bash
set -e

# SPARQL Import Script - Import task context data into SPARQL server
TASK_CONTEXT_DIR="/mnt/data1/kant/pastebin/scripts/task-contexts"
SPARQL_ENDPOINT="${SPARQL_ENDPOINT:-http://localhost:3030/task-context/data}"

echo "📥 Importing task contexts into SPARQL..."

# Generate RDF triples from task contexts
turtle_file="/tmp/task-contexts-$(date +%s).ttl"
python3 << PYEOF > "$turtle_file"
import json, os

task_context_dir = os.environ.get('TASK_CONTEXT_DIR', '/mnt/data1/kant/pastebin/scripts/task-contexts')

triples = []
triples.append("@prefix task: <http://task-runner.kant/context/> .")
triples.append("@prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .")
triples.append("@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .")
triples.append("@prefix xsd: <http://www.w3.org/2001/XMLSchema#> .")
triples.append("")

task_count = 0

for task_dir in sorted(os.listdir(task_context_dir)):
    task_path = os.path.join(task_context_dir, task_dir)
    if not os.path.isdir(task_path):
        continue
    
    task_uri = f"<http://task-runner.kant/context/{task_dir}>"
    task_count += 1
    
    triples.append(f"{task_uri} rdf:type <http://task-runner.kant/Task> .")
    triples.append(f"{task_uri} <http://task-runner.kant/id> \"{task_dir}\" .")
    
    # Metadata
    metadata_path = os.path.join(task_path, 'metadata.json')
    if os.path.exists(metadata_path):
        with open(metadata_path) as f:
            metadata = json.load(f)
        triples.append(f"{task_uri} <http://task-runner.kant/type> \"{metadata.get('type', 'task_context')}\" .")
        triples.append(f"{task_uri} <http://task-runner.kant/created> \"{metadata.get('created', 'unknown')}\" .")
        triples.append(f"{task_uri} <http://task-runner.kant/prefetched> \"{str(metadata.get('prefetched', False))}\" .")
        triples.append(f"{task_uri} <http://task-runner.kant/path> \"{metadata.get('path', task_dir)}\" .")
    
    # Component presence
    for comp in ['shmem_chats.json', 'git_history.txt', 'related_tasks.json', 'planner_status.txt', 'recent_commits.txt']:
        comp_path = os.path.join(task_path, comp)
        if os.path.exists(comp_path):
            with open(comp_path) as f:
                content = f.read().strip()
            if content:
                comp_name = comp.split('.')[0]
                triples.append(f"{task_uri} <http://task-runner.kant/has{comp_name}> \"true\" .")

triples.append("")
print("\n".join(triples))
PYEOF

echo "Generated RDF data: $turtle_file"
echo "Task contexts imported: $(grep -c 'rdf:type.*Task' "$turtle_file")"

if command -v curl &>/dev/null; then
    echo "Attempting to upload to SPARQL endpoint: $SPARQL_ENDPOINT"
    curl -X POST -H "Content-Type: text/turtle" \
        --data-binary @"$turtle_file" \
        "$SPARQL_ENDPOINT" 2>/dev/null || echo "SPARQL endpoint not reachable (expected if server not running)"
else
    echo "curl not available - upload skipped"
fi

echo "✅ SPARQL import preparation complete"
echo "Turtle file: $turtle_file"
echo ""
echo "To load into SPARQL server:"
echo "  curl -X POST -H \"Content-Type: text/turtle\" --data-binary @$turtle_file \$SPARQL_ENDPOINT"
