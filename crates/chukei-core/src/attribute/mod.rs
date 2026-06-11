//! ⑥ `plug-attr` — per-query cost attribution (PRD §11.6).
//!
//! Provenance is assembled, in priority order, from:
//! 1. chukei hint comments (`/*+ chukei:team=growth dag=daily_revenue */`)
//! 2. dbt's injected JSON comment (`/* {"app": "dbt", "node_id": …} */`)
//! 3. the JDBC/Python `APPLICATION_NAME` session property
//!
//! The plugin only ever `Annotate`s — it never changes the query. The wire
//! layer turns annotations into `QUERY_TAG` (when blank) and OTEL/OpenLineage
//! attributes.

use async_trait::async_trait;

use crate::config::AttributeConfig;
use crate::plugin::{Decision, Plugin, QueryContext, QueryTags};
use crate::Result;

pub struct AttributePlugin {
    config: AttributeConfig,
}

impl AttributePlugin {
    pub fn new(config: AttributeConfig) -> Self {
        Self { config }
    }

    fn source_enabled(&self, name: &str) -> bool {
        self.config.sources.iter().any(|s| s == name)
    }

    pub fn tags_for(&self, ctx: &QueryContext<'_>) -> QueryTags {
        let mut tags = QueryTags::new();

        if self.source_enabled("hint_comment") {
            for (k, v) in &ctx.analysis.hints.values {
                // size/tolerance are control hints, not provenance.
                if k != "size" && k != "tolerance" {
                    tags.insert(format!("chukei.{k}"), v.clone());
                }
            }
        }

        if self.source_enabled("dbt_meta") && self.config.dbt_metadata_parser {
            if let Some(meta) = parse_dbt_meta(ctx.raw_sql()) {
                if let Some(node) = meta.node_id {
                    tags.entry("chukei.dag".into()).or_insert(node);
                }
                if let Some(app) = meta.app {
                    tags.entry("chukei.app".into()).or_insert(app);
                }
            }
        }

        if self.source_enabled("application_name") {
            if let Some(app) = &ctx.session.application_name {
                tags.entry("chukei.app".into())
                    .or_insert_with(|| app.clone());
            }
        }

        if let Some(user) = &ctx.session.user {
            tags.entry("chukei.user".into())
                .or_insert_with(|| user.clone());
        }

        if self.config.auto_query_tag && ctx.session.query_tag.as_deref().unwrap_or("").is_empty() {
            if let Some(team) = tags.get("chukei.team").cloned() {
                tags.insert(
                    "chukei.auto_query_tag".into(),
                    format!("chukei:team={team}"),
                );
            }
        }

        tags
    }
}

#[derive(Debug, Default)]
struct DbtMeta {
    app: Option<String>,
    node_id: Option<String>,
}

/// dbt prepends a JSON block comment to every query it issues:
/// `/* {"app": "dbt", "dbt_version": "1.8.0", "node_id": "model.proj.x"} */`
fn parse_dbt_meta(raw_sql: &str) -> Option<DbtMeta> {
    let mut rest = raw_sql;
    while let Some(start) = rest.find("/*") {
        let after = &rest[start + 2..];
        let end = after.find("*/")?;
        let body = after[..end].trim();
        if body.starts_with('{') {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(body) {
                return Some(DbtMeta {
                    app: json.get("app").and_then(|v| v.as_str()).map(String::from),
                    node_id: json
                        .get("node_id")
                        .and_then(|v| v.as_str())
                        .map(String::from),
                });
            }
        }
        rest = &after[end + 2..];
    }
    None
}

#[async_trait]
impl Plugin for AttributePlugin {
    fn name(&self) -> &'static str {
        "attribute"
    }
    fn order(&self) -> i32 {
        60
    }

    async fn decide(&self, ctx: &QueryContext<'_>) -> Result<Decision> {
        let tags = self.tags_for(ctx);
        if tags.is_empty() {
            Ok(Decision::Passthrough)
        } else {
            Ok(Decision::Annotate(tags))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::Session;
    use crate::sql::analyze;

    fn ctx_tags(sql: &str, session: &Session) -> QueryTags {
        let analysis = analyze(sql).unwrap();
        let ctx = QueryContext {
            analysis: &analysis,
            session,
        };
        AttributePlugin::new(AttributeConfig {
            enabled: true,
            ..Default::default()
        })
        .tags_for(&ctx)
    }

    #[test]
    fn hint_comment_tags_extracted() {
        let tags = ctx_tags(
            "/*+ chukei:team=growth dag=daily_revenue */ SELECT 1",
            &Session::default(),
        );
        assert_eq!(tags["chukei.team"], "growth");
        assert_eq!(tags["chukei.dag"], "daily_revenue");
    }

    #[test]
    fn dbt_meta_parsed() {
        let sql = r#"/* {"app": "dbt", "node_id": "model.proj.daily_revenue"} */ SELECT 1"#;
        let tags = ctx_tags(sql, &Session::default());
        assert_eq!(tags["chukei.dag"], "model.proj.daily_revenue");
        assert_eq!(tags["chukei.app"], "dbt");
    }

    #[test]
    fn hint_beats_dbt_meta_for_dag() {
        let sql = r#"/*+ chukei:dag=pinned */ /* {"app": "dbt", "node_id": "model.x"} */ SELECT 1"#;
        let tags = ctx_tags(sql, &Session::default());
        assert_eq!(tags["chukei.dag"], "pinned");
    }

    #[test]
    fn session_facts_fill_gaps_and_auto_query_tag() {
        let session = Session {
            user: Some("SARA".into()),
            application_name: Some("looker".into()),
            query_tag: Some(String::new()),
            ..Default::default()
        };
        let tags = ctx_tags("/*+ chukei:team=growth */ SELECT 1", &session);
        assert_eq!(tags["chukei.app"], "looker");
        assert_eq!(tags["chukei.user"], "SARA");
        assert_eq!(tags["chukei.auto_query_tag"], "chukei:team=growth");
    }
}
