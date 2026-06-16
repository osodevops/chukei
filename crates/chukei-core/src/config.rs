//! YAML configuration model (PRD §12).
//!
//! Override order (lowest precedence first): compiled defaults → config file
//! → environment variables → CLI flags. Env-var interpolation uses the same
//! `${VAR}` syntax as kafka-backup.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

use crate::error::{Error, Result};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub listen: ListenConfig,
    #[serde(default)]
    pub upstream: UpstreamConfig,
    #[serde(default)]
    pub storage: StorageConfig,
    #[serde(default)]
    pub control_plane: ControlPlaneConfig,
    #[serde(default)]
    pub coalesce: CoalesceConfig,
    #[serde(default)]
    pub savings: SavingsConfig,
    #[serde(default)]
    pub service_account: ServiceAccountConfig,
    #[serde(default)]
    pub plugins: PluginsConfig,
    #[serde(default)]
    pub observability: ObservabilityConfig,
    #[serde(default)]
    pub evidence: EvidenceConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListenConfig {
    #[serde(default = "default_bind")]
    pub bind: String,
    #[serde(default)]
    pub tls: Option<TlsConfig>,
}

impl Default for ListenConfig {
    fn default() -> Self {
        Self {
            bind: default_bind(),
            tls: None,
        }
    }
}

fn default_bind() -> String {
    "0.0.0.0:8443".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TlsConfig {
    pub cert: String,
    pub key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct UpstreamConfig {
    #[serde(default)]
    pub snowflake: Option<SnowflakeUpstream>,
    /// P1 — accepted in config so files validate, but unused in P0.
    #[serde(default)]
    pub databricks: Option<DatabricksUpstream>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnowflakeUpstream {
    /// Account locator, e.g. `abc12345.us-east-1`. Auth is passed through
    /// from the client; no credentials live here.
    pub account: String,
    /// Overrides the derived snowflakecomputing.com URL. Intended for the
    /// chukei-mock-sf test harness; do not set in production.
    #[serde(default)]
    pub base_url_override: Option<String>,
}

impl SnowflakeUpstream {
    pub fn base_url(&self) -> String {
        self.base_url_override
            .clone()
            .unwrap_or_else(|| format!("https://{}.snowflakecomputing.com", self.account))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DatabricksUpstream {
    pub workspace_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StorageConfig {
    #[serde(default = "default_backend")]
    pub backend: StorageBackend,
    #[serde(default)]
    pub bucket: Option<String>,
    #[serde(default)]
    pub region: Option<String>,
    #[serde(default)]
    pub prefix: Option<String>,
    #[serde(default)]
    pub access_key: Option<String>,
    #[serde(default)]
    pub secret_key: Option<String>,
    /// Used when `backend: filesystem`.
    #[serde(default)]
    pub path: Option<String>,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            backend: default_backend(),
            bucket: None,
            region: None,
            prefix: None,
            access_key: None,
            secret_key: None,
            path: None,
        }
    }
}

fn default_backend() -> StorageBackend {
    StorageBackend::Filesystem
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StorageBackend {
    S3,
    Azure,
    Gcs,
    Filesystem,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ControlPlaneConfig {
    #[serde(default)]
    pub postgres_url: Option<String>,
}

/// In-flight request coalescing (wire-layer, not a plugin): concurrent
/// identical deterministic reads share one upstream execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoalesceConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_coalesce_scope")]
    pub scope: CoalesceScope,
}

impl Default for CoalesceConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            scope: default_coalesce_scope(),
        }
    }
}

fn default_coalesce_scope() -> CoalesceScope {
    CoalesceScope::Session
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CoalesceScope {
    /// Only requests carrying the same session token coalesce. Safe under
    /// row-level security and per-role masking.
    Session,
    /// All sessions coalesce. Only enable when no RLS/masking policies can
    /// make the same SQL return different rows for different sessions.
    Account,
}

/// Realized-savings ledger (see `savings` module for the methodology).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavingsConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// SQLite path; defaults to `<tmpdir>/chukei-savings.db`.
    #[serde(default)]
    pub db_path: Option<String>,
    #[serde(default = "default_usd_per_credit")]
    pub usd_per_credit: f64,
    /// Discount for avoided executions that would have shared
    /// already-running warehouse time. Under-claim by default.
    #[serde(default = "default_conservative_factor")]
    pub conservative_factor: f64,
    #[serde(default = "default_warehouse_size")]
    pub default_warehouse_size: String,
    /// Warehouse name → size (XS..4XL); the proxy can't see sizes itself.
    #[serde(default)]
    pub warehouse_sizes: BTreeMap<String, String>,
}

impl Default for SavingsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            db_path: None,
            usd_per_credit: default_usd_per_credit(),
            conservative_factor: default_conservative_factor(),
            default_warehouse_size: default_warehouse_size(),
            warehouse_sizes: BTreeMap::new(),
        }
    }
}

