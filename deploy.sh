#!/usr/bin/env bash
set -euo pipefail

# ── deploy.sh — Build, commit, and deploy kant-pastebin ─────────────────────
#
# Usage:
#   ./deploy.sh                    # build, commit, push to current branch
#   ./deploy.sh --target deploy-beta   # build, commit, start beta service
#   ./deploy.sh --target beta          # alias for deploy-beta
#   ./deploy.sh --help               # show this help
#
# Environment overrides:
#   PASTEBIN_DIR       Project root (default: script directory)
#
# What it does:
#   1. Runs `make build` to compile the release binary
#   2. Stages all changes and commits if there are any
#   3. Deploys:
#      - deploy-beta / beta → restarts the kant-pastebin-beta systemd service
#      - <branch>           → pushes to origin/<branch>
#      - (default)          → pushes to the current branch on origin

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
PASTEBIN_DIR="${PASTEBIN_DIR:-$SCRIPT_DIR}"

# ── Logging ─────────────────────────────────────────────────────────────────
log()  { echo "[$(date -u +%Y-%m-%dT%H:%M:%SZ)] $*"; }
warn() { echo "[$(date -u +%Y-%m-%dT%H:%M:%SZ)] WARNING: $*" >&2; }
die()  { echo "[$(date -u +%Y-%m-%dT%H:%M:%SZ)] ERROR: $*" >&2; exit 1; }

# ── Usage ───────────────────────────────────────────────────────────────────
usage() {
    cat <<EOF
Usage: $0 [--target TARGET] [--help]

Deploy kant-pastebin.

Arguments:
  --target TARGET   Deployment target:
                      deploy-beta  — build, commit, start beta service on port 8085
                      beta         — alias for deploy-beta
                      <branch>    — build, commit, push to origin/<branch>
  --help            Show this help message.

Environment overrides:
  PASTEBIN_DIR       Project root (default: script directory)

Examples:
  $0                          # deploy to current branch
  $0 --target deploy-beta     # deploy to beta service
  $0 --target beta            # same as above
  $0 --target main            # push to origin/main
EOF
}

# ── Parse arguments ─────────────────────────────────────────────────────────
DEPLOY_TARGET=""
while [[ $# -gt 0 ]]; do
    case "$1" in
        --target)
            DEPLOY_TARGET="$2"
            shift 2
            ;;
        --target=*)
            DEPLOY_TARGET="${1#*=}"
            shift
            ;;
        --help|-h)
            usage
            exit 0
            ;;
        *)
            die "Unknown option: $1"
            ;;
    esac
done

# ── Build the project ───────────────────────────────────────────────────────
log "Building kant-pastebin..."
cd "$PASTEBIN_DIR"
make build

# ── Commit and push ─────────────────────────────────────────────────────────
log "Committing changes..."
git add -A
if ! git diff --cached --quiet; then
    git commit -m "Deploy: $(date -u +%Y-%m-%dT%H:%Z)"
    log "Changes committed"
else
    log "No changes to commit"
fi

# ── Deploy ──────────────────────────────────────────────────────────────────
case "$DEPLOY_TARGET" in
    deploy-beta|beta)
        log "Deploying to beta port 8085..."
        # Stop any existing beta service if running
        pkill -f "kant-pastebin-beta" 2>/dev/null || true
        sleep 1
        # Start the beta service using systemd
        if ! systemctl start kant-pastebin-beta 2>/dev/null; then
            log "Starting beta service with systemd..."
            systemctl daemon-reload
            systemctl start kant-pastebin-beta
        fi
        log "Beta service started on port 8085"
        ;;
    "")
        # Default: push to current branch
        CURRENT_BRANCH="$(git rev-parse --abbrev-ref HEAD)"
        log "Deploying to server (branch: $CURRENT_BRANCH)..."
        git push origin "$CURRENT_BRANCH"
        ;;
    *)
        log "Deploying to server (branch: $DEPLOY_TARGET)..."
        git push origin "$DEPLOY_TARGET"
        ;;
esac

log "Deployment complete!"
exit 0