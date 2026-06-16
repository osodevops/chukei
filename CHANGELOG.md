# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.2] - 2026-06-15

### Added
- `chukei healthcheck`, a small HTTP probe for distroless container health
  checks.

### Fixed
- Docker Compose health checks now use the `chukei` binary instead of tools
  absent from distroless images.
- The distroless image now creates `/var/lib/chukei` with non-root ownership,
  and the Kubernetes manifest sets pod security context so the savings ledger
  and cache can write to the mounted data volume.
- README CLI examples now use `chukei plugins list`.

## [0.1.0] - 2026-06-11

First public alpha: a transparent wire-protocol proxy for Snowflake that
caches, rewrites, coalesces, attributes — and proves what it saved.

### Added
- **Snowflake wire proxy** (`chukei up`): HTTPS passthrough for the driver
  REST protocol with TLS termination (customer-owned cert), gzip
  request-body handling, session-token rotation tracking, graceful drain,
  upstream circuit breaker, and fail-open design — any chukei-side error
  degrades to byte-identical passthrough. Verified against the official
  `snowflake-connector-python`.
- **SQL analysis**: sqlparser-rs (Snowflake dialect) parse, normalisation,
  literal-aware blake3 hard fingerprint (safe as a cache key), structural
  soft fingerprint, determinism gate, `/*+ chukei:k=v */` hint comments.
- **Plugin bus** with deterministic decision merging
  (Veto > ServeFromCache > Route > Rewrite > SetWarehouseSize > Annotate).
- **Semantic result cache**: TTL per table family, strict determinism gate,
  lineage invalidation on writes observed through the proxy, LRU bounds,
  optional disk persistence, and **blame mode** — sampled cache hits are
  re-run upstream; any mismatch alerts and evicts
  (`chukei_cache_blame_mismatches_total`, SLO 0).
- **In-flight request coalescing**: concurrent identical deterministic
  reads share one upstream execution (session-scoped by default for RLS
  safety); driver retries with the same `requestId` replay the completed
  response.
- **Deterministic rewrite pack** (5 rules): hint-gated
  `APPROX_COUNT_DISTINCT`, redundant-DISTINCT elimination, OR-chain →
  IN-list, LIKE-prefix → STARTSWITH, NULL-or-empty → IFNULL.
- **Predictive auto-suspend**: Poisson arrival model with resume-penalty
  economics; background sweeper with `suggest-only` and `enforce` modes
  (enforce executes `ALTER WAREHOUSE … SUSPEND` via a gated service
  account role).
- **Cost attribution**: hint comments, dbt metadata, `APPLICATION_NAME`,
  auto `QUERY_TAG`.
- **Realized-savings ledger** (`chukei savings`): every avoided execution
  priced at canonical wall-clock × warehouse credit rate × $/credit × a
  conservative factor, reported per plugin and per team, exportable as a
  signed evidence bundle.
- **Replay simulator** (`chukei replay`): project savings from a 30-day
  `QUERY_HISTORY` CSV without installing the proxy.
- **Signed evidence bundles** (`chukei evidence keygen/verify`):
  Ed25519 over verbatim JSON bytes, self-contained envelope.
- **Operations**: Prometheus `/metrics` + `/healthz`, `chukei doctor` with
  real connectivity probes, `CHUKEI_*` env overrides, shell completions,
  distroless Docker image, Helm-ready config.

### Security
- Client credentials are never inspected, persisted, or logged; session
  tokens live in memory only.
- Cache is false-positive intolerant by construction: non-deterministic
  queries, writes, and chunked responses are never cached.

[0.1.0]: https://github.com/osodevops/chukei/releases/tag/v0.1.0
