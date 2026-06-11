# Live Snowflake pilot runbook

Goal: run real traffic through chukei against a live Snowflake account and
walk away with a **signed realized-savings report**. Budget ~2 hours.
Everything below is reversible at any moment by pointing the driver back at
`*.snowflakecomputing.com`.

## 0. What you need

- A Snowflake account locator (e.g. `abc12345.us-east-1`) and a user that
  can run normal queries (your own is fine — auth passes through).
- A host to run chukei on (your laptop works for a pilot).
- A TLS certificate the *client machine trusts* for the hostname you'll
  point drivers at. For a pilot the easy path is an internal CA cert for
  `chukei.internal.<company>.com`, or `mkcert` locally:

  ```bash
  mkcert -install
  mkcert chukei.local localhost 127.0.0.1   # → chukei.local+2.pem / -key.pem
  echo "127.0.0.1 chukei.local" | sudo tee -a /etc/hosts
  ```

- Optional, for `suspend.mode: enforce` later: a service account +
  `CHUKEI_SUSPENDER` role with `OPERATE` on target warehouses:

  ```sql
  CREATE ROLE IF NOT EXISTS CHUKEI_SUSPENDER;
  GRANT OPERATE ON WAREHOUSE <wh> TO ROLE CHUKEI_SUSPENDER;
  CREATE USER chukei_svc PASSWORD='…' DEFAULT_ROLE=CHUKEI_SUSPENDER;
  GRANT ROLE CHUKEI_SUSPENDER TO USER chukei_svc;
  ```

## 1. Configure and pre-flight

```bash
cargo build --release
cp config/chukei-example.yaml pilot.yaml
# edit pilot.yaml:
#   upstream.snowflake.account: <your locator>
#   listen.tls.cert/key: the mkcert files
#   savings.warehouse_sizes: your real sizes (for accurate $ figures)
#   plugins.suspend.mode: suggest-only   ← keep for the first hours

./target/release/chukei doctor --config pilot.yaml
# expect: ✓ config ✓ tls ✓ upstream ✓ listen ✓ savings
# with a service account: chukei doctor --config pilot.yaml --probe-login
```

`doctor` exercising "upstream" proves DNS + TCP + TLS to your real account.

## 2. Start the proxy and point one client at it

```bash
RUST_LOG=info ./target/release/chukei up --config pilot.yaml
```

**Python connector / dbt profiles.yml:**

```python
snowflake.connector.connect(
    user=..., password=..., account="abc12345.us-east-1",
    host="chukei.local", port=8443,      # ← the only change
)
```

**snowsql / Snowflake CLI** (`~/.snowflake/config.toml`):

```toml
[connections.chukei]
account  = "abc12345.us-east-1"
user     = "you"
host     = "chukei.local"
port     = 8443
```

**JDBC:** `jdbc:snowflake://chukei.local:8443/?account=abc12345...`
(if the driver enforces OCSP against the proxy cert, add
`&ocspFailOpen=true` for the pilot — documented Greybeam-pattern caveat,
PRD §8.5).

## 3. Verify each stage (10 minutes)

```bash
# transport + session
snowsql -c chukei -q "SELECT CURRENT_USER(), CURRENT_WAREHOUSE()"

# observability
curl -s localhost:9090/metrics | grep chukei_queries_total

# cache: run any deterministic SELECT twice; second should log "cache hit"
snowsql -c chukei -q "SELECT COUNT(*) FROM <small_table>"
snowsql -c chukei -q "SELECT COUNT(*) FROM <small_table>"
curl -s localhost:9090/metrics | grep -E "cache_hits|blame_mismatch"

# CRITICAL SLO: chukei_cache_blame_mismatches_total must stay 0.
```

Auth-mode matrix to tick off (F-002): password ✓ above; then repeat the
login with key-pair (`private_key_path`), `authenticator=externalbrowser`
(the browser flow goes direct to Snowflake — only token exchange passes
through chukei), and a PAT.

Known pilot limits (by design, fail to passthrough): chunked large results
are never cached (chunk URLs go driver→S3 directly), and any parse/gzip
hiccup degrades to byte-identical passthrough.

## 4. Let it soak, then pull the savings report

Leave a dashboard, dbt schedule, or your team's ad-hoc traffic on it for a
few hours. Then:

```bash
./target/release/chukei savings --config pilot.yaml --since 24h
./target/release/chukei evidence keygen --out pilot-signing.key   # once
# add to pilot.yaml: evidence.signing.enabled: true + private_key_path
./target/release/chukei savings --config pilot.yaml --since 24h \
    --evidence pilot-savings.evidence.json
./target/release/chukei evidence verify --file pilot-savings.evidence.json
```

The figures use the conservative methodology (canonical wall-clock ×
credit rate × 0.7), and the report says so. To reconcile against the real
bill, compare warehouse credits for the pilot window in
`SNOWFLAKE.ACCOUNT_USAGE.WAREHOUSE_METERING_HISTORY` with the prior week.

For the bigger headline number, also run the 30-day projection:

```bash
# export ACCOUNT_USAGE.QUERY_HISTORY (30 days) to queries.csv, then:
./target/release/chukei replay --query-history queries.csv \
    --output projection.json --evidence
```

## 5. (Optional) turn on enforce-mode suspend

Only after `suggest-only` logs look sane for a day:

```yaml
service_account: { user: chukei_svc, password: "${CHUKEI_SVC_PASSWORD}", role: CHUKEI_SUSPENDER }
plugins.suspend: { enabled: true, mode: enforce, role: CHUKEI_SUSPENDER }
```

Watch `chukei_suspends_executed_total` and the `suspend` line in
`chukei savings`. In our 30-day simulation this was **94% of total
savings** — it's the lever that makes the pilot number big.

## 6. Rollback

Point the driver host back at `<account>.snowflakecomputing.com`. chukei
holds no client credentials and no state a rollback depends on.

## Escalation signals

| Signal | Action |
|---|---|
| `chukei_cache_blame_mismatches_total` > 0 | Disable cache, keep proxying, file a bug with the fingerprint from the log |
| `chukei_circuit_breaker_fast_fails_total` climbing | Upstream trouble; chukei is protecting clients — check Snowflake status |
| Driver errors mentioning OCSP/certificates | Client cert trust issue — see §2 JDBC note |
| p99 `chukei_proxy_overhead_seconds` > 0.005 | File a bug with `/metrics` output; budget regression |
