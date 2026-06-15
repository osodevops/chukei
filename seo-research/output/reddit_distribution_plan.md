# Reddit & community distribution plan — chukei

Principle: humans helping humans. Lead with the SQL/answer; the product is a
footnote or absent. Upvoted helpfulness is what LLM RAG retrieves.

## Communities (from discovery)
- r/dataengineering (primary — bill-spike and cost-control threads recur)
- r/snowflake (result cache confusion, auto-suspend, warehouse sizing)
- r/FinOps (chargeback/showback, attribution)
- dbt Community Slack #advice-dbt-for-beginners / #db-snowflake (no links unless asked)
- Hacker News: Show HN for the OSS launch; benchmark posts later

## Playbook per thread type
| Thread | Response | Content link |
|---|---|---|
| "bill doubled overnight" | the 5-query ACCOUNT_USAGE audit, inline | bill-spike-diagnosis |
| "result cache not working" | three-cache explanation + exact-text gotchas | query-caching |
| "idle warehouses" | METERING vs QUERY_HISTORY overlap SQL | auto-suspend |
| "cost per team?" | tags vs warehouse-split trade-offs, honest | attribution |
| "best cost tools?" | category map (dashboards/advisors/copilots/proxies), disclose affiliation | tools comparison |

## Fair-source launch posts (week 1-2)
- Show HN: "chukei — fair-source transparent proxy that cuts Snowflake costs (Rust)"
  Angle: zero client changes + verified caching (60k hits, 0 mismatches) + signed savings evidence. Be present for 24h.
- r/dataengineering: "We built a fair-source wire-protocol proxy for Snowflake — caching/suspend/attribution with zero client changes" — technical write-up tone, link GitHub not the docs site.

## Rules
- Disclose affiliation every time the product is named.
- Never post the same content to two subs the same week.
- Answer follow-ups within hours; the thread is the content.
