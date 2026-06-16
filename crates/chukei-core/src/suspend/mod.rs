//! ⑤ `plug-suspend` — predictive warehouse auto-suspend (PRD §11.5).
//!
//! Snowflake's `AUTO_SUSPEND` is a static timeout; we model per-warehouse
//! query arrivals as a Poisson process with an exponentially-weighted
//! arrival rate, and recommend suspending when the probability of another
//! query arriving inside the suspend horizon drops below threshold — but
//! never when the expected resume penalty (60 s minimum billing on resume)
//! exceeds the expected idle savings.
//!
//! P0 ships `suggest-only`: recommendations are emitted as spans/metrics,
//! state is never altered. `enforce` (gated on an explicit role) is the
//! wire layer's job, later.

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::config::SuspendConfig;
use crate::plugin::{Decision, Plugin, QueryContext};
use crate::Result;

/// Snowflake bills a minimum of 60 s of credits on every resume.
const RESUME_PENALTY_SECS: f64 = 60.0;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SuspendRecommendation {
    pub warehouse: String,
    /// P(at least one query arrives in the next `horizon_secs`).
    pub p_arrival_in_horizon: f64,
    /// Expected idle seconds saved if we suspend now instead of waiting for
    /// the static AUTO_SUSPEND timeout.
    pub expected_idle_secs_saved: f64,
}

#[derive(Debug, Default)]
struct WarehouseStats {
    last_arrival_ms: Option<u64>,
    /// EWMA of inter-arrival gaps, in seconds.
    mean_gap_secs: Option<f64>,
    observations: u64,
}

pub struct SuspendModel {
    stats: Mutex<HashMap<String, WarehouseStats>>,
    /// Smoothing factor for the inter-arrival EWMA.
    alpha: f64,
    /// Suspend horizon: "would a query arrive within this window?"
    horizon_secs: f64,
    /// Recommend suspend when P(arrival in horizon) is below this.
    p_threshold: f64,
    /// Minimum observations before the model speaks at all (cold-start gate,
    /// same philosophy as the bandit's 1 000-observation rule).
    min_observations: u64,
}

impl Default for SuspendModel {
    fn default() -> Self {
        Self {
            stats: Mutex::new(HashMap::new()),
            alpha: 0.2,
            horizon_secs: 60.0,
            p_threshold: 0.2,
            min_observations: 20,
        }
    }
}

impl SuspendModel {
    /// Record a query arrival on a warehouse at `now_ms` (caller supplies the
    /// clock so the model is deterministic under test).
    pub fn record_arrival(&self, warehouse: &str, now_ms: u64) {
        let mut stats = self.stats.lock().unwrap();
        let entry = stats.entry(warehouse.to_string()).or_default();
        if let Some(last) = entry.last_arrival_ms {
            let gap_secs = (now_ms.saturating_sub(last)) as f64 / 1000.0;
            entry.mean_gap_secs = Some(match entry.mean_gap_secs {
                Some(mean) => self.alpha * gap_secs + (1.0 - self.alpha) * mean,
                None => gap_secs,
            });
        }
        entry.last_arrival_ms = Some(now_ms);
        entry.observations += 1;
    }

    /// Every warehouse the model has observed traffic for.
    pub fn warehouses(&self) -> Vec<String> {
        self.stats.lock().unwrap().keys().cloned().collect()
    }

    /// Should `warehouse` be suspended as of `now_ms`?
    pub fn recommend(&self, warehouse: &str, now_ms: u64) -> Option<SuspendRecommendation> {
        let stats = self.stats.lock().unwrap();
        let entry = stats.get(warehouse)?;
        if entry.observations < self.min_observations {
            return None;
        }
        let mean_gap = entry.mean_gap_secs?;
        if mean_gap <= 0.0 {
            return None;
        }
        let idle_secs = (now_ms.saturating_sub(entry.last_arrival_ms?)) as f64 / 1000.0;

        // Exponential inter-arrival: P(arrival within h) = 1 − e^(−h/μ).
        // (Memoryless, so time already idle doesn't change the hazard; it
        // does change the savings calculus below.)
        let lambda = 1.0 / mean_gap;
        let p_arrival = 1.0 - (-lambda * self.horizon_secs).exp();
        if p_arrival >= self.p_threshold {
            return None;
        }

        // Expected idle time still to come ≈ μ − idle already burned, floor 0,
        // and we only save what exceeds the resume penalty we'll pay later.
        let expected_remaining_idle = (mean_gap - idle_secs).max(0.0);
        let expected_saved = expected_remaining_idle - RESUME_PENALTY_SECS;
        if expected_saved <= 0.0 {
            return None;
        }

        Some(SuspendRecommendation {
            warehouse: warehouse.to_string(),
            p_arrival_in_horizon: p_arrival,
            expected_idle_secs_saved: expected_saved,
        })
    }
}

