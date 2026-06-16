//! Prometheus metrics (PRD §15.1). One `Metrics` instance per daemon,
//! shared by the wire layer and background sweepers.

use prometheus_client::encoding::text::encode;
use prometheus_client::encoding::EncodeLabelSet;
use prometheus_client::metrics::counter::Counter;
use prometheus_client::metrics::family::Family;
use prometheus_client::metrics::histogram::{exponential_buckets, Histogram};
use prometheus_client::registry::Registry;

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
pub struct RouteLabels {
    /// passthrough | cache_hit | coalesced | rewritten | veto
    pub route: String,
}

pub struct Metrics {
    registry: Registry,
    pub queries_total: Family<RouteLabels, Counter>,
    pub cache_hits_total: Counter,
    pub cache_misses_total: Counter,
    /// Critical SLO — must stay 0 (PRD §15.1).
    pub cache_blame_mismatches_total: Counter,
    pub coalesced_total: Counter,
    pub rewrites_total: Counter,
    pub upstream_errors_total: Counter,
    pub circuit_breaker_fast_fails_total: Counter,
    pub suspend_recommendations_total: Counter,
    pub suspends_executed_total: Counter,
    pub saved_usd_total: Counter<f64>,
    /// Hot-path budget: must stay <5 ms p99 (PRD §17).
    pub proxy_overhead_seconds: Histogram,
}

impl Metrics {
    pub fn new() -> Self {
        let mut registry = Registry::with_prefix("chukei");
        let queries_total = Family::<RouteLabels, Counter>::default();
        registry.register(
            "queries",
            "Queries by decision route",
            queries_total.clone(),
        );

        let cache_hits_total = Counter::default();
        registry.register("cache_hits", "Cache hits", cache_hits_total.clone());
        let cache_misses_total = Counter::default();
        registry.register(
            "cache_misses",
            "Cache misses (eligible queries)",
            cache_misses_total.clone(),
        );
        let cache_blame_mismatches_total = Counter::default();
        registry.register(
            "cache_blame_mismatches",
            "Blame-mode mismatches between cache and upstream (SLO: 0)",
            cache_blame_mismatches_total.clone(),
        );
        let coalesced_total = Counter::default();
        registry.register(
            "coalesced",
            "Queries served by in-flight coalescing",
            coalesced_total.clone(),
        );
        let rewrites_total = Counter::default();
        registry.register(
            "rewrites",
            "Queries rewritten before submission",
            rewrites_total.clone(),
        );
        let upstream_errors_total = Counter::default();
        registry.register(
            "upstream_errors",
            "Upstream request failures",
            upstream_errors_total.clone(),
        );
        let circuit_breaker_fast_fails_total = Counter::default();
        registry.register(
            "circuit_breaker_fast_fails",
            "Requests fast-failed while the upstream breaker was open",
            circuit_breaker_fast_fails_total.clone(),
        );
        let suspend_recommendations_total = Counter::default();
        registry.register(
            "suspend_recommendations",
            "Predictive suspend recommendations emitted",
            suspend_recommendations_total.clone(),
        );
        let suspends_executed_total = Counter::default();
        registry.register(
            "suspends_executed",
            "ALTER WAREHOUSE SUSPEND statements executed (enforce mode)",
            suspends_executed_total.clone(),
        );
        let saved_usd_total = Counter::<f64>::default();
        registry.register(
            "saved_usd",
            "Estimated realized savings in USD",
            saved_usd_total.clone(),
        );

        let proxy_overhead_seconds = Histogram::new(exponential_buckets(0.0001, 2.0, 12));
        registry.register(
            "proxy_overhead_seconds",
            "Hot-path overhead added by chukei before forwarding",
            proxy_overhead_seconds.clone(),
        );

        Self {
            registry,
            queries_total,
            cache_hits_total,
            cache_misses_total,
            cache_blame_mismatches_total,
            coalesced_total,
            rewrites_total,
            upstream_errors_total,
            circuit_breaker_fast_fails_total,
            suspend_recommendations_total,
            suspends_executed_total,
            saved_usd_total,
            proxy_overhead_seconds,
        }
    }

    pub fn route(&self, route: &str) {
        self.queries_total
            .get_or_create(&RouteLabels {
                route: route.to_string(),
            })
            .inc();
    }

    /// OpenMetrics text exposition.
    pub fn render(&self) -> String {
        let mut out = String::new();
        let _ = encode(&mut out, &self.registry);
        out
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_registered_metrics() {
        let m = Metrics::new();
        m.route("cache_hit");
        m.cache_hits_total.inc();
        m.saved_usd_total.inc_by(1.25);
        m.proxy_overhead_seconds.observe(0.002);
        let text = m.render();
        assert!(
            text.contains("chukei_queries_total{route=\"cache_hit\"} 1"),
            "{text}"
        );
        assert!(text.contains("chukei_cache_hits_total 1"), "{text}");
        assert!(text.contains("chukei_saved_usd_total 1.25"), "{text}");
        assert!(text.contains("chukei_proxy_overhead_seconds"), "{text}");
    }
}
