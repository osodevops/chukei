You probably should not block launch on Databricks support, but you should design the proxy so Databricks is an obvious second target and line up 1–2 design partners there. Day‑1 Snowflake, Databricks‑ready architecture, Databricks GA shortly after is the most defensible plan.

Below is the reasoning, structured by your points and ending with a concrete recommendation.

---

## 1) How Databricks SQL warehouse billing differs from Snowflake

**Snowflake basics**

- **Metering unit:** **credits**. Warehouses are sized (XS–6XL etc.), each size burns a fixed number of credits per hour when running; billed per second with 60‑second minimum.[1][2]  
- **What you pay for:**
  - Compute: credits × regional list price (roughly \$1.5–4 per credit, edition/commit dependent).[2]
  - Storage: separate, ~\$23/TB/month after compression.[2]  
- **Lifecycle / autosuspend:**
  - Virtual warehouses have **auto‑suspend and auto‑resume**; this is core to Snowflake’s “no ops” story and a big cost lever.[1][2]
  - Defaults vary by account, but many orgs still misconfigure them (e.g., 30–60 minutes instead of 5), which is a major waste pattern.

**Databricks SQL basics**

- **Metering unit:** **DBUs (Databricks Units)**.[1][2][6]
  - SQL Classic, SQL Pro, Serverless all priced as DBUs with different rates.
  - Example from recent pricing: on AWS, SQL Classic ~\$0.22/DBU, SQL Pro \$0.55/DBU, SQL Serverless \$0.70/DBU including infra.[1][6]
- **Dual billing model:**
  - For **non‑serverless** SQL (Classic/Pro), you pay:
    - Databricks DBUs **plus**
    - Cloud provider costs (VMs, storage, networking) directly to AWS/Azure/GCP.[2]
  - For **serverless SQL**, DBU rates are higher but **include cloud instance cost** (no separate EC2/VM bill).[1][6]
  - This **dual‑billing** makes TCO estimation and optimization trickier than Snowflake; infra can add “50–200% on top of DBU charges.”[2]
- **Warehouse types and lifecycle:**
  - Databricks has **Classic, Pro, and Serverless SQL warehouses** with different capabilities.[4][6]
  - Warehouses can be configured with **auto‑stop** and min/max clusters, but Databricks does not enforce aggressive auto‑suspend defaults the way Snowflake markets its warehouses.[1][4][7]
  - For non‑serverless, there is still underlying cluster lifecycle to worry about: start/stop delays, min node counts, etc.[2][7]

**Key differences relevant to a cost proxy**

| Aspect                      | Snowflake                                                  | Databricks SQL                                                                                                          |
|-----------------------------|------------------------------------------------------------|-------------------------------------------------------------------------------------------------------------------------|
| Metering unit               | Credits                                                    | DBUs                                                                                                                    |
| Infra billing               | Included in credit rate                                    | Separate cloud bill for non‑serverless; included only for Serverless SQL                                               |
| Autosuspend / autostop      | Core primitive, strongly encouraged                        | Available but more like cluster config; defaults vary and can be conservative                                          |
| Cost estimation complexity  | Relatively straightforward (credits × price)              | Harder: DBUs + VM types, autoscaling, spot/on‑demand, etc.; especially complex for Jobs / non‑SQL workloads            |
| Scope of engine             | Primarily SQL DW & services                                | Lakehouse: SQL, Spark jobs, ML, streaming, model serving – much broader cost surface                                    |

For your proxy: Snowflake = “one meter, one bill.” Databricks SQL = “one meter plus hidden meter,” and your proxy cannot fully optimize costs without being infra‑aware (node types, autoscaling, serverless vs classic).

---

## 2) Waste patterns: do the same issues exist on Databricks?

You listed three: idle warehouses, repeated identical queries, missing per‑team attribution.

### 2.1 Idle warehouses / clusters

- On **Snowflake**, idle virtual warehouses are a primary waste driver if auto‑suspend is misconfigured or disabled; Snowflake relies on warehouses to spin down when inactive.[1][2]
- On **Databricks SQL**:
  - SQL warehouses have **auto‑stop** but many teams set generous timeouts because startup latency annoys users, leading to **long‑running but lightly used warehouses**.[4][7]
  - Non‑SQL clusters (Jobs / All‑Purpose) are notorious for being left up by data scientists and engineers, especially in shared dev environments.[7][8]
  - Several Databricks cost guides explicitly mention **cluster sprawl and idle clusters** as the central FinOps issue.[7][8]

