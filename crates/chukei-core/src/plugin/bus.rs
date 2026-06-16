//! Ordered plugin execution and deterministic decision merging (PRD §10.2).
//!
//! Conflict precedence (highest first):
//! 1. `Veto` — short-circuits everything (admin policy).
//! 2. `ServeFromCache` — short-circuits the response.
//! 3. `Route` — chooses the target engine.
//! 4. `Rewrite` — applied before submission to the chosen engine.
//! 5. `SetWarehouseSize` — applied iff route = Snowflake.
//! 6. `Annotate` — additive.
//!
//! Within one kind, the earliest plugin (lowest `order()`) wins; annotations
//! merge with first-writer-wins per key. Plugin failures are fail-open: the
//! proxy must never break traffic because an optimisation plugin errored.

use std::sync::Arc;

use super::{Decision, Plugin, QueryContext, ResultSnapshot, VetoReason};
use crate::plugin::{CacheKey, Engine, QueryTags, WarehouseSize};

/// The merged outcome of one bus pass.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Resolution {
    pub veto: Option<VetoReason>,
    pub serve_from_cache: Option<CacheKey>,
    pub route: Option<Engine>,
    pub rewrite: Option<String>,
    pub warehouse_size: Option<WarehouseSize>,
    pub annotations: QueryTags,
    /// (plugin name, decision) in execution order — for spans and replay.
    pub trail: Vec<(String, Decision)>,
}

impl Resolution {
    pub fn is_passthrough(&self) -> bool {
        self.veto.is_none()
            && self.serve_from_cache.is_none()
            && self.route.is_none()
            && self.rewrite.is_none()
            && self.warehouse_size.is_none()
    }
}

pub struct PluginBus {
    plugins: Vec<Arc<dyn Plugin>>,
}

impl PluginBus {
    pub fn new(mut plugins: Vec<Arc<dyn Plugin>>) -> Self {
        plugins.sort_by_key(|p| p.order());
        Self { plugins }
    }

    pub fn plugins(&self) -> impl Iterator<Item = &Arc<dyn Plugin>> {
        self.plugins.iter()
    }

    /// Run every plugin and merge their decisions.
    pub async fn decide(&self, ctx: &QueryContext<'_>) -> Resolution {
        let mut resolution = Resolution::default();
        for plugin in &self.plugins {
            match plugin.decide(ctx).await {
                Ok(decision) => {
                    resolution
                        .trail
                        .push((plugin.name().to_string(), decision.clone()));
                    merge(&mut resolution, decision);
                    if resolution.veto.is_some() {
                        break; // admin policy short-circuits the whole chain
                    }
                }
                Err(e) => {
                    tracing::warn!(plugin = plugin.name(), error = %e, "plugin failed; fail-open");
                }
            }
        }
        resolution
    }

    /// Fan the result out to every plugin's `on_result`.
    pub async fn on_result(&self, ctx: &QueryContext<'_>, result: &ResultSnapshot) {
        for plugin in &self.plugins {
            if let Err(e) = plugin.on_result(ctx, result).await {
                tracing::warn!(plugin = plugin.name(), error = %e, "on_result failed");
            }
        }
    }
}

