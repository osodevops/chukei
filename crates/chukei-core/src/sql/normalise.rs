//! Canonicalisation: literals → placeholders, stable rendering, case-folded.
//!
//! The canonical form is what the hard fingerprint hashes. Two queries that
//! differ only in literal values, whitespace, or keyword/identifier case
//! must canonicalise identically. (Snowflake folds unquoted identifiers, so
//! case-folding the rendered text is safe; quoted identifiers are rare in
//! generated SQL and a collision there only costs a spurious cache miss.)

use std::ops::ControlFlow;

use sqlparser::ast::{visit_expressions_mut, Expr, Statement, Value};

/// Render the statement with every literal replaced by a `?` placeholder,
/// then fold case and collapse whitespace. Returns the canonical text plus
/// the extracted literals **in visit order**.
///
/// The literal vector is part of the result's identity: two queries with
/// the same canonical text but different literals return different data, so
/// the hard fingerprint must hash both (a cache keyed on canonical text
/// alone serves wrong results — the one failure mode chukei must never
/// have). The canonical text alone is still useful for clustering.
pub fn canonicalise(statement: &Statement) -> (String, Vec<String>) {
    let mut stmt = statement.clone();
    let mut literals = Vec::new();
    let _ = visit_expressions_mut(&mut stmt, |expr: &mut Expr| {
        if let Expr::Value(v) = expr {
            literals.push(v.value.to_string());
            v.value = Value::Placeholder("?".to_string());
        }
        ControlFlow::<()>::Continue(())
    });
    let rendered = stmt.to_string();
    let canonical = rendered
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    (canonical, literals)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sql::parse::parse_single;

    #[test]
    fn literals_are_parameterised_and_extracted() {
        let stmt = parse_single("SELECT a FROM t WHERE b = 'x' AND c = 42").unwrap();
        let (canon, literals) = canonicalise(&stmt);
        assert!(!canon.contains("'x'"), "canon: {canon}");
        assert!(!canon.contains("42"), "canon: {canon}");
        assert!(canon.contains('?'), "canon: {canon}");
        assert_eq!(literals, vec!["'x'".to_string(), "42".to_string()]);
    }

    #[test]
    fn whitespace_and_case_insensitive() {
        let a = canonicalise(&parse_single("SELECT  a\nFROM   t").unwrap());
        let b = canonicalise(&parse_single("select a from T").unwrap());
        assert_eq!(a, b);
    }

    #[test]
    fn literal_extraction_order_is_stable() {
        let sql = "SELECT a FROM t WHERE b = 'x' AND c IN (1, 2, 3)";
        let (_, a) = canonicalise(&parse_single(sql).unwrap());
        let (_, b) = canonicalise(&parse_single(sql).unwrap());
        assert_eq!(a, b);
        assert_eq!(a.len(), 4);
    }
}
