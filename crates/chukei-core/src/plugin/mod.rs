//! Plugin contract (PRD §10).
//!
//! Plugins inspect an analysed query and emit a `Decision`. The bus (see
//! `bus.rs`) runs them in order and merges decisions deterministically.

pub mod bus;
pub mod registry;

use std::collections::BTreeMap;
use std::str::FromStr;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::sql::QueryAnalysis;
use crate::Result;

/// Connection/session facts the wire layer knows about the client.
#[derive(Debug, Clone, Default)]
pub struct Session {
    pub user: Option<String>,
    pub application_name: Option<String>,
    pub warehouse: Option<String>,
    pub role: Option<String>,
    pub query_tag: Option<String>,
}

/// Everything a plugin may look at. Immutable; plugins communicate only
/// through their returned `Decision`.
#[derive(Debug, Clone, Copy)]
pub struct QueryContext<'a> {
    pub analysis: &'a QueryAnalysis,
    pub session: &'a Session,
}

impl<'a> QueryContext<'a> {
    pub fn raw_sql(&self) -> &str {
        &self.analysis.raw_sql
    }
    pub fn fingerprint(&self) -> [u8; 32] {
        self.analysis.hard_fingerprint
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CacheKey {
    pub hard_fingerprint: [u8; 32],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Engine {
    Snowflake,
    DuckDb,
    /// P1.
    Trino,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum WarehouseSize {
    Xs,
    S,
    M,
    L,
    Xl,
    X2l,
    X3l,
    X4l,
}

impl FromStr for WarehouseSize {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_uppercase().as_str() {
            "XS" | "XSMALL" | "X-SMALL" => Ok(Self::Xs),
            "S" | "SMALL" => Ok(Self::S),
            "M" | "MEDIUM" => Ok(Self::M),
            "L" | "LARGE" => Ok(Self::L),
            "XL" | "XLARGE" | "X-LARGE" => Ok(Self::Xl),
            "2XL" | "X2L" | "XXLARGE" | "2X-LARGE" => Ok(Self::X2l),
            "3XL" | "X3L" | "3X-LARGE" => Ok(Self::X3l),
            "4XL" | "X4L" | "4X-LARGE" => Ok(Self::X4l),
            other => Err(format!("unknown warehouse size '{other}'")),
        }
    }
}

impl WarehouseSize {
    /// Snowflake credits/hour for a standard warehouse of this size.
    pub fn credits_per_hour(&self) -> f64 {
        match self {
            Self::Xs => 1.0,
            Self::S => 2.0,
            Self::M => 4.0,
            Self::L => 8.0,
            Self::Xl => 16.0,
            Self::X2l => 32.0,
            Self::X3l => 64.0,
            Self::X4l => 128.0,
        }
    }
}

pub type QueryTags = BTreeMap<String, String>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VetoReason(pub String);

/// What a plugin wants done with the query (PRD §10.1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Decision {
    Passthrough,
    ServeFromCache(CacheKey),
    Rewrite(String),
    Route(Engine),
    SetWarehouseSize(WarehouseSize),
    Annotate(QueryTags),
    Compose(Vec<Decision>),
    Veto(VetoReason),
}

/// Outcome facts handed to `Plugin::on_result` after the query completes.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ResultSnapshot {
    pub engine: Option<Engine>,
    pub row_count: u64,
    pub bytes_scanned: u64,
    pub wall_clock_ms: u64,
    pub served_from_cache: bool,
    /// Result payload, when a plugin (the cache) needs to persist it.
    pub data: Option<serde_json::Value>,
}

#[async_trait]
pub trait Plugin: Send + Sync {
    fn name(&self) -> &'static str;

    /// Bus position; lower runs earlier. Built-ins use 10/20/30/… so custom
    /// plugins can interleave.
    fn order(&self) -> i32;

    async fn decide(&self, ctx: &QueryContext<'_>) -> Result<Decision>;

    /// Called after the query completes (cache fill, model updates, …).
    async fn on_result(&self, ctx: &QueryContext<'_>, result: &ResultSnapshot) -> Result<()> {
        let _ = (ctx, result);
        Ok(())
    }
}
