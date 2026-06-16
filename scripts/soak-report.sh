#!/usr/bin/env bash
# Morning-after soak gate. Exits non-zero if any go/no-go gate is red.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DATA="$ROOT/pilot-data"
FAIL=0
# pilot.yaml interpolates ${SNOWFLAKE_ACCOUNT}
[ -f "$ROOT/pilot.env" ] && { set -a; source "$ROOT/pilot.env"; set +a; }

echo "── soak gates ──"

MISMATCH=$(curl -sf localhost:19090/metrics | awk '/^chukei_cache_blame_mismatches_total/ {print $2}' || echo "metrics-unreachable")
if [ "$MISMATCH" = "metrics-unreachable" ]; then
  echo "✗ chukei not reachable — did it crash overnight? check $DATA/chukei.log"; FAIL=1
elif [ "${MISMATCH%.*}" != "0" ] && [ -n "$MISMATCH" ]; then
  echo "✗ blame mismatches: $MISMATCH (must be 0)"; FAIL=1
else
  echo "✓ blame mismatches: ${MISMATCH:-0}"
fi

if [ -f "$DATA/soak-rss.csv" ]; then
  FIRST=$(head -1 "$DATA/soak-rss.csv" | cut -d, -f2)
  LAST=$(tail -1 "$DATA/soak-rss.csv" | cut -d, -f2)
  PEAK=$(cut -d, -f2 "$DATA/soak-rss.csv" | sort -n | tail -1)
  SAMPLES=$(wc -l < "$DATA/soak-rss.csv" | tr -d ' ')
  echo "  rss: first ${FIRST}KB → last ${LAST}KB (peak ${PEAK}KB over $SAMPLES min)"
  if [ "$LAST" -gt $((FIRST * 3 + 100000)) ]; then
    echo "✗ rss grew >3×: possible leak"; FAIL=1
  else
    echo "✓ rss flat enough"
  fi
fi

ERRS=$(grep -c "error:" "$DATA/soak-traffic.log" 2>/dev/null || true)
OKS=$(grep -oE "ok=[0-9]+" "$DATA/soak-traffic.log" 2>/dev/null | tail -1 || echo "ok=in-progress")
echo "  traffic: $OKS, client errors: ${ERRS:-0}"

PANICS=$(grep -ci "panic" "$DATA/chukei.log" || true)
if [ "${PANICS:-0}" != "0" ]; then echo "✗ panics in chukei log: $PANICS"; FAIL=1; else echo "✓ no panics"; fi

ROTATIONS=$(grep -ci "token.*renew\|session.*rekey\|rotat" "$DATA/chukei.log" || true)
echo "  session rotation events observed: ${ROTATIONS:-0}"

echo "── savings after soak ──"
"$ROOT/target/release/chukei" savings --config "$DATA/pilot.yaml" --since 24h || true

[ "$FAIL" = "0" ] && echo "── SOAK GATES: ALL GREEN ──" || { echo "── SOAK GATES: RED ──"; exit 1; }
