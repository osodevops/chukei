//! ② `plug-router` — rules-based engine routing (PRD §11.2).
//!
//! P0 decision rule (no ML):
//!
//! ```text
//! read-only ∧ deterministic
//!   ∧ every table has an Iceberg replica
//!   ∧ scan_estimate_rows < small_scan_rows   → DuckDB
//! otherwise                                  → Snowflake (passthrough)
//! ```
//!
//! Unknown scan estimates route to Snowflake: bypassing the warehouse is an
//! optimisation we must *prove* safe per-query, never assume. Execution on
//! DuckDB itself lives behind the wire layer's engine abstraction.

use std::collections::HashSet;

use async_trait::async_trait;

use crate::config::RouterConfig;
use crate::plugin::{Decision, Engine, Plugin, QueryContext};
use crate::sql::parse::is_read_only;
use crate::Result;

pub struct RouterPlugin {
    config: RouterConfig,
    replicated_tables: HashSet<String>,
}

impl RouterPlugin {
    pub fn new(config: RouterConfig) -> Self {
        let replicated_tables = config
            .duckdb
            .replicas
            .iter()
            .map(|r| r.source.to_uppercase())
            .collect();
        Self {
            config,
            replicated_tables,
        }
    }

    fn route_for(&self, ctx: &QueryContext<'_>) -> Engine {
        if !self.config.duckdb.enabled || self.replicated_tables.is_empty() {
            return Engine::Snowflake;
        }
        if !is_read_only(&ctx.analysis.statement) || !ctx.analysis.features.deterministic {
            return Engine::Snowflake;
        }
        let features = &ctx.analysis.features;
        if features.tables.is_empty()
            || !features
                .tables
                .iter()
                .all(|t| self.replicated_tables.contains(t))
        {
            return Engine::Snowflake;
        }
        let threshold = self.config.small_scan_rows.unwrap_or(1_000_000);
        match features.scan_estimate_rows {
            Some(rows) if rows < threshold => Engine::DuckDb,
            _ => Engine::Snowflake,
        }
    }
}

#[async_trait]
impl Plugin for RouterPlugin {
    fn name(&self) -> &'static str {
        "router"
    }
    fn order(&self) -> i32 {
        20
    }

    async fn decide(&self, ctx: &QueryContext<'_>) -> Result<Decision> {
        match self.route_for(ctx) {
            Engine::Snowflake => Ok(Decision::Passthrough),
            engine => Ok(Decision::Route(engine)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{DuckDbConfig, ReplicaConfig};
    use crate::plugin::Session;
    use crate::sql::analyze;

    fn router(replicas: &[&str]) -> RouterPlugin {
        RouterPlugin::new(RouterConfig {
            enabled: true,
            duckdb: DuckDbConfig {
                enabled: true,
                replicas: replicas
                    .iter()
                    .map(|s| ReplicaConfig {
                        source: s.to_string(),
                        iceberg_path: format!("s3://x/{s}"),
                        refresh: "continuous".into(),
                    })
                    .collect(),
            },
            ..Default::default()
        })
    }

    fn route(router: &RouterPlugin, sql: &str, scan_rows: Option<u64>) -> Engine {
        let mut analysis = analyze(sql).unwrap();
        analysis.features.scan_estimate_rows = scan_rows;
        let session = Session::default();
        let ctx = QueryContext {
            analysis: &analysis,
            session: &session,
        };
        router.route_for(&ctx)
    }

    #[test]
    fn small_replicated_read_goes_to_duckdb() {
        let r = router(&["DIM.PUBLIC.COUNTRIES"]);
        assert_eq!(
            route(&r, "SELECT * FROM dim.public.countries", Some(5_000)),
            Engine::DuckDb
        );
    }

    #[test]
    fn unknown_scan_estimate_stays_on_snowflake() {
        let r = router(&["DIM.PUBLIC.COUNTRIES"]);
        assert_eq!(
            route(&r, "SELECT * FROM dim.public.countries", None),
            Engine::Snowflake
        );
    }

    #[test]
    fn unreplicated_table_stays_on_snowflake() {
        let r = router(&["DIM.PUBLIC.COUNTRIES"]);
        assert_eq!(
            route(&r, "SELECT * FROM analytics.raw.events", Some(10)),
            Engine::Snowflake
        );
    }

    #[test]
    fn join_with_one_unreplicated_table_stays() {
        let r = router(&["DIM.PUBLIC.COUNTRIES"]);
        let sql = "SELECT * FROM dim.public.countries c JOIN analytics.raw.events e ON e.c = c.id";
        assert_eq!(route(&r, sql, Some(10)), Engine::Snowflake);
    }

    #[test]
    fn large_scan_stays_on_snowflake() {
        let r = router(&["DIM.PUBLIC.COUNTRIES"]);
        assert_eq!(
            route(&r, "SELECT * FROM dim.public.countries", Some(50_000_000)),
            Engine::Snowflake
        );
    }

    #[test]
    fn non_deterministic_stays_on_snowflake() {
        let r = router(&["DIM.PUBLIC.COUNTRIES"]);
        assert_eq!(
            route(
                &r,
                "SELECT UUID_STRING() FROM dim.public.countries",
                Some(10)
            ),
            Engine::Snowflake
        );
    }
}
