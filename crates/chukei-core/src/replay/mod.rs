//! `chukei replay` — the offline savings simulator (PRD F-010, §27.1 wk 7).
//!
//! Ingests a Snowflake `ACCOUNT_USAGE.QUERY_HISTORY` CSV export and projects,
//! per plugin, what chukei would have saved. No Snowflake account needed.
//!
//! Honesty rules, stated in the report itself:
//! - Cache/router savings are priced at each query's own measured cost.
//! - Rewrite opportunities are *counted*, never priced — wall-clock impact
//!   is workload-specific and must be benchmarked, not asserted.
//! - Suspend savings use a transparent idle model (see `SuspendAssumptions`).

use std::collections::HashMap;
use std::io::Read;
use std::str::FromStr;

use chrono::{DateTime, NaiveDateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::error::{Error, Result};
use crate::plugin::WarehouseSize;
use crate::rewrite::RewritePlugin;
use crate::sql::analyze;

/// One row of query history we care about. Header matching is
/// case-insensitive; only `query_text` is required.
#[derive(Debug, Clone, Default)]
pub struct HistoryRow {
    pub query_id: Option<String>,
    pub query_text: String,
    pub user_name: Option<String>,
    pub warehouse_name: Option<String>,
    pub warehouse_size: Option<String>,
    pub start_time: Option<DateTime<Utc>>,
    pub total_elapsed_ms: Option<u64>,
    pub bytes_scanned: Option<u64>,
    pub rows_produced: Option<u64>,
    pub query_tag: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayOptions {
    /// USD per Snowflake credit (list price default).
    pub usd_per_credit: f64,
    /// Assumed static AUTO_SUSPEND timeout the account runs today (secs).
    pub assumed_auto_suspend_secs: f64,
    /// Scans below this byte count are DuckDB-routable candidates.
    pub router_max_bytes_scanned: u64,
    /// A table set must repeat at least this often to assume a replica.
    pub router_min_occurrences: usize,
}

impl Default for ReplayOptions {
    fn default() -> Self {
        Self {
            usd_per_credit: 3.0,
            assumed_auto_suspend_secs: 600.0,
            router_max_bytes_scanned: 100 * 1024 * 1024,
            router_min_occurrences: 5,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayReport {
    pub generated_at: DateTime<Utc>,
    pub options: ReplayOptions,
    pub parse: ParseStats,
    pub cache: CacheProjection,
    pub router: RouterProjection,
    pub rewrite: RewriteProjection,
    pub suspend: SuspendProjection,
    pub attribution: AttributionStats,
    /// Cache + router + suspend (rewrite is counted, not priced).
    pub projected_savings_usd: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ParseStats {
    pub total_rows: usize,
    pub parsed: usize,
    pub parse_failures: usize,
    pub coverage_pct: f64,
    /// Distinct hard fingerprints among parsed queries.
    pub distinct_hard_fingerprints: usize,
    /// Distinct soft fingerprints — the dedup ratio dashboards never see.
    pub distinct_soft_fingerprints: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CacheProjection {
    pub eligible_queries: usize,
    pub projected_hits: usize,
    pub hit_rate_pct: f64,
    pub saved_usd: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RouterProjection {
    pub routable_queries: usize,
    pub saved_usd: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RewriteProjection {
    pub queries_with_rewrites: usize,
    pub rule_hits: HashMap<String, usize>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SuspendProjection {
    pub warehouses: usize,
    pub idle_hours_saved: f64,
    pub saved_usd: f64,
    pub assumptions: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AttributionStats {
    pub queries_with_query_tag: usize,
    pub queries_with_chukei_hints: usize,
    pub queries_with_dbt_meta: usize,
    pub coverage_pct: f64,
}

/// Read query-history rows from CSV with case-insensitive headers.
pub fn read_csv(reader: impl Read) -> Result<Vec<HistoryRow>> {
    let mut csv_reader = csv::ReaderBuilder::new().flexible(true).from_reader(reader);
    let headers: Vec<String> = csv_reader
        .headers()
        .map_err(|e| Error::Replay(format!("cannot read CSV headers: {e}")))?
        .iter()
        .map(|h| h.trim().to_lowercase())
        .collect();
    let col = |name: &str| headers.iter().position(|h| h == name);

    let idx_text = col("query_text")
        .ok_or_else(|| Error::Replay("CSV is missing required column QUERY_TEXT".into()))?;
    let idx = (
        col("query_id"),
        col("user_name"),
        col("warehouse_name"),
        col("warehouse_size"),
        col("start_time"),
        col("total_elapsed_time"),
        col("bytes_scanned"),
        col("rows_produced"),
        col("query_tag"),
    );

    let mut rows = Vec::new();
    for record in csv_reader.records() {
        let record = record.map_err(|e| Error::Replay(format!("CSV parse error: {e}")))?;
        let get = |i: Option<usize>| {
            i.and_then(|i| record.get(i))
                .map(str::trim)
                .filter(|s| !s.is_empty())
        };
        let Some(text) = record
            .get(idx_text)
            .map(str::trim)
            .filter(|s| !s.is_empty())
        else {
            continue;
        };
        rows.push(HistoryRow {
            query_text: text.to_string(),
            query_id: get(idx.0).map(String::from),
            user_name: get(idx.1).map(String::from),
            warehouse_name: get(idx.2).map(String::from),
            warehouse_size: get(idx.3).map(String::from),
            start_time: get(idx.4).and_then(parse_time),
            total_elapsed_ms: get(idx.5).and_then(|s| s.parse().ok()),
            bytes_scanned: get(idx.6).and_then(|s| s.parse().ok()),
            rows_produced: get(idx.7).and_then(|s| s.parse().ok()),
            query_tag: get(idx.8).map(String::from),
        });
    }
    Ok(rows)
}

fn parse_time(s: &str) -> Option<DateTime<Utc>> {
    if let Ok(t) = DateTime::parse_from_rfc3339(s) {
        return Some(t.with_timezone(&Utc));
    }
    for fmt in [
        "%Y-%m-%d %H:%M:%S%.f %z",
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%d %H:%M:%S",
    ] {
        if let Ok(t) = DateTime::parse_from_str(s, fmt) {
            return Some(t.with_timezone(&Utc));
        }
        if let Ok(t) = NaiveDateTime::parse_from_str(s, fmt) {
            return Some(t.and_utc());
        }
    }
    None
}

/// USD cost of one query: size credit rate × wall-clock, per-second billing.
fn query_cost_usd(row: &HistoryRow, options: &ReplayOptions) -> f64 {
    let size = row
        .warehouse_size
        .as_deref()
        .and_then(|s| WarehouseSize::from_str(s).ok())
        .unwrap_or(WarehouseSize::Xs);
    let elapsed_secs = row.total_elapsed_ms.unwrap_or(0) as f64 / 1000.0;
    size.credits_per_hour() / 3600.0 * elapsed_secs * options.usd_per_credit
}

pub fn simulate(rows: &[HistoryRow], config: &Config, options: ReplayOptions) -> ReplayReport {
    let rewrite_plugin = RewritePlugin::new(config.plugins.rewrite.clone());
    let cache_ttl_ms = config.plugins.cache.default_ttl_secs as f64 * 1000.0;

    // CSV exports are not guaranteed time-ordered, and every time-based
    // projection (cache TTL windows, suspend gaps) is order-sensitive.
    // Rows without a timestamp sort first and are skipped by those sims.
    let mut rows: Vec<&HistoryRow> = rows.iter().collect();
    rows.sort_by_key(|r| r.start_time);
    let rows = rows;

    let mut parse = ParseStats {
        total_rows: rows.len(),
        ..Default::default()
    };
    let mut hard_fps: HashMap<[u8; 32], ()> = HashMap::new();
    let mut soft_fps: HashMap<[u8; 16], ()> = HashMap::new();

    let mut cache = CacheProjection::default();
    let mut cache_state: HashMap<[u8; 32], f64> = HashMap::new(); // fp → inserted ms

    let mut router = RouterProjection::default();
    let mut table_set_counts: HashMap<String, usize> = HashMap::new();

    let mut rewrite = RewriteProjection::default();
    let mut attribution = AttributionStats::default();
    let mut arrivals: HashMap<String, Vec<f64>> = HashMap::new(); // warehouse → ms
    let mut warehouse_rate: HashMap<String, f64> = HashMap::new(); // credits/hr

    // First pass for router: how often does each exact table set appear?
    for row in &rows {
        if let Ok(analysis) = analyze(&row.query_text) {
            table_set_counts
                .entry(analysis.features.tables.join(","))
                .and_modify(|c| *c += 1)
                .or_insert(1);
        }
    }

    for row in &rows {
        let analysis = match analyze(&row.query_text) {
            Ok(a) => a,
            Err(_) => {
                parse.parse_failures += 1;
                continue;
            }
        };
        parse.parsed += 1;
        hard_fps.insert(analysis.hard_fingerprint, ());
        soft_fps.insert(analysis.soft_fingerprint, ());

        let cost = query_cost_usd(row, &options);
        let ts_ms = row.start_time.map(|t| t.timestamp_millis() as f64);
        let read_only = crate::sql::parse::is_read_only(&analysis.statement);

        // ── cache projection ────────────────────────────────────────────
        if read_only && analysis.features.deterministic && !analysis.features.tables.is_empty() {
            cache.eligible_queries += 1;
            if let Some(now) = ts_ms {
                match cache_state.get(&analysis.hard_fingerprint) {
                    Some(inserted) if now - inserted <= cache_ttl_ms => {
                        cache.projected_hits += 1;
                        cache.saved_usd += cost;
                    }
                    _ => {
                        cache_state.insert(analysis.hard_fingerprint, now);
                    }
                }
            }
        }

        // ── router projection ───────────────────────────────────────────
        let table_key = analysis.features.tables.join(",");
        let repeats = table_set_counts.get(&table_key).copied().unwrap_or(0);
        if read_only
            && analysis.features.deterministic
            && !analysis.features.tables.is_empty()
            && repeats >= options.router_min_occurrences
            && row.bytes_scanned.unwrap_or(u64::MAX) < options.router_max_bytes_scanned
        {
            router.routable_queries += 1;
            router.saved_usd += cost;
        }

        // ── rewrite opportunities ───────────────────────────────────────
        if let Some((_, fired)) = rewrite_plugin.rewrite(&analysis.statement, &analysis.hints) {
            rewrite.queries_with_rewrites += 1;
            for rule in fired {
                *rewrite.rule_hits.entry(rule.to_string()).or_insert(0) += 1;
            }
        }

        // ── attribution coverage ────────────────────────────────────────
        if row.query_tag.is_some() {
            attribution.queries_with_query_tag += 1;
        }
        if !analysis.hints.is_empty() {
            attribution.queries_with_chukei_hints += 1;
        }
        if row.query_text.contains("\"app\": \"dbt\"") || row.query_text.contains("\"app\":\"dbt\"")
        {
            attribution.queries_with_dbt_meta += 1;
        }

        // ── suspend inputs ──────────────────────────────────────────────
        if let (Some(wh), Some(ts)) = (&row.warehouse_name, ts_ms) {
            arrivals.entry(wh.clone()).or_default().push(ts);
            let rate = row
                .warehouse_size
                .as_deref()
                .and_then(|s| WarehouseSize::from_str(s).ok())
                .unwrap_or(WarehouseSize::Xs)
                .credits_per_hour();
            warehouse_rate.insert(wh.clone(), rate);
        }
    }

    parse.coverage_pct = pct(parse.parsed, parse.total_rows);
    parse.distinct_hard_fingerprints = hard_fps.len();
    parse.distinct_soft_fingerprints = soft_fps.len();
    cache.hit_rate_pct = pct(cache.projected_hits, cache.eligible_queries);
    attribution.coverage_pct = pct(
        attribution.queries_with_query_tag
            + attribution.queries_with_chukei_hints
            + attribution.queries_with_dbt_meta,
        parse.total_rows,
    );

    // ── suspend projection ──────────────────────────────────────────────
    // Today: every inter-query gap bills min(gap, AUTO_SUSPEND) of idle.
    // With chukei: suspend after a 60 s grace window, and pay Snowflake's
    // 60 s minimum on resume → 120 s effective floor per gap.
    let mut suspend = SuspendProjection {
        assumptions: format!(
            "billed idle today = min(gap, {}s AUTO_SUSPEND); chukei = 60s grace + 60s resume minimum",
            options.assumed_auto_suspend_secs
        ),
        ..Default::default()
    };
    for (wh, mut times) in arrivals {
        times.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let rate = warehouse_rate.get(&wh).copied().unwrap_or(1.0);
        let mut saved_secs = 0.0;
        for pair in times.windows(2) {
            let gap_secs = (pair[1] - pair[0]) / 1000.0;
            let billed_today = gap_secs.min(options.assumed_auto_suspend_secs);
            saved_secs += (billed_today - 120.0).max(0.0);
        }
        suspend.warehouses += 1;
        suspend.idle_hours_saved += saved_secs / 3600.0;
        suspend.saved_usd += saved_secs / 3600.0 * rate * options.usd_per_credit;
    }

    let projected = cache.saved_usd + router.saved_usd + suspend.saved_usd;
    ReplayReport {
        generated_at: Utc::now(),
        options,
        parse,
        cache,
        router,
        rewrite,
        suspend,
        attribution,
        projected_savings_usd: projected,
    }
}

fn pct(part: usize, whole: usize) -> f64 {
    if whole == 0 {
        0.0
    } else {
        part as f64 / whole as f64 * 100.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn csv_fixture() -> String {
        let mut out = String::from(
            "QUERY_ID,QUERY_TEXT,USER_NAME,WAREHOUSE_NAME,WAREHOUSE_SIZE,START_TIME,TOTAL_ELAPSED_TIME,BYTES_SCANNED,ROWS_PRODUCED,QUERY_TAG\n",
        );
        // A dashboard query repeated every minute for 10 minutes (cache bait),
        // plus one big query and one unparseable line.
        for i in 0..10 {
            out.push_str(&format!(
                "q{i},\"SELECT region, SUM(amount) FROM analytics.marts.revenue GROUP BY region\",SARA,DASH_WH,L,2026-06-01 09:{i:02}:00,4000,1048576,12,\n"
            ));
        }
        out.push_str(
            "qbig,\"SELECT * FROM analytics.raw.events WHERE ts > '2026-01-01'\",ETL,ETL_WH,XL,2026-06-01 12:00:00,90000,99999999999,5000000,etl\n",
        );
        out.push_str(
            "qbad,\"THIS IS NOT SQL AT ALL ;;;\",X,ETL_WH,XS,2026-06-01 13:00:00,1,1,1,\n",
        );
        out
    }

    #[test]
    fn csv_reader_maps_columns() {
        let rows = read_csv(csv_fixture().as_bytes()).unwrap();
        assert_eq!(rows.len(), 12);
        assert_eq!(rows[0].warehouse_size.as_deref(), Some("L"));
        assert_eq!(rows[0].total_elapsed_ms, Some(4000));
        assert!(rows[0].start_time.is_some());
        assert_eq!(rows[10].query_tag.as_deref(), Some("etl"));
    }

    #[test]
    fn simulation_projects_cache_hits_and_counts_failures() {
        let rows = read_csv(csv_fixture().as_bytes()).unwrap();
        let config = Config::default();
        let report = simulate(&rows, &config, ReplayOptions::default());

        assert_eq!(report.parse.total_rows, 12);
        assert_eq!(report.parse.parse_failures, 1);
        assert!(report.parse.coverage_pct > 90.0);

        // 10 identical dashboard queries inside a 15-min TTL → 9 hits.
        assert_eq!(report.cache.projected_hits, 9);
        assert!(report.cache.saved_usd > 0.0);

        // Dashboard query: 4 s on L (8 cr/hr) at $3 → ~$0.0267/run × 9.
        let expected = 8.0 / 3600.0 * 4.0 * 3.0 * 9.0;
        assert!(
            (report.cache.saved_usd - expected).abs() < 1e-9,
            "{}",
            report.cache.saved_usd
        );

        // The repeated small query family is routable; the big one is not.
        assert_eq!(report.router.routable_queries, 10);

        assert!(report.projected_savings_usd >= report.cache.saved_usd);
    }

    #[test]
    fn suspend_projection_prices_idle_gaps() {
        // Two queries 30 min apart on an XS warehouse: today bills 600 s of
        // idle; chukei keeps 120 s → 480 s saved at 1 cr/hr × $3.
        let csv = "QUERY_TEXT,WAREHOUSE_NAME,WAREHOUSE_SIZE,START_TIME,TOTAL_ELAPSED_TIME\n\
                   SELECT 1 FROM t,WH,XS,2026-06-01 09:00:00,1000\n\
                   SELECT 2 FROM t,WH,XS,2026-06-01 09:30:00,1000\n";
        let rows = read_csv(csv.as_bytes()).unwrap();
        let report = simulate(&rows, &Config::default(), ReplayOptions::default());
        let expected = 480.0 / 3600.0 * 1.0 * 3.0;
        assert!(
            (report.suspend.saved_usd - expected).abs() < 1e-9,
            "{}",
            report.suspend.saved_usd
        );
    }

    #[test]
    fn out_of_order_rows_cannot_create_spurious_cache_hits() {
        // Same query, 1 h apart (≫ 15 min TTL), but file order reversed:
        // a naive in-file-order simulation sees a negative gap and counts a
        // hit; the simulator must sort by start_time and count zero.
        let csv = "QUERY_TEXT,WAREHOUSE_NAME,WAREHOUSE_SIZE,START_TIME,TOTAL_ELAPSED_TIME\n\
                   SELECT a FROM t,WH,XS,2026-06-01 10:00:00,1000\n\
                   SELECT a FROM t,WH,XS,2026-06-01 09:00:00,1000\n";
        let rows = read_csv(csv.as_bytes()).unwrap();
        let report = simulate(&rows, &Config::default(), ReplayOptions::default());
        assert_eq!(report.cache.projected_hits, 0);
    }

    #[test]
    fn shuffled_input_equals_sorted_input() {
        let rows = read_csv(csv_fixture().as_bytes()).unwrap();
        let mut reversed = rows.clone();
        reversed.reverse();
        let a = simulate(&rows, &Config::default(), ReplayOptions::default());
        let b = simulate(&reversed, &Config::default(), ReplayOptions::default());
        assert_eq!(a.cache.projected_hits, b.cache.projected_hits);
        assert_eq!(a.suspend.saved_usd, b.suspend.saved_usd);
        assert_eq!(a.projected_savings_usd, b.projected_savings_usd);
    }

    #[test]
    fn report_serialises() {
        let rows = read_csv(csv_fixture().as_bytes()).unwrap();
        let report = simulate(&rows, &Config::default(), ReplayOptions::default());
        let json = serde_json::to_string_pretty(&report).unwrap();
        assert!(json.contains("projected_savings_usd"));
    }
}
