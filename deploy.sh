#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
PASTEBIN_DIR="${PASTEBIN_DIR:-$SCRIPT_DIR}"
SYSTEM_MANAGER_DIR="${SYSTEM_MANAGER_DIR:-$HOME/projects/system-manager}"

# Build the project
log "Building kant-pastebin..."
cd "$PASTEBIN_DIR"
make build

# Commit and push
log "Committing changes..."
git add -A
if ! git diff --quiet; then
    git commit -m "Deploy: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
fi

log "Pushing to origin..."
git push origin main

log "Deployment complete!"