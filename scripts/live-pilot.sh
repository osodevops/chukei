#!/usr/bin/env bash
# Live pilot: official Snowflake driver → chukei → YOUR REAL Snowflake account.
#
# Runs the docs/live_pilot.md verification matrix (scripts/live_matrix.py)
# end-to-end, then asserts chukei-side SLOs: interception happened, and
# chukei_cache_blame_mismatches_total == 0.
#
# Credentials come from pilot.env (gitignored) — never logged, never
# persisted by chukei (invariant #4).
#
# Usage: scripts/live-pilot.sh [options]
#   --tls            terminate TLS at chukei (pilot CA in pilot-data/tls,
#                    trusted only inside the test venv) instead of loopback HTTP
#   --auth MODE      password (default) | keypair | pat
#   --stages "a b"   core (default) | shapes | concurrency — space-separated
#   --keep-up        leave chukei running after tests (soak / ad-hoc traffic)
#   --log-level LVL  RUST_LOG for chukei (default info; trace for leak audit)
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DATA="$ROOT/pilot-data"
CHUKEI_PORT=18920
TLS_PORT=18443
METRICS_PORT=19090
KEEP_UP=0 TLS=0 AUTH=password STAGES="core" LOG_LEVEL=info
PIDS=()

while [ $# -gt 0 ]; do
  case "$1" in
    --keep-up) KEEP_UP=1 ;;
    --tls) TLS=1 ;;
    --auth) AUTH="$2"; shift ;;
    --stages) STAGES="$2"; shift ;;
    --log-level) LOG_LEVEL="$2"; shift ;;
    *) echo "unknown flag: $1"; exit 2 ;;
  esac
  shift
done

cleanup() {
  if [ "$KEEP_UP" != "1" ]; then
    for pid in "${PIDS[@]:-}"; do kill "$pid" 2>/dev/null || true; done
  fi
}
trap cleanup EXIT

[ -f "$ROOT/pilot.env" ] || { echo "FAIL: pilot.env missing — cp pilot.env.example pilot.env and fill it in"; exit 1; }
set -a; source "$ROOT/pilot.env"; set +a
for v in SNOWFLAKE_ACCOUNT SNOWFLAKE_USER SNOWFLAKE_PASSWORD; do
  [ -n "${!v:-}" ] || { echo "FAIL: $v is empty in pilot.env"; exit 1; }
done

mkdir -p "$DATA"

echo "── building chukei ──"
cargo build --release --manifest-path "$ROOT/Cargo.toml"

if [ "$TLS" = "1" ]; then
  [ -f "$DATA/tls/chukei.crt" ] || { echo "FAIL: pilot-data/tls/chukei.crt missing"; exit 1; }
  LISTEN_PORT=$TLS_PORT
  TLS_YAML="
  tls:
    cert: \"$DATA/tls/chukei.crt\"
    key: \"$DATA/tls/chukei.key\""
  export PILOT_TEST_PROTOCOL=https PILOT_TEST_HOST=localhost PILOT_TEST_PORT=$TLS_PORT
else
  LISTEN_PORT=$CHUKEI_PORT
  TLS_YAML=""
  export PILOT_TEST_PROTOCOL=http PILOT_TEST_HOST=127.0.0.1 PILOT_TEST_PORT=$CHUKEI_PORT
fi
export PILOT_TEST_AUTH="$AUTH"
if [ "$AUTH" = "keypair" ]; then
  [ -f "$DATA/keypair/rsa_key.p8" ] || { echo "FAIL: pilot-data/keypair/rsa_key.p8 missing (register its .pub via ALTER USER first)"; exit 1; }
  export PILOT_TEST_KEY="$DATA/keypair/rsa_key.p8"
fi
if [ "$AUTH" = "pat" ] && [ -z "${PILOT_TEST_PAT:-}" ]; then
  echo "FAIL: pat mode needs PILOT_TEST_PAT in pilot.env"; exit 1
fi

echo "── writing pilot config (account: ${SNOWFLAKE_ACCOUNT%%.*}…, tls=$TLS, auth=$AUTH) ──"
cat > "$DATA/pilot.yaml" <<EOF
listen:
  bind: "127.0.0.1:$LISTEN_PORT"$TLS_YAML
