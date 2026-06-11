# Deploying chukei: customer pilot guide

chukei is a transparent proxy in front of `*.snowflakecomputing.com`. Your
drivers keep their credentials and their SQL; they just point at a
different host. Any chukei-side failure degrades to verbatim passthrough,
and rollback is repointing the driver back — chukei holds no state your
queries depend on.

## 1. Install

Pick one:

```bash
docker pull osodevops/chukei:latest          # distroless, runs as non-root
brew install osodevops/tap/chukei            # macOS
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/osodevops/chukei/releases/latest/download/chukei-cli-installer.sh | sh
```

## 2. Configure

Start from [`config/customer-pilot.yaml`](../config/customer-pilot.yaml) —
the conservative profile: cache with strict determinism gate and 5% blame
sampling, no result data persisted at rest, warehouse suspend in
suggest-only, router off. Three values to change: TLS cert/key, account
locator, your $/credit rate.

**TLS**: clients must trust the cert for the hostname they'll use
(e.g. `chukei.internal.example.com`). An internal-CA cert is the normal
choice. JDBC enforces OCSP against the proxy cert; add `ocspFailOpen=true`
to the JDBC URL for the pilot (the Python connector and snowsql soft-fail
by default — validated against Snowflake 10.20).

Pre-flight before any client touches it:

```bash
chukei doctor --config chukei.yaml
# ✓ config ✓ tls ✓ upstream ✓ listen ✓ savings ✓ cache
```

## 3. Run

```bash
# docker
docker run -d --restart unless-stopped \
  -p 8443:8443 -p 9090:9090 \
  -v /etc/chukei:/etc/chukei:ro -v /var/lib/chukei:/var/lib/chukei \
  osodevops/chukei:latest up --config /etc/chukei/chukei.yaml
```

or `deploy/docker-compose.yaml`, or `deploy/k8s.yaml` (liveness/readiness
on `/healthz`, one replica — the cache is per-instance). chukei drains
gracefully on SIGTERM.

## 4. Point clients at it — a subset first

Pilot with **one team or workload via explicit host override**, not an
account-wide DNS cutover. The account name stays the same everywhere;
only the host changes.

```python
# Python / dbt profiles.yml
snowflake.connector.connect(..., account="abc12345.eu-west-2.aws",
                            host="chukei.internal.example.com", port=8443)
```

```toml
# ~/.snowflake/config.toml (snowsql / Snowflake CLI)
[connections.chukei]
account = "abc12345.eu-west-2.aws"
host    = "chukei.internal.example.com"
port    = 8443
```

```text
# JDBC
jdbc:snowflake://chukei.internal.example.com:8443/?account=abc12345...&ocspFailOpen=true
```

Auth modes validated end-to-end through chukei: password, key-pair (JWT),
programmatic access tokens. `externalbrowser`/SSO: the browser leg goes
directly to Snowflake; only the token exchange transits chukei.

## 5. Watch (the alert table)

`:9090/metrics`, Prometheus format.

| Signal | Threshold | Action |
|---|---|---|
| `chukei_cache_blame_mismatches_total` | > 0 | **Page.** Set `CHUKEI_PLUGINS_CACHE_ENABLED=false` and restart (queries keep flowing, uncached). File a bug with the fingerprint from the log. |
| `chukei_circuit_breaker_fast_fails_total` | climbing | Snowflake-side trouble; chukei is shedding load to protect clients. Check Snowflake status. |
| p99 `chukei_proxy_overhead_seconds` | > 0.005 | File a bug with `/metrics` output. |
| `/healthz` | non-200 | Restart policy should handle it; if flapping, roll back and report. |

## 6. Rollback (rehearse it once)

Point the driver host back at `<account>.snowflakecomputing.com`. That's
it — chukei holds no client credentials and no state a rollback depends
on. Per-plugin kill switches need only an env var and restart:
`CHUKEI_PLUGINS_CACHE_ENABLED=false`, `CHUKEI_PLUGINS_REWRITE_ENABLED=false`,
`CHUKEI_PLUGINS_SUSPEND_ENABLED=false`.

**If chukei itself dies**, clients pointed at it retry with backoff and
then error — they do not silently fall back to Snowflake. This is why the
restart policy in §3 is mandatory and the pilot scope in §4 is a subset.

## 7. Known pilot limitations (by design)

- Large chunked results are never cached — chunk downloads go directly
  from the driver to Snowflake's presigned cloud-storage URLs. They pass
  through unmodified (validated to 200k rows).
- Suspend stays suggest-only in week one. Enforce mode — most of the
  savings — needs a `CHUKEI_SUSPENDER` service account and a review of the
  suggest log. In simulation it was 94% of total savings.
- One chukei instance per deployment for now (per-instance cache).
- The savings ledger deliberately under-claims (×0.7) and says so in
  every report; reconcile against `WAREHOUSE_METERING_HISTORY` after a
  week.

## 8. What the pilot produces

```bash
chukei savings --config chukei.yaml --since 7d            # running total
chukei savings ... --evidence report.evidence.json        # Ed25519-signed
chukei replay --query-history queries.csv --output projection.json
```

`savings` is what chukei actually avoided; `replay` projects 30 days from
your `QUERY_HISTORY` export. Both carry the methodology string.
