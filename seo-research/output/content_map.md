# chukei — Semantic Topical Map (Koray GUBUR Topical Authority framework)

Domain placeholder: `chukei.oso.sh` (subdomain decision pending — same hosting
as kafkabackup.com). All URLs below are relative to that root.

## Central Entity

**Entity:** chukei — the open source transparent Snowflake cost-optimization proxy
**Source Context:** cut Snowflake compute spend at the wire protocol, with zero client changes
**Central Search Intent:** learn ("why is my Snowflake bill so high / how do I reduce it") → adopt (open source install)

Positioning wedge (from discovery): every competitor is a dashboard, advisor,
or copilot; only Sundeck shares the proxy pattern and it messages
routing/guardrails. chukei owns four uncontested topics: **deterministic
result caching with correctness proof**, **inline SQL rewriting**,
**wire-level per-team cost attribution**, and **signed savings evidence**.

---

## Core Sections (Pillars)

### Pillar 1: Snowflake Cost Optimization (the guide hub)
- **Target keyword:** snowflake cost optimization (260/mo, KD 27) + reduce snowflake costs (50, KD 20)
- **Intent:** Commercial/informational
- **URL:** /guides/snowflake-cost-optimization
- **Attributes:** cost drivers, warehouse compute vs storage vs serverless, the optimization hierarchy (suspend > size > cache > rewrite), measurement, tooling landscape

### Pillar 2: Snowflake Query Caching
- **Target keyword:** snowflake caching (70, KD 23) + snowflake result cache (20, KD 0)
- **Intent:** Informational → product
- **URL:** /guides/snowflake-query-caching
- **Attributes:** result cache vs warehouse cache vs proxy cache, 24h window limits, determinism, cross-user/cross-tool reuse, correctness verification (blame), cache invalidation on writes

### Pillar 3: Snowflake Warehouse Management
- **Target keyword:** snowflake warehouse sizing (140, KD 26) + snowflake auto suspend (20, KD 0)
- **Intent:** Informational
- **URL:** /guides/snowflake-warehouse-management
- **Attributes:** sizes & credit rates, auto-suspend/auto-resume semantics, the 60-second billing minimum, idle detection, right-sizing methodology, multi-cluster

### Pillar 4: Snowflake FinOps & Cost Attribution
- **Target keyword:** snowflake finops (40, KD 18) + snowflake cost management (110, KD 24) + chargeback/showback long-tail
- **Intent:** Informational/commercial
- **URL:** /guides/snowflake-finops
- **Attributes:** ACCOUNT_USAGE / METERING_HISTORY, query tags, per-team/per-dbt-model attribution, chargeback vs showback, savings evidence & audit

### Pillar 5: chukei product docs (transactional core)
- **Target keyword:** snowflake proxy (20, KD 0) + branded
- **Intent:** Transactional
- **URL:** /docs/ (getting-started, deployment, reference…)
- **Attributes:** install, TLS, plugins, config, fail-open guarantees, evidence reports

---

## Inner Sections (per pillar; target keyword, vol/KD)

**P1 Cost Optimization:**
- /guides/snowflake-pricing-explained — snowflake pricing (1300, KD 36), pricing model (140, KD 35)
- /guides/snowflake-credits — snowflake credits (70, KD 48), credit cost (170, KD 37), "how much does a snowflake credit cost" question cluster
- /guides/how-much-does-snowflake-cost — how much does snowflake cost (70, KD 37), how does snowflake pricing work (20, KD 0)
- /guides/snowflake-cost-per-query — long-tail, zero competition (discovery: "one poorly written query can cost hundreds")
- /guides/snowflake-bill-spike-diagnosis — "bill doubled overnight" pain point, no good content exists
- /guides/snowflake-cost-optimization-tools — comparison listicle (AI-citation magnet per visibility audit)

**P2 Caching:**
- /guides/snowflake-result-cache-limitations — why native cache misses (per-session sensitivity, 24h, invalidation)
- /guides/deterministic-query-caching — chukei's wedge; zero competition
- /guides/snowflake-proxy-caching-vs-materialized-views — cost trade-off ("paying for maintenance we don't need")
- /guides/bi-dashboard-snowflake-costs — "BI tool loves SELECT *" pain

