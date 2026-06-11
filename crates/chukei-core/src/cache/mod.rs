//! ① `plug-cache` — semantic result cache (PRD §11.1).
//!
//! P0 scope here: hard-fingerprint exact-match cache with per-table-family
//! TTLs, a strict determinism gate, and lineage-aware invalidation by table.
//! The store is a trait so the in-memory implementation can be swapped for
//! the Iceberg/Arrow-IPC backend without touching the plugin.
//!
//! Correctness creed (PRD R-3): **false-negative friendly, false-positive
//! intolerant.** When in doubt, miss.

use std::collections::HashMap;
use std::sync::RwLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;

use crate::config::CacheConfig;
use crate::plugin::{CacheKey, Decision, Plugin, QueryContext, ResultSnapshot};
use crate::sql::parse::is_read_only;
use crate::Result;

pub mod disk;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CacheEntry {
    pub data: serde_json::Value,
    pub tables: Vec<String>,
    pub inserted_unix_ms: u64,
    pub ttl: Duration,
    /// Wall clock of the canonical upstream execution — every hit "avoids"
    /// this much warehouse time, which is what the savings ledger prices.
    pub canonical_wall_clock_ms: u64,
}

impl CacheEntry {
    fn expired(&self, now_ms: u64) -> bool {
        now_ms.saturating_sub(self.inserted_unix_ms) > self.ttl.as_millis() as u64
    }
}

/// Storage behind the cache. In-memory now; Iceberg/Arrow-IPC later.
pub trait CacheStore: Send + Sync {
    fn get(&self, key: &[u8; 32], now_ms: u64) -> Option<CacheEntry>;
    fn put(&self, key: [u8; 32], entry: CacheEntry);
    /// Tombstone one entry (blame-mode mismatch eviction).
    fn remove(&self, key: &[u8; 32]);
    /// Tombstone every entry that read `table` (lineage-aware invalidation).
    fn invalidate_table(&self, table: &str) -> usize;
    fn len(&self) -> usize;
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

pub struct MemoryStore {
    entries: RwLock<HashMap<[u8; 32], CacheEntry>>,
    max_entries: usize,
}

impl Default for MemoryStore {
    fn default() -> Self {
        Self::with_capacity(10_000)
    }
}

impl MemoryStore {
    pub fn with_capacity(max_entries: usize) -> Self {
        Self {
            entries: RwLock::new(HashMap::new()),
            max_entries: max_entries.max(1),
        }
    }
}

/// Evict the oldest ~10 % of entries when over capacity. O(n log n) on the
/// eviction path only; puts stay O(1) otherwise.
fn evict_oldest(entries: &mut HashMap<[u8; 32], CacheEntry>, max_entries: usize) {
    if entries.len() <= max_entries {
        return;
    }
    let mut by_age: Vec<([u8; 32], u64)> = entries
        .iter()
        .map(|(k, e)| (*k, e.inserted_unix_ms))
        .collect();
    by_age.sort_by_key(|(_, at)| *at);
    let target = max_entries.saturating_sub(max_entries / 10);
    for (key, _) in by_age.iter().take(entries.len().saturating_sub(target)) {
        entries.remove(key);
    }
}

impl CacheStore for MemoryStore {
    fn get(&self, key: &[u8; 32], now_ms: u64) -> Option<CacheEntry> {
        let entries = self.entries.read().unwrap();
        entries.get(key).filter(|e| !e.expired(now_ms)).cloned()
    }

    fn put(&self, key: [u8; 32], entry: CacheEntry) {
        let mut entries = self.entries.write().unwrap();
        entries.insert(key, entry);
        evict_oldest(&mut entries, self.max_entries);
    }

    fn remove(&self, key: &[u8; 32]) {
        self.entries.write().unwrap().remove(key);
    }

    fn invalidate_table(&self, table: &str) -> usize {
        let table = table.to_uppercase();
        let mut entries = self.entries.write().unwrap();
        let before = entries.len();
        entries.retain(|_, e| !e.tables.iter().any(|t| t == &table));
        before - entries.len()
    }