upstream:
  snowflake:
    account: "\${SNOWFLAKE_ACCOUNT}"
storage:
  backend: filesystem
  path: "$DATA"
coalesce:
  enabled: true
  scope: session
savings:
  enabled: true
  db_path: "$DATA/savings.db"
  usd_per_credit: 3.0
  conservative_factor: 0.7
  default_warehouse_size: XS
plugins:
  cache:
    enabled: true
    default_ttl_secs: 900
    determinism_gate: strict
    blame_sample_rate: 0.25
    persist_path: "$DATA/cache"
  rewrite:
    enabled: true
    rules: [all]
  suspend:
    enabled: true
    mode: suggest-only
    sweep_interval_secs: 30
  attribute:
    enabled: true
observability:
  prometheus:
    enabled: true
    port: $METRICS_PORT
EOF

echo "── doctor pre-flight ──"
"$ROOT/target/release/chukei" doctor --config "$DATA/pilot.yaml"

echo "── starting chukei on :$LISTEN_PORT → $SNOWFLAKE_ACCOUNT ──"
: > "$DATA/chukei.log"
RUST_LOG="$LOG_LEVEL" "$ROOT/target/release/chukei" up --config "$DATA/pilot.yaml" \
  >> "$DATA/chukei.log" 2>&1 &
PIDS+=($!)
sleep 1

if [ ! -d "$DATA/venv" ]; then
  echo "── installing snowflake-connector-python into a venv ──"
  python3 -m venv "$DATA/venv"
  "$DATA/venv/bin/pip" install --quiet snowflake-connector-python
fi

echo "── driving the official Python connector through chukei (live) ──"
# shellcheck disable=SC2086
"$DATA/venv/bin/python" "$ROOT/scripts/live_matrix.py" $STAGES

echo "── metrics + blame SLO ──"
METRICS=$(curl -sf "localhost:$METRICS_PORT/metrics")
echo "$METRICS" | grep -E "chukei_(queries_total|cache_hits)" | head -5
MISMATCH=$(echo "$METRICS" | awk '/^chukei_cache_blame_mismatches_total/ {print $2}')
if [ -n "${MISMATCH:-}" ] && [ "${MISMATCH%.*}" != "0" ]; then
  echo "FAIL: blame mismatches = $MISMATCH — cache served a wrong result. Disable cache, file a bug."
  exit 1
fi
echo "  ✓ blame mismatches: ${MISMATCH:-0}"

case " $STAGES " in *" core "*)
  echo "── asserting interception (not just passthrough) ──"
  REWRITES=$(grep -c "rewrite applied" "$DATA/chukei.log" || true)
  HITS=$(grep -c "cache hit" "$DATA/chukei.log" || true)
  echo "  rewrites applied: $REWRITES, cache hits: $HITS"
  if [ "$REWRITES" -lt 1 ] || [ "$HITS" -lt 1 ]; then
    echo "FAIL: chukei did not intercept live-driver traffic"
    exit 1
  fi ;;
esac

echo "── credential-leak audit (log, cache dir, ledger) ──"
for needle in "$SNOWFLAKE_PASSWORD" "${PILOT_TEST_PAT:-__unset__}"; do
  [ "$needle" = "__unset__" ] && continue
  if grep -rqF -- "$needle" "$DATA/chukei.log" "$DATA/cache" "$DATA/savings.db" 2>/dev/null; then
    echo "FAIL: a credential value appears in chukei artifacts"
    exit 1
  fi
done
echo "  ✓ no credential values in log/cache/ledger (log level: $LOG_LEVEL)"

echo "── realized savings so far ──"
"$ROOT/target/release/chukei" savings --config "$DATA/pilot.yaml" --since 24h || true

echo "── PASS: live matrix [$STAGES] green (tls=$TLS auth=$AUTH) against $SNOWFLAKE_ACCOUNT ──"
[ "$KEEP_UP" = "1" ] && echo "chukei left running on :$LISTEN_PORT (metrics :$METRICS_PORT, log $DATA/chukei.log)"
exit 0