**P3 Warehouse:**
- /guides/snowflake-auto-suspend-best-practices — auto suspend best practices (visibility audit: weak AI answers today)
- /guides/snowflake-warehouse-sizes-credit-rates — reference table (quotable)
- /guides/snowflake-idle-warehouse-costs — "idle 90% of the time"
- /guides/snowflake-multi-cluster-warehouses-cost

**P4 FinOps:**
- /guides/snowflake-cost-attribution-by-team — "can't tell who to yell at"
- /guides/snowflake-chargeback-showback
- /guides/dbt-snowflake-cost-optimization — dbt long-tail (what is dbt = 14800/mo is outer-bridge)
- /guides/snowflake-query-history-cost-analysis — QUERY_HISTORY/METERING SQL recipes (quotable SQL = AI citations)
- /guides/snowflake-savings-evidence — signed reports; unique to chukei

**P5 Product docs:** mirrors kafka-backup-docs sections — see site structure below.

---

## Outer Sections (authority builders, /blog/)

| Topic | Keyword (vol/KD) | Bridge to |
|---|---|---|
| Is Snowflake a data warehouse? | 320, KD 15 | P1 (cost model follows architecture) |
| Snowflake vs Databricks cost comparison | databricks-vs cluster (7 kw) | P1 |
| What is a virtual warehouse in Snowflake | question cluster, KD 0 | P3 |
| Data warehouse cost optimization (general) | data warehouse cluster (32 kw) | P1 |
| What is dbt (+ dbt cost angle) | 14800, KD 71 — long play | P4 dbt inner |
| Snowflake for FinOps practitioners (FOCUS spec) | finops cluster | P4 |
| How AI agents optimize Snowflake warehouses | 30, KD 0 (trending) | P3/P5 |
| Wire-protocol proxies for databases explained | zero competition | P5 architecture |

## Contextual Bridges

| From | To | Bridge concept | Anchor |
|---|---|---|---|
| Pricing explained (P1) | Credits (P1 inner) | credits are the billing unit | "what a Snowflake credit costs" |
| Caching pillar (P2) | Result-cache limitations | native cache ≠ deterministic cache | "where Snowflake's result cache falls short" |
| Auto-suspend (P3) | chukei suspend plugin (P5) | suggest vs enforce | "automating suspend safely" |
| dbt cost (P4) | Attribution (P4) | dbt meta → query tags | "attributing dbt model spend" |
| Every guide | /docs/getting-started | "measure your own savings" CTA | "run the replay simulator on your QUERY_HISTORY" |

---

## Architecture section (7 pages) — fully specified

GEO note (2026-06-18 audit): "wire-protocol proxy" is genuinely uncontested by
Snowflake's own content; the strict suspend>size>cache>rewrite hierarchy is not
canonical anywhere. Own both. Architecture pages are informational/educational and
bridge to the product (P5) and to the proxy/security guides.

| Page (URL `/docs/architecture/...`) | Target keyword (vol/KD) | Intent | Diagram | Links to |
|---|---|---|---|---|
| overview | snowflake proxy architecture (long-tail) / snowflake architecture (720, KD 33 outer bridge) | informational | end-to-end request path + fail-open | all arch pages, P5 getting-started |
| wire-protocol-shim | wire protocol proxy (zero comp) / database proxy (1300 RDS-dominated) | informational | login/query/result/abort sequence vs passthrough | overview, security-model, P2 caching |
| plugin-bus | (long-tail "snowflake query plugin") | informational | Decision precedence flow | overview, reference/plugins |
| fingerprinting | deterministic query caching (wedge) | informational | hard blake3 vs soft LSH pipeline | P2 caching pillar, plugin-bus |
| fail-open-design | fail open proxy (zero comp) | informational | failure-mode → passthrough decision tree | overview, troubleshooting |
| request-coalescing | (long-tail) | informational | concurrent identical-query timeline | overview, P2 |
| security-model (absorbs proxy-safety) | snowflake proxy security / snowflake security (480, KD 49 bridge) | informational | credential/session in-memory boundary + TLS | overview, deployment/tls |