pub struct SuspendPlugin {
    pub model: SuspendModel,
    #[allow(dead_code)] // mode drives the (P1) enforce path in the daemon
    config: SuspendConfig,
}

/// Does this statement consume warehouse compute? Metadata-only statements
/// (SHOW, DESCRIBE, USE, EXPLAIN, session/transaction control) run on cloud
/// services, so they must not reset the idle model — a monitoring loop
/// polling SHOW WAREHOUSES would otherwise keep every warehouse "busy"
/// forever. Unknown shapes default to true: counting phantom activity only
/// delays a suspend, never breaks one.
fn runs_on_warehouse(stmt: &sqlparser::ast::Statement) -> bool {
    use sqlparser::ast::Statement as S;
    !matches!(
        stmt,
        S::ShowVariable { .. }
            | S::ShowVariables { .. }
            | S::ShowTables { .. }
            | S::ShowSchemas { .. }
            | S::ShowDatabases { .. }
            | S::ShowColumns { .. }
            | S::ShowFunctions { .. }
            | S::ShowObjects { .. }
            | S::ShowViews { .. }
            | S::ShowStatus { .. }
            | S::ShowCollation { .. }
            | S::ShowCreate { .. }
            | S::ExplainTable { .. }
            | S::Explain { .. }
            | S::Use(_)
            | S::Set(_)
            | S::StartTransaction { .. }
            | S::Commit { .. }
            | S::Rollback { .. }
    )
}

impl SuspendPlugin {
    pub fn new(config: SuspendConfig) -> Self {
        Self {
            model: SuspendModel {
                min_observations: config.min_observations,
                horizon_secs: config.horizon_secs,
                p_threshold: config.p_threshold,
                ..SuspendModel::default()
            },
            config,
        }
    }
}

