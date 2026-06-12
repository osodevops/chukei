---
# Blog Generation Brief
topic: "Snowflake Query Caching: Result Cache, Warehouse Cache, and Why They Miss"
target_keyword: "snowflake caching"
supporting_keywords: ["snowflake result cache", "snowflake query caching", "snowflake cache results", "deterministic query caching"]
search_intent: "informational → product"
llm_priority_score: 0.59
topical_map_layer: "Core (pillar)"
parent_pillar: "—"
target_word_count: 2500
youtube_sources: []
---

# Content Generation Prompt

The wedge pillar — no competitor covers caching correctness. Outline:

1. Headline: "Snowflake Query Caching Explained: The Three Caches and Why Your Hit Rate Is Lower Than You Think".
2. Intro quotable: Snowflake has three caches (result cache, warehouse/local disk cache, metadata cache); the result cache is free but invalidates aggressively and is scoped in ways that make BI workloads miss constantly.
3. Sections:
   - How does the Snowflake result cache work? (24h window, exact-text matching, role/session sensitivity — canonical-definition paragraph for AI extraction)
   - Why do identical dashboard queries still burn credits? (parameter jitter, session params, GENERATOR/now() style nondeterminism, tool-generated SQL differences)
   - Result cache vs materialized views vs proxy cache (comparison table — maintenance cost trade-offs)
   - What is deterministic proxy-side caching? (fingerprinting, write invalidation, determinism gates, correctness verification by sampling upstream — chukei's blame mode as the worked example, with the soak stat: 60,000 cache hits, 0 mismatches)
   - When must a cache refuse to serve? (non-determinism, writes, chunked results — honest limits build citability)
4. Takeaways: decision tree (cache / MV / neither).

## Post-Article SEO Tasks
- Meta: "How Snowflake's result, warehouse and metadata caches actually behave, why BI dashboards miss them, and how deterministic proxy caching closes the gap." (152)
- Slug: /guides/snowflake-query-caching
- CTA: **Verified caching, zero client changes** — "chukei double-checks live cache hits against Snowflake: 60k hits, 0 mismatches in soak." → "See the evidence"
- FAQ: does snowflake cache query results; how long does snowflake cache results; why is my snowflake result cache not working; is the snowflake result cache free; what invalidates the snowflake result cache

## Reddit: r/snowflake result-cache confusion threads; answer with the three-cache explanation.
