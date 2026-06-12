# chukei SEO & content strategy research

Produced 2026-06-12 by the seo-research skill (same pipeline as
kafka-backup-docs/seo-research). Domain placeholder `chukei.oso.sh` —
final subdomain TBD; same hosting as kafkabackup.com.

## Contents
- `discovery/` — Perplexity research: market landscape (Sundeck = closest
  analog; caching/attribution/rewriting wedges uncontested), competitor
  content audit, LLM visibility audit (what AI engines cite and why)
- `semrush-data/` — raw SEMrush exports: 717 unique keywords (seeds,
  related, broad, questions; competitor organics for keebo.ai,
  espresso.ai, select.dev)
- `scripts/score_keywords.py` — LLM-weighted scoring + clustering
- `output/keyword_masterlist.csv` — scored, clustered masterlist
- `output/content_map.md` — semantic topical map (central entity, 5
  pillars, inner/outer, bridges) + the ~88-page Docusaurus site structure
- `output/blog_outlines/` — 8 priority content briefs, generation-ready
- `output/llms_txt_template.md` — GEO: llms.txt / llms-full.txt plan
- `output/editorial_calendar.md` — 6-week launch sequence + refresh rota
- `output/reddit_distribution_plan.md` — community playbook

## Key findings
- Niche is low-difficulty: most targets KD < 40 (fine for a new subdomain);
  flagship `snowflake pricing` 1300/mo KD36, quick win `is snowflake a
  data warehouse` 320/mo KD15.
- AI engines cite: numbered checklists, canonical one-paragraph
  definitions, tables, and pages with original numbers. chukei has real
  numbers competitors lack (60k verified cache hits / 0 mismatches,
  +2ms p99, signed evidence) — every page gets a stats box.
- Not yet run: YouTube transcript enrichment (yt-dlp not installed);
  Master Research Prompt + domain_organic for own domain (pending
  subdomain decision).
