# chukei-docs — content plan & build roadmap (search-intent mapped)

Generated 2026-06-18. Operationalizes `content_map.md` against the live
Docusaurus site. Tracks what's built and sequences what's left, with the
**search intent** that should govern each page's structure (informational →
teach + quotable answer; commercial → comparison + proof; transactional →
task-complete, minimal fluff).

Domain: `https://docs.chukei.dev` (routeBasePath `/`). Keyword vol/KD from
`keyword_masterlist.csv` (refreshed 2026-06-18).

## Status snapshot

| Section | Built | Target | Remaining |
|---|---|---|---|
| getting-started | 3 | 4 | 1 |
| guides (4 pillars + inner) | 4 | 19 | 15 |
| **architecture** | **8** | 7 | ✅ done (+proxy-safety) |
| **well-architected (best practices)** | **8** | 12 | ✅ core done; 4 optional checklist stubs |
| deployment | 4 | 8 | 4 |
| reference | 2 | 9 | 7 |
| examples | 3 | 10 | 7 |
| use-cases | 0 | 6 | 6 |
| benchmarks | 1 | 2 | 1 |
| troubleshooting | 1 | 6 | 5 |

Delivered in this pass: full **architecture** section (7 new diagrams) and the
**Snowflake Cost Optimization Framework** best-practices pillar, plus the
persona-hub homepage, navbar/footer links, and `static/llms.txt`.

## Priority tiers (build order)

Sequencing logic (carried from `editorial_calendar.md`): quick KD wins to
establish the domain → the caching/attribution wedge nobody else covers → the
dbt audience bridge → commercial use-cases → long-tail reference/examples.

### Tier 1 — the money guide cluster (highest search value, do first)

| Page | Target keyword | Vol/KD | Intent | Links to |
|---|---|---|---|---|
| guides/snowflake-pricing-explained | snowflake pricing | 1300 / 36 | informational | credits, how-much, P1 pillar |
| guides/snowflake-credits | snowflake credits / credit cost | 70 / 48, 170 / 37 | informational | pricing, P1 pillar |
| guides/how-much-does-snowflake-cost | how much does snowflake cost | 70 / 37 | commercial | pricing, credits |
| guides/snowflake-result-cache-limitations | snowflake result cache | 20 / 0 | informational | P2 pillar, fingerprinting |
| guides/snowflake-auto-suspend-best-practices | snowflake auto suspend | 20 / 0 | informational | P3 pillar, well-architected/elimination |

### Tier 2 — the wedge + warehouse/attribution inner pages

| Page | Target keyword | Vol/KD | Intent | Links to |
|---|---|---|---|---|
| guides/deterministic-query-caching | deterministic query caching | wedge / 0 | informational | P2, fingerprinting |
| guides/snowflake-warehouse-sizes-credit-rates | snowflake warehouse sizing | 140 / 26 | informational | P3, well-architected/efficiency |
| guides/snowflake-idle-warehouse-costs | snowflake idle warehouse | long-tail | informational | P3, elimination |
| guides/snowflake-cost-attribution-by-team | cost attribution by team | wedge | informational | P4, well-architected/attribution |
| guides/snowflake-chargeback-showback | chargeback showback | long-tail | commercial | P4, governance |
| guides/snowflake-query-history-cost-analysis | snowflake query history | 480 / 24 | informational | P4, visibility (SQL recipe) |

### Tier 3 — dbt bridge + comparison/spike (commercial-intent capture)

| Page | Target keyword | Vol/KD | Intent | Links to |
|---|---|---|---|---|
| guides/dbt-snowflake-cost-optimization | dbt snowflake | 1600 / 35 | commercial | P4, examples/dbt |
| guides/snowflake-cost-optimization-tools | snowflake cost optimization solutions | 140 / 24 | commercial | P1 (listicle, AI-citation format) |
| guides/snowflake-bill-spike-diagnosis | "bill doubled overnight" | long-tail / 0 | informational | P1, visibility |
| guides/snowflake-cost-per-query | snowflake cost per query | 50 / 30 | informational | P1, visibility |
| guides/snowflake-query-optimization | snowflake query optimization | 110 / 23 | informational | well-architected/efficiency |

### Tier 4 — use-cases (commercial; convert framework readers to pilots)

| Page | Intent | Angle |
|---|---|---|
| use-cases/bi-dashboard-caching | commercial | dashboard herd → coalescing + cache |
| use-cases/dbt-ci-cost-control | commercial | dbt CI duplicate runs |
| use-cases/ad-hoc-analytics | commercial | analyst sandbox spend |
| use-cases/embedded-analytics | commercial | per-tenant attribution |
| use-cases/multi-team-chargeback | commercial | governance pillar applied |
| use-cases/audit-grade-savings-evidence | commercial | signed evidence for finance/M&A |

### Tier 5 — product completeness (transactional/long-tail; build as needed)

- reference (7): env overrides, metrics catalogue, plugin reference, determinism-gate rules, exit codes, OpenLineage/OTEL, evidence-bundle format.
- examples (7): snowsql, Airflow, Tableau, Power BI, Looker, key-pair auth, PAT auth.
- deployment (4): TLS & certs, sizing/HA, observability/Prometheus, rollback runbook.
- troubleshooting (5): doctor, OCSP/cert issues, cache misses explained, blame mismatches, connection errors.
- getting-started (1): 10-min quickstart w/ savings report (if not already covered by quickstart.md).
- benchmarks (1): proxy-overhead methodology deep-dive.

### Optional — well-architected checklist stubs (low priority)

The 12-page target included 5 per-pillar checklist pages; this pass shipped one
consolidated `checklist.md` (better for GEO than thin stubs). Only split into
per-pillar checklist pages if keyword data later shows demand for
"snowflake {pillar} checklist" long-tails.

## Per-page requirements (apply to every new page)

- **First paragraph = a direct, quotable answer < 300 chars** (GEO extractability).
- **One original-number stat** where relevant (60k hits/0 mismatch; ~2ms p99; suspend ≈ 94% of simulated savings).
- **Internal links** per `content_map.md` rules: pillar → all inner; inner → parent + 2 siblings; every guide ends with the replay-simulator CTA.
- **Add to `static/llms.txt`** the day it ships.
- **Front-matter**: title, description, keywords[] (mirror the existing guides).

## Refresh rota

Pricing + credits pages every 90 days (rates change). Re-run the
`llm_visibility_audit` queries monthly and check whether `docs.chukei.dev`
surfaces for the framework/architecture/wire-protocol queries (currently
uncontested per the 2026-06-18 GEO audit).
