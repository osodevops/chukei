<p align="center">
  <h1 align="center">chukei</h1>
  <p align="center">
    The fair-source cost optimization engine for Snowflake — zero client changes
  </p>
</p>

<p align="center">
  <a href="https://github.com/osodevops/chukei/actions/workflows/test.yml">
    <img src="https://github.com/osodevops/chukei/actions/workflows/test.yml/badge.svg" alt="CI Status">
  </a>
  <a href="https://github.com/osodevops/chukei/blob/main/LICENSE">
    <img src="https://img.shields.io/badge/license-FSL--1.1--ALv2-blue.svg" alt="License: FSL-1.1-ALv2">
  </a>
  <a href="https://github.com/osodevops/chukei/releases">
    <img src="https://img.shields.io/github/v/release/osodevops/chukei" alt="Release">
  </a>
</p>

<p align="center">
  <em>中継 — "relay" — the station that sits between you and the warehouse</em>
</p>

---

**chukei** is a production-grade engine written in Rust that cuts your Snowflake bill automatically. It deploys as a **transparent wire-protocol proxy** in your own VPC: your drivers (JDBC, snowflake-connector-python, dbt, …) change one hostname and nothing else. Every optimization is **deterministic** — no LLM on the hot path — and every avoided dollar lands in a **cryptographically signed savings ledger**.

## Features

- **Verified result caching** — deterministic reads served from cache and continuously double-checked against live Snowflake (60k hits, zero mismatches in soak)
- **Warehouse auto-suspend** — a Poisson idle model suggests or executes suspends; ~94% of total savings in 30-day simulation
- **SQL rewriting** — equivalence-tested rules turn expensive query shapes into cheaper ones inline
- **Per-team cost attribution** — every query attributed to team, BI tool, or dbt model at the wire; no tagging discipline required
- **Signed savings evidence** — Ed25519-signed reports with a conservative, auditable methodology
- **Replay simulator** — project 30-day savings from a `QUERY_HISTORY` export before deploying anything
- **Fail open by design** — parse errors, cache misses, plugin panics all degrade to byte-identical passthrough
- **~2 ms p99 overhead** — deterministic Rust hot path, +5 ms budget enforced as an alert
- **Deployment agnostic** — single static binary, distroless Docker, or Kubernetes

## Installation