fn default_usd_per_credit() -> f64 {
    3.0
}
fn default_conservative_factor() -> f64 {
    0.7
}
fn default_warehouse_size() -> String {
    "M".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct PluginsConfig {
    #[serde(default)]
    pub cache: CacheConfig,
    #[serde(default)]
    pub router: RouterConfig,
    #[serde(default)]
    pub rewrite: RewriteConfig,
    #[serde(default)]
    pub bandit: BanditConfig,
    #[serde(default)]
    pub suspend: SuspendConfig,
    #[serde(default)]
    pub attribute: AttributeConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CacheConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_cache_ttl")]
    pub default_ttl_secs: u64,
    /// Glob-ish table-family patterns → TTL seconds, e.g. `ANALYTICS.MARTS.*: 86400`.
    #[serde(default)]
    pub table_ttls: BTreeMap<String, u64>,
    #[serde(default = "default_determinism_gate")]
    pub determinism_gate: DeterminismGate,
    /// Fraction of cache hits double-checked against upstream (blame mode).
    #[serde(default = "default_blame_sample_rate")]
    pub blame_sample_rate: f64,
    /// Hard cap on cached entries; oldest are evicted past this.
    #[serde(default = "default_cache_max_entries")]
    pub max_entries: usize,
    /// When set, cache entries persist to this directory and survive
    /// restarts. Unset = in-memory only.
    #[serde(default)]
    pub persist_path: Option<String>,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            default_ttl_secs: default_cache_ttl(),
            table_ttls: BTreeMap::new(),
            determinism_gate: default_determinism_gate(),
            blame_sample_rate: default_blame_sample_rate(),
            max_entries: default_cache_max_entries(),
            persist_path: None,
        }
    }
}

fn default_cache_max_entries() -> usize {
    10_000
}

