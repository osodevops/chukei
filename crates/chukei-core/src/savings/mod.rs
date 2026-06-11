//! Realized-savings ledger.
//!
//! The replay simulator *projects*; this ledger records what the running
//! proxy actually avoided, with a methodology designed to survive FinOps
//! scrutiny:
//!
//! - Every avoided execution (cache hit, coalesced follower, retry replay,
//!   executed suspend) is priced at its **canonical run's measured wall
//!   clock** × the warehouse's credit rate × the configured $/credit.
//! - A **conservative factor** (default 0.7) discounts hits that occurred
//!   while the warehouse was likely running anyway — chukei cannot observe
//!   warehouse state from the proxy alone, so it under-claims by default.
//! - The report carries the methodology string and is exportable as a
//!   signed evidence bundle; reconcile against `WAREHOUSE_METERING_HISTORY`.

use std::collections::BTreeMap;
use std::path::Path;
use std::str::FromStr;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::config::SavingsConfig;
use crate::error::{Error, Result};
use crate::plugin::WarehouseSize;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavingsEvent {
    pub at_unix_ms: u64,
    /// cache_hit | coalesced | retry_replay | suspend
    pub kind: String,
    pub fingerprint_hex: Option<String>,
    pub team: Option<String>,
    pub warehouse: Option<String>,
    pub avoided_credits: f64,
    pub avoided_usd: f64,
    pub canonical_elapsed_ms: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct KindAggregate {
    pub events: u64,
    pub avoided_usd: f64,
    pub avoided_credits: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavingsReport {
    pub since_unix_ms: u64,
    pub until_unix_ms: u64,
    pub total_events: u64,
    pub total_avoided_usd: f64,
    pub total_avoided_credits: f64,
    pub by_kind: BTreeMap<String, KindAggregate>,
    pub by_team: BTreeMap<String, f64>,
    pub methodology: String,
}

/// Prices avoided executions. Warehouse sizes are configured (the proxy
/// can't see them); unknown warehouses use the default size.
#[derive(Debug, Clone)]
pub struct Pricing {
    pub usd_per_credit: f64,
    pub conservative_factor: f64,
    pub default_size: WarehouseSize,
    pub warehouse_sizes: BTreeMap<String, WarehouseSize>,
}

impl Pricing {
    pub fn from_config(config: &SavingsConfig) -> Self {
        let warehouse_sizes = config
            .warehouse_sizes
            .iter()
            .filter_map(|(wh, size)| {
                WarehouseSize::from_str(size)
                    .ok()
                    .map(|s| (wh.to_uppercase(), s))
            })
            .collect();
        Self {
            usd_per_credit: config.usd_per_credit,
            conservative_factor: config.conservative_factor,
            default_size: WarehouseSize::from_str(&config.default_warehouse_size)
                .unwrap_or(WarehouseSize::M),
            warehouse_sizes,
        }
    }

    fn size_for(&self, warehouse: Option<&str>) -> WarehouseSize {
        warehouse
            .and_then(|wh| self.warehouse_sizes.get(&wh.to_uppercase()).copied())
            .unwrap_or(self.default_size)
    }

    /// (credits, usd) avoided by not running `elapsed_ms` on `warehouse`.
    pub fn avoided(&self, warehouse: Option<&str>, elapsed_ms: u64) -> (f64, f64) {
        let credits = self.size_for(warehouse).credits_per_hour() * elapsed_ms as f64 / 3_600_000.0
            * self.conservative_factor;
        (credits, credits * self.usd_per_credit)
    }

    pub fn methodology(&self) -> String {
        format!(
            "avoided_usd = canonical_wall_clock × warehouse_credit_rate × \
             ${}/credit × {} conservative factor (discount for executions \
             that would have shared already-running warehouse time). \
             Reconcile against WAREHOUSE_METERING_HISTORY.",
            self.usd_per_credit, self.conservative_factor
        )
    }
}

pub struct Ledger {
    conn: Mutex<rusqlite::Connection>,
    pub pricing: Pricing,
}

impl Ledger {
    pub fn open(path: impl AsRef<Path>, pricing: Pricing) -> Result<Self> {
        let conn = rusqlite::Connection::open(path.as_ref())
            .map_err(|e| Error::Storage(format!("cannot open savings db: {e}")))?;
        Self::with_connection(conn, pricing)
    }

    pub fn open_in_memory(pricing: Pricing) -> Result<Self> {
        let conn = rusqlite::Connection::open_in_memory()
            .map_err(|e| Error::Storage(format!("cannot open savings db: {e}")))?;
        Self::with_connection(conn, pricing)
    }

    fn with_connection(conn: rusqlite::Connection, pricing: Pricing) -> Result<Self> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS savings (
                id INTEGER PRIMARY KEY,
                at_unix_ms INTEGER NOT NULL,
                kind TEXT NOT NULL,
                fingerprint TEXT,
                team TEXT,
                warehouse TEXT,
                avoided_credits REAL NOT NULL,
                avoided_usd REAL NOT NULL,
                canonical_elapsed_ms INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_savings_at ON savings(at_unix_ms);",
        )
        .map_err(|e| Error::Storage(format!("cannot init savings schema: {e}")))?;
        Ok(Self {
            conn: Mutex::new(conn),
            pricing,
        })
    }

    pub fn record(&self, event: &SavingsEvent) -> Result<()> {
        self.conn
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO savings
                 (at_unix_ms, kind, fingerprint, team, warehouse,
                  avoided_credits, avoided_usd, canonical_elapsed_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                rusqlite::params![
                    event.at_unix_ms as i64,
                    event.kind,
                    event.fingerprint_hex,
                    event.team,
                    event.warehouse,
                    event.avoided_credits,
                    event.avoided_usd,
                    event.canonical_elapsed_ms as i64,
                ],
            )
            .map_err(|e| Error::Storage(format!("ledger insert failed: {e}")))?;
        Ok(())
    }

    pub fn report(&self, since_unix_ms: u64, until_unix_ms: u64) -> Result<SavingsReport> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT kind, team, avoided_credits, avoided_usd FROM savings
                 WHERE at_unix_ms >= ?1 AND at_unix_ms <= ?2",
            )
            .map_err(|e| Error::Storage(e.to_string()))?;
        let rows = stmt
            .query_map(
                rusqlite::params![since_unix_ms as i64, until_unix_ms as i64],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, f64>(2)?,
                        row.get::<_, f64>(3)?,
                    ))
                },
            )
            .map_err(|e| Error::Storage(e.to_string()))?;

        let mut report = SavingsReport {
            since_unix_ms,
            until_unix_ms,
            total_events: 0,
            total_avoided_usd: 0.0,
            total_avoided_credits: 0.0,
            by_kind: BTreeMap::new(),
            by_team: BTreeMap::new(),
            methodology: self.pricing.methodology(),
        };
        for row in rows {
            let (kind, team, credits, usd) = row.map_err(|e| Error::Storage(e.to_string()))?;
            report.total_events += 1;
            report.total_avoided_usd += usd;
            report.total_avoided_credits += credits;
            let agg = report.by_kind.entry(kind).or_default();
            agg.events += 1;
            agg.avoided_usd += usd;
            agg.avoided_credits += credits;
            *report
                .by_team
                .entry(team.unwrap_or_else(|| "unattributed".into()))
                .or_insert(0.0) += usd;
        }
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pricing() -> Pricing {
        Pricing::from_config(&SavingsConfig {
            usd_per_credit: 3.0,
            conservative_factor: 0.7,
            default_warehouse_size: "M".into(),
            warehouse_sizes: [("BI_WH".to_string(), "L".to_string())].into(),
            ..Default::default()
        })
    }

    #[test]
    fn pricing_uses_configured_sizes_and_conservative_factor() {
        let p = pricing();
        // BI_WH is L (8 cr/hr): 3600s × 8/hr × $3 × 0.7 = $16.80
        let (credits, usd) = p.avoided(Some("bi_wh"), 3_600_000);
        assert!((credits - 8.0 * 0.7).abs() < 1e-9, "{credits}");
        assert!((usd - 16.8).abs() < 1e-9, "{usd}");
        // Unknown warehouse → default M (4 cr/hr)
        let (_, usd_default) = p.avoided(Some("MYSTERY"), 3_600_000);
        assert!((usd_default - 8.4).abs() < 1e-9, "{usd_default}");
    }

    #[test]
    fn ledger_records_and_aggregates() {
        let ledger = Ledger::open_in_memory(pricing()).unwrap();
        for (i, (kind, team)) in [
            ("cache_hit", Some("growth")),
            ("cache_hit", Some("growth")),
            ("coalesced", None),
        ]
        .iter()
        .enumerate()
        {
            let (credits, usd) = ledger.pricing.avoided(Some("BI_WH"), 5_000);
            ledger
                .record(&SavingsEvent {
                    at_unix_ms: 1_000 + i as u64,
                    kind: kind.to_string(),
                    fingerprint_hex: Some("abcd".into()),
                    team: team.map(String::from),
                    warehouse: Some("BI_WH".into()),
                    avoided_credits: credits,
                    avoided_usd: usd,
                    canonical_elapsed_ms: 5_000,
                })
                .unwrap();
        }
        let report = ledger.report(0, 10_000).unwrap();
        assert_eq!(report.total_events, 3);
        assert_eq!(report.by_kind["cache_hit"].events, 2);
        assert_eq!(report.by_kind["coalesced"].events, 1);
        assert!(report.by_team["growth"] > 0.0);
        assert!(report.total_avoided_usd > 0.0);
        // Time filtering works.
        assert_eq!(ledger.report(5_000, 10_000).unwrap().total_events, 0);
    }

    #[test]
    fn ledger_persists_across_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("savings.db");
        {
            let ledger = Ledger::open(&path, pricing()).unwrap();
            let (credits, usd) = ledger.pricing.avoided(None, 1_000);
            ledger
                .record(&SavingsEvent {
                    at_unix_ms: 42,
                    kind: "cache_hit".into(),
                    fingerprint_hex: None,
                    team: None,
                    warehouse: None,
                    avoided_credits: credits,
                    avoided_usd: usd,
                    canonical_elapsed_ms: 1_000,
                })
                .unwrap();
        }
        let reopened = Ledger::open(&path, pricing()).unwrap();
        assert_eq!(reopened.report(0, 100).unwrap().total_events, 1);
    }
}
