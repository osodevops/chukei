//! Disk-backed cache store: one JSON file per entry under a directory,
//! with an in-memory index for table lookups and eviction. Survives
//! restarts. The Iceberg/object-store backend replaces this for shared,
//! multi-instance deployments; the trait boundary is identical.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use super::{CacheEntry, CacheStore};
use crate::error::{Error, Result};
use crate::sql::fingerprint::hex;

struct IndexEntry {
    tables: Vec<String>,
    inserted_unix_ms: u64,
}

pub struct DiskStore {
    dir: PathBuf,
    max_entries: usize,
    index: RwLock<HashMap<[u8; 32], IndexEntry>>,
}

impl DiskStore {
    pub fn open(dir: impl AsRef<Path>, max_entries: usize) -> Result<Self> {
        let dir = dir.as_ref().to_path_buf();
        std::fs::create_dir_all(&dir).map_err(|e| {
            Error::Storage(format!("cannot create cache dir {}: {e}", dir.display()))
        })?;

        // Rebuild the index from whatever survived the restart.
        let mut index = HashMap::new();
        for file in std::fs::read_dir(&dir).map_err(|e| Error::Storage(e.to_string()))? {
            let Ok(file) = file else { continue };
            let path = file.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let Some(key) = path
                .file_stem()
                .and_then(|s| s.to_str())
                .and_then(parse_key)
            else {
                continue;
            };
            let Ok(raw) = std::fs::read(&path) else {
                continue;
            };
            let Ok(entry) = serde_json::from_slice::<CacheEntry>(&raw) else {
                let _ = std::fs::remove_file(&path); // corrupt → drop, it's a cache
                continue;
            };
            index.insert(
                key,
                IndexEntry {
                    tables: entry.tables,
                    inserted_unix_ms: entry.inserted_unix_ms,
                },
            );
        }
        tracing::info!(entries = index.len(), dir = %dir.display(), "persistent cache loaded");
        Ok(Self {
            dir,
            max_entries: max_entries.max(1),
            index: RwLock::new(index),
        })
    }

    fn path_for(&self, key: &[u8; 32]) -> PathBuf {
        self.dir.join(format!("{}.json", hex(key)))
    }

    fn delete(&self, key: &[u8; 32]) {
        let _ = std::fs::remove_file(self.path_for(key));
    }
}

fn parse_key(stem: &str) -> Option<[u8; 32]> {
    if stem.len() != 64 {
        return None;
    }
    let mut key = [0u8; 32];
    for (i, byte) in key.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&stem[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(key)
}

impl CacheStore for DiskStore {
    fn get(&self, key: &[u8; 32], now_ms: u64) -> Option<CacheEntry> {
        if !self.index.read().unwrap().contains_key(key) {
            return None;
        }
        let raw = std::fs::read(self.path_for(key)).ok()?;
        let entry: CacheEntry = serde_json::from_slice(&raw).ok()?;
        if entry.expired(now_ms) {
            self.index.write().unwrap().remove(key);
            self.delete(key);
            return None;
        }
        Some(entry)
    }

    fn put(&self, key: [u8; 32], entry: CacheEntry) {
        let Ok(raw) = serde_json::to_vec(&entry) else {
            return;
        };
        // Write-then-rename so a crash never leaves a half-written entry.
        let tmp = self.dir.join(format!("{}.tmp", hex(&key)));
        if std::fs::write(&tmp, &raw).is_err() {
            return;
        }
        if std::fs::rename(&tmp, self.path_for(&key)).is_err() {
            let _ = std::fs::remove_file(&tmp);
            return;
        }

        let mut index = self.index.write().unwrap();
        index.insert(
            key,
            IndexEntry {
                tables: entry.tables.clone(),
                inserted_unix_ms: entry.inserted_unix_ms,
            },
        );
        if index.len() > self.max_entries {
            let mut by_age: Vec<([u8; 32], u64)> = index
                .iter()
                .map(|(k, e)| (*k, e.inserted_unix_ms))
                .collect();
            by_age.sort_by_key(|(_, at)| *at);
            let target = self.max_entries.saturating_sub(self.max_entries / 10);
            let excess = index.len().saturating_sub(target);
            for (old_key, _) in by_age.into_iter().take(excess) {
                index.remove(&old_key);
                self.delete(&old_key);
            }
        }
    }

    fn remove(&self, key: &[u8; 32]) {
        self.index.write().unwrap().remove(key);
        self.delete(key);
    }

    fn invalidate_table(&self, table: &str) -> usize {
        let table = table.to_uppercase();
        let mut index = self.index.write().unwrap();
        let doomed: Vec<[u8; 32]> = index
            .iter()
            .filter(|(_, e)| e.tables.iter().any(|t| t == &table))
            .map(|(k, _)| *k)
            .collect();
        for key in &doomed {
            index.remove(key);
            self.delete(key);
        }
        doomed.len()
    }

    fn len(&self) -> usize {
        self.index.read().unwrap().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn entry(table: &str, inserted: u64) -> CacheEntry {
        CacheEntry {
            data: serde_json::json!({"rowset": [[1]]}),
            tables: vec![table.to_uppercase()],
            inserted_unix_ms: inserted,
            ttl: Duration::from_secs(3600),
            canonical_wall_clock_ms: 1000,
        }
    }

    #[test]
    fn entries_survive_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let key = [7u8; 32];
        {
            let store = DiskStore::open(dir.path(), 100).unwrap();
            store.put(key, entry("T1", 1_000));
        }
        let reopened = DiskStore::open(dir.path(), 100).unwrap();
        assert_eq!(reopened.len(), 1);
        let got = reopened
            .get(&key, 2_000)
            .expect("entry must survive restart");
        assert_eq!(got.tables, vec!["T1"]);
        assert_eq!(got.canonical_wall_clock_ms, 1000);
    }

    #[test]
    fn expired_entries_are_dropped_on_read() {
        let dir = tempfile::tempdir().unwrap();
        let store = DiskStore::open(dir.path(), 100).unwrap();
        let key = [1u8; 32];
        let mut e = entry("T1", 0);
        e.ttl = Duration::from_secs(1);
        store.put(key, e);
        assert!(store.get(&key, 10_000).is_none());
        assert_eq!(store.len(), 0, "expired entry must be tombstoned");
    }

    #[test]
    fn eviction_keeps_newest() {
        let dir = tempfile::tempdir().unwrap();
        let store = DiskStore::open(dir.path(), 10).unwrap();
        for i in 0..20u8 {
            let mut key = [0u8; 32];
            key[0] = i;
            store.put(key, entry("T", i as u64));
        }
        assert!(store.len() <= 10, "cap enforced, len={}", store.len());
        // The newest entry must still be there.
        let mut newest = [0u8; 32];
        newest[0] = 19;
        assert!(store.get(&newest, 100).is_some());
    }

    #[test]
    fn invalidate_table_removes_files() {
        let dir = tempfile::tempdir().unwrap();
        let store = DiskStore::open(dir.path(), 100).unwrap();
        store.put([1u8; 32], entry("A", 1));
        store.put([2u8; 32], entry("B", 2));
        assert_eq!(store.invalidate_table("a"), 1);
        assert_eq!(store.len(), 1);
        let reopened = DiskStore::open(dir.path(), 100).unwrap();
        assert_eq!(reopened.len(), 1, "invalidation must persist");
    }
}