fn default_cache_ttl() -> u64 {
    900
}
fn default_determinism_gate() -> DeterminismGate {
    DeterminismGate::Strict
}
fn default_blame_sample_rate() -> f64 {
    0.01
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DeterminismGate {
    Strict,
    Relaxed,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct RouterConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub duckdb: DuckDbConfig,
    #[serde(default)]
    pub trino: TrinoConfig,
    /// Route to DuckDB only below this estimated scan size.
    #[serde(default = "default_small_scan_rows")]
    pub small_scan_rows: Option<u64>,
}

fn default_small_scan_rows() -> Option<u64> {
    Some(1_000_000)
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct DuckDbConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub replicas: Vec<ReplicaConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplicaConfig {
    /// Fully-qualified source table, e.g. `ANALYTICS.RAW.EVENTS`.
    pub source: String,
    pub iceberg_path: String,
    #[serde(default = "default_refresh")]
    pub refresh: String,
}

fn default_refresh() -> String {
    "continuous".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct TrinoConfig {
    #[serde(default)]
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct RewriteConfig {
    #[serde(default)]
    pub enabled: bool,
    /// Rule names, or the single entry `all`.
    #[serde(default)]
    pub rules: Vec<String>,
    #[serde(default)]
    pub experimental: ExperimentalRewrite,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ExperimentalRewrite {
    #[serde(default)]
    pub llm_rewrite: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct BanditConfig {
    #[serde(default)]
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SuspendConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_suspend_mode")]
    pub mode: SuspendMode,
    #[serde(default)]
    pub role: Option<String>,
    /// How often the background sweeper evaluates warehouses.
    #[serde(default = "default_sweep_interval_secs")]
    pub sweep_interval_secs: u64,
    /// Arrivals the model must see per warehouse before it will recommend
    /// anything (cold-start gate). Lower for pilots, keep high for prod.
    #[serde(default = "default_suspend_min_observations")]
    pub min_observations: u64,
    /// "Would a query arrive within this window?" horizon, seconds.
    #[serde(default = "default_suspend_horizon_secs")]
    pub horizon_secs: f64,
    /// Recommend suspend when P(arrival within horizon) is below this.
    #[serde(default = "default_suspend_p_threshold")]
    pub p_threshold: f64,
    /// Per-warehouse cooldown between suspend actions, seconds.
    #[serde(default = "default_suspend_cooldown_secs")]
    pub cooldown_secs: u64,
}

fn default_suspend_cooldown_secs() -> u64 {
    300
}

fn default_suspend_min_observations() -> u64 {
    20
}
fn default_suspend_horizon_secs() -> f64 {
    60.0
}
fn default_suspend_p_threshold() -> f64 {
    0.2
}

impl Default for SuspendConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: default_suspend_mode(),
            role: None,
            sweep_interval_secs: default_sweep_interval_secs(),
            min_observations: default_suspend_min_observations(),
            horizon_secs: default_suspend_horizon_secs(),
            p_threshold: default_suspend_p_threshold(),
            cooldown_secs: default_suspend_cooldown_secs(),
        }
    }
}

fn default_sweep_interval_secs() -> u64 {
    30
}

/// chukei's own Snowflake service account — required only for actions the
/// proxy takes itself (`suspend.mode: enforce`). Never used for client
/// traffic, which always passes through with the client's own auth.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ServiceAccountConfig {
    #[serde(default)]
    pub user: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default)]
    pub role: Option<String>,
}

impl ServiceAccountConfig {
    pub fn is_configured(&self) -> bool {
        self.user.is_some() && self.password.is_some()
    }
}

fn default_suspend_mode() -> SuspendMode {
    SuspendMode::SuggestOnly
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SuspendMode {
    SuggestOnly,
    Enforce,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttributeConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_true")]
    pub auto_query_tag: bool,
    #[serde(default = "default_true")]
    pub dbt_metadata_parser: bool,
    #[serde(default = "default_attr_sources")]
    pub sources: Vec<String>,
}

impl Default for AttributeConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            auto_query_tag: true,
            dbt_metadata_parser: true,
            sources: default_attr_sources(),
        }
    }
}

fn default_true() -> bool {
    true
}
fn default_attr_sources() -> Vec<String> {
    vec![
        "hint_comment".into(),
        "application_name".into(),
        "dbt_meta".into(),
    ]
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ObservabilityConfig {
    #[serde(default)]
    pub prometheus: PrometheusConfig,
    #[serde(default)]
    pub otlp: OtlpConfig,
    #[serde(default)]
    pub openlineage: OpenLineageConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrometheusConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_prom_port")]
    pub port: u16,
}

impl Default for PrometheusConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            port: default_prom_port(),
        }
    }
}

fn default_prom_port() -> u16 {
    9090
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct OtlpConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub endpoint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct OpenLineageConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub endpoint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct EvidenceConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub signing: SigningConfig,
    #[serde(default)]
    pub retention_days: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct SigningConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub private_key_path: Option<String>,
}