fn merge(resolution: &mut Resolution, decision: Decision) {
    match decision {
        Decision::Passthrough => {}
        Decision::Veto(reason) => {
            resolution.veto.get_or_insert(reason);
        }
        Decision::ServeFromCache(key) => {
            resolution.serve_from_cache.get_or_insert(key);
        }
        Decision::Route(engine) => {
            resolution.route.get_or_insert(engine);
        }
        Decision::Rewrite(sql) => {
            resolution.rewrite.get_or_insert(sql);
        }
        Decision::SetWarehouseSize(size) => {
            resolution.warehouse_size.get_or_insert(size);
        }
        Decision::Annotate(tags) => {
            for (k, v) in tags {
                resolution.annotations.entry(k).or_insert(v);
            }
        }
        Decision::Compose(decisions) => {
            for d in decisions {
                merge(resolution, d);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::Session;
    use crate::sql::analyze;
    use crate::Result;
    use async_trait::async_trait;

    struct Fixed {
        name: &'static str,
        order: i32,
        decision: Decision,
    }

    #[async_trait]
    impl Plugin for Fixed {
        fn name(&self) -> &'static str {
            self.name
        }
        fn order(&self) -> i32 {
            self.order
        }
        async fn decide(&self, _ctx: &QueryContext<'_>) -> Result<Decision> {
            Ok(self.decision.clone())
        }
    }

    struct Failing;

    #[async_trait]
    impl Plugin for Failing {
        fn name(&self) -> &'static str {
            "failing"
        }
        fn order(&self) -> i32 {
            0
        }
        async fn decide(&self, _ctx: &QueryContext<'_>) -> Result<Decision> {
            Err(crate::Error::Plugin {
                plugin: "failing".into(),
                message: "boom".into(),
            })
        }
    }

    async fn run(plugins: Vec<Arc<dyn Plugin>>) -> Resolution {
        let analysis = analyze("SELECT a FROM t").unwrap();
        let session = Session::default();
        let ctx = QueryContext {
            analysis: &analysis,
            session: &session,
        };
        PluginBus::new(plugins).decide(&ctx).await
    }

    fn tags(pairs: &[(&str, &str)]) -> QueryTags {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[tokio::test]
    async fn veto_short_circuits() {
        let r = run(vec![
            Arc::new(Fixed {
                name: "policy",
                order: 1,
                decision: Decision::Veto(VetoReason("blocked".into())),
            }),
            Arc::new(Fixed {
                name: "rewriter",
                order: 2,
                decision: Decision::Rewrite("SELECT 1".into()),
            }),
        ])
        .await;
        assert!(r.veto.is_some());
        assert!(r.rewrite.is_none(), "later plugins must not run after veto");
        assert_eq!(r.trail.len(), 1);
    }

    #[tokio::test]
    async fn compose_flattens_and_merges() {
        let r = run(vec![Arc::new(Fixed {
            name: "multi",
            order: 1,
            decision: Decision::Compose(vec![
                Decision::Rewrite("SELECT b FROM t".into()),
                Decision::Annotate(tags(&[("team", "growth")])),
            ]),
        })])
        .await;
        assert_eq!(r.rewrite.as_deref(), Some("SELECT b FROM t"));
        assert_eq!(r.annotations["team"], "growth");
    }

    #[tokio::test]
    async fn earliest_plugin_wins_per_kind_and_annotations_merge() {
        let r = run(vec![
            Arc::new(Fixed {
                name: "a",
                order: 1,
                decision: Decision::Route(Engine::DuckDb),
            }),
            Arc::new(Fixed {
                name: "b",
                order: 2,
                decision: Decision::Route(Engine::Snowflake),
            }),
            Arc::new(Fixed {
                name: "c",
                order: 3,
                decision: Decision::Annotate(tags(&[("team", "x")])),
            }),
            Arc::new(Fixed {
                name: "d",
                order: 4,
                decision: Decision::Annotate(tags(&[("team", "y"), ("dag", "z")])),
            }),
        ])
        .await;
        assert_eq!(r.route, Some(Engine::DuckDb));
        assert_eq!(r.annotations["team"], "x", "first writer wins");
        assert_eq!(r.annotations["dag"], "z");
    }

    #[tokio::test]
    async fn failing_plugin_is_fail_open() {
        let r = run(vec![
            Arc::new(Failing),
            Arc::new(Fixed {
                name: "ok",
                order: 1,
                decision: Decision::Rewrite("SELECT 1".into()),
            }),
        ])
        .await;
        assert_eq!(r.rewrite.as_deref(), Some("SELECT 1"));
    }

    #[tokio::test]
    async fn plugins_run_in_order_regardless_of_registration() {
        let r = run(vec![
            Arc::new(Fixed {
                name: "late",
                order: 50,
                decision: Decision::Rewrite("late".into()),
            }),
            Arc::new(Fixed {
                name: "early",
                order: 10,
                decision: Decision::Rewrite("early".into()),
            }),
        ])
        .await;
        assert_eq!(r.rewrite.as_deref(), Some("early"));
        assert_eq!(r.trail[0].0, "early");
    }

    #[tokio::test]
    async fn passthrough_resolution() {
        let r = run(vec![Arc::new(Fixed {
            name: "noop",
            order: 1,
            decision: Decision::Passthrough,
        })])
        .await;
        assert!(r.is_passthrough());
    }
}