#[async_trait]
impl Plugin for SuspendPlugin {
    fn name(&self) -> &'static str {
        "suspend"
    }
    fn order(&self) -> i32 {
        50
    }

    /// The plugin only observes; recommendations are pulled by the daemon's
    /// background sweep, never inline with a query.
    async fn decide(&self, _ctx: &QueryContext<'_>) -> Result<Decision> {
        Ok(Decision::Passthrough)
    }

    /// Arrivals are recorded post-result, NOT in `decide`, so that
    /// (a) cache hits never count — a fully-cached dashboard must let its
    /// warehouse sleep (the cache-hit path returns before `on_result`), and
    /// (b) metadata-only statements never count — see `runs_on_warehouse`.
    async fn on_result(
        &self,
        ctx: &QueryContext<'_>,
        result: &crate::plugin::ResultSnapshot,
    ) -> Result<()> {
        if result.served_from_cache || !runs_on_warehouse(&ctx.analysis.statement) {
            return Ok(());
        }
        if let Some(warehouse) = &ctx.session.warehouse {
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            self.model.record_arrival(warehouse, now_ms);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Feed `n` arrivals spaced `gap_secs` apart, return (model, last_ts).
    fn warm_model(gap_secs: u64, n: u64) -> (SuspendModel, u64) {
        let model = SuspendModel::default();
        let mut ts = 0;
        for _ in 0..n {
            model.record_arrival("WH", ts);
            ts += gap_secs * 1000;
        }
        (model, ts - gap_secs * 1000)
    }

    #[test]
    fn cold_start_stays_silent() {
        let (model, last) = warm_model(600, 5); // only 5 observations
        assert_eq!(model.recommend("WH", last + 300_000), None);
    }

    #[test]
    fn sparse_traffic_recommends_suspend() {
        // Queries every 10 minutes: P(arrival in 60s) ≈ 0.095 < 0.2,
        // expected idle ≈ 600 s >> 60 s resume penalty.
        let (model, last) = warm_model(600, 30);
        let rec = model
            .recommend("WH", last + 1_000)
            .expect("should recommend");
        assert!(rec.p_arrival_in_horizon < 0.2, "{rec:?}");
        assert!(rec.expected_idle_secs_saved > 0.0, "{rec:?}");
    }

    #[test]
    fn busy_warehouse_never_suspended() {
        // Queries every 5 s: P(arrival in 60 s) ≈ 1.
        let (model, last) = warm_model(5, 100);
        assert_eq!(model.recommend("WH", last + 1_000), None);
    }

    #[test]
    fn resume_penalty_blocks_marginal_savings() {
        // Gap of 70 s: sparse-ish, but savings (≤70 s) barely clear the 60 s
        // penalty once any idle has elapsed.
        let (model, last) = warm_model(70, 50);
        // After 30 s idle, remaining idle ≈ 40 s < 60 s penalty → no rec.
        assert_eq!(model.recommend("WH", last + 30_000), None);
    }

    #[test]
    fn unknown_warehouse_is_none() {
        let model = SuspendModel::default();
        assert_eq!(model.recommend("NOPE", 0), None);
    }

    use crate::plugin::{Plugin, QueryContext, ResultSnapshot, Session};
    use crate::sql::analyze;

    fn wh_session() -> Session {
        Session {
            warehouse: Some("WH".into()),
            ..Default::default()
        }
    }

    async fn feed(plugin: &SuspendPlugin, sql: &str, from_cache: bool) {
        let analysis = analyze(sql).unwrap();
        let session = wh_session();
        let ctx = QueryContext {
            analysis: &analysis,
            session: &session,
        };
        let snapshot = ResultSnapshot {
            served_from_cache: from_cache,
            ..Default::default()
        };
        plugin.on_result(&ctx, &snapshot).await.unwrap();
    }

    #[tokio::test]
    async fn metadata_statements_do_not_reset_idle() {
        // Regression: a monitoring loop polling SHOW WAREHOUSES through the
        // proxy must not keep the warehouse "busy" forever.
        let p = SuspendPlugin::new(SuspendConfig::default());
        feed(&p, "SELECT 1 FROM t", false).await;
        let before = p
            .model
            .stats
            .lock()
            .unwrap()
            .get("WH")
            .map(|e| e.observations);
        feed(&p, "SHOW WAREHOUSES", false).await;
        feed(&p, "USE WAREHOUSE WH", false).await;
        let after = p
            .model
            .stats
            .lock()
            .unwrap()
            .get("WH")
            .map(|e| e.observations);
        assert_eq!(
            before, after,
            "metadata statements must not record arrivals"
        );
    }

    #[tokio::test]
    async fn cache_hits_do_not_reset_idle() {
        // A fully-cached dashboard must let its warehouse sleep.
        let p = SuspendPlugin::new(SuspendConfig::default());
        feed(&p, "SELECT 1 FROM t", false).await;
        let before = p
            .model
            .stats
            .lock()
            .unwrap()
            .get("WH")
            .map(|e| e.observations);
        feed(&p, "SELECT 1 FROM t", true).await;
        let after = p
            .model
            .stats
            .lock()
            .unwrap()
            .get("WH")
            .map(|e| e.observations);
        assert_eq!(before, after, "cache hits must not record arrivals");
    }

    #[test]
    fn model_gates_come_from_config() {
        let p = SuspendPlugin::new(SuspendConfig {
            min_observations: 3,
            horizon_secs: 15.0,
            p_threshold: 0.5,
            ..Default::default()
        });
        assert_eq!(p.model.min_observations, 3);
        assert!((p.model.horizon_secs - 15.0).abs() < f64::EPSILON);
    }
}