impl Config {
    /// Load from a YAML file, interpolating `${VAR}`, then applying
    /// `CHUKEI_*` environment overrides (PRD §12.2 precedence: file < env).
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self> {
        let raw = std::fs::read_to_string(path.as_ref())
            .map_err(|e| Error::Config(format!("cannot read {}: {e}", path.as_ref().display())))?;
        let overrides: Vec<(String, String)> = std::env::vars()
            .filter(|(k, _)| k.starts_with("CHUKEI_"))
            .collect();
        Self::from_yaml_with_overrides(&raw, &overrides)
    }

    /// Parse YAML after env-var interpolation (no CHUKEI_* overrides).
    pub fn from_yaml(raw: &str) -> Result<Self> {
        Self::from_yaml_with_overrides(raw, &[])
    }

    /// Parse YAML and apply `CHUKEI_LISTEN_BIND`-style overrides. Env-name
    /// segments greedily match nested keys, so
    /// `CHUKEI_SAVINGS_USD_PER_CREDIT` reaches `savings.usd_per_credit`.
    pub fn from_yaml_with_overrides(raw: &str, overrides: &[(String, String)]) -> Result<Self> {
        let interpolated = interpolate_env(raw)?;
        let yaml: serde_yaml::Value = serde_yaml::from_str(&interpolated)?;
        let mut json = serde_json::to_value(&yaml)
            .map_err(|e| Error::Config(format!("config transcode failed: {e}")))?;
        if json.is_null() {
            json = serde_json::Value::Object(Default::default());
        }
        for (key, value) in overrides {
            let Some(path) = key.strip_prefix("CHUKEI_") else {
                continue;
            };
            let segments: Vec<String> = path.to_lowercase().split('_').map(String::from).collect();
            if !apply_override(&mut json, &segments, value) {
                tracing::warn!(env = %key, "CHUKEI_ override did not match any config field");
            }
        }
        let config: Config = serde_json::from_value(json)
            .map_err(|e| Error::Config(format!("invalid configuration: {e}")))?;
        config.validate()?;
        Ok(config)
    }

    /// Cross-field validation beyond what serde can express.
    pub fn validate(&self) -> Result<()> {
        if let Some(tls) = &self.listen.tls {
            if tls.cert.is_empty() || tls.key.is_empty() {
                return Err(Error::Config(
                    "listen.tls.cert and listen.tls.key must be non-empty".into(),
                ));
            }
        }
        if self.plugins.cache.enabled {
            let rate = self.plugins.cache.blame_sample_rate;
            if !(0.0..=1.0).contains(&rate) {
                return Err(Error::Config(format!(
                    "plugins.cache.blame_sample_rate must be in [0,1], got {rate}"
                )));
            }
        }
        if self.plugins.suspend.enabled && self.plugins.suspend.mode == SuspendMode::Enforce {
            if self.plugins.suspend.role.is_none() {
                return Err(Error::Config(
                    "plugins.suspend.mode=enforce requires plugins.suspend.role".into(),
                ));
            }
            if !self.service_account.is_configured() {
                return Err(Error::Config(
                    "plugins.suspend.mode=enforce requires service_account.user/password".into(),
                ));
            }
        }
        match self.storage.backend {
            StorageBackend::Filesystem => {}
            _ => {
                if self.storage.bucket.is_none() {
                    return Err(Error::Config(format!(
                        "storage.backend={:?} requires storage.bucket",
                        self.storage.backend
                    )));
                }
            }
        }
        Ok(())
    }
}

/// Apply one `CHUKEI_*` override into the config JSON. Path segments are
/// matched greedily (longest-first) against the *default* Config's key tree,
/// so multi-word fields like `usd_per_credit` resolve unambiguously even
/// when the target file omits the section entirely.
fn apply_override(target: &mut serde_json::Value, segments: &[String], raw: &str) -> bool {
    let schema = serde_json::to_value(Config::default()).unwrap_or_default();
    walk_override(&schema, target, segments, raw)
}

