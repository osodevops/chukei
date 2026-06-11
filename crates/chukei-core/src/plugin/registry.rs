//! Built-in plugin registry: config → live bus, plus the metadata behind
//! `chukei plugins list/describe` (PRD §13).

use std::sync::Arc;

use crate::config::Config;
use crate::plugin::{bus::PluginBus, Plugin};

/// Static descriptor for `chukei plugins list/describe`.
#[derive(Debug, Clone)]
pub struct PluginInfo {
    pub name: &'static str,
    pub order: i32,
    pub summary: &'static str,
    pub status: &'static str,
}

pub fn catalog() -> Vec<PluginInfo> {
    vec![
        PluginInfo {
            name: "cache",
            order: 10,
            summary: "Semantic result cache (TTL + determinism gate; Iceberg backing planned)",
            status: "p0",
        },
        PluginInfo {
            name: "router",
            order: 20,
            summary: "Rules-based engine routing to embedded DuckDB on Iceberg replicas",
            status: "p0",
        },
        PluginInfo {
            name: "rewrite",
            order: 30,
            summary: "Deterministic SQL rewrite rule pack",
            status: "p0",
        },
        PluginInfo {
            name: "bandit",
            order: 40,
            summary: "Contextual bandit warehouse sizing",
            status: "p1",
        },
        PluginInfo {
            name: "suspend",
            order: 50,
            summary: "Predictive warehouse auto-suspend (suggest-only by default)",
            status: "p0",
        },
        PluginInfo {
            name: "attribute",
            order: 60,
            summary: "Per-query cost attribution via hints, dbt meta, APPLICATION_NAME",
            status: "p0",
        },
    ]
}

/// The live bus plus direct handles to plugins the wire layer must talk to
/// outside the `Plugin` trait (the cache serves payloads, not decisions).
pub struct BusBundle {
    pub bus: PluginBus,
    pub cache: Option<Arc<crate::cache::CachePlugin>>,
    pub suspend: Option<Arc<crate::suspend::SuspendPlugin>>,
}

/// Build the live bus from configuration. Disabled plugins simply don't
/// appear on the bus — there is no per-query enabled check on the hot path.
pub fn build_bus(config: &Config) -> BusBundle {
    let mut plugins: Vec<Arc<dyn Plugin>> = Vec::new();
    let mut cache_handle = None;
    let mut suspend_handle = None;
    if config.plugins.cache.enabled {
        let cache = Arc::new(crate::cache::CachePlugin::new(config.plugins.cache.clone()));
        cache_handle = Some(cache.clone());
        plugins.push(cache);
    }
    if config.plugins.router.enabled {
        plugins.push(Arc::new(crate::router::RouterPlugin::new(
            config.plugins.router.clone(),
        )));
    }
    if config.plugins.rewrite.enabled {
        plugins.push(Arc::new(crate::rewrite::RewritePlugin::new(
            config.plugins.rewrite.clone(),
        )));
    }
    if config.plugins.suspend.enabled {
        let suspend = Arc::new(crate::suspend::SuspendPlugin::new(
            config.plugins.suspend.clone(),
        ));
        suspend_handle = Some(suspend.clone());
        plugins.push(suspend);
    }
    if config.plugins.attribute.enabled {
        plugins.push(Arc::new(crate::attribute::AttributePlugin::new(
            config.plugins.attribute.clone(),
        )));
    }
    BusBundle {
        bus: PluginBus::new(plugins),
        cache: cache_handle,
        suspend: suspend_handle,
    }
}
