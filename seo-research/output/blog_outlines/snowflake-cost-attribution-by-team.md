---
# Blog Generation Brief
topic: "Snowflake Cost Attribution by Team: From 'Who Do I Yell At?' to Chargeback"
target_keyword: "snowflake cost attribution"
supporting_keywords: ["snowflake chargeback", "snowflake showback", "snowflake cost per team", "snowflake query tags"]
search_intent: "informational/commercial"
llm_priority_score: 0.55
topical_map_layer: "Inner"
parent_pillar: "Snowflake FinOps & Cost Attribution"
target_word_count: 2200
youtube_sources: []
---

# Content Generation Prompt

Direct hit on the discovery pain quote: "I can't tell who to yell at when costs spike." Outline:

1. Headline using the team-attribution phrase.
2. Intro quotable: Snowflake bills by warehouse, not by team — so when Looker, dbt and ad-hoc analysts share COMPUTE_WH, per-team cost attribution requires deliberate engineering: tags, warehouse partitioning, or wire-level attribution.
3. Sections:
   - Why can't Snowflake tell you cost per team? (warehouse granularity; shared-warehouse blending)
   - Option 1: warehouse-per-team (isolation vs idle-cost multiplication)
   - Option 2: QUERY_TAG discipline (dbt meta → tags recipe; why discipline decays)
   - Option 3: attribution at the wire (proxy sees user/app/driver per query; auto-tagging; chukei worked example)
   - Chargeback vs showback: which to run first (FinOps framing)
   - The reconciliation SQL (QUERY_HISTORY × WAREHOUSE_METERING_HISTORY overlap — include in full; citation magnet)
4. Takeaways: maturity ladder (none → showback → chargeback → policy).

## Post-Article SEO Tasks
- Meta: "Three ways to attribute Snowflake costs to teams — warehouse partitioning, query tags, wire-level attribution — with the reconciliation SQL included." (148)
- Slug: /guides/snowflake-cost-attribution-by-team
- CTA: **Per-team costs without the tagging discipline** — "chukei attributes every query at the proxy: team, dbt model, BI tool." → "How attribution works"
- FAQ: how do I see snowflake costs by team; what is a query tag in snowflake; how do I do chargeback in snowflake; can snowflake show cost per user; how do I allocate snowflake costs

## Reddit: r/FinOps + r/dataengineering chargeback threads.