## Well-Architected section (12 pages) — fully specified

Positioning (post-GEO audit): Snowflake publishes its own "Well-Architected
Framework / cost pillar" — do NOT compete on the generic phrase. chukei's wedge is
the **enforceable** framework: every principle maps to a deterministic mechanism
(a chukei plugin) and a quotable checklist, not just advice. Target the opinionated
hierarchy + best-practices long-tail Snowflake's docs under-serve.

| Page (URL `/docs/well-architected/...`) | Target keyword (vol/KD) | Intent | Quotable artifact |
|---|---|---|---|
| index | snowflake best practices (170, KD 21) / snowflake cost optimization framework | commercial/info | the suspend>size>cache>rewrite hierarchy diagram |
| visibility | snowflake cost visibility / query history cost analysis (480, KD 24) | informational | METERING/QUERY_HISTORY SQL recipe |
| attribution | snowflake cost attribution by team (wedge) | informational | per-team/per-dbt-model attribution table |
| elimination | reduce snowflake costs (50, KD 20) / snowflake idle warehouse | informational | "queries you should never run on Snowflake" do/don't |
| efficiency | snowflake warehouse sizing (140, KD 26) / query optimization (110, KD 23) | informational | warehouse size→credit rate table |
| governance | snowflake finops (40, KD 18) / chargeback showback | commercial | governance maturity checklist |
| maturity-model | (long-tail "snowflake finops maturity") | informational | ad-hoc→measured→managed→optimized scorecard |
| checklist-visibility, -attribution, -elimination, -efficiency, -governance (5) | "snowflake cost checklist" long-tail | informational | one copy-paste checklist + SQL each |

Internal linking: index → 5 pillars + maturity; each pillar → its checklist + the
matching cost guide (P1–P4) + the enforcing chukei plugin in reference/. Every page
ends with the standard replay-simulator CTA.

## Docs site structure (Docusaurus, mirrors kafka-backup-docs; ~88 pages)

| Section | Pages | Content |
|---|---|---|
| getting-started | 4 | intro, install (brew/docker/binary), 10-min quickstart w/ savings report, replay simulator on your QUERY_HISTORY |
| guides | 19 | the four SEO pillars + inner pages above |
| architecture | 7 | wire protocol shim, plugin bus & precedence, fail-open design, fingerprinting (hard/soft), coalescing, session handling, security model |
| deployment | 8 | docker, k8s, TLS & certs, sizing, HA notes, observability/Prometheus, alert runbook, rollback |
| reference | 9 | config YAML, env overrides, CLI (up/doctor/replay/savings/evidence/plugins), metrics catalogue, plugin reference, determinism gate rules, exit codes |
| examples | 10 | python connector, dbt profiles, JDBC (+ocspFailOpen), snowsql, Airflow, Tableau, Power BI, Looker, key-pair auth, PAT auth |
| use-cases | 6 | BI dashboard caching, dbt CI cost control, ad-hoc analytics, embedded analytics, multi-team chargeback, audit-grade savings evidence |
| operator (post-pilot) | 2 | k8s operator roadmap, CRDs preview |
| benchmarks | 2 | proxy overhead methodology + numbers, soak/blame results |
| well-architected | 12 | **Snowflake Cost Optimization Framework** (kafka-backup's well-architected analog): principles, pillars (visibility/attribution/elimination/efficiency/governance), maturity model, checklists |
| troubleshooting | 6 | doctor, OCSP/cert issues, cache misses explained, blame mismatches, connection errors, FAQ |
| intro.md | 1 | what is chukei + llms.txt-aligned summary |

## Internal linking rules
- Homepage → 4 guide pillars + getting-started
- Every pillar → all its inner pages; inner → parent + 2 siblings
- Every blog post → exactly one pillar via contextual bridge (anchor above)
- Every guide page ends with the same CTA block → /docs/getting-started + replay simulator
- /docs/* cross-links to guides for concepts (never duplicate concept content in docs)
