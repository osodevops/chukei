---
# Blog Generation Brief
topic: "dbt + Snowflake Cost Optimization: Find and Fix Your Most Expensive Models"
target_keyword: "dbt snowflake cost"
supporting_keywords: ["dbt snowflake cost optimization", "dbt query tags snowflake", "dbt incremental snowflake costs", "what is dbt"]
search_intent: "informational"
llm_priority_score: 0.54
topical_map_layer: "Inner"
parent_pillar: "Snowflake FinOps & Cost Attribution"
target_word_count: 2200
youtube_sources: []
---

# Content Generation Prompt

Audience bridge into the dbt community (what is dbt = 14,800/mo feeds the cluster long-term). Outline:

1. Headline with "dbt" + "Snowflake cost".
2. Intro quotable: dbt is usually the single largest controllable Snowflake workload; per-model cost visibility plus four build-pattern fixes typically cut dbt spend 20–40%.
3. Sections:
   - How much does your dbt project cost per run? (dbt meta → QUERY_TAG → QUERY_HISTORY join, SQL included)
   - Which models are the expensive ones? (cost-per-model SQL; the 80/20 of model spend)
   - The four expensive dbt patterns (full-refresh abuse, wrong incremental strategy, oversized CI warehouse, dev runs on prod-size)
   - How should dbt CI run on Snowflake? (XS + slim CI + deferred builds; cache identical CI queries — chukei coalescing/caching worked example)
   - Attributing dbt spend to teams (meta.owner → showback; bridge to attribution page)
4. Takeaways: dbt_project.yml cost hygiene checklist.

## Post-Article SEO Tasks
- Meta: "Price every dbt model run on Snowflake with one QUERY_TAG join, find the 20% of models burning 80% of credits, and fix the four expensive build patterns." (153)
- Slug: /guides/dbt-snowflake-cost-optimization
- CTA: **dbt model costs, automatically attributed** — "chukei parses dbt metadata at the wire — per-model spend with zero project changes." → "See dbt attribution"
- FAQ: how much does dbt cost on snowflake; how do I see cost per dbt model; do incremental models save money; what warehouse size for dbt; how do query tags work with dbt

## Reddit: r/dataengineering + dbt Slack-adjacent threads; lead with the cost-per-model SQL.