fn walk_override(
    schema: &serde_json::Value,
    target: &mut serde_json::Value,
    segments: &[String],
    raw: &str,
) -> bool {
    let serde_json::Value::Object(schema_map) = schema else {
        return false;
    };
    if !target.is_object() {
        *target = serde_json::Value::Object(Default::default());
    }
    for take in (1..=segments.len()).rev() {
        let key = segments[..take].join("_");
        let Some(child_schema) = schema_map.get(&key) else {
            continue;
        };
        if take == segments.len() {
            let parsed = parse_override_scalar(raw, child_schema);
            target
                .as_object_mut()
                .expect("target coerced to object above")
                .insert(key, parsed);
            return true;
        }
        let obj = target
            .as_object_mut()
            .expect("target coerced to object above");
        let child_target = obj
            .entry(key.clone())
            .or_insert(serde_json::Value::Object(Default::default()));
        if walk_override(child_schema, child_target, &segments[take..], raw) {
            return true;
        }
    }
    false
}

fn parse_override_scalar(raw: &str, hint: &serde_json::Value) -> serde_json::Value {
    match hint {
        serde_json::Value::Bool(_) => {
            return serde_json::Value::Bool(raw.eq_ignore_ascii_case("true") || raw == "1");
        }
        serde_json::Value::Number(_) => {
            if let Ok(n) = raw.parse::<i64>() {
                return serde_json::json!(n);
            }
            if let Ok(f) = raw.parse::<f64>() {
                return serde_json::json!(f);
            }
        }
        _ => {}
    }
    // No hint (Option fields serialize as null): infer.
    if raw.eq_ignore_ascii_case("true") || raw.eq_ignore_ascii_case("false") {
        return serde_json::Value::Bool(raw.eq_ignore_ascii_case("true"));
    }
    if let Ok(n) = raw.parse::<i64>() {
        return serde_json::json!(n);
    }
    if let Ok(f) = raw.parse::<f64>() {
        return serde_json::json!(f);
    }
    serde_json::Value::String(raw.to_string())
}

/// Replace `${VAR}` with the value of environment variable `VAR`.
/// Unset variables are an error — silent empty strings hide misconfiguration.
/// Full-line YAML comments are left untouched so docs can mention `${VAR}`.
fn interpolate_env(raw: &str) -> Result<String> {
    let mut out = String::with_capacity(raw.len());
    for (i, line) in raw.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        if line.trim_start().starts_with('#') {
            out.push_str(line);
        } else {
            out.push_str(&interpolate_line(line)?);
        }
    }
    Ok(out)
}

fn interpolate_line(raw: &str) -> Result<String> {
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find('}') else {
            return Err(Error::Config(format!(
                "unterminated ${{...}} expression near: {}",
                &rest[start..rest.len().min(start + 30)]
            )));
        };
        let var = &after[..end];
        if var.is_empty() || !var.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return Err(Error::Config(format!(
                "invalid env var name in interpolation: '{var}'"
            )));
        }
        let value = std::env::var(var)
            .map_err(|_| Error::Config(format!("environment variable '{var}' is not set")))?;
        out.push_str(&value);
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE: &str = r#"
listen:
  bind: "0.0.0.0:8443"
upstream:
  snowflake:
    account: "abc12345.us-east-1"
storage:
  backend: filesystem
  path: "/tmp/chukei"
plugins:
  cache:
    enabled: true
    default_ttl_secs: 900
    table_ttls:
      "ANALYTICS.MARTS.*": 86400
    blame_sample_rate: 0.01
  suspend:
    enabled: true
    mode: suggest-only
  attribute:
    enabled: true
