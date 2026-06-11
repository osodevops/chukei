#!/usr/bin/env bash
# Integration test: real Snowflake driver → chukei → local "Snowflake".
#
# Snowflake itself cannot run locally (it's a proprietary SaaS), so we use
# fakesnow's server mode: an open-source HTTP server that speaks the actual
# Snowflake REST wire protocol (/session/v1/login-request,
# /queries/v1/query-request, …) backed by DuckDB. The official
# snowflake-connector-python connects to it unmodified — which means this
# script exercises chukei against genuine driver traffic: real login flow,
# real auth headers, real result parsing.
#
# Requirements: python3, cargo. Everything else is installed into a venv.
#
# Usage: scripts/integration-fakesnow.sh
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WORK="$(mktemp -d)"
FAKESNOW_PORT=18910
CHUKEI_PORT=18911
PIDS=()

cleanup() {
  for pid in "${PIDS[@]:-}"; do kill "$pid" 2>/dev/null || true; done
  rm -rf "$WORK"
}
trap cleanup EXIT

echo "── building chukei ──"
cargo build --release --manifest-path "$ROOT/Cargo.toml"

echo "── installing fakesnow + snowflake-connector-python into a venv ──"
python3 -m venv "$WORK/venv"
"$WORK/venv/bin/pip" install --quiet 'fakesnow[server]' snowflake-connector-python

echo "── starting fakesnow server on :$FAKESNOW_PORT ──"
"$WORK/venv/bin/fakesnow" -s -p "$FAKESNOW_PORT" &
PIDS+=($!)
sleep 2

echo "── starting chukei on :$CHUKEI_PORT → fakesnow ──"
cat > "$WORK/chukei.yaml" <<EOF
listen:
  bind: "127.0.0.1:$CHUKEI_PORT"
upstream:
  snowflake:
    account: "fakesnow"
    base_url_override: "http://127.0.0.1:$FAKESNOW_PORT"
plugins:
  cache:
    enabled: true
  rewrite:
    enabled: true
  attribute:
    enabled: true
EOF
RUST_LOG=info "$ROOT/target/release/chukei" up --config "$WORK/chukei.yaml" \
  > "$WORK/chukei.log" 2>&1 &
PIDS+=($!)
sleep 1

echo "── driving the official Python connector through chukei ──"
"$WORK/venv/bin/python" - "$CHUKEI_PORT" <<'PYEOF'
import sys
import snowflake.connector

port = int(sys.argv[1])
conn = snowflake.connector.connect(
    user="fake", password="snow", account="fakesnow",
    host="127.0.0.1", port=port, protocol="http",
    database="test_db", schema="test_schema",
    # fakesnow accepts any credentials and auto-creates the database;
    # chukei passes auth through verbatim
)
cur = conn.cursor()

cur.execute("CREATE OR REPLACE TABLE t (region VARCHAR, amount INT)")
cur.execute("INSERT INTO t VALUES ('emea', 10), ('apac', 20), ('amer', 30)")

# Rewrite bait: OR-chain should reach fakesnow as an IN-list.
cur.execute("SELECT region, amount FROM t WHERE region = 'emea' OR region = 'apac' ORDER BY amount")
rows = cur.fetchall()
assert rows == [('emea', 10), ('apac', 20)], rows

# Same query again: eligible for chukei's cache.
cur.execute("SELECT region, amount FROM t WHERE region = 'emea' OR region = 'apac' ORDER BY amount")
assert cur.fetchall() == rows

# Different literals must NOT come from the first query's cache entry.
cur.execute("SELECT region, amount FROM t WHERE region = 'amer' OR region = 'apac' ORDER BY amount")
assert cur.fetchall() == [('apac', 20), ('amer', 30)]

conn.close()
print("OK: login, DDL, DML, rewritten reads, and cache-correctness all "
      "round-tripped through chukei with the official driver")
PYEOF

# Transport working is not enough — assert chukei actually INTERCEPTED the
# gzipped driver traffic (decoded, rewrote, cache-hit), not just passed it
# through. This is the regression that bit us once already.
echo "── asserting interception (not just passthrough) ──"
REWRITES=$(grep -c "rewrite applied" "$WORK/chukei.log" || true)
HITS=$(grep -c "cache hit" "$WORK/chukei.log" || true)
echo "rewrites applied: $REWRITES, cache hits: $HITS"
if [ "$REWRITES" -lt 1 ] || [ "$HITS" -lt 1 ]; then
  echo "FAIL: chukei did not intercept real-driver traffic (gzip handling regression?)"
  exit 1
fi

echo "── PASS ──"
