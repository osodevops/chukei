#!/usr/bin/env bash
# Overnight soak: start chukei via the live harness (--keep-up), drive
# mixed traffic with soak_traffic.py, sample chukei RSS every 60s.
#
# Usage:  scripts/soak.sh [hours]      (default 8)
# Stop:   touch pilot-data/soak.stop
# Report: scripts/soak-report.sh
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DATA="$ROOT/pilot-data"
export SOAK_HOURS="${1:-8}"
export SOAK_STOP_FILE="$DATA/soak.stop"
rm -f "$SOAK_STOP_FILE"

"$ROOT/scripts/live-pilot.sh" --tls --stages core --keep-up

CHUKEI_PID=$(pgrep -f "chukei up --config $DATA/pilot.yaml" | head -1)
[ -n "$CHUKEI_PID" ] || { echo "FAIL: chukei not running after harness"; exit 1; }
echo "soak: chukei pid $CHUKEI_PID"

set -a; source "$ROOT/pilot.env"; set +a
export PILOT_TEST_PROTOCOL=https PILOT_TEST_HOST=localhost PILOT_TEST_PORT=18443 PILOT_TEST_AUTH=password

# RSS sampler (CSV: unix_ts,rss_kb) — dies with the soak driver.
(
  while kill -0 "$CHUKEI_PID" 2>/dev/null && [ ! -f "$SOAK_STOP_FILE" ]; do
    echo "$(date +%s),$(ps -o rss= -p "$CHUKEI_PID" | tr -d ' ')" >> "$DATA/soak-rss.csv"
    sleep 60
  done
) &
SAMPLER=$!

"$DATA/venv/bin/python" -u "$ROOT/scripts/soak_traffic.py" 2>&1 | tee "$DATA/soak-traffic.log"
kill "$SAMPLER" 2>/dev/null || true
echo "soak: traffic finished; chukei (pid $CHUKEI_PID) left running for the morning report"
