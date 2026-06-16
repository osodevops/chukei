# Cost-saving innovations beyond the PRD's six plugins

The PRD's plugin set (cache, route, rewrite, bandit, suspend, attribute)
covers the table stakes that Greybeam/Keebo/Espresso also chase. The ideas
below come from reviewing what the proxy position makes possible that those
six don't capture. Ordered roughly by (savings × confidence) / effort.

## 1. In-flight request coalescing — ✅ implemented

When a dashboard refresh storm or a retrying orchestrator fires N identical
deterministic reads concurrently, chukei now sends **one** upstream
execution and fans the result out. The cache dedupes across *time*;
coalescing dedupes across the *in-flight window* — it works before the cache
is warm, and even with the cache disabled.

- Key = hard fingerprint (canonical SQL + literals) + bind variables +
  session token. The default `coalesce.scope: session` means row-level
  security can never leak rows across sessions; `account` scope is opt-in
  for RLS-free accounts.
- Verified: 10 concurrent identical queries → exactly 1 upstream call
  (`proxy_e2e::concurrent_identical_queries_coalesce_to_one_upstream_call`).

## 2. Snowflake result-cache maximizer (zero-infrastructure savings)

Snowflake's own RESULT_CACHE is free and 24 h — but only hits on
**byte-identical** query text. BI tools and ORMs emit the same query with
cosmetic differences (whitespace, casing, generated comments), silently
missing it. chukei already computes a canonical rendering; submitting that
canonical text upstream makes Snowflake's free cache do the saving.

- Effort: small (a rewrite rule that fires when canonical ≠ raw).
- Risk: low; semantics identical by construction. Gate: must not strip
  dbt's metadata comment when Snowflake-side attribution depends on it.
- This is savings *we don't even have to serve* — pure hit-rate transfer.

## 3. Time-predicate cache splitting (partial cache hits)

"Last 30 days" dashboards re-scan 29 unchanged days every morning. For
queries with a monotonic time predicate, serve the historical sub-range from
the Iceberg cache and query upstream only for the delta window, merging
results. PRD §11.1 already mentions predicate-aware *invalidation*; this is
predicate-aware *composition*, and it's the biggest cache-hit-rate unlock
on dashboard workloads.

- Effort: large (needs result merging + strict aggregation rules: only
  decomposable aggregates, GROUP BY containing the time bucket).
- Risk: medium → mitigate with blame mode sampling exactly as for whole-query
  cache. Ship behind `experimental`.

## 4. Warm-warehouse affinity (resume avoidance)

The inverse of plug-suspend: when a query targets a *suspended* warehouse
and an equivalent-or-larger warehouse is already running (and the role has
USAGE), submit there instead via session `USE WAREHOUSE`. Saves the 60 s
resume minimum plus a fresh idle tail. Snowflake's own queueing can't do
this across warehouses.

- Effort: medium; needs a live warehouse-state view (SHOW WAREHOUSES poll).
- Risk: low-medium (per-warehouse chargeback shifts — tag the query with the
  original target so plug-attr reattributes the cost).

## 5. Retry-storm insurance (negative-result caching)

Failed queries still bill warehouse time, and orchestrators retry the same
failing SQL on a tight loop. Cache *deterministic compile-time failures*
(syntax errors, missing objects, permission errors) for a short cooldown and
short-circuit retries with the same error response.

- Effort: small (the cache machinery exists; key on fingerprint + error class).
- Risk: low if limited to error classes that cannot self-heal within the
  cooldown (never cache resource/transient errors).

## 6. Materialized-view candidate mining (cold path, zero risk)

Soft-fingerprint clusters are exactly "the same query shape with different
literals." A cluster with high aggregate spend over a small table set is a
materialized-view / pre-aggregation candidate. `chukei replay` can rank
candidates with projected savings and emit them in the evidence bundle —
a consulting-grade artifact (Devon persona) no competitor produces.

- Effort: small-medium, entirely offline; extends the replay report.

## 7. Interactive LIMIT guard

Sessions identified as interactive BI exploration (APPLICATION_NAME +
SELECT * + no LIMIT) get a configurable LIMIT injected, with a
`/*+ chukei:nolimit */` escape hatch. Crude but brutally effective against
the "analyst selects star from the events table" bill spike.

- Effort: small. Risk: visible to users — default off, suggest-only first.

## 8. Dev/CI redirection to clones

Queries tagged `ci`/`dev` (hint, APPLICATION_NAME, or user pattern) are
rewritten to target a zero-copy clone database and routed to an XS
warehouse. CI suites against production-sized warehouses are a quietly huge
line item.

- Effort: medium (object-name rewriting is already a rewrite-rule shape).

## 9. Downsize recommendations from spill telemetry

plug-bandit (P1) explores sizes; a cheaper P0 step is deterministic: queries
that complete on L with zero local/remote spill and low scan are flagged
"would fit M" in suggest-only mode, feeding both the operator and, later,
the bandit's prior. Pure observability → trust-building before enforcement.

## Sequencing recommendation

| Order | Item | Why now |
|---|---|---|
| ✅ | Coalescing (1) | done, on by default, session-safe |
| next | Result-cache maximizer (2) | hours of work, zero infra, immediately measurable in replay |
| next | Retry-storm insurance (5) | small, protects the worst pathological spend |
| then | MV mining (6) | strengthens the replay/evidence funnel asset |
| then | Warm-affinity (4) → LIMIT guard (7) → dev/CI clones (8) | each needs live-traffic confidence |
| later | Time-predicate splitting (3) | biggest unlock, hardest correctness story — after blame mode is live |