So **yes, the idle‑compute problem exists, arguably worse** on Databricks because:
- There are more cluster types and lifecycle paths (jobs, interactive, SQL, ML).
- Auto‑stop is less “forced” and often tuned for UX rather than cost.

For your proxy, Databricks gives you a richer set of optimization levers (min/max clusters, pool configs, job schedules) but also more complexity.

### 2.2 Repeated identical queries and result reuse

- **Snowflake**:
  - Has a transparent **result cache** keyed by query text, session settings, and underlying table state; repeated identical queries often hit cache at no extra compute cost.
  - Many BI tools still issue redundant queries (slightly different predicates, lots of `SELECT *`), so query‑level optimization and routing can still save money.
- **Databricks SQL**:
  - Supports **caching mechanisms** (Delta cache and SQL cache), and the Query History can surface repeated queries.[3][4]
  - However, Databricks is less “warehouse‑as‑black‑box” and more “Spark engine,” and teams often use Databricks via multiple clients (SQL editor, notebooks, BI tools). This creates **duplicate or near‑duplicate query patterns** across surfaces.
  - There is no strong evidence that Databricks solves cross‑client query deduplication for you; it mostly provides per‑warehouse caching and compute metrics.[4][7]

So **the same waste signature (repeated or inefficient queries) exists**, but exploitation is a bit different:
- Snowflake: focus on routing, caching, warehouse sizing.
- Databricks: also consider tuning Spark configs, file layout, Delta ZORDERing, etc., which is beyond just a proxy.

Your proxy’s “query fingerprint → routing/caching” model is transferable, but you will need deeper integration with Databricks’ caching semantics and query history.

### 2.3 Per‑team / per‑project attribution

- **Snowflake**:
  - Native support via **warehouses per team**, roles, and RESOURCE MONITORs, but many orgs still end up with shared warehouses and fuzzy attribution.[1][8]
  - Third‑party tools (Metrist, Ternary, Observe, etc.) explicitly market better attribution for Snowflake.
- **Databricks**:
  - Databricks cost visibility is more complex because costs span:
    - DBUs (by workspace, cluster, SQL warehouse) and
    - Cloud infra (EC2/VMs, storage, network) that is not labeled by Databricks concepts by default.[2][7][8]
  - Databricks’ own **cost reports and usage tables** do allow grouping by workspace, cluster, job, and sometimes tags, but connecting that cleanly to **teams and products** requires consistent tagging and external tooling.[7][8]
  - This gap is precisely what tools like **Overwatch** and others target: mapping Databricks usage to meaningful business dimensions.

So **no, Databricks does not “already solve” attribution natively in a way that removes the need for external tools**. It has primitives but still leaves a lot of stitching work, especially across DBU and cloud bills.

---

## 3) Who competes in Databricks cost optimization, and how crowded vs Snowflake?

This ecosystem is more fragmented and, today, **less crowded than Snowflake’s** for pure cost‑optimization proxies, but Databricks‑specific tools do exist.

### Databricks‑focused or Databricks‑centric tools

A non‑exhaustive but representative list:

- **Databricks Overwatch**
  - An open‑source monitoring and cost‑attribution solution maintained with Databricks involvement.
  - Focus: ingest Databricks audit / billing logs into Delta tables, provide **dashboards for performance and cost**, cluster utilization, job failures, etc.
  - Strength: deep Databricks integration; weakness: more of a *BI layer* than an active optimization proxy.
- **Sync Computing – Gradient**
  - Targets Spark / Databricks cost‑performance optimization.
  - Focus: recommending **optimal cluster configurations and autoscaling settings** for jobs and workflows, not primarily SQL query routing.
  - Competes partly with any system that claims to right‑size Spark/Jobs clusters.
- **Yeedu** (and similar Databricks observability / FinOps platforms)
  - Typically focus on **Databricks usage analytics, cost allocation, and performance insights** rather than being in‑path compute proxies.
  - Often pitch to central data/FinOps teams.
- **Cloud‑wide FinOps platforms with Databricks support**
  - e.g., Flexera, CloudZero, DoiT, Ternary, ProsperOps, etc. Many ingest Databricks billing exports for reporting and show **Databricks as a line item**.[1][7][8]
  - Their Databricks support is mostly read‑only reporting/alerting, not active query routing or auto‑tuning.

