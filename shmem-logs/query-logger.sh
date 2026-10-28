#!/bin/bash
# shmem query logger - logs all shmem client commands with timestamps
# Run this in a loop to capture all queries

LOGFILE="/mnt/data1/kant/pastebin/shmem-logs/shmem-queries.log"
CLIENT_LOG="/mnt/data1/kant/pastebin/shmem-logs/client-commands.log"

echo "=== SHMEM QUERY LOGGER STARTED: $(date -u +"%Y-%m-%dT%H:%M:%SZ") ===" >> "$LOGFILE"

# Watch journalctl for new shmem client connections
journalctl -u ipld-car-shmem.service --no-pager -f 2>/dev/null | \
  while IFS= read -r line; do
    if echo "$line" | grep -q "letta-ipld-memory"; then
      TIMESTAMP=$(echo "$line" | sed -n 's/.*\([0-9]\{2\}:[0-9]\{2\}:[0-9]\{2\}\).*/\1/p')
      CMD=$(echo "$line" | grep -o "letta-ipld-memory [a-z]* [^)]*" | sed 's/letta-ipld-memory /shmem /')
      
      echo "$(date -u +"%Y-%m-%dT%H:%M:%SZ") | $CMD" >> "$LOGFILE"
      echo "$CMD" >> "$CLIENT_LOG"
    fi
  done