    fn len(&self) -> usize {
        self.entries.read().unwrap().len()
    }
}

pub struct CachePlugin {
    config: CacheConfig,
    store: Box<dyn CacheStore>,
}

impl CachePlugin {
    pub fn new(config: CacheConfig) -> Self {
        let store: Box<dyn CacheStore> = match &config.persist_path {
            Some(path) => match disk::DiskStore::open(path, config.max_entries) {
                Ok(store) => Box::new(store),
                Err(e) => {
                    tracing::error!(error = %e, path,
                        "cannot open persistent cache; falling back to in-memory");
                    Box::new(MemoryStore::with_capacity(config.max_entries))
                }
            },
            None => Box::new(MemoryStore::with_capacity(config.max_entries)),
        };
        Self::with_store(config, store)
    }

    pub fn with_store(config: CacheConfig, store: Box<dyn CacheStore>) -> Self {
        Self { config, store }
    }

    /// TTL for a query, taking the *minimum* across the table families it
    /// touches — the most volatile table bounds the answer's freshness.
    fn ttl_for(&self, tables: &[String]) -> Duration {
        let mut ttl = self.config.default_ttl_secs;
        for table in tables {
            for (pattern, secs) in &self.config.table_ttls {
                if glob_match(pattern, table) {
                    ttl = ttl.min(*secs);
                }
            }
        }
        Duration::from_secs(ttl)
    }

    fn cacheable(&self, ctx: &QueryContext<'_>) -> bool {
        is_read_only(&ctx.analysis.statement)
            && ctx.analysis.features.deterministic
            && !ctx.analysis.features.tables.is_empty()
    }

    pub fn lookup(&self, key: &[u8; 32]) -> Option<CacheEntry> {
        self.store.get(key, now_ms())
    }

    /// Evict one entry (blame-mode mismatch: the entry is wrong, kill it).
    pub fn remove(&self, key: &[u8; 32]) {
        self.store.remove(key);
    }

    /// Sampling rate for blame-mode double-checks of cache hits.
    pub fn blame_sample_rate(&self) -> f64 {
        self.config.blame_sample_rate
    }

    /// Called by the lineage watcher when a write to `table` is observed.
    pub fn invalidate_table(&self, table: &str) -> usize {
        self.store.invalidate_table(table)
    }

