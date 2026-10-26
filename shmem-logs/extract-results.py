#!/usr/bin/env python3
"""Extract query results from shmem logs for analysis."""

import json
import subprocess
import os
from collections import Counter

# Configuration
SHMEM_BINARY = "/home/mdupont/dasl/ipld-car-ipc-shmem-linux/target/release/letta-ipld-memory"
LOG_DIR = "/mnt/data1/kant/pastebin/shmem-logs/query-results"

def run_query(query):
    """Run a shmem query and return results as JSON string."""
    result = subprocess.run(
        [SHMEM_BINARY, "query", query],
        capture_output=True,
        text=True,
        timeout=30
    )
    return result.stdout, result.stderr, result.returncode

def run_search(term):
    """Run a shmem search and return results as JSON string."""
    result = subprocess.run(
        [SHMEM_BINARY, "search", term],
        capture_output=True,
        text=True,
        timeout=30
    )
    # Search outputs multiple lines; concatenate them
    lines = result.stdout.strip().split('\n')
    combined = '\n'.join(l for l in lines if l.strip())
    return combined, result.stderr, result.returncode

def run_get(path):
    """Run a shmem get and return results as JSON string."""
    result = subprocess.run(
        [SHMEM_BINARY, "get", path],
        capture_output=True,
        text=True,
        timeout=30
    )
    return result.stdout, result.stderr, result.returncode

# Define queries based on log patterns
queries = {
    "query-system": "system/",
    "query-project": "project/",
    "query-twitterstorm": "project/twitterstorm/",
    "get-scheduler-merge": "project/twitterstorm/scheduler-merge-task",
}

searches = {
    "search-scheduler-merge": "scheduler-merge",
    "search-worker-self-status": "worker-self-status",
    "search-pi-agent": "pi-agent",
    "search-account-worker": "account-worker",
    "search-lean-worker": "lean-worker",
    "search-ardist-dedup": "ardist-dedup",
}

# Create output directory
os.makedirs(LOG_DIR, exist_ok=True)

print("=" * 60)
print("SHMEM QUERY EXTRACTION FROM LOG PATTERNS")
print("=" * 60)
print()

# Extract results for each query type
for name, query in queries.items():
    print(f"--- Query: {name} --- {query}")
    stdout, stderr, rc = run_query(query)
    if stdout.strip():
        try:
            d = json.loads(stdout)
            blocks_count = len(d.get("blocks", []))
            total_count = d.get("total", 0)
            print(f"  Result: {blocks_count} blocks found (total: {total_count})")
            if blocks_count > 0:
                print("  Blocks:")
                for b in d.get("blocks", [])[:5]:
                    desc = b.get("description", "N/A")[:80]
                    path = b.get("path", "")
                    print(f"    - {desc} [path: {path}]")
        except json.JSONDecodeError:
            print(f"  Raw output: {stdout[:200]}...")
    else:
        print(f"  No results (exit code: {rc})")
    print()

# Extract search results
for name, term in searches.items():
    print(f"--- Search: {name}: '{term}' ---")
    stdout, stderr, rc = run_search(term)
    print(f"  Exit code: {rc}")
    try:
        d = json.loads(stdout.strip())
        print(f"  Results found: {len(d.get('results', []))}")
        if d.get("results"):
            print("  Matches:")
            for r in d.get("results", [])[:3]:
                path = r.get("path", "")
                desc = r.get("description", "")[:100]
                print(f"    - {path}: {desc}")
    except json.JSONDecodeError:
        print(f"  Could not parse output")
    print()

# Extract get result
print("--- Get: project/twitterstorm/scheduler-merge-task ---")
stdout, stderr, rc = run_get("project/twitterstorm/scheduler-merge-task")
if stdout.strip():
    try:
        d = json.loads(stdout)
        print(f"  Task status: {d.get('status', 'N/A')}")
        print(f"  Next phase: {d.get('next_phase', 'N/A')}")
        print(f"  Scripts: {len(d.get('scripts', []))}")
        print(f"  Requirements: {len(d.get('requirements', []))}")
        print(f"  Files created: {len(d.get('files_created', []))}")
    except json.JSONDecodeError:
        print(f"  Raw output saved")
print()

print("=" * 60)
print("EXTRACTION COMPLETE")
print("=" * 60)