Contrast with **Snowflake**:

- There is a noticeably **larger number of Snowflake‑specific optimization tools** and more generic data cost tools whose first case study is Snowflake:
  - Snowflake‑only or Snowflake‑first tools doing warehouse sizing, auto‑suspend tuning, usage anomaly detection, and per‑team attribution.
  - BI vendors and data observability platforms frequently ship **Snowflake integration ahead of Databricks**, reinforcing Snowflake as the first platform for optimization add‑ons.[1][7][8]

So, **Databricks optimization market exists but is less saturated**, especially if your angle is:

- *In‑path proxy* for SQL workloads (Databricks SQL warehouses) rather than just monitoring.
- **Multi‑engine awareness** (Snowflake + Databricks + maybe BigQuery/Redshift next), which almost no Databricks‑specific player offers today.

---

## 4) Evidence of demand: multi‑engine cost tools vs per‑platform best‑of‑breed

From available industry commentary and platform features:

- Many mid‑ to large‑size data teams are **already multi‑engine**:
  - Snowflake + Databricks is a common pairing:[2][7][8]
    - Snowflake for BI / reporting.
    - Databricks for ML/streaming/lakehouse workloads.
- FinOps and central data platform teams increasingly want **cross‑platform views of spend and efficiency**:
  - Cloud‑centric FinOps tools (Flexera, etc.) position themselves as **multi‑cloud, multi‑service cost observability**, with data platforms (Snowflake, Databricks, BigQuery) as top‑level entities in the same UI.[1][7]
  - Data observability tools also highlight “end‑to‑end” cost visibility across warehouses, lakes, and pipelines.

However, the **execution‑level optimization tooling** pattern is mixed:

- **Per‑platform best‑of‑breed reality today:**
  - Snowflake teams adopt Snowflake‑specific tools (or native features like Resource Monitors, object tagging).
  - Databricks teams adopt Databricks‑centric solutions (Overwatch, cost dashboards, Gradient), or build internal tooling.
- **Multi‑engine optimization demand is more pronounced at the FinOps / leadership layer**:
  - Finance and central platform owners want a unified view of “cost per product / per user journey” that spans Snowflake, Databricks, and cloud services.
  - Individual domain teams often care most about the engine they live in day‑to‑day.

For a **proxy** that sits in the query path:

- Many orgs will adopt it first on their **primary analytics engine** (often Snowflake).
- Once value is proven, platform teams are likely to **ask for second/third engine support** so they can standardize on one control plane for cost policies and routing.

So there is **clear directional demand for multi‑engine tools** at the platform/FinOps layer, but you gain adoption fastest by being very good on **one engine first**, then expanding.

---

## 5) Devtools launch patterns: multi‑platform day 1 vs single‑platform first

Looking across adjacent categories (data quality, ELT, BI/semantic layers, workflow engines, query performance tools), a common pattern emerges:

**Single‑platform first (most common, often successful):**

- Data quality/observability: many start with **Snowflake‑only** or **Databricks‑only** integration before expanding to BigQuery/Redshift.
- ELT tools: dbt Core originally had a strong focus on a small set of warehouses, and commercial tools like Fivetran rolled out connectors and warehouse support over time.
- Performance tuning and query analyzers: typically **Snowflake‑first** or **Postgres‑first**, later generalized.
- Rationale:
  - Deep platform integration takes real time: billing exports, query history APIs, role/permission models, tagging, and engine quirks.
  - Early users expect **sharp, opinionated behavior** that matches their platform; superficial multi‑platform support feels “lowest common denominator” and underwhelms.

**Multi‑platform day 1 (rarer, higher lift):**

- Some **cloud‑agnostic observability / monitoring / APM** tools launch with multiple database integrations day 1.
- They typically operate at a **network or host level** (e.g., sniff queries, scrape metrics) or via standard interfaces (JDBC), which cuts down per‑engine work but also limits how deep they can go.
- The success stories here are usually:
  - Broad but shallow v1 (just basic metrics / query stats everywhere), then go deeper on the engines where they see traction.
  - Strong enterprise GTM where multi‑engine support is a sales requirement from day 0.

