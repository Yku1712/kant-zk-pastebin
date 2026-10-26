#!/bin/bash
# shmem service monitor - runs continuously to monitor service health
# and log context data

LOGFILE="/mnt/data1/kant/pastebin/shmem-logs/monitor-log.txt"
WRAPPER="/mnt/data1/kant/pastebin/shmem-context-logs/shmem-call-wrapper.sh"
INTERVAL=10

mkdir -p "$(dirname "$LOGFILE")"

echo "=== SHMEM SERVICE MONITOR STARTED $(date -u) ===" >> "$LOGFILE"

while true; do
    # 1. Service health
    SERVICE_STATUS=$(systemctl is-active ipld-car-shmem.service 2>/dev/null)
    echo "[$(date -u)] SERVICE_STATUS: $service_status" >> "$LOGFILE"

    # 2. Socket check
    SOCKET_LISTENING=$(ss -xlp | grep -c ipld_car_shmem 2>/dev/null || echo "0")
    echo "[$(date -u)] SOCKET: ${SOCKET_LISTENING} sockets listening" >> "$LOGFILE"

    # 3. HTTP API check
    HTTP_CODE=$(curl -s -o /dev/null -w "%{http_code}" http://127.0.0.1:8150/health 2>/dev/null)
    echo "[$(date -u)] HTTP_HEALTH: $HTTP_CODE" >> "$LOGFILE"

    # 4. Block count
    BLOCK_COUNT=$(/home/mdupont/dasl/ipld-car-ipc-shmem-linux/target/release/letta-ipld-memory stats 2>/dev/null | python3 -c "import json,sys; d=json.load(sys.stdin); print(d.get('block_entries',0))" 2>/dev/null)
    echo "[$(date -u)] BLOCKS: $BLOCK_COUNT" >> "$LOGFILE"

    # 5. Memory Usage
    MEM_USAGE=$(systemctl show -p MemoryCurrent ipld-car-shmem.service 2>/dev/null | cut -d= -f2)
    echo "[$(date -u)] MEMORY_BYTES: $MEM_USAGE" >> "$LOGFILE"

    echo "---" >> "$LOGFILE"
    sleep "$INTERVAL"
done
