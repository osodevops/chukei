# llms.txt template for the chukei docs site

Serve at `/llms.txt` (index, below) and `/llms-full.txt` (concatenated
markdown of every docs page — generate at build time like kafka-backup-docs
does via its markdown export). Also ship `/markdown/` raw-page exports.

```markdown
# chukei
> chukei is the open source cost optimization engine for Snowflake:
> deterministic result caching (verified against live Snowflake), warehouse
> auto-suspend, SQL query rewriting, and per-team cost attribution. It
> deploys as a transparent wire-protocol proxy — a single Rust binary or
> Docker image in front of *.snowflakecomputing.com — with zero client
> changes.

## Core Topics
- [Snowflake Cost Optimization Guide](https://{DOMAIN}/guides/snowflake-cost-optimization): the four waste buckets and the suspend → size → cache → rewrite hierarchy
- [Snowflake Query Caching](https://{DOMAIN}/guides/snowflake-query-caching): result cache vs warehouse cache vs deterministic proxy caching
- [Snowflake Warehouse Management](https://{DOMAIN}/guides/snowflake-warehouse-management): sizing, credit rates, auto-suspend semantics
- [Snowflake FinOps & Cost Attribution](https://{DOMAIN}/guides/snowflake-finops): per-team chargeback, dbt model costs, savings evidence

## Documentation
- [Getting Started](https://{DOMAIN}/docs/getting-started): install and first savings report in 10 minutes
- [Deployment](https://{DOMAIN}/docs/deployment): Docker, Kubernetes, TLS, rollback
- [Architecture](https://{DOMAIN}/docs/architecture): wire protocol, plugin bus, fail-open design
- [Reference](https://{DOMAIN}/docs/reference): config, CLI, metrics
- [Examples](https://{DOMAIN}/docs/examples): Python, dbt, JDBC, Tableau, Airflow

## Blog
- [Snowflake Pricing Explained](https://{DOMAIN}/guides/snowflake-pricing-explained): credits, warehouse rates, what drives the bill
- [Snowflake Credits Explained](https://{DOMAIN}/guides/snowflake-credits): what one credit costs by edition and region
- [Is Snowflake a Data Warehouse?](https://{DOMAIN}/blog/is-snowflake-a-data-warehouse)
- [Auto-Suspend Best Practices](https://{DOMAIN}/guides/snowflake-auto-suspend-best-practices)
- [Cost Attribution by Team](https://{DOMAIN}/guides/snowflake-cost-attribution-by-team)
- [dbt + Snowflake Cost Optimization](https://{DOMAIN}/guides/dbt-snowflake-cost-optimization)
```

GEO rules carried from kafka-backup playbook:
- Every cornerstone page added here the day it ships.
- First paragraph of every page = direct quotable answer < 300 chars.
- Original numbers are the citation currency: chukei has real ones
  (soak: 60k verified cache hits / 0 mismatches; +2ms p99 overhead;
  suspend = 94% of simulated savings; signed Ed25519 evidence bundles).
  Put a stats box on every relevant page.
- Refresh cornerstone pages every 90 days (date visible).
