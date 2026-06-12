<p align="center">
  <h1 align="center">chukei</h1>
  <p align="center">
    The open-source cost optimization engine for Snowflake — zero client changes
  </p>
</p>

<p align="center">
  <em>中継 — "relay" — the station that sits between you and the warehouse</em>
</p>

---

**chukei** cuts your Snowflake bill automatically. It deploys as a
transparent wire-protocol proxy in your own VPC: your drivers (JDBC,
snowflake-connector-python, dbt, …) change one hostname and nothing else,
and chukei transparently:

1. **Caches** semantically-equivalent query results.
2. **Routes** small reads to embedded DuckDB on Iceberg replicas.
3. **Rewrites** suboptimal SQL with a deterministic rule pack.
4. **Right-sizes** the destination warehouse *(P1)*.
5. **Predicts** idle windows and recommends early auto-suspend.
6. **Attributes** cost back to query → user → team → DAG node.

No client changes. No SaaS sign-up. One binary.

> Status: **pilot-ready** — live-validated against real Snowflake: TLS,
> four auth modes, async/long-running queries, JDBC, enforce-mode suspend
> (real `ALTER WAREHOUSE SUSPEND`, verified in `QUERY_HISTORY`), and a
> 13.5-hour soak with 60k verified cache hits and **zero** mismatches.
> Iceberg/Arrow cache backing and DuckDB routing are post-pilot. Full
> spec: [`docs/chukei_prd.md`](docs/chukei_prd.md); deployment guide:
> [`docs/deployment.md`](docs/deployment.md).

## Try the replay simulator (no Snowflake account changes)

Project savings from 30 days of query history before installing anything:

```bash
# Export SNOWFLAKE.ACCOUNT_USAGE.QUERY_HISTORY to CSV, then:
chukei replay --query-history queries.csv --output report.json --evidence
```

You get parse coverage, projected cache hit-rate, routable queries, rewrite
opportunities, suspend savings — and an ECDSA-signed report.

## Run the proxy

```bash
chukei validate config --file config/chukei-example.yaml
chukei up --config config/chukei-example.yaml
# then point your driver at chukei instead of *.snowflakecomputing.com
```

## Development

```bash
cargo build && cargo test
cargo clippy --all-targets --all-features -- -D warnings
```

## License

MIT — same as [kafka-backup](https://github.com/osodevops/kafka-backup).

---

<p align="center">
  <em>Made with ❤️ by <a href="https://oso.sh">OSO</a></em>
</p>