    pub fn store_len(&self) -> usize {
        self.store.len()
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// `ANALYTICS.MARTS.*` style matching: a trailing `*` matches any suffix;
/// otherwise exact (case-insensitive) match.
fn glob_match(pattern: &str, value: &str) -> bool {
    let pattern = pattern.to_uppercase();
    let value = value.to_uppercase();
    match pattern.strip_suffix('*') {
        Some(prefix) => value.starts_with(prefix),
        None => value == pattern,
    }
}

#[async_trait]
impl Plugin for CachePlugin {
    fn name(&self) -> &'static str {
        "cache"
    }
    fn order(&self) -> i32 {
        10
    }

    async fn decide(&self, ctx: &QueryContext<'_>) -> Result<Decision> {
        if !self.cacheable(ctx) {
            return Ok(Decision::Passthrough);
        }
        let key = ctx.fingerprint();
        if self.store.get(&key, now_ms()).is_some() {
            Ok(Decision::ServeFromCache(CacheKey {
                hard_fingerprint: key,
            }))
        } else {
            Ok(Decision::Passthrough)
        }
    }

    async fn on_result(&self, ctx: &QueryContext<'_>, result: &ResultSnapshot) -> Result<()> {
        if result.served_from_cache || !self.cacheable(ctx) {
            return Ok(());
        }
        let Some(data) = &result.data else {
            return Ok(());
        };
        let tables = ctx.analysis.features.tables.clone();
        let ttl = self.ttl_for(&tables);
        self.store.put(
            ctx.fingerprint(),
            CacheEntry {
                data: data.clone(),
                tables,
                inserted_unix_ms: now_ms(),
                ttl,
                canonical_wall_clock_ms: result.wall_clock_ms,
            },
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::Session;
    use crate::sql::analyze;

    fn plugin(table_ttls: &[(&str, u64)]) -> CachePlugin {
        CachePlugin::new(CacheConfig {
            enabled: true,
            default_ttl_secs: 900,
            table_ttls: table_ttls
                .iter()
                .map(|(k, v)| (k.to_string(), *v))
                .collect(),
            ..Default::default()
        })
    }

    async fn decide_and_fill(plugin: &CachePlugin, sql: &str) -> Decision {
        let analysis = analyze(sql).unwrap();
        let session = Session::default();
        let ctx = QueryContext {
            analysis: &analysis,
            session: &session,
        };
        let first = plugin.decide(&ctx).await.unwrap();
        let snapshot = ResultSnapshot {
            data: Some(serde_json::json!([{"n": 1}])),
            row_count: 1,
            ..Default::default()
        };
        plugin.on_result(&ctx, &snapshot).await.unwrap();
        first
    }

    #[tokio::test]
    async fn miss_then_hit() {
        let p = plugin(&[]);
        let sql = "SELECT a FROM analytics.marts.revenue";
        assert_eq!(decide_and_fill(&p, sql).await, Decision::Passthrough);
        // Second pass: same fingerprint → hit.
        let analysis = analyze(sql).unwrap();
        let session = Session::default();
        let ctx = QueryContext {
            analysis: &analysis,
            session: &session,
        };
        let second = p.decide(&ctx).await.unwrap();
        assert!(matches!(second, Decision::ServeFromCache(_)), "{second:?}");
    }

    #[tokio::test]
    async fn non_deterministic_never_cached() {
        let p = plugin(&[]);
        let sql = "SELECT a, CURRENT_TIMESTAMP() FROM t";
        decide_and_fill(&p, sql).await;
        assert_eq!(
            p.store_len(),
            0,
            "non-deterministic query must not be stored"
        );
    }

    #[tokio::test]
    async fn writes_never_cached() {
        let p = plugin(&[]);
        decide_and_fill(&p, "INSERT INTO t VALUES (1)").await;
        assert_eq!(p.store_len(), 0);
    }

    #[tokio::test]
    async fn lineage_invalidation_tombstones_by_table() {
        let p = plugin(&[]);
        decide_and_fill(&p, "SELECT a FROM analytics.raw.events").await;
        decide_and_fill(&p, "SELECT b FROM analytics.marts.revenue").await;
        assert_eq!(p.store_len(), 2);
        let removed = p.invalidate_table("ANALYTICS.RAW.EVENTS");
        assert_eq!(removed, 1);
        assert_eq!(p.store_len(), 1);
    }

    #[test]
    fn ttl_uses_minimum_across_families() {
        let p = plugin(&[("ANALYTICS.MARTS.*", 86_400), ("ANALYTICS.RAW.*", 60)]);
        assert_eq!(
            p.ttl_for(&["ANALYTICS.MARTS.REVENUE".into()]),
            Duration::from_secs(900)
        );
        // default (900) is still the floor vs MARTS (86400) — min wins…
        // …and RAW (60) undercuts both when present.
        assert_eq!(
            p.ttl_for(&[
                "ANALYTICS.MARTS.REVENUE".into(),
                "ANALYTICS.RAW.EVENTS".into()
            ]),
            Duration::from_secs(60)
        );
    }

    #[test]
    fn expired_entries_miss() {
        let store = MemoryStore::default();
        store.put(
            [0u8; 32],
            CacheEntry {
                data: serde_json::json!(1),
                tables: vec![],
                inserted_unix_ms: 0,
                ttl: Duration::from_secs(10),
                canonical_wall_clock_ms: 0,
            },
        );
        assert!(store.get(&[0u8; 32], 5_000).is_some());
        assert!(store.get(&[0u8; 32], 11_000).is_none());
    }
}
