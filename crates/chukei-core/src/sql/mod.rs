//! SQL AST pipeline (PRD §9):
//!
//! ```text
//! raw_sql → parse (SnowflakeDialect) → normalise/parameterise
//!         → features → hard fingerprint (blake3) + soft fingerprint
//! ```
//!
//! Everything here is deterministic and synchronous — this module sits on
//! the hot path and must respect the +5 ms p99 budget. LLMs never run here.

pub mod features;
pub mod fingerprint;
pub mod hints;
pub mod normalise;
pub mod parse;

use sqlparser::ast::Statement;

pub use features::QueryFeatures;
pub use hints::Hints;

/// The full analysis of one SQL statement, produced once per query and
/// shared (immutably) with every plugin via `QueryContext`.
#[derive(Debug, Clone)]
pub struct QueryAnalysis {
    pub raw_sql: String,
    pub statement: Statement,
    /// Parameterised, whitespace-normalised SQL.
    pub canonical_sql: String,
    /// Literal values extracted during parameterisation, in visit order.
    /// Hashed into the hard fingerprint: same shape + same literals = same
    /// result identity.
    pub literals: Vec<String>,
    pub hard_fingerprint: [u8; 32],
    pub soft_fingerprint: [u8; 16],
    pub features: QueryFeatures,
    pub hints: Hints,
}

/// Run the whole pipeline over one statement.
///
/// Multi-statement payloads are rejected: the proxy submits one statement per
/// query-request, matching Snowflake driver behaviour.
pub fn analyze(raw_sql: &str) -> crate::Result<QueryAnalysis> {
    let statement = parse::parse_single(raw_sql)?;
    let (canonical, literals) = normalise::canonicalise(&statement);
    let features = features::extract(&statement);
    let hard = fingerprint::hard(&canonical, &literals);
    let soft = fingerprint::soft(&features);
    Ok(QueryAnalysis {
        raw_sql: raw_sql.to_string(),
        statement,
        canonical_sql: canonical,
        literals,
        hard_fingerprint: hard,
        soft_fingerprint: soft,
        features,
        hints: hints::parse(raw_sql),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whitespace_and_case_variants_share_hard_fingerprint() {
        let a = analyze("SELECT id, name FROM customers WHERE region = 'EMEA'").unwrap();
        let b = analyze("select id,   name from CUSTOMERS where region='EMEA'").unwrap();
        assert_eq!(a.hard_fingerprint, b.hard_fingerprint);
        assert_eq!(a.soft_fingerprint, b.soft_fingerprint);
    }

    #[test]
    fn different_literals_share_soft_but_not_hard_fingerprint() {
        let a = analyze("SELECT id, name FROM customers WHERE region = 'EMEA'").unwrap();
        let b = analyze("SELECT id, name FROM customers WHERE region = 'APAC'").unwrap();
        // Different literals → different results → must never share a cache
        // key — but they do cluster for workload analysis.
        assert_ne!(a.hard_fingerprint, b.hard_fingerprint);
        assert_eq!(a.soft_fingerprint, b.soft_fingerprint);
        assert_eq!(a.canonical_sql, b.canonical_sql);
    }

    #[test]
    fn different_queries_differ() {
        let a = analyze("SELECT id FROM customers").unwrap();
        let b = analyze("SELECT id FROM orders").unwrap();
        assert_ne!(a.hard_fingerprint, b.hard_fingerprint);
        assert_ne!(a.soft_fingerprint, b.soft_fingerprint);
    }

    #[test]
    fn non_deterministic_query_flagged() {
        let a = analyze("SELECT id, CURRENT_TIMESTAMP() FROM events").unwrap();
        assert!(!a.features.deterministic);
        let b = analyze("SELECT id, name FROM events").unwrap();
        assert!(b.features.deterministic);
    }
}
