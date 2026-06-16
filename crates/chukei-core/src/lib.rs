//! chukei-core — the library behind `chukeid` / the `chukei` CLI.
//!
//! A transparent, wire-protocol-level proxy for Snowflake that caches, routes,
//! rewrites, right-sizes, suspends, and attributes — without client changes.

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

/// Install the process-default rustls crypto provider. With both ring
/// (reqwest) and aws-lc-rs (axum-server) in the dependency graph nothing is
/// installed implicitly, and the failure mode is a panic deep inside any
/// code path that builds a rustls config (the proxy acceptor, but also
/// `doctor`'s TLS probe). Idempotent; call once at process start.
pub fn init_crypto() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}
