<p align="center">
  <h1 align="center">chukei</h1>
  <p align="center">
    An open source proxy for Snowflake that quietly cuts the bill
  </p>
</p>

<p align="center">
  <em>中継 — "relay" — the station that sits between you and the warehouse</em>
</p>

---

# Product Requirements Document

**Project:** chukei
**Codename / domain:** [chukei.dev](https://chukei.dev) (to be registered)
**Repository:** github.com/osodevops/chukei
**Status:** Pre-MVP, drafting
**Author:** Sion (Keito.ai / OSO)
**Version:** 0.1.0 (PRD draft)
**Last updated:** 2026-06-10
**Companion docs:** [`wharf_mvp_and_seo.md`](./wharf_mvp_and_seo.md), [`snowflake_databricks_oss_opportunity.md`](./snowflake_databricks_oss_opportunity.md)

---

## Table of Contents

1. [Product Summary](#1-product-summary)
2. [Why This Project Exists](#2-why-this-project-exists)
3. [Goals and Non-Goals](#3-goals-and-non-goals)
4. [Personas and Core Use Cases](#4-personas-and-core-use-cases)
5. [Competitive Landscape](#5-competitive-landscape)
6. [Functional Requirements](#6-functional-requirements)
7. [Architecture Overview](#7-architecture-overview)
8. [Wire-Protocol Layer (`chukei-wire-sf`)](#8-wire-protocol-layer-chukei-wire-sf)
9. [SQL AST and Fingerprinting (`chukei-sql`)](#9-sql-ast-and-fingerprinting-chukei-sql)
10. [Plugin System](#10-plugin-system)
11. [The Six P0 Plugins](#11-the-six-p0-plugins)
12. [Configuration Model](#12-configuration-model)
13. [CLI Reference](#13-cli-reference)
14. [Storage and Cache Layout](#14-storage-and-cache-layout)
15. [Observability, Metrics, Compliance](#15-observability-metrics-compliance)
16. [Security Model](#16-security-model)
17. [Non-Functional Requirements](#17-non-functional-requirements)
18. [Technical Stack](#18-technical-stack)
19. [Repository Layout](#19-repository-layout)
20. [Release Engineering and Distribution](#20-release-engineering-and-distribution)
21. [Testing Strategy](#21-testing-strategy)
22. [Documentation Strategy](#22-documentation-strategy)
23. [Agent Engineering Loops (`chukei-lab`)](#23-agent-engineering-loops-chukei-lab)
24. [Open Source Strategy and Licensing](#24-open-source-strategy-and-licensing)
25. [Go-to-Market and SEO Hooks](#25-go-to-market-and-seo-hooks)
26. [Enterprise Edition (chukei Pro)](#26-enterprise-edition-chukei-pro)
27. [Milestones and Roadmap](#27-milestones-and-roadmap)
28. [Risks and Mitigations](#28-risks-and-mitigations)
29. [Success Metrics](#29-success-metrics)
30. [Open Questions](#30-open-questions)
31. [Appendices](#31-appendices)

---

## 1. Product Summary

**chukei** (中継, *chūkei*, "relay") is a transparent, open source, wire-protocol-level proxy for Snowflake. It sits between a database driver (JDBC, ODBC, snowflake-connector-python, etc.) and the warehouse, and **transparently** does six things, in this order:

1. **Cache** semantically-equivalent query results to Apache Iceberg.
2. **Route** "small" reads to embedded DuckDB on Iceberg replicas, bypassing the warehouse.
3. **Rewrite** suboptimal SQL using a deterministic rule pack (and, async-only, LLM-assisted rules).
4. **Right-size** the destination warehouse using a contextual bandit.
5. **Predict** idle windows and recommend / execute early auto-suspend.
6. **Attribute** cost back to query → user → team → DAG node, exporting OpenLineage + OpenTelemetry.

No client changes. No SaaS sign-up. One static binary or a container. **Same pattern as `kafka-backup`**: production-grade Rust core, thin CLI wrapper, multi-cloud storage, deployment-agnostic, Apache-2.0 licensed.

> **One-line pitch:** *Greybeam / Keebo / Espresso AI, but open source, single-binary, and run by you on your own infrastructure.*

---

## 2. Why This Project Exists

### 2.1 The bill problem

Snowflake bills now routinely consume 5–15 % of a mid-market company's gross revenue. Public references and primary-source data from our [opportunity analysis](./snowflake_databricks_oss_opportunity.md):

- Auto-suspend lag alone is ~15 % of Snowflake bills ([Flexera 2025 State of the Cloud](https://www.flexera.com/blog/finops/cloud-cost-optimization-statistics)).
- DuckDB-on-Iceberg routing has achieved 79 % cost reduction in production ([r/dataengineering case study](https://www.reddit.com/r/dataengineering/comments/1m7mw87)).

### 2.2 The market gap

Every commercial competitor — Greybeam, Keebo, Espresso AI, SELECT.dev, Sundeck, Capital One Slingshot — is **closed source**, sells through enterprise sales, and charges either a per-hour fee or a percentage of warehouse spend. Pricing references:

- Greybeam: $0.75/hr proxy + $100/mo platform ([greybeam.ai](https://www.greybeam.ai))
- SELECT.dev: 4 % of Snowflake spend, $1,499/mo minimum ([select.dev](https://select.dev))
- Capital One Slingshot: $30 000–$72 000/year

There is **no OSS equivalent at the wire-protocol layer**. The closest OSS projects are dashboards ([get-select/dbt-snowflake-monitoring](https://github.com/get-select/dbt-snowflake-monitoring)), CLI utilities ([silverton-io/snowflakecli](https://github.com/silverton-io/snowflakecli)), or platform-coupled ([databricks-labs/overwatch](https://github.com/databricks-labs/overwatch)).

### 2.3 The SEO gap

From our [SEO research](./wharf_mvp_and_seo.md):

- "snowflake cost optimization": **KD 27**, CPC **$22.23**, US volume 260/mo — winnable from a fresh domain in 4–6 months.
- "snowflake finops": **KD 18**, CPC **$40.93** — the highest commercial signal on the board.
- Greybeam gets **79 %** of its traffic from the literal word "greybeam"; Keebo **79 %**; Espresso **96 %**. None of them publish content. No OSS native ranker exists.
- 85 keywords sit at KD < 30 with non-trivial volume — open arbitrage.

### 2.4 Why OSO should build this

OSO already has the playbook from [`kafka-backup`](https://github.com/osodevops/kafka-backup): production-grade Rust, multi-cloud `object_store`, plugin-ready architecture, compliance-grade evidence reports, and a mature release pipeline. We can reuse ~30 % of the infrastructure (release CI, signing, evidence reports, observability sidecar) directly.

---

## 3. Goals and Non-Goals

### 3.1 Goals (MVP v0.1)

**Functional:**
- Transparently proxy Snowflake's HTTPS wire protocol with **zero client changes**.
- Support all four real-world Snowflake auth modes: username/password, key-pair (`snowflake_jwt`), OAuth/SSO (`externalbrowser`), Programmatic Access Token.
- Ship six P0 plugins (cache, router, rewrite, bandit, suspend, attribute) with each independently enableable.
- Ship a `chukei replay` simulator: ingest 30 days of Snowflake `query_history`, project savings each plugin would have produced, output a signed report.

**Operational:**
- Single static binary (~15 MB) and a distroless container.
- Run on bare metal, VM, Docker, or Kubernetes — same code path.
- Configuration via YAML + environment variables + CLI flags (in that override order).
- Helm chart and Docker Compose example shipped from day 1.

**Performance:**
- **+5 ms p99 hot-path latency budget.** Proxy must add at most 5 ms to any query that flows through unchanged.
- **10 000 queries/sec/instance** sustained throughput.
- **<1 GB** RAM at 100 concurrent connections; **<4 GB** at 1 000.
- Cache hit → return Arrow IPC in **<50 ms p99**.

**Observability and compliance:**
- OpenTelemetry traces, metrics, logs (OTLP exporter).
- OpenLineage events for every query.
- Signed compliance-evidence reports per replay run (Ed25519 signatures over canonical JSON evidence bundles).

### 3.2 Goals (P1, post-alpha)

- Databricks SQL wire protocol (`chukei-wire-db`).
- Web control-plane UI (read-only initially).
- Trino federated routing for medium queries.
- LLM-assisted Tier-2 SQL rewrites (LITHE pattern, async only).
- Python plugin host via PyO3 (community plugins).
- Kubernetes operator with CRDs (`Chukei`, `ChukeiPlugin`, `ChukeiPolicy`).

### 3.3 Non-Goals (explicit)

- ❌ **Not a BI tool.** chukei has no dashboards beyond the read-only control-plane.
- ❌ **Not a data catalog.** No table-level metadata management.
- ❌ **Not a write-path tool.** P0 is read-only; we never proxy `INSERT`/`MERGE`/`COPY INTO`.
- ❌ **Not a query engine.** chukei delegates to DuckDB / Trino / Snowflake; it never executes plans itself.
- ❌ **Not Snowpark.** Stored procedures and Python UDFs are passed through unchanged in P0.
- ❌ **Not a replacement for Snowflake.** Goal is to cut the bill, not to replace the warehouse.
- ❌ **No GUI control plane in MVP.** CLI-first. Web UI is P1.

---

## 4. Personas and Core Use Cases

### 4.1 Personas

| Persona | Title | Pain | Win |
|---|---|---|---|
| **Sara, the data-platform lead** | Sr. Data Engineer at a 200-person scale-up | Snowflake bill grew 4× in 18 months; CFO is asking why. | Drops in chukei behind dbt; sees 30 % savings within two weeks, with a per-query attribution dashboard. |
| **Kenji, the FinOps analyst** | FinOps lead at a regulated enterprise | Needs auditable cost attribution per team, but `QUERY_TAG` discipline is poor. | chukei auto-stamps queries with provenance derived from JDBC `APPLICATION_NAME` + dbt metadata + SQL hints. |
| **Alex, the platform engineer** | Platform eng at a fintech | Already built an internal "Snowflake gateway"; it's brittle and they hate maintaining it. | Replaces internal gateway with chukei + 1 custom Rust plugin. |
| **Devon, the indie dbt consultant** | Self-employed consultant | Clients run small Snowflake deployments where SELECT.dev's $1 499/mo floor doesn't make sense. | Installs chukei in `--suggest-only` mode, runs `chukei replay`, hands the client a signed savings report. |

### 4.2 Core Use Cases

1. **Drop-in proxy** — change one connection string, see savings within 1–7 days.
2. **Replay simulator** — ingest historic `query_history`, project savings without installing the proxy.
3. **Per-team chargeback** — emit OpenLineage + per-team cost attribution to existing observability stack (Datadog / Grafana / Honeycomb / Snowflake itself via `QUERY_TAG`).
4. **DuckDB routing for dbt models** — selectively run small dbt models on DuckDB-on-Iceberg replicas, bypassing the warehouse, transparent to dbt.
5. **Audit and compliance** — signed replay reports (`chukei replay --evidence`) for SOX, FinOps, or M&A due-diligence narratives.
6. **Pre-flight bill simulation before warehouse-size change** — `chukei plan --warehouse=L → XL` simulates the bill impact.

---

## 5. Competitive Landscape

| Tool | Layer | Source model | Self-hostable? | Pricing | Plugin-extensible? |
|---|---|---|---|---|---|
| **chukei** | Wire-protocol proxy | Apache-2.0 | ✅ | $0 self-host | ✅ Rust + Python (P1) |
| [Greybeam](https://www.greybeam.ai) | Wire-protocol proxy | ❌ | partial | $0.75/hr + $100/mo | ❌ |
| [Keebo](https://keebo.ai) | Control-plane only | ❌ | ❌ | % of spend | ❌ |
| [Espresso AI](https://www.espresso.ai) | Wire-protocol proxy | ❌ | ❌ | undisclosed | ❌ |
| [SELECT.dev](https://select.dev) | Dashboard / FinOps | ❌ | ❌ | 4 % spend, $1 499/mo min | ❌ |
| [Sundeck](https://www.sundeck.io) | Wire-protocol proxy | ❌ | partial | enterprise | ❌ |
| [Revefi](https://www.revefi.com) | Observability | ❌ | ❌ | enterprise | ❌ |
| [get-select/dbt-snowflake-monitoring](https://github.com/get-select/dbt-snowflake-monitoring) | dbt dashboards | ✅ Apache | ✅ | $0 | ❌ |
| [silverton-io/snowflakecli](https://github.com/silverton-io/snowflakecli) | CLI utilities | OSS | ✅ | $0 | ❌ |
| [databricks-labs/overwatch](https://github.com/databricks-labs/overwatch) | Databricks observability | ✅ | requires Databricks | $0 | ❌ |
| [hystax/optscale](https://github.com/hystax/optscale) | Cloud FinOps | ✅ Apache | ✅ | $0 | partial |

**chukei is the only open source tool at the wire-protocol layer**, and the only one with a real plugin contract.

---

## 6. Functional Requirements

### 6.1 P0 (must-have for v0.1.0)

| ID | Requirement | Acceptance |
|---|---|---|
| F-001 | Pass through 100 % of `snowflake-connector-python` and JDBC traffic with no perceived change | `snowsql` works through chukei against a real Snowflake account |
| F-002 | Pass through username/password, key-pair JWT, OAuth/SSO, and PAT auth | All four auth modes have integration tests |
| F-003 | Parse 95 %+ of a real-world `query_history` sample with `sqlparser-rs` Snowflake dialect | Reproducible test from a customer's anonymised dump |
| F-004 | Compute hard + soft fingerprints for every query | Deterministic, recorded in spans |
| F-005 | Plugin: semantic result cache with Iceberg backing | Repeated dashboard query served in <50 ms p99 |
| F-006 | Plugin: rules-based router to embedded DuckDB on Iceberg replicas | Small `SELECT … FROM dim_table` runs locally, transparent |
| F-007 | Plugin: 10-rule deterministic rewrite pack | TPC-H benchmark shows ≥20 % wall-clock improvement on flagged queries |
| F-008 | Plugin: per-query cost attribution via tags + SQL hint comments | Emitted as OTEL spans and OpenLineage events |
| F-009 | Plugin: predictive suspend in `--suggest-only` mode | OTEL span emitted with prediction and recommendation |
| F-010 | `chukei replay` CLI: ingest `query_history` CSV, output projected savings + signed PDF report | Single command, no Snowflake account required to demo |
| F-011 | YAML configuration with env-var interpolation | Same `${VAR}` syntax as kafka-backup |
| F-012 | Prometheus metrics endpoint + OTLP exporter | `chukei_*` metrics documented |
| F-013 | Single static binary build for x86_64-linux, aarch64-linux, x86_64-darwin, aarch64-darwin, x86_64-windows | Built by cargo-dist in CI |
| F-014 | Helm chart + Docker Compose example | Both in `/deploy` |
| F-015 | `chukei doctor` health check | Exit code 0 / nonzero with structured diagnostic |

### 6.2 P1 (post-alpha)

- F-101: Databricks SQL wire protocol passthrough + plugin reuse.
- F-102: Contextual ε-greedy bandit for warehouse sizing (`plug-bandit`).
- F-103: LLM-assisted rewrite pipeline (Tier 2, async, gated).
- F-104: PyO3 Python plugin host.
- F-105: Read-only web control plane.
- F-106: Schema Registry passthrough for Snowflake's native catalog.
- F-107: Multi-tenancy mode (single proxy, multiple Snowflake accounts).

### 6.3 P2 (later)

- F-201: Kubernetes Operator (CRDs).
- F-202: Multi-region active-active deployment.
- F-203: SCIM-based RBAC.
- F-204: Federated query across multiple Snowflake accounts.

---

## 7. Architecture Overview

### 7.1 High-level diagram

```
┌──────────────────────────────────────────────────────────────────┐
│  Client driver  (JDBC / snowflake-connector-python / Go / dbt)   │
└────────────────────────────┬─────────────────────────────────────┘
                             │  Snowflake HTTPS wire protocol
                             ▼
┌──────────────────────────────────────────────────────────────────┐
│                       chukeid  (Rust, Tokio)                     │
│  ┌────────────────────────────────────────────────────────────┐  │
│  │ chukei-wire-sf  — protocol shim                            │  │
│  │   • Login / auth passthrough (4 modes)                     │  │
│  │   • Statement multiplex, Arrow chunked download            │  │
│  │   • Streaming result rewriting                             │  │
│  └──────────────────────┬─────────────────────────────────────┘  │
│                         ▼                                        │
│  ┌────────────────────────────────────────────────────────────┐  │
│  │ chukei-sql  — parse, normalise, fingerprint                │  │
│  │   sqlparser-rs (Snowflake) → AST → features → blake3       │  │
│  └──────────────────────┬─────────────────────────────────────┘  │
│                         ▼                                        │
│  ┌────────────────────────────────────────────────────────────┐  │
│  │ Plugin Bus  — ordered chain                                │  │
│  │   ① plug-cache    ② plug-router   ③ plug-rewrite           │  │
│  │   ④ plug-bandit   ⑤ plug-suspend  ⑥ plug-attr              │  │
│  └──────────────────────┬─────────────────────────────────────┘  │
│                         ▼                                        │
│   ┌───────────┬─────────┴────────┬─────────────┐                 │
│   ▼           ▼                  ▼             ▼                 │
│ Snowflake  DuckDB embedded   Iceberg cache   Trino (P1)          │
│ (upstream) (in-process)      (Arrow IPC)                         │
└──────────────────────────────────────────────────────────────────┘
                             │
       ┌─────────────────────┼─────────────────────┐
       ▼                     ▼                     ▼
   Postgres            OTEL collector         Object store
   (control plane)     (traces, metrics)      (S3 / Azure / GCS)
```

### 7.2 Process model

- **One `chukeid` per Snowflake account** (or per VPC) — stateless re: queries, stateful re: connection pool + plugin state.
- **Stateless mode**: no Iceberg cache, no learned components. 5-minute eval mode.
- **Stateful mode**: Postgres + S3/GCS/Azure-blob for cache and history.
- Designed for **sidecar deployment** (Kubernetes pod + HPA) and **single-binary** on a laptop for dev/replay.

### 7.3 Crate layout (workspace)

Mirrors `kafka-backup`'s split between a `*-core` library and a `*-cli` binary.

```
chukei/
├── Cargo.toml                        # workspace
├── crates/
│   ├── chukei-core/                  # the library (no daemon dependencies)
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── config.rs             # YAML config types
│   │   │   ├── error.rs              # thiserror enum + Result alias
│   │   │   ├── wire/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── sf/               # Snowflake wire impl
│   │   │   │   └── db/               # Databricks (feature-gated, P1)
│   │   │   ├── sql/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── parse.rs          # sqlparser-rs wrapper
│   │   │   │   ├── normalise.rs
│   │   │   │   ├── fingerprint.rs    # blake3 + soft hash
│   │   │   │   └── features.rs
│   │   │   ├── plugin/
│   │   │   │   ├── mod.rs            # Plugin trait, Decision enum
│   │   │   │   ├── bus.rs            # ordered execution + composition
│   │   │   │   └── registry.rs
│   │   │   ├── cache/                # Iceberg-backed result cache
│   │   │   ├── router/               # engine routing
│   │   │   ├── rewrite/              # deterministic + LLM rules
│   │   │   ├── bandit/               # P1 warehouse-size bandit
│   │   │   ├── suspend/              # workload forecasting
│   │   │   ├── attribute/            # cost attribution
│   │   │   ├── replay/               # query-history simulator
│   │   │   ├── evidence/             # signed JSON/PDF reports (reused pattern)
│   │   │   ├── observability/        # OTEL + OpenLineage emitters
│   │   │   ├── storage/              # object_store abstraction
│   │   │   ├── circuit_breaker.rs    # (pattern reused from kafka-backup)
│   │   │   ├── health.rs
│   │   │   └── metrics/              # Prometheus
│   │   └── tests/
│   └── chukei-cli/                   # binary wrapper
│       └── src/
│           ├── main.rs
│           └── commands/
│               ├── mod.rs
│               ├── up.rs             # `chukei up`  — run the daemon
│               ├── replay.rs         # `chukei replay`
│               ├── attr.rs           # `chukei attr`
│               ├── plan.rs           # `chukei plan`
│               ├── doctor.rs
│               ├── plugins.rs
│               ├── validate.rs
│               ├── evidence.rs
│               └── version.rs
├── plugins-py/                       # P1 — community Python plugins host
├── config/                           # example configs
│   ├── chukei-example.yaml
│   ├── replay-example.yaml
│   └── evidence-example.yaml
├── docs/                             # MDX docs source (docs.chukei.dev)
├── deploy/
│   ├── helm/                         # Helm chart
│   ├── docker-compose/
│   └── kustomize/
├── scripts/
│   ├── ci/check-release-version.py
│   └── stress-test/
├── tests/
│   ├── reproduce-bug1/
│   ├── integration-snowflake/
│   └── e2e-replay/
├── CLAUDE.md
├── CHANGELOG.md
├── LICENSE                           # Apache-2.0
└── README.md
```

---

## 8. Wire-Protocol Layer (`chukei-wire-sf`)

### 8.1 What we actually proxy

Snowflake's JDBC and Python drivers speak HTTPS against the public REST API. We need to faithfully proxy these endpoints:

| Endpoint | Verb | Purpose |
|---|---|---|
| `/session/v1/login-request` | POST | login, returns session token |
| `/queries/v1/query-request` | POST | submit SQL (sync or async) |
| `/queries/v1/abort-request` | POST | cancel |
| `/queries/{id}/result` | GET | result chunks (Arrow IPC by default) |
| `/session/token-request` | POST | OAuth/JWT refresh |
| `/session` | DELETE | logout |

We pin `JDBC_QUERY_RESULT_FORMAT=ARROW` on the upstream session so our cache is wire-compatible. JSON is a supported fallback but degraded.

### 8.2 TLS strategy

We **terminate TLS at chukei** with a customer-owned cert (the same way Greybeam does — customers point their driver at `chukei.internal.<company>.com` and provide a cert). Pass-through TLS (SNI-routing) is documented as a future option but not in P0.

### 8.3 Auth passthrough

| Mode | Status in P0 | Notes |
|---|---|---|
| Username + password | ✅ | Trivial passthrough |
| Key-pair (`snowflake_jwt`) | ✅ | Easiest for service accounts |
| OAuth / `externalbrowser` | ✅ | Browser flow MUST NOT route through chukei; we only proxy the token-exchange |
| Programmatic Access Token | ✅ | Treated like a bearer token |

### 8.4 Auth modes wharf does not support in P0

- ❌ Snowpipe streaming inserts (write path).
- ❌ External OAuth providers requiring custom flows (P1).
- ❌ Snowflake OAuth-token-exchange between accounts (P1).

### 8.5 Failure modes

- **chukei can't reach Snowflake** → return 503 with structured error; client treats as transient.
- **TLS cert mismatch** → fail fast at start-up; `chukei doctor` reports.
- **OCSP / certificate pinning in driver** → documented incompatibility; mitigation is to disable strict pinning client-side (matches Greybeam's approach).

---

## 9. SQL AST and Fingerprinting (`chukei-sql`)

### 9.1 Pipeline

```
raw_sql
   ↓ sqlparser-rs (SnowflakeDialect)
AST
   ↓ normalise (lowercase, sort SELECT cols where ORDER absent, dedent)
canonical AST
   ↓ canonicalise literals (constants → $param tokens)
parameterised AST
   ↓ extract features:
       { tables, joins_count, aggregates, predicates,
         scan_estimate_rows, udfs, time_predicate,
         determinism: bool, touches_clustering_key: bool }
QueryFeatures
   ↓ blake3 hash of canonical SQL
hard_fingerprint  (32 bytes)
   ↓ locality-sensitive hash over AST features
soft_fingerprint  (16 bytes)
```

- **hard_fingerprint** — used for exact-match cache lookups.
- **soft_fingerprint** — clusters semantically-equivalent queries that differ only in trivial whitespace/alias/literal ways. Drives cache hits dashboards never see otherwise.

### 9.2 LLMs are never in the hot path

- **Hot path:** deterministic only. Latency budget +5 ms p99.
- **Cold path (async, batched):** sqlglot transpile attempts (Python sidecar), LLM rewrite suggestions, embedding generation. **Always async, always opt-in, off by default.**

### 9.3 Local ONNX for embeddings

`all-MiniLM-L6-v2` embeddings for semantic-cache nearest-neighbour search. Served via the `ort` Rust crate. No Python runtime in the hot path.

---

## 10. Plugin System

### 10.1 Core trait

```rust
// crates/chukei-core/src/plugin/mod.rs

pub struct QueryContext<'a> {
    pub raw_sql: &'a str,
    pub ast: &'a Statement,
    pub fingerprint: [u8; 32],
    pub soft_fingerprint: [u8; 16],
    pub features: &'a QueryFeatures,
    pub session: &'a Session,
}

pub enum Decision {
    Passthrough,
    ServeFromCache(CacheKey),
    Rewrite(String),
    Route(Engine),
    SetWarehouseSize(WarehouseSize),
    Annotate(QueryTags),
    Compose(Vec<Decision>),
    Veto(VetoReason),
}

#[async_trait]
pub trait Plugin: Send + Sync {
    fn name(&self) -> &'static str;
    fn order(&self) -> i32;
    async fn decide(&self, ctx: &QueryContext<'_>) -> Result<Decision>;
    async fn on_result(
        &self,
        ctx: &QueryContext<'_>,
        result: &ResultSnapshot,
    ) -> Result<()> {
        Ok(())
    }
}
```

### 10.2 Composition rules

A small composition engine merges `Decision::Compose` lists deterministically. Conflict precedence (highest first):

1. `Veto` — short-circuits everything (admin policy).
2. `ServeFromCache` — short-circuits everything else for the response.
3. `Route` — chooses target engine.
4. `Rewrite` — applied before submission to chosen engine.
5. `SetWarehouseSize` — applied if route = Snowflake.
6. `Annotate` — additive.

The merger is exhaustively tested with property-based tests.

### 10.3 Plugin discovery

- Built-in plugins compiled into `chukei-core`.
- External Rust plugins (P1) loaded as `cdylib`s — same model as Wasm-friendly trait objects.
- External Python plugins (P1) loaded via PyO3 host.

---

## 11. The Six P0 Plugins

### 11.1 ① `plug-cache` — semantic result cache

- **Storage:** Apache Iceberg tables on S3/GCS/Azure-blob.
- **Why Iceberg?** Snapshot isolation, time-travel for cache eviction-by-snapshot, queryable from DuckDB/Trino/Snowflake itself for debugging.
- **Invalidation:**
  - TTL per table family (default 15 min for warehouses, 24 h for read-only marts).
  - **Lineage-aware**: when a write to table X is observed (via Snowflake `INFORMATION_SCHEMA.QUERY_HISTORY` polling), all cache entries whose `tables` list includes X are tombstoned.
  - **Predicate-aware**: time-range queries that fall outside the cached time-range fall through to upstream.
- **Determinism gate:** queries containing `CURRENT_TIMESTAMP`, `UUID_STRING()`, `RANDOM()`, non-deterministic UDFs are tagged non-deterministic and **never** cached.
- **Blame mode:** sampling rate of cached queries re-runs against upstream and asserts equality. False-positive cache hits are the worst possible failure; blame mode is the watchdog.

### 11.2 ② `plug-router` — engine routing

- **What:** Decide per-query whether to (a) pass to Snowflake, (b) run on embedded DuckDB against an Iceberg replica, (c) run on Trino (P1), or (d) reuse a warm Snowflake warehouse.
- **Selection rules (P0, no ML):**
  ```rust
  if scan_estimate_rows < 1_000_000 && all_tables_have_iceberg_replica(&ctx) {
      Engine::DuckDb
  } else if scan_estimate_rows < 50_000_000 && trino_cluster_idle().await {
      Engine::Trino   // P1
  } else {
      Engine::Snowflake
  }
  ```
- **Iceberg replica setup:** chukei can be told (via config) which tables to mirror to Iceberg, or it can auto-mirror tables seen ≥ N times in the last hour using `COPY INTO …` to S3 + Iceberg manifest write.

### 11.3 ③ `plug-rewrite` — SQL rewrites

**Tier 1 — deterministic rule pack (sqlparser-rs AST rewriters):**

1. Eliminate `SELECT *` when calling app only consumes 4 cols (only with surrounding `LIMIT` evidence).
2. Push predicates through views.
3. Replace `COUNT(DISTINCT)` with `APPROX_COUNT_DISTINCT` for queries flagged `tolerance=approximate` by hint comment.
4. Force clustering-key predicates first.
5. Rewrite `OR` chains to `IN`-lists where types match.
6. Hoist constant subqueries.
7. Eliminate redundant `DISTINCT` after `GROUP BY` of all selected cols.
8. Replace `LIKE 'foo%'` followed by lower-cased compare with `STARTSWITH`.
9. Replace `IS NULL OR = ''` chains with `IFNULL(col,'') = ''`.
10. Eliminate self-join-on-unique-key.

**Tier 2 — LLM-assisted (LITHE pattern, async only, P0 *experimental* flag):**

- Periodic background job samples slow queries, generates rewrite candidates via prompt, validates equivalence by running both against a small sample table.
- Persists the winner as a deterministic rewrite rule that shows up in Tier 1 on next start.
- **No LLM ever in the hot path.** Off by default. Requires explicit `experimental.llm_rewrite: true`.

### 11.4 ④ `plug-bandit` — warehouse-size selection (P1, sketched here for spec completeness)

- Contextual ε-greedy bandit. Features = (table cardinality, joins, aggregations, predicate selectivity, hour-of-day). Reward = `-(wall_clock_sec × size_credits_per_hour)`.
- ONNX-exported decision tree, evaluated in Rust via `ort`.
- **Cold-start safety:** until 1 000 observations per (query-class × size), default to user-specified warehouse. Bandits only run for ≥ 1 000-observation classes.
- **Override hint:** `/*+ chukei:size=L */` SQL comment pins the size.

### 11.5 ⑤ `plug-suspend` — predictive auto-suspend

- Snowflake's `AUTO_SUSPEND` is a static timeout. chukei observes inter-query arrival times and recommends (or, in `aggressive` mode, executes via `ALTER WAREHOUSE … SUSPEND`) early suspends.
- **Model:** Hawkes process / Holt-Winters on per-warehouse arrival rate. When P(next query in next N seconds) < threshold, suspend.
- **Resume penalty:** Snowflake charges 60 s minimum on resume. The recommender accounts for that — never suspends if expected resume cost > savings.
- **Modes:**
  - `--suggest-only` (default): emits OTEL spans, never alters state.
  - `--enforce`: requires explicit `ROLE CHUKEI_SUSPENDER` with `OPERATE` on the warehouse.

### 11.6 ⑥ `plug-attr` — cost attribution

- Stamp every query with structured tags pulled from:
  - **SQL hint comments**: `/*+ chukei:team=growth dag=daily_revenue */`
  - **dbt run metadata**: parse `__dbt_meta__` comment dbt injects
  - **JDBC `APPLICATION_NAME`** session property
  - **Snowflake `QUERY_TAG`** (we set it ourselves if blank)
- **Exports:**
  - OpenLineage events.
  - OTEL spans with `chukei.team`, `chukei.dag`, `chukei.cost_usd` attributes.
  - Optional Slack/PagerDuty webhook on anomaly (per-team daily spend > 2× rolling mean).

---

## 12. Configuration Model

YAML-driven (matches `kafka-backup`'s pattern), with env-var interpolation.

### 12.1 Example: `chukei.yaml`

```yaml
# chukei.yaml
listen:
  bind: "0.0.0.0:8443"
  tls:
    cert: "/etc/chukei/tls.crt"
    key:  "/etc/chukei/tls.key"

upstream:
  snowflake:
    account: "abc12345.us-east-1"
    # auth is passed through from client; no creds here
  databricks:        # P1
    workspace_url: "https://adb-12345.azuredatabricks.net"

storage:
  backend: s3                          # s3 | azure | gcs | filesystem
  bucket: "chukei-cache"
  region: "us-east-1"
  prefix: "prod/"
  access_key: "${S3_ACCESS_KEY}"
  secret_key: "${S3_SECRET_KEY}"

control_plane:
  postgres_url: "${PG_URL}"

plugins:
  cache:
    enabled: true
    default_ttl_secs: 900
    table_ttls:
      "ANALYTICS.MARTS.*": 86400
    determinism_gate: strict
    blame_sample_rate: 0.01           # 1 % of cache hits double-checked against upstream

  router:
    enabled: true
    duckdb:
      enabled: true
      replicas:
        - source: "ANALYTICS.RAW.EVENTS"
          iceberg_path: "s3://chukei-iceberg/events/"
          refresh: continuous
    trino:
      enabled: false                   # P1

  rewrite:
    enabled: true
    rules:
      - approx_count_distinct
      - clustering_key_predicate_first
      - eliminate_select_star
      # ... or `all`
    experimental:
      llm_rewrite: false

  bandit:                              # P1
    enabled: false

  suspend:
    enabled: true
    mode: suggest-only                 # suggest-only | enforce
    role: CHUKEI_SUSPENDER             # only used in enforce mode

  attribute:
    enabled: true
    auto_query_tag: true
    dbt_metadata_parser: true
    sources:
      - hint_comment
      - application_name
      - dbt_meta

observability:
  prometheus:
    enabled: true
    port: 9090
  otlp:
    enabled: true
    endpoint: "http://otel-collector:4317"
  openlineage:
    enabled: true
    endpoint: "http://marquez:5000/api/v1/lineage"

evidence:                              # mirrors kafka-backup `evidence/` module
  enabled: true
  signing:
    enabled: true
    private_key_path: "/etc/chukei/signing-key.pem"
  retention_days: 2555                 # 7 years (SOX)
```

### 12.2 Override order (lowest precedence first)

1. Defaults compiled into the binary.
2. `/etc/chukei/chukei.yaml`.
3. `--config <file>` argument.
4. Environment variables (`CHUKEI_LISTEN_BIND`, etc., generated from the YAML structure).
5. CLI flags.

---

## 13. CLI Reference

The CLI mirrors `kafka-backup`'s shape: one binary, subcommands per feature.

```bash
# ─── Daemon ───────────────────────────────────────────────────────────
chukei up --config chukei.yaml                # run the proxy daemon
chukei doctor --config chukei.yaml            # exit 0 if healthy
chukei status --watch                         # live status

# ─── Replay simulator ─────────────────────────────────────────────────
chukei replay \
    --query-history queries.csv \
    --config chukei.yaml \
    --output report.json \
    --evidence report.pdf
# Ingest 30 days of Snowflake query_history, simulate every plugin's effect,
# write projected savings as JSON + signed PDF.

# ─── Cost attribution / inspection ────────────────────────────────────
chukei attr daily --since 2026-06-01 --format table
chukei attr team --team growth --format json
chukei attr dag  --dag daily_revenue
chukei show-cache --backup-id <id> --format json

# ─── Planning / what-if ───────────────────────────────────────────────
chukei plan warehouse-resize --from L --to XL
chukei plan plugin --plugin cache --enable
chukei plan plugin --plugin suspend --mode enforce

# ─── Plugins ──────────────────────────────────────────────────────────
chukei plugins list
chukei plugins describe cache
chukei plugins enable suspend
chukei plugins disable bandit

# ─── Validation & evidence ────────────────────────────────────────────
chukei validate config --file chukei.yaml
chukei validate cache  --deep                 # walks Iceberg, checks hashes
chukei evidence list
chukei evidence get   --report-id <id> --format pdf --output report.pdf
chukei evidence verify --report report.json --signature report.sig \
                       --public-key pubkey.pem
```

### 13.1 Exit codes (consistent with `kafka-backup`)

| Code | Meaning |
|---|---|
| 0 | Success |
| 1 | Generic error |
| 2 | Config error |
| 3 | Connectivity / upstream error |
| 4 | Auth error |
| 5 | Plugin error |
| 10 | Validation failed (warnings/errors found, but command itself ran) |
| 64–113 | sysexits.h-compatible mapping for shell scripts |

---

## 14. Storage and Cache Layout

Inspired directly by `kafka-backup`'s storage layout.

```
s3://chukei-cache/
└── {prefix}/
    ├── manifest.json                  # cache catalog
    ├── state/
    │   └── plugin-state.db            # SQLite checkpoint, synced from local
    ├── cache/
    │   └── {table_family}/
    │       └── {soft_fp_prefix=ab}/
    │           ├── {hard_fp}.arrow    # Arrow IPC file
    │           └── {hard_fp}.meta.json
    ├── iceberg/                       # auto-mirrored Iceberg replicas
    │   └── {schema}/{table}/
    └── evidence/
        ├── reports/
        │   ├── {report_id}.json
        │   ├── {report_id}.pdf
        │   └── {report_id}.sig
        └── compliance/
            └── retention.json
```

A local SQLite database at `$TMPDIR/chukei-state.db` (configurable) holds the hot state and is periodically synced to remote storage for durability.

---

## 15. Observability, Metrics, Compliance

### 15.1 Prometheus metrics (excerpt)

| Metric | Type | Purpose |
|---|---|---|
| `chukei_queries_total{route,plugin}` | counter | every decision recorded |
| `chukei_cache_hits_total` / `_misses_total` | counter | cache health |
| `chukei_cache_blame_mismatches_total` | counter | **critical SLO** — must be 0 |
| `chukei_route_latency_seconds{engine}` | histogram | per-engine response time |
| `chukei_proxy_overhead_seconds` | histogram | hot-path budget (must be <5 ms p99) |
| `chukei_active_sessions` | gauge | concurrent driver sessions |
| `chukei_saved_usd_total{team}` | counter | projected savings |
| `chukei_suspend_recommendations_total` | counter | predictive suspend hits |
| `chukei_plugin_errors_total{plugin}` | counter | plugin failure tracking |

### 15.2 OpenTelemetry

- Traces: every query produces a root span; plugins emit child spans.
- Metrics: same set as Prometheus, exported via OTLP.
- Logs: structured JSON via `tracing`, optional OTLP log exporter.

### 15.3 OpenLineage

Every query emits a `START` and `COMPLETE` lineage event with:
- `inputs`: tables read
- `outputs`: tables written (P1, write-path only)
- `facets.chukei`: `{plugin_decisions, cost_usd, latency_ms, cached: bool}`

### 15.4 Compliance evidence (reuses `kafka-backup`'s pattern)

`chukei replay --evidence` and `chukei savings --evidence` produce
**signed JSON evidence envelopes** suitable for:

- **SOX ITGC** (change-control evidence around cost-saving infra).
- **FinOps Foundation maturity assessments** (cost-attribution evidence).
- **GDPR Article 32** (data-flow integrity proofs when cache is involved).
- **M&A due-diligence** (auditable historical bill-impact narrative).

Reports are signed with Ed25519 over the verbatim bundle JSON. Each envelope
is self-contained: bundle JSON, signature, and public key.

---

## 16. Security Model

### 16.1 Threat model

| Threat | Mitigation |
|---|---|
| Compromised chukei → leak of customer data | TLS terminates with customer cert; cache encrypted at rest; cache never includes auth tokens |
| Compromised chukei → falsified cache returns wrong data | Blame mode samples cached results vs upstream; `chukei_cache_blame_mismatches_total` is an alertable SLO |
| Insider abuse → operator dumps cache | Cache files signed; access logged via OTEL audit log; (Pro: RBAC on cache namespaces) |
| Snowflake credentials in flight | We never store credentials; auth packets pass through with the JWT/PAT/SSO token never persisted to disk |
| Supply-chain compromise of dependencies | `cargo deny`, `cargo audit`, SBOM emitted with every release, `cosign`-signed container images |

### 16.2 Required Snowflake privileges (least privilege)

| Mode | Privileges |
|---|---|
| Read-only proxy (P0) | None beyond what the client already has |
| `plug-suspend --enforce` | `ROLE CHUKEI_SUSPENDER` with `OPERATE` on the warehouse |
| `plug-router auto-mirror` | `USAGE` on schema + `SELECT` on tables + `WRITE` on the Iceberg target stage |
| `plug-attr auto_query_tag` | none — `QUERY_TAG` is a session property |

### 16.3 Secrets handling

Same pattern as `kafka-backup`: env-var interpolation in YAML (`${PG_URL}`), no secrets in logs, secrets redacted in `chukei doctor` output. Pro adds Vault / AWS Secrets Manager / Azure Key Vault integration.

---

## 17. Non-Functional Requirements

| NFR | Target | Verification |
|---|---|---|
| Hot-path overhead | <5 ms p99 added latency | `chukei_proxy_overhead_seconds` |
| Throughput | 10 000 queries/s/instance sustained | Stress test in `scripts/stress-test/` |
| Memory at 100 connections | <1 GB RSS | Stress test |
| Memory at 1 000 connections | <4 GB RSS | Stress test |
| Cold start | <500 ms `up` to listening | Integration test |
| Binary size | <20 MB stripped | CI gate |
| Container image | <40 MB distroless | CI gate |
| Cache hit response | <50 ms p99 | Metric |
| Cache blame mismatch rate | **0** (alertable) | SLO |
| CPU at idle (100 conns idle) | <2 % of one core | Stress test |
| Graceful shutdown | <5 s drain of in-flight queries | Integration test |

---

## 18. Technical Stack

Direct lift-and-extend from `kafka-backup`'s `Cargo.toml`, with proxy-specific additions.

| Concern | Crate | Why |
|---|---|---|
| Language | Rust 2024 edition | predictable hot-path latency |
| Async | `tokio = { version = "1", features = ["full"] }` | same as kafka-backup |
| HTTP server | `hyper`, `axum` (control plane) | low-level + ergonomic split |
| HTTP client | `reqwest` with `rustls-tls` | reuses kafka-backup's choice |
| TLS | `tokio-rustls`, `rustls`, `rustls-pemfile` | matches kafka-backup |
| SQL parser | `sqlparser-rs` (Snowflake dialect) | only credible Rust option; used by DataFusion + Polars |
| SQL transpiler | `sqlglot` via PyO3 sidecar | `sqlparser-rs` can't transpile Snowflake↔DuckDB; `sqlglot` can |
| Embedded engine | DuckDB v1.4+ (Rust bindings) | proven OLAP-on-Arrow, Iceberg reader native |
| Cache storage | `iceberg-rust` | mature OSS spec, queryable from any engine |
| Wire format | `arrow` / Arrow IPC | matches Snowflake's default |
| ML serving | `ort` (ONNX Runtime) | no Python in hot path |
| Object storage | `object_store` (with `aws`, `azure`, `gcp`, `http` features) | **identical to kafka-backup** |
| Control plane DB | Postgres via `sqlx` 0.8 | matches kafka-backup |
| Local checkpoint | SQLite via `sqlx` | **identical to kafka-backup** |
| CLI | `clap` v4 derive | matches kafka-backup |
| Logging | `tracing` + `tracing-subscriber` | matches kafka-backup |
| Errors | `thiserror` + `anyhow` | matches kafka-backup |
| Metrics | `prometheus-client` 0.24 | matches kafka-backup |
| Hashing | `blake3`, `crc32fast`, `murmur2` | murmur2 for partition routing (shared) |
| Signing | `ed25519-dalek`, `sha2` | Ed25519 signatures over canonical JSON evidence bundles |
| PDF | n/a for P0 | Signed JSON evidence bundles are the shipped report format |
| HTTP client (webhooks) | `reqwest` | matches kafka-backup |
| UUID | `uuid` v4 | matches kafka-backup |
| Testing | `testcontainers`, `tokio-test`, `tempfile` | matches kafka-backup |
| Container | distroless static binary | matches kafka-backup |

### 18.1 Workspace `Cargo.toml` (excerpt)

```toml
[workspace]
resolver = "2"
members = [
  "crates/chukei-core",
  "crates/chukei-cli",
]

[workspace.package]
version = "0.1.0"
edition = "2024"
license = "Apache-2.0"
authors = ["OSO"]
description = "Open source, transparent Snowflake query proxy"
repository = "https://github.com/osodevops/chukei"
homepage = "https://chukei.dev"

[profile.dist]
inherits = "release"
lto = "thin"
```

---

## 19. Repository Layout

Already shown in section 7.3. The shape matches `kafka-backup` to keep maintenance muscle-memory consistent across OSO projects.

---

## 20. Release Engineering and Distribution

### 20.1 cargo-dist

Reuse `kafka-backup`'s [cargo-dist](https://opensource.axo.dev/cargo-dist/) release pipeline verbatim. That gives us, out of the box:

- Cross-compiled artifacts for `x86_64-linux-gnu`, `aarch64-linux-gnu`, `x86_64-apple-darwin`, `aarch64-apple-darwin`, `x86_64-pc-windows-msvc`.
- Shell installer (`curl … | sh`) and PowerShell installer.
- GitHub release with checksums.
- Distroless Docker image at `osodevops/chukei`.

### 20.2 Version policy (lifted from `kafka-backup` `CLAUDE.md`)

Workspace single-version policy. Any PR touching `crates/`, `Cargo.toml`, `Cargo.lock`, or `Dockerfile` **must** bump `[workspace.package].version`. CI enforces via `scripts/ci/check-release-version.py` (port of kafka-backup's script).

**Semver rules for 0.x crates (current):**

- **Patch bump** (0.1.0 → 0.1.1): bug fixes, internal changes, no public API changes.
- **Minor bump** (0.1.x → 0.2.0): any breaking change to `chukei-core` public API.

Breakage detected by `cargo-semver-checks` in CI.

### 20.3 CI workflows

Mirror `kafka-backup`'s `.github/workflows/`:

| Workflow | Purpose |
|---|---|
| `test.yml` | `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test`, `cargo-deny`, `cargo-audit` |
| `release.yml` | cargo-dist generated; runs on tag push |
| `release-tagger.yml` | auto-tag when version bumps land on `main` |
| `release-version-guard.yml` | enforces version bumps |
| `semver-check.yml` | `cargo-semver-checks` against last published version |

### 20.4 Pre-commit checklist (identical wording to kafka-backup)

```bash
# 1. Format code (CI runs: cargo fmt --all -- --check)
cargo fmt --all

# 2. Clippy with CI-identical flags (CI runs with -D warnings)
cargo clippy --all-targets --all-features -- -D warnings

# 3. Run tests
cargo test
```

---

## 21. Testing Strategy

Three-layered approach, **directly cribbed from kafka-backup**:

### 21.1 Unit tests

Per-crate, fast, no external deps. `cargo test`.

### 21.2 Integration tests with `testcontainers`

`tests/integration-snowflake/` spins up a fake Snowflake HTTP origin (we maintain our own mock — `chukei-mock-sf` — that speaks just enough of the protocol for the test matrix), plus a DuckDB instance and a local MinIO for object storage. Same pattern as kafka-backup's `testcontainers-modules` usage.

### 21.3 End-to-end / chaos tests

`tests/reproduce-bugN/` mirrors kafka-backup's "every bug we shipped fixes a reproducer test" discipline. Each customer-reported bug becomes a permanent test fixture.

### 21.4 Stress tests

`scripts/stress-test/` ports kafka-backup's stress-test harness. Targets:
- 10 000 q/s/instance sustained for 10 minutes
- 1 000 concurrent connections
- 24 h soak with leak detection

### 21.5 Benchmark harness

`benches/` with `criterion` — same pattern as kafka-backup's perf-PRD section. Public `chukei-benchmark` companion repo (mirroring `kafka-backup-demos`) reproduces TPC-H and TPC-DS through-the-proxy benchmarks.

---

## 22. Documentation Strategy

Match `kafka-backup-docs` structure: docs live in a separate repo, published as a docusaurus site at [docs.chukei.dev](https://docs.chukei.dev).

| Doc | Source | Notes |
|---|---|---|
| Quick Start | `docs/quickstart.md` | 5-minute install + first query |
| Configuration Reference | `docs/configuration.md` | every YAML field |
| Architecture | `docs/architecture.md` | this PRD, distilled |
| Plugin Author Guide | `docs/plugins/author.md` | how to write a Rust or Python plugin |
| Plugin Catalog | `docs/plugins/catalog/*.md` | one page per built-in plugin |
| Replay Guide | `docs/replay.md` | step-by-step using a `query_history` CSV |
| Storage Guide | `docs/storage.md` | S3 / Azure / GCS setup |
| Evidence / Compliance | `docs/evidence.md` | signed reports, SOX/FinOps mapping |
| Snowflake-specific notes | `docs/snowflake.md` | auth, TLS pinning workarounds |
| Databricks (P1) | `docs/databricks.md` | the second adapter |
| Migrating from Greybeam | `docs/migrate/greybeam.md` | SEO-targeted |
| Migrating from Keebo | `docs/migrate/keebo.md` | SEO-targeted |
| Migrating from SELECT.dev | `docs/migrate/select-dev.md` | SEO-targeted |
| FAQ | `docs/faq.md` | |

---

## 23. Agent Engineering Loops (`chukei-lab`)

chukei ships with a first-class research subsystem, `chukei-lab`, that turns the proxy into a self-improving cost-optimisation platform. The thesis: the most valuable cost-saving rules in Snowflake are not the ones we wrote at v0.1; they are the ones an LLM-driven agent will discover by mining replay corpora, proposing rewrites, validating them in a sandbox, and submitting PRs against the rule pack.

This is the explicit mechanism by which chukei out-iterates Greybeam, Keebo, Espresso, and SELECT.dev. They guard a closed rule set behind a SaaS. We open the rule-discovery loop to the community and let agents contribute.

### 23.1 Design principles

1. **LLM never on the hot path.** Reiterating the architecture rule: every request handled by the data plane is served by deterministic Rust. Agents operate exclusively in cold-path, async, off-cluster mode against replayed traffic.
2. **Proposes, never alters.** An agent run produces a candidate artefact (a new rewrite rule, a router heuristic, a bandit reward function). The artefact is reviewed and merged by humans through the normal PR flow.
3. **Falsifiable by construction.** Every proposal must ship with a replay-based validation report demonstrating equivalence (blame mode) plus a measured cost delta on a representative `query_history` corpus.
4. **Evolutionary, not heroic.** Many cheap mutations beat one large rewrite. We bias the search toward small, composable transforms (LITHE-style projection pruning, predicate hoisting, Bao-style hint selection) over monolithic plan rewrites.
5. **Open contribution.** The bar to land an agent-proposed rule is the same as any maintainer-authored rule: passes CI, equivalence-checked, signed evidence report, two maintainer reviews.

### 23.2 The loop

```
   ┌──────────────────────────────────────────────────────────────┐
   │                       chukei-lab loop                        │
   │                                                              │
   │   observe  →  hypothesise  →  propose  →  validate  →  ship  │
   │      ▲                                                │      │
   │      └─────────────── telemetry feedback ─────────────┘      │
   └──────────────────────────────────────────────────────────────┘
```

| Stage | Input | Output | Mechanism |
|---|---|---|---|
| **Observe** | Replay corpus (`query_history` dumps), plugin telemetry, blame-mode reports | Workload fingerprint clusters, expensive-fingerprint shortlist | Deterministic Rust: `chukei lab observe` runs an offline pass over the corpus, produces a Parquet/JSON workload digest |
| **Hypothesise** | Workload digest + existing rule pack + literature index | Natural-language hypotheses ("projection pruning on fingerprint X cuts bytes scanned by ~30%") | LLM agent (Claude, GPT, local) reads digest, consults a literature index (Bao, LITHE, RELOAD, ByteCard papers), produces ranked hypotheses |
| **Propose** | Hypothesis | Concrete artefact: a new `RewriteRule` impl, a `RouterPolicy`, a `BanditReward`, or a config diff | Agent emits Rust code (rewrite rules, router heuristics) or YAML (config tuning). For Rust artefacts: scaffolded against the plugin trait, compiled in sandbox. For YAML: validated against schema |
| **Validate** | Proposal + replay corpus | Equivalence report + cost-delta report + risk flags | Replay harness runs every fingerprint in blame mode (parse → fingerprint → rewrite → re-parse for equivalence). Cost delta measured against a cached baseline. Risk flags (semantic drift, plan regressions on tail) emitted |
| **Ship** | Validated proposal | PR against `chukei` or a rule-pack repo | Agent opens a PR with the artefact, the validation evidence (signed JSON+PDF), and a draft `CHANGELOG.md` entry. Two maintainers must approve; CI must pass |

### 23.3 Search strategies

`chukei-lab` is a search framework, not a single algorithm. The MVP ships three strategies, each gated behind config:

#### 23.3.1 Genetic / evolutionary search over rewrite rules

Tier-2 LITHE-style rewrites (projection pruning, predicate pushdown, CTE inlining) are expressible as tree transforms over the `sqlparser-rs` AST. The lab encodes each rule as a typed mutation operator and runs a population-based search:

- **Population:** N candidate rule sets (each a list of mutation operators with parameters).
- **Fitness:** weighted sum of `bytes_scanned_delta`, `wall_clock_delta`, `equivalence_pass_rate`, `plan_stability_index` measured on the replay corpus.
- **Operators:** crossover (combine rule sets), mutation (perturb parameters), introduction (insert a new operator from the catalogue).
- **Termination:** fixed generation budget, or convergence on Pareto front of (cost_saving, risk).
- **Reference:** [AlphaEvolve-style program discovery](https://deepmind.google/discover/blog/alphaevolve-a-gemini-powered-coding-agent-for-designing-advanced-algorithms/), applied to rewrite-rule synthesis.

#### 23.3.2 Multi-armed bandit over warehouse strategies

Extends the P0 `bandit` plugin from online operation to offline discovery. Given a fingerprint and a candidate warehouse-routing policy (size, cluster, suspend-after, target queue depth), the lab:

- Replays representative samples against the policy in a simulator (warehouse cost model derived from Snowflake credit pricing + `query_history` runtime distributions).
- Uses Thompson sampling / LinUCB to explore the policy space.
- Promotes converged policies as proposed defaults for a workload signature.
- **Reference:** [Bao (2020)](https://dl.acm.org/doi/10.1145/3448016.3452838) — learned query optimiser using contextual bandits over hint sets.

#### 23.3.3 LLM-proposed plugins via the PyO3 host

For higher-risk, more-creative explorations, the lab can ask an LLM to author a brand-new plugin in Python, dropped into the PyO3 plugin host (cold-path only — Python plugins are explicitly forbidden from the hot path per §10 plugin system rules). Example: a custom `RewritePlugin` that injects `QUALIFY` clauses to deduplicate result rows for analytics tables.

- Agent receives: plugin trait definition, schema of the replay corpus, examples of existing plugins.
- Agent produces: Python module implementing the trait.
- Validation: replay harness, equivalence check, performance benchmark.
- Promotion: maintainer-reviewed PR to a `chukei-community-plugins` repo.

#### 23.3.4 Learned-cost-model assistance

When budget allows, the lab can train a small ONNX model (matches the existing v0.1 ONNX dependency) on the replay corpus to predict per-query bytes_scanned and runtime. The model is consulted by the bandit and the genetic search to prune obviously bad mutations before paying for a full replay. Inspired by [ByteCard (ByteDance, 2023)](https://arxiv.org/abs/2310.16121) and [RELOAD (CIDR 2024)](https://www.cidrdb.org/cidr2024/).

### 23.4 The replay sandbox

The sandbox is the safety boundary that lets agents iterate freely without ever touching production:

- **Input:** a Parquet/CSV dump of `SNOWFLAKE.ACCOUNT_USAGE.QUERY_HISTORY` (or the Databricks equivalent), plus optional anonymised result hashes for equivalence checking.
- **Isolation:** the sandbox runs in its own process, with no network access to Snowflake; all "execution" is simulated via the cost model.
- **Determinism:** every run is reproducible from `(corpus_hash, rule_pack_version, seed)`.
- **Storage:** sandbox artefacts (proposed rules, validation reports, traces) live under `~/.chukei/lab/runs/<run_id>/`.
- **Cost cap:** every run is bounded by `lab.max_runtime_minutes` and `lab.max_llm_tokens`; the agent host (LLM) is rate-limited via the user's own API key.

### 23.5 Safety, governance, and human-in-the-loop gates

This is the non-negotiable section. Agents are creative; production traffic is not the place to be creative.

| Gate | What it enforces | Who/what enforces it |
|---|---|---|
| **Blame-mode equivalence check** | Every proposed rewrite, applied to every fingerprint in the validation corpus, produces an AST that is logically equivalent to the input under a defined equivalence relation | Automated in `chukei lab validate` |
| **Cost-delta confidence** | Proposed change must show a statistically significant cost reduction (Wilcoxon signed-rank, p < 0.05) on the workload corpus | Automated in `chukei lab validate` |
| **Plan-regression guard** | No fingerprint in the corpus may regress beyond a configurable threshold (`lab.max_tail_regression: 1.5x`) | Automated in `chukei lab validate` |
| **Two-maintainer review** | Every agent-authored PR requires two human approvals before merge to `main` | GitHub branch protection |
| **Signed evidence report** | The PR must include a signed Ed25519 JSON evidence envelope describing the proposal, the corpus, and the validation outcome | Automated in CI |
| **Rule-pack versioning** | Agent-authored rules ship in a separately-versioned `chukei-rules-community` pack; users opt in by adding `rule_packs: [community]` to `chukei.yaml` | Default config ships with `rule_packs: [core]` only |
| **Kill switch** | `chukei rule disable <rule_id>` works instantly, persisted in state DB | Operator |

The agent never has write access to the running proxy. The most an agent can do is open a PR.

### 23.6 CLI surface

```
chukei lab observe   --corpus path/to/query_history.parquet \
                     --out runs/2026-06-10-obs.json

chukei lab propose   --observation runs/2026-06-10-obs.json \
                     --strategy genetic|bandit|llm-plugin \
                     --budget-minutes 60 \
                     --out runs/2026-06-10-prop/

chukei lab validate  --proposal runs/2026-06-10-prop/ \
                     --corpus  path/to/query_history.parquet \
                     --out     runs/2026-06-10-val/

chukei lab promote   --validated runs/2026-06-10-val/ \
                     --target    rule-pack=community \
                     # opens a PR via gh CLI, attaches signed evidence
```

A single `chukei lab run` macro chains observe → propose → validate → promote-as-draft-PR for hands-off operation.

### 23.7 Configuration

```yaml
lab:
  enabled: false                # off by default; opt-in
  corpus_path: ${CHUKEI_CORPUS}
  strategies:
    genetic:
      enabled: true
      population_size: 32
      generations: 20
      mutation_rate: 0.15
    bandit:
      enabled: true
      algorithm: linucb         # thompson | linucb | epsilon-greedy
      exploration: 0.1
    llm_plugin:
      enabled: false            # opt-in; consumes LLM tokens
      provider: anthropic       # anthropic | openai | local
      model: claude-sonnet-4-5
      api_key_env: ANTHROPIC_API_KEY
      max_tokens_per_run: 1_000_000
  validation:
    max_tail_regression: 1.5
    min_cost_delta_pct: 5.0
    significance_p: 0.05
  governance:
    auto_open_pr: false         # require manual `chukei lab promote`
    require_signed_evidence: true
    target_repo: osodevops/chukei-rules-community
```

### 23.8 Telemetry

Added to the Prometheus surface in §15:

| Metric | Type | Purpose |
|---|---|---|
| `chukei_lab_runs_total{strategy,outcome}` | counter | how often the lab runs and how often it ships a proposal |
| `chukei_lab_proposals_total{strategy,status}` | counter | proposals broken down by `accepted|rejected|merged` |
| `chukei_lab_acceptance_rate{strategy}` | gauge | rolling 30-day acceptance rate per strategy |
| `chukei_lab_validation_duration_seconds{strategy}` | histogram | how long validation takes (sandbox cost) |
| `chukei_lab_cost_delta_pct{rule_id}` | gauge | measured cost delta for each shipped rule once it's running on real traffic |
| `chukei_lab_llm_tokens_total{provider,model}` | counter | LLM spend per run (helps cap costs) |

These feed back into the loop: a rule whose live `chukei_lab_cost_delta_pct` deviates significantly from the validated estimate becomes a candidate for re-proposal.

### 23.9 Reference patterns and prior art

| Pattern | Where it came from | How chukei-lab uses it |
|---|---|---|
| AlphaEvolve-style program discovery | [DeepMind AlphaEvolve (2025)](https://deepmind.google/discover/blog/alphaevolve-a-gemini-powered-coding-agent-for-designing-advanced-algorithms/) | Genetic search over rewrite-rule programs |
| Bao learned optimiser | [Marcus et al., SIGMOD 2021](https://dl.acm.org/doi/10.1145/3448016.3452838) | Contextual bandit over warehouse-routing policies |
| ByteCard cardinality estimation | [ByteDance, CIKM 2023](https://arxiv.org/abs/2310.16121) | Learned cost model to prune the search space |
| RELOAD adaptive query processing | [CIDR 2024](https://www.cidrdb.org/cidr2024/) | Online adaptation hooks for the bandit plugin |
| ReAct agent loops | [Yao et al., ICLR 2023](https://arxiv.org/abs/2210.03629) | Reasoning + action structure for the LLM proposal step |
| AutoML for query optimisation | broad literature | Framing: "rule discovery is a hyperparameter search problem" |

### 23.10 Community angle

`chukei-lab` is also a flywheel for the open source project:

- **Community rule pack repo** (`osodevops/chukei-rules-community`) accepts agent-authored PRs from any contributor.
- **Public leaderboard** at [chukei.dev/lab/leaderboard](https://chukei.dev/lab/leaderboard) — ranks contributors by total measured cost savings across all deployed instances that opt in to anonymous telemetry.
- **Reproducible challenges:** monthly "workload of the month" — a published anonymised replay corpus and a target cost; community runs `chukei lab` and submits proposals.
- **Cited papers:** every accepted rule that maps cleanly onto a published technique gets a citation in the rule's docstring; this turns the rule pack into a navigable index of database-research literature.

### 23.11 What's in scope for v0.1.0 vs later

| Capability | v0.1.0 | v0.2.x | v1.0 |
|---|:---:|:---:|:---:|
| `chukei lab observe` (offline workload digest) | ✅ | | |
| Replay sandbox + equivalence checker | ✅ | | |
| Genetic search over Tier-2 rewrite rules | ✅ | | |
| Signed evidence reports for proposals | ✅ | | |
| `chukei lab validate` + `promote` CLI | ✅ | | |
| Bandit search over warehouse-routing policies | | ✅ | |
| Learned cost model (ONNX) | | ✅ | |
| LLM-authored Python plugin proposals | | ✅ | |
| `chukei-rules-community` rule pack + leaderboard | | ✅ | |
| Workload-of-the-month programme | | | ✅ |
| Cross-tenant federated rule discovery (Enterprise) | | | ✅ |

### 23.12 Cost model

The lab is async, cold-path, and opt-in. Default config sets `lab.enabled: false`. When enabled:

- **CPU:** bounded by `--budget-minutes` (default 60).
- **LLM tokens:** bounded by `max_tokens_per_run` and the user's own API key.
- **Storage:** ~10 MB per run for artefacts and traces, garbage-collected after `lab.retention_days` (default 30).
- **Network:** zero — sandbox has no egress; only the `promote` step uses `gh` CLI to open a PR.

---

## 24. Open Source Strategy and Licensing

### 24.1 Licence: Apache-2.0

> **Decision 2026-06-16:** chukei ships under the **Apache License, Version 2.0**.

- **Why Apache-2.0?** It removes buyer confusion, makes the public launch credible as open source, and gives platform teams a standard permissive license with an express patent grant.
- **What's allowed:**
  - Self-hosting for production use ✅
  - Modifying the source ✅
  - Forking and redistributing under Apache-2.0 terms ✅
  - Building internal or commercial services on top of chukei ✅

### 24.2 Contribution model

- **CLA-free.** Inbound = outbound (Apache-style DCO sign-off).
- **`CLAUDE.md`** in repo root (kafka-backup pattern) — single source for AI-assisted-development context, build commands, semver rules, architecture overview.
- **Issue templates** at `.github/ISSUE_TEMPLATE/{bug,feature}.yml` (matching kafka-backup).
- **`good first issue`** label curated, target ≥10 open at any time.
- **Triage SLOs:** first response in 3 business days; CVE patches in 24 h.
- **`CHANGELOG.md`** in Keep-a-Changelog format (matching kafka-backup).

### 24.3 Community channels

- GitHub Discussions for Q&A.
- A `#chukei` channel in the Snowflake Slack community (we ask permission first).
- Monthly office-hours stream on YouTube.

---

## 25. Go-to-Market and SEO Hooks

Driven by the [`wharf_mvp_and_seo.md`](./wharf_mvp_and_seo.md) keyword research. Every GitHub-as-SEO asset listed there gets a separate repo:

| Repo | Purpose | Keyword cluster targeted |
|---|---|---|
| `osodevops/chukei` | main repo | brand + `snowflake-cost-optimization` |
| `osodevops/chukei-replay` | standalone replay CLI | `snowflake-finops`, `reduce snowflake costs` |
| `osodevops/awesome-snowflake-cost` | curated list of OSS tools, papers, blog posts | `best snowflake cost management tools` |
| `osodevops/chukei-benchmark` | reproducible TPC-H/DS through-the-proxy benches | `snowflake query optimization` |
| `osodevops/chukei-demos` | docker-compose ready demos (mirrors `kafka-backup-demos`) | brand + use-case landing |
| `chukei.dev` | marketing site, docs gateway | all clusters |
| `docs.chukei.dev` | full docusaurus docs | long-tail keywords |

90-day content schedule is in the SEO doc; pillar pages map directly to this product's plugin set.

---

## 26. Enterprise Edition (chukei Pro)

Same pattern as `kafka-backup`'s "Enterprise Edition" footer in its README — a clear, dignified upsell that doesn't pollute the open source core.

| Feature category | Enterprise capability |
|---|---|
| **Security & Compliance** | AES-256 client-side cache encryption with customer-managed keys (BYOK) |
| | GDPR PII masking plugin (right-to-be-forgotten on cache rows) |
| | Comprehensive audit logging (separate from OTEL) |
| | SCIM-based RBAC on cache namespaces |
| **Advanced Integrations** | Snowflake Schema Registry passthrough + ID remap |
| | Secrets management (Vault / AWS Secrets Manager / Azure Key Vault) |
| | SSO / OIDC (Okta, Azure AD, Google Workspace) for the control-plane UI |
| **Scale & Operations** | Multi-region active-active with consensus on cache writes |
| | Log shipping (Datadog, Splunk, Grafana Loki) |
| | Advanced web control plane (cost dashboards, drill-down UI) |
| | Kubernetes Operator |
| **Support** | 24/7 SLA-backed support and dedicated Snowflake cost consulting |

The footer copy mirrors kafka-backup:

> [OSO](https://oso.sh) engineers are solely focused on data infrastructure for regulated enterprises. If you need SLA-backed support or advanced features for compliance and security, our Enterprise Edition extends the core tool with capabilities designed for large-scale, regulated environments. 👉 [Talk with an expert today](https://oso.sh/contact/) or email **enquiries@oso.sh**.

---

## 27. Milestones and Roadmap

### 27.1 8-week MVP plan (alpha v0.1.0)

| Week | Milestone | Demo / verification |
|---|---|---|
| **1** | `chukei-wire-sf` passes through 100 % of queries from `snowflake-connector-python` + JDBC. Zero feature flags. | `snowsql` works through chukei, transparent. |
| **2** | `chukei-sql` parses + fingerprints 95 % of a real query log (you'll dump 30 days of `query_history` to test). | `chukei replay --parse-only --query-history queries.csv` reports parse coverage + soft-fingerprint dedup ratio. |
| **3** | `plug-cache` v0: TTL-based memoization on hard fingerprint. Iceberg writer working. | Repeated dashboard query returns from cache, ≤50 ms p99. |
| **4** | `plug-router` v0: rules-based DuckDB routing for read-only Iceberg replicas. | A `SELECT … FROM small_dim_table` runs locally without touching Snowflake. |
| **5** | `plug-rewrite` Tier 1: 10-rule deterministic pack. | `cargo bench` shows ≥20 % wall-clock improvement on flagged queries. |
| **6** | `plug-attr` + OTEL/OpenLineage export. `plug-suspend` in `--suggest-only` mode. | Datadog dashboard shows query-level cost by team tag pulled from comments. |
| **7** | `chukei replay` CLI complete with signed PDF evidence reports. | Marketing-grade asset; the top-of-funnel tool. |
| **8** | Helm chart + Docker Compose + `chukei doctor` + cargo-dist release pipeline + docs site. v0.1.0 published. | `helm install chukei …` then point your JDBC URL at it. |

### 27.2 Beta v0.2.x — v0.5.x (weeks 9–24)

- `chukei-wire-db` Databricks adapter (v0.2).
- `plug-bandit` warehouse-size selection (v0.3).
- LLM-assisted Tier-2 rewrites behind `experimental.llm_rewrite` (v0.3).
- PyO3 Python plugin host (v0.4).
- Read-only web control plane (v0.4).
- Trino federated routing (v0.5).
- 1 000 GitHub stars; first three production logos in `ADOPTERS.md`.

### 27.3 GA v1.0 (target month 9)

- Stable plugin ABI.
- Cargo-semver-checks-gated minor releases.
- Helm chart published to OCI registry.
- Kubernetes Operator beta.
- chukei Pro v1.0 launched in parallel.

---

## 28. Risks and Mitigations

| # | Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|---|
| R-1 | Snowflake dislikes a proxy that bypasses metering for cache hits | Medium | High | Ship as a customer-side tool with no Snowflake API key required. Compatibility is purely client-side. Document a "compliance mode" that disables cache and is audit-equivalent to direct Snowflake usage. |
| R-2 | JDBC drivers do certificate validation in ways that break naïve TLS termination | High | Medium | Terminate TLS at chukei with a customer-owned cert (Greybeam pattern). Document a `--no-verify` debug mode for evaluation, with prominent warnings. |
| R-3 | Cache correctness — a false-positive hit destroys trust forever | Medium | **Critical** | "False-negative friendly, false-positive intolerant." Blame mode samples cached results vs upstream; `chukei_cache_blame_mismatches_total` is an alertable SLO. |
| R-4 | Iceberg writes are slow on first cache fill | High | Low | Optional "cache_writer" warehouse. Fall back to async S3 + manifest job. Document a "warm-up" phase. |
| R-5 | sqlglot's Snowflake↔DuckDB coverage isn't 100 % | High | Medium | Router falls back to Snowflake whenever transpilation fails. Coverage % per release published as marketing artefact. |
| R-6 | "Yet another proxy" objection — installation friction kills adoption | High | High | Pizzeria test: install must be <10 minutes; demo must show "$ saved in 1 hour." Replay CLI lets people see savings *before* installing the proxy. |
| R-7 | Commercial competitors hire away or sue | Low | High | Apache-2.0 patent grant + clean-room dev + careful patent landscape review. Same posture OSO has on kafka-backup. |
| R-8 | Snowflake ships a native cache that obsoletes the project | Low–Medium | High | Differentiate on (a) cross-warehouse cost attribution, (b) DuckDB routing — Snowflake will not bypass itself, (c) plugin extensibility. |
| R-9 | Latency overhead exceeds 5 ms p99 budget | Medium | High | Hot-path budget enforced as a CI gate via `criterion` benchmarks. Plugins that fail the budget cannot ship. |
| R-10 | Community contributors are scared off by Rust + Snowflake protocol complexity | Medium | Medium | PyO3 plugin host (P1) lowers the bar. Curated `good first issue` queue, weekly office hours. |

---

## 29. Success Metrics

### 29.1 Engineering KPIs (track from v0.1.0)

| KPI | Target by v1.0 (month 9) |
|---|---|
| `chukei_proxy_overhead_seconds` p99 | <5 ms |
| `chukei_cache_hit_ratio` on typical dashboarding | ≥40 % |
| `chukei_cache_blame_mismatches_total` over rolling 30 d | 0 |
| Parse coverage on real query logs | ≥98 % |
| TPC-H wall-clock improvement on flagged queries | ≥20 % |
| Cold start to ready | <500 ms |

### 29.2 Community KPIs

| KPI | Day-90 | Day-180 | Day-365 |
|---|---|---|---|
| GitHub stars | 500 | 1 500 | 4 000 |
| Production deployments in `ADOPTERS.md` | 3 | 12 | 30 |
| External contributors with ≥1 merged PR | 5 | 20 | 60 |
| Plugin catalog entries | 6 (P0 only) | 12 | 25 |
| Hacker News front page launch | ✓ | n/a | n/a |

### 29.3 SEO KPIs (from `wharf_mvp_and_seo.md`)

| KPI | Day-90 | Day-365 |
|---|---|---|
| Ranking for "snowflake cost optimization" (US) | top 50 | top 10 |
| Ranking for "snowflake finops" (US) | top 30 | top 5 |
| Ranking for "best snowflake cost management tools 2025" | top 10 | top 3 |
| Non-branded organic sessions / month | 1 000 | 15 000 |
| Referring domains | 50 | 300 |

### 29.4 Commercial KPIs (chukei Pro, P1+)

| KPI | Year-1 |
|---|---|
| Pro design partners | 5 |
| Pro paying customers | 10 |
| Pro ARR | $300k |

---

## 30. Open Questions

1. **Final name confirmation.** chukei.dev availability has been verified — confirm and register.
2. **Logo / visual identity.** The "中継" kanji is striking and could be the wordmark. Need a designer.
3. **Wire-protocol pass-through for write-path queries.** P0 declares this off-limits, but customers will ask. Confirm Phase-2 (P1) policy.
4. **Pricing for Pro.** Mirror kafka-backup Pro structure or differentiate?
5. **Hosted demo / playground.** Should we operate a public, throwaway Snowflake account that runs chukei for the docs site's "try it" button?
6. **Snowflake Marketplace listing.** Once mature, do we publish a Snowflake Native App for the cost-attribution side?
7. **Snowflake partnership.** Do we approach Snowflake's developer / partner team proactively, or wait until traction makes the conversation easier?
8. **Trademark.** Is "chukei" trademarkable for software in our key jurisdictions (US, UK, EU)? Check before the launch post.

---

## 31. Appendices

### 31.1 Glossary

| Term | Meaning |
|---|---|
| **chukei** | 中継, "relay" — the project name |
| **chukeid** | the proxy daemon binary |
| **hot path** | the synchronous request/response loop a query passes through |
| **cold path** | async background jobs (LLM rewrites, embedding generation, etc.) |
| **hard fingerprint** | blake3 hash of canonical SQL — exact-match key |
| **soft fingerprint** | locality-sensitive hash over AST features — clusters equivalents |
| **blame mode** | the cache's self-audit: sample-rerun against upstream, alert on mismatch |
| **suggest-only** | a mode where a plugin emits a recommendation but never alters state |
| **enforce** | a mode where a plugin actually executes its recommendation |
| **replay** | offline simulation of every plugin's effect against a `query_history` dump |

### 31.2 References

**Technical:**
- Snowflake REST API: [docs.snowflake.com/en/developer-guide/snowflake-rest-api](https://docs.snowflake.com/en/developer-guide/snowflake-rest-api/snowflake-rest-api)
- Snowflake JDBC config: [docs.snowflake.com/en/user-guide/jdbc-configure](https://docs.snowflake.com/en/user-guide/jdbc-configure)
- sqlparser-rs: [github.com/apache/datafusion-sqlparser-rs](https://github.com/apache/datafusion-sqlparser-rs)
- sqlglot: [github.com/tobymao/sqlglot](https://github.com/tobymao/sqlglot)
- DuckDB Iceberg reader: [duckdb.org/docs/extensions/iceberg](https://duckdb.org/docs/extensions/iceberg.html)
- Apache Iceberg Rust: [github.com/apache/iceberg-rust](https://github.com/apache/iceberg-rust)
- OpenLineage: [openlineage.io](https://openlineage.io)
- Apache License 2.0: [apache.org/licenses/LICENSE-2.0](https://www.apache.org/licenses/LICENSE-2.0)
- cargo-dist: [opensource.axo.dev/cargo-dist](https://opensource.axo.dev/cargo-dist/)

**OSO prior art:**
- `kafka-backup`: [github.com/osodevops/kafka-backup](https://github.com/osodevops/kafka-backup) — release pipeline, evidence module, storage abstraction, CLI patterns, CLAUDE.md template all transplant directly.
- `kafka-backup-demos`: [github.com/osodevops/kafka-backup-demos](https://github.com/osodevops/kafka-backup-demos) — pattern for `chukei-demos`.
- `kafka-backup-docs`: pattern for `docs.chukei.dev`.

**Commercial competitors:**
- [greybeam.ai](https://www.greybeam.ai), [keebo.ai](https://keebo.ai), [espresso.ai](https://www.espresso.ai), [select.dev](https://select.dev), [revefi.com](https://www.revefi.com), [sundeck.io](https://www.sundeck.io)

**Background / opportunity sizing:**
- [`snowflake_databricks_oss_opportunity.md`](./snowflake_databricks_oss_opportunity.md) — original opportunity analysis
- [`wharf_mvp_and_seo.md`](./wharf_mvp_and_seo.md) — MVP scope + SEO research (the original name was "wharf" before chukei was chosen)

---

<p align="center">
  <em>Made with ❤️ by <a href="https://oso.sh">OSO</a></em>
</p>