Download the latest binary from the [GitHub Releases](https://github.com/osodevops/chukei/releases) page.

### macOS (Homebrew)

```bash
brew install osodevops/tap/chukei
```

### Linux / macOS (Shell Installer)

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/osodevops/chukei/releases/latest/download/chukei-cli-installer.sh | sh
```

### Windows (PowerShell Installer)

```powershell
powershell -ExecutionPolicy ByPass -c "irm https://github.com/osodevops/chukei/releases/latest/download/chukei-cli-installer.ps1 | iex"
```

### Windows (Scoop)

```powershell
scoop bucket add oso https://github.com/osodevops/scoop-bucket.git
scoop install chukei
```

### Docker

```bash
docker pull osodevops/chukei
docker run --rm -p 8443:8443 -p 9090:9090 \
  -v /etc/chukei:/etc/chukei:ro osodevops/chukei up --config /etc/chukei/chukei.yaml
```

See the image on [Docker Hub](https://hub.docker.com/r/osodevops/chukei).

### From Source

```bash
git clone https://github.com/osodevops/chukei.git
cd chukei
cargo build --release
```

Binary location: `target/release/chukei`

## Try It Yourself (no Snowflake changes)

Project savings from 30 days of your own query history before installing anything in the query path:

```bash
# Export SNOWFLAKE.ACCOUNT_USAGE.QUERY_HISTORY to CSV, then:
chukei replay --query-history queries.csv --output projection.json --evidence
```

You get parse coverage, projected cache hit-rate, rewrite candidates, suspend savings, and an Ed25519-signed projection anyone can verify with `chukei evidence verify`.

## Quick Start

Create `chukei.yaml` (or start from the [conservative pilot profile](config/customer-pilot.yaml)):

```yaml
listen:
  bind: "127.0.0.1:8443"
upstream:
  snowflake:
    account: "abc12345.eu-west-2.aws"   # your locator; no credentials here, ever
savings:
  enabled: true
  db_path: "./savings.db"
plugins:
  cache:   { enabled: true }
  rewrite: { enabled: true }
  suspend: { enabled: true, mode: suggest-only }
  attribute: { enabled: true }
observability:
  prometheus: { enabled: true, port: 9090 }
```

Pre-flight, run, and point one client at it:

```bash
chukei doctor --config chukei.yaml   # ✓ config ✓ upstream ✓ listen ✓ savings
chukei up --config chukei.yaml
```

```python
snowflake.connector.connect(
    user=..., password=..., account="abc12345.eu-west-2.aws",
    host="127.0.0.1", port=8443, protocol="http",   # ← the only change
)
```

```bash
chukei savings --config chukei.yaml --since 24h
```

## Why chukei?

| | chukei | Keebo | Espresso AI | Sundeck | Snowflake native | dbt monitoring packages |
|---------|:---:|:---:|:---:|:---:|:---:|:---:|
| **Zero client changes** | Yes (hostname only) | Agent/integration | Integration | Yes (proxy) | — | No |
| **Verified result caching** | Yes (blame-checked) | No | No | No | Exact-text, 24h, session-sensitive | No |
| **Auto-suspend** | Predictive, suggest→enforce | Yes | No | No | Static timeout | No |
| **Inline SQL rewriting** | Yes (deterministic rules) | Query tuning | ML suggestions | Routing/guardrails | No | No |
| **Cost attribution** | Wire-level, per dbt model | Tags | No | Partial | Warehouse-level | Model-level (read-only) |
| **Signed savings evidence** | Yes (Ed25519) | No | No | No | No | No |
| **Deterministic hot path** | Yes (no LLM inline) | ML-driven | LLM-driven | Yes | — | — |
| **Self-hosted / your VPC** | Yes | SaaS | SaaS | SaaS | — | Yes |
| **License** | FSL 1.1 → Apache-2.0 | Commercial | Commercial | Commercial | Included | OSS (dashboards only) |

**chukei is the only option that combines verified caching, predictive suspend, and wire-level attribution in a single fair-source binary you run yourself — with the savings cryptographically provable.**

## When NOT to use chukei

- **You want advice, not automation** — dashboards like [dbt-snowflake-monitoring](https://github.com/get-select/dbt-snowflake-monitoring) are simpler if a human will act on the findings
- **Sub-millisecond latency budgets** — chukei adds ~2 ms p99; if that matters more than the bill, stay direct
- **Large-result caching** — chunked results stream driver→cloud-storage directly and are never cached (they pass through untouched)
- **Databricks today** — the wire adapter is on the [roadmap](docs/chukei_prd.md) (v0.2+); Snowflake is the validated target

## Documentation

Full docs at **[docs.chukei.dev](https://docs.chukei.dev)**.

| Document | Description |
|----------|-------------|
| [Quickstart](https://docs.chukei.dev/getting-started/quickstart) | First savings report in 10 minutes |
| [Production Pilot Guide](https://docs.chukei.dev/deployment/production-pilot) | TLS, subset cutover, alerts, rehearsed rollback |
| [Is a proxy safe?](https://docs.chukei.dev/architecture/proxy-safety) | Every objection, answered with measured numbers |
| [Production Validation](https://docs.chukei.dev/benchmarks/production-validation) | The live test matrix, soak results, signed evidence |
| [Configuration Reference](https://docs.chukei.dev/reference/configuration) | All options incl. suspend model gates |
| [Examples](https://docs.chukei.dev/examples/python-connector) | Python, dbt, JDBC (with the OCSP and truststore gotchas) |

## CLI Reference

```bash
chukei up --config chukei.yaml            # start the engine
chukei doctor --config chukei.yaml        # pre-flight probes (add --probe-login for service account)
chukei replay --query-history q.csv       # offline savings projection
chukei savings --since 7d [--evidence f]  # realized savings (optionally signed)
chukei evidence keygen|verify             # signing keys / verify bundles
chukei validate config --file f           # schema-check a config
chukei plugins list                       # list plugins and status
chukei healthcheck                        # probe /healthz (for containers)
```

## Production validation

Every claim above is measured, not promised — against a live Snowflake account with official drivers: TLS, four auth modes (password, key-pair, PAT, SSO), async/long-running queries, JDBC, PUT/GET, 12-way concurrency, a 13.5-hour soak (~120k queries, zero cache mismatches, flat memory), kill-mid-traffic drills, and real `ALTER WAREHOUSE SUSPEND` executions verified in `QUERY_HISTORY`. Reproduce it against your own account: [`scripts/live-pilot.sh`](scripts/live-pilot.sh). Full numbers: [production validation](https://docs.chukei.dev/benchmarks/production-validation).

## Enterprise

The fair-source engine ships under FSL-1.1-ALv2. For teams that want more, [OSO](https://oso.sh) offers:

| Category | Capability |
|----------|------------|
| **Scale** | Kubernetes operator (CRDs), multi-instance shared cache (Iceberg backend) |
| **Governance** | RBAC on cache namespaces, SSO/OIDC, audit log shipping |
| **Optimization** | DuckDB read routing on Iceberg replicas, advanced rewrite packs |
| **Support** | 24/7 SLA-backed support and dedicated Snowflake cost consulting |

👉 **[Talk with an expert today](https://oso.sh/contact/)** or email **enquiries@oso.sh**.

## Contributing

We welcome contributions of all kinds!

- **Report bugs:** Open an [issue on GitHub](https://github.com/osodevops/chukei/issues)
- **Suggest features:** [Request a feature](https://github.com/osodevops/chukei/issues)
- **Improve docs:** PRs against [chukei-docs](https://github.com/osodevops/chukei-docs) welcome

See [CLAUDE.md](CLAUDE.md) for development guidelines and the non-negotiable invariants (fail open, deterministic hot path, false-positive-intolerant cache).

## License

chukei is licensed under the [Functional Source License 1.1, ALv2 Future License](LICENSE) © [OSO](https://oso.sh). Each release converts to Apache-2.0 on the second anniversary of the date it is made available.

## Acknowledgments

Built with these excellent Rust crates:
- [sqlparser-rs](https://crates.io/crates/sqlparser) — SQL parsing (Snowflake dialect)
- [axum](https://crates.io/crates/axum) / [tokio](https://tokio.rs) — async HTTP and runtime
- [blake3](https://crates.io/crates/blake3) — query fingerprinting
- [rusqlite](https://crates.io/crates/rusqlite) — the savings ledger
- [ed25519-dalek](https://crates.io/crates/ed25519-dalek) — signed evidence

---

<p align="center">
  Made with ❤️ by <a href="https://oso.sh">OSO</a>
</p>