For a **proxy that rewrites/reroutes queries and optimizes spend**, you are closer to the “deep integration” camp: you must understand:

- How to start/stop warehouses (Snowflake) vs clusters/warehouses (Databricks).
- How to interpret query history and cost metrics.
- How to safely attribute and enforce policies.

This strongly favors **single‑platform first, but architect for multi‑engine**.

---

## Recommendation: should you ship Databricks support on day 1?

**Short answer:**  
Focus on a **Snowflake‑first GA**, but architect the proxy as engine‑agnostic and **validate Databricks with a small set of design partners**. Do not delay launch to finish Databricks support, but commit to Databricks as your first post‑GA platform.

Here’s the reasoning, aligned to your questions:

1. **Billing models and complexity:**
   - Snowflake is simpler to start with: one meter, strong autosuspend, clean warehouse semantics.[1][2]
   - Databricks SQL adds DBU + infra complexity and diverse workload types, which require more engineering and UX thought to get right.[2][7]
   - Shipping both day 1 increases your surface area dramatically and can dilute the sharpness of your Snowflake story.

2. **Waste patterns and impact:**
   - The **same waste patterns exist on Databricks**—idle clusters/warehouses, repeated queries, fuzzy attribution—and arguably are more severe because of cluster sprawl and dual billing.[2][7][8]
   - This means Databricks is a **high‑value target**, but also a **harder target** for a first release.
   - Proving your optimization model on Snowflake first gives you cleaner, measurable wins and lets you refine your UX before tackling Databricks’ complexity.

3. **Competitive landscape:**
   - Snowflake side: more competition, but also more buyer awareness and clear problem framing (“Snowflake is expensive; we need help.”).
   - Databricks side: some serious players (Overwatch, Gradient, Yeedu), yet still **less crowded** for a proxy‑style optimizer.
   - Entering Databricks with a **credible Snowflake success story** gives you differentiation versus incumbents that are Databricks‑only dashboards.

4. **Demand patterns:**
   - Org‑level demand is clearly moving towards **multi‑engine cost controls**, but adoption will start where the pain is sharpest and your product is clearly excellent.
   - A great Snowflake experience that visibly cuts spend will naturally drive customers to **ask for Databricks next**, giving you validated roadmap pull rather than speculative platform breadth.

5. **Execution risk and developer velocity:**
   - Supporting Databricks on day 1 forces you to:
     - Implement the DBU + infra cost model.
     - Integrate with Databricks SQL warehouse APIs and possibly Jobs/cluster APIs.
     - Design UI and policies that make sense across two very different billing semantics.
   - That complexity likely slows initial release and makes your first users test a less polished product on both platforms.

**Concrete plan**

- **Architecture:**
  - Build the proxy with an explicit **“engine driver” abstraction**:
    - `SnowflakeEngineDriver`, `DatabricksSQLEngineDriver`, etc.
    - Shared policy layer (e.g., “suspend idle warehouse after X minutes”, “dedupe identical queries within Y seconds”).
  - Normalize cost and usage metrics to a **common internal schema** so multi‑engine views are natural later.

- **Day‑1 GA:**
  - Ship with **Snowflake only**.
  - Include **public design notes** or docs showing how Databricks would fit, signalling multi‑engine intent.
  - Make sure your policy model, logging, and tagging assume that “engine” is a dimension, not a hard‑coded Snowflake world.

- **Databricks alpha/beta:**
  - While Snowflake GA is in early adoption, run a **closed beta with 1–3 Databricks design partners**.
  - Start with **Databricks SQL warehouses only**, not all cluster types, to keep scope manageable.
  - Focus on:
    - Idle warehouse detection and auto‑stop suggestions.
    - Query deduplication / caching awareness.
    - Per‑team attribution using Databricks tags/workspaces plus cloud tags.

- **Public Databricks support:**
  - Release Databricks once:
    - You’ve demonstrated **measurable savings** for at least one design partner.
    - You have patterns for mapping DBUs + infra to a unified cost model.
  - Market the product explicitly as **“multi‑engine cost proxy”** with Snowflake + Databricks as the initial pair.

If you have enough runway and Databricks design partners already begging for this, you could aim for a **Databricks “tech preview” at the same time as Snowflake GA**. But shipping fully polished, fully supported Databricks on day 1 is likely not the best trade‑off versus nailing the Snowflake experience and keeping your small team focused.