"#;

    #[test]
    fn parses_example_config() {
        let cfg = Config::from_yaml(EXAMPLE).unwrap();
        assert_eq!(cfg.listen.bind, "0.0.0.0:8443");
        assert!(cfg.plugins.cache.enabled);
        assert_eq!(cfg.plugins.cache.table_ttls["ANALYTICS.MARTS.*"], 86400);
        assert_eq!(cfg.plugins.suspend.mode, SuspendMode::SuggestOnly);
        assert_eq!(
            cfg.upstream.snowflake.unwrap().base_url(),
            "https://abc12345.us-east-1.snowflakecomputing.com"
        );
    }

    #[test]
    fn defaults_are_sane() {
        let cfg = Config::from_yaml("{}").unwrap();
        assert_eq!(cfg.listen.bind, "0.0.0.0:8443");
        assert!(!cfg.plugins.cache.enabled);
        assert_eq!(cfg.plugins.cache.default_ttl_secs, 900);
        assert_eq!(cfg.storage.backend, StorageBackend::Filesystem);
    }

    #[test]
    fn env_interpolation_works() {
        std::env::set_var("CHUKEI_TEST_BUCKET", "my-bucket");
        let yaml = r#"
storage:
  backend: s3
  bucket: "${CHUKEI_TEST_BUCKET}"
"#;
        let cfg = Config::from_yaml(yaml).unwrap();
        assert_eq!(cfg.storage.bucket.as_deref(), Some("my-bucket"));
    }

    #[test]
    fn chukei_env_overrides_apply_with_greedy_matching() {
        let overrides = vec![
            (
                "CHUKEI_LISTEN_BIND".to_string(),
                "127.0.0.1:9999".to_string(),
            ),
            (
                "CHUKEI_PLUGINS_CACHE_ENABLED".to_string(),
                "true".to_string(),
            ),
            (
                "CHUKEI_SAVINGS_USD_PER_CREDIT".to_string(),
                "2.5".to_string(),
            ),
            (
                "CHUKEI_PLUGINS_CACHE_DEFAULT_TTL_SECS".to_string(),
                "120".to_string(),
            ),
        ];
        // File omits every overridden section: env must still land.
        let cfg = Config::from_yaml_with_overrides("{}", &overrides).unwrap();
        assert_eq!(cfg.listen.bind, "127.0.0.1:9999");
        assert!(cfg.plugins.cache.enabled);
        assert_eq!(cfg.savings.usd_per_credit, 2.5);
        assert_eq!(cfg.plugins.cache.default_ttl_secs, 120);
    }

    #[test]
    fn env_overrides_beat_file_values() {
        let yaml = "listen:\n  bind: \"0.0.0.0:8443\"\n";
        let overrides = vec![("CHUKEI_LISTEN_BIND".to_string(), "1.2.3.4:1".to_string())];
        let cfg = Config::from_yaml_with_overrides(yaml, &overrides).unwrap();
        assert_eq!(cfg.listen.bind, "1.2.3.4:1");
    }

    #[test]
    fn comments_mentioning_interpolation_are_ignored() {
        let yaml = "# env vars interpolate with ${VAR}\nlisten:\n  bind: \"0.0.0.0:1\" # not ${THIS} though — inline comments do interpolate\n";
        // Full-line comment is fine; the inline one still errors (documented
        // wart: we don't parse YAML before interpolating).
        assert!(Config::from_yaml(yaml).is_err());
        let yaml = "# env vars interpolate with ${VAR}\nlisten:\n  bind: \"0.0.0.0:1\"\n";
        Config::from_yaml(yaml).unwrap();
    }

    #[test]
    fn missing_env_var_is_an_error() {
        let yaml = r#"
storage:
  backend: s3
  bucket: "${CHUKEI_DEFINITELY_NOT_SET_12345}"
"#;
        let err = Config::from_yaml(yaml).unwrap_err();
        assert!(matches!(err, Error::Config(_)), "got: {err}");
    }

    #[test]
    fn enforce_without_role_rejected() {
        let yaml = r#"
plugins:
  suspend:
    enabled: true
    mode: enforce
"#;
        assert!(Config::from_yaml(yaml).is_err());
    }

    #[test]
    fn s3_without_bucket_rejected() {
        let yaml = "storage:\n  backend: s3\n";
        assert!(Config::from_yaml(yaml).is_err());
    }

    #[test]
    fn unknown_fields_rejected() {
        let yaml = "listen:\n  bindd: \"0.0.0.0:1\"\n";
        assert!(Config::from_yaml(yaml).is_err());
    }
}
