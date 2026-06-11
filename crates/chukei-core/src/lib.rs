//! chukei-core — the library behind `chukeid` / the `chukei` CLI.
//!
//! A transparent, wire-protocol-level proxy for Snowflake (and, later,
//! Databricks SQL) that caches, routes, rewrites, right-sizes, suspends,
//! and attributes — without client changes.

pub mod attribute;
pub mod cache;
pub mod circuit_breaker;
pub mod config;
pub mod error;
pub mod evidence;
pub mod metrics;
pub mod plugin;
pub mod replay;
pub mod rewrite;
pub mod router;
pub mod savings;
pub mod sql;
pub mod suspend;
pub mod wire;

pub use error::{Error, Result};
