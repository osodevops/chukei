# Should chukei call itself a "proxy"? — positioning analysis (2026-06-12)

## Evidence

**How the two relevant worlds use the word:**
- Infra tools whose *core value is the proxying* lead with "proxy" and are
  beloved for it: pgbouncer ("connection proxy"), ProxySQL ("high-performance
  MySQL proxy"), Envoy ("edge and service proxy"), Amazon RDS Proxy (it's the
  product name). Self-hosted OSS engineers do not fear the word.
- Tools whose value is an *outcome* push "proxy" one level down: PlanetScale
  (architecturally proxy-ish → markets "serverless MySQL platform"),
  Polyscale (sits in the path → markets "database edge cache"; "acts as a
  proxy" appears only in technical docs).
- Snowflake cost-opt competitors avoid the word entirely — Keebo: "AI-native
  data warehouse copilot", "autonomous optimization"; the field uses
  copilot / FinOps platform / autonomous optimization. Sundeck (closest
  architectural analog) chose "traffic control layer". Nobody owns "proxy".

**Buyer-side objections to "proxy in front of the warehouse"** (all real,
all enumerable): added latency, single point of failure, credential/security
review surface, ops ownership, driver compatibility, blast radius. chukei
has a *measured* rebuttal for every one (2ms p99; fail-open + sub-10s
restarts invisible; credentials never persisted, trace-audited; drivers
validated incl. JDBC/async; single static binary in YOUR VPC) — but the
word triggers the security-review reflex before the rebuttals get heard.

**Search demand (SEMrush, US/mo):** the category words are small —
"snowflake proxy" 20 (KD0), "snowflake optimizer" 10, "snowflake
gateway"/"caching layer"/"autopilot"/"finops tool" ≈ 0. Generic infra terms
have volume ("data gateway" 720, "transparent proxy" 480, "database proxy"
110) but wrong intent. The traffic is all in OUTCOME terms ("snowflake
pricing" 1300, "cost optimization" 260). Category naming is therefore a
trust/AI-citation decision, not a traffic decision.

## Recommendation: two-layer positioning (Polyscale pattern)

**Layer 1 — the category (hero, README first line, llms.txt):** outcome-led.

> **chukei — the fair-source cost optimization engine for Snowflake.**
> Verified result caching, warehouse auto-suspend, SQL rewriting, and
> per-team cost attribution — with zero client changes.

"Engine" over "platform" (overclaim for a single binary), over "layer"
(passive), over "copilot" (crowded, AI-washed, and chukei is deterministic
— a genuine differentiator vs Keebo/Espresso).

**Layer 2 — the architecture (subhead, how-it-works, all technical docs):**
keep "transparent proxy" *proudly and precisely*.

> Deploys as a transparent wire-protocol proxy in your own VPC: drivers
> change one hostname, and any chukei failure degrades to verbatim
> passthrough.

Rationale:
- The zero-client-changes promise is incomprehensible without the proxy
  mechanics; engineers (the OSS audience) trust precise words — pgbouncer
  precedent.
- AI search cites canonical definitions: keeping "transparent proxy for
  Snowflake" in the intro paragraph wins the uncontested "snowflake proxy"
  retrieval space while the hero carries the outcome category.
- The objection list becomes a *strength*: a "Why a proxy is safe here"
  section answering each objection with our measured numbers is exactly
  the citable, original-data content the GEO strategy calls for.

**Banned words:** copilot (crowded + implies LLM-in-the-loop, which the
hot path deliberately is not), middleware (enterprise-stale), gateway
(taken by API gateways), agent (security-alarming).

## Where to apply
- README + repo description, docs-site hero/tagline, llms.txt first line,
  deploy workflow tagline, PRD §1 framing: outcome-first, proxy-second.
- Architecture/deployment/validation pages: unchanged — proxy language
  stays.
- New docs page (week-2 content): "Why a proxy in front of Snowflake is
  safe" — the objection table with measured answers.
