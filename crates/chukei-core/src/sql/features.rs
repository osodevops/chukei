//! Feature extraction over the AST (PRD §9.1).
//!
//! `QueryFeatures` drives the soft fingerprint, the cache determinism gate,
//! and the router's size heuristics.

use std::collections::BTreeSet;
use std::ops::ControlFlow;

use serde::{Deserialize, Serialize};
use sqlparser::ast::{visit_expressions, visit_relations, Expr, Query, SetExpr, Statement};

/// Functions whose result depends on more than their arguments.
/// A query touching any of these is never cached (PRD §11.1).
const NON_DETERMINISTIC_FNS: &[&str] = &[
    "CURRENT_TIMESTAMP",
    "CURRENT_TIME",
    "CURRENT_DATE",
    "LOCALTIME",
    "LOCALTIMESTAMP",
    "SYSDATE",
    "GETDATE",
    "RANDOM",
    "RANDSTR",
    "UUID_STRING",
    "SEQ1",
    "SEQ2",
    "SEQ4",
    "SEQ8",
    "LAST_QUERY_ID",
];

const AGGREGATE_FNS: &[&str] = &[
    "COUNT",
    "SUM",
    "AVG",
    "MIN",
    "MAX",
    "MEDIAN",
    "STDDEV",
    "VARIANCE",
    "LISTAGG",
    "ARRAY_AGG",
    "APPROX_COUNT_DISTINCT",
    "HLL",
    "PERCENTILE_CONT",
    "PERCENTILE_DISC",
];

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QueryFeatures {
    /// Fully-qualified (as written), upper-cased, sorted, deduplicated.
    pub tables: Vec<String>,
    pub joins_count: usize,
    /// Upper-cased aggregate function names present.
    pub aggregates: Vec<String>,
    /// Leaf comparison predicates in WHERE clauses.
    pub predicates_count: usize,
    pub has_group_by: bool,
    pub has_order_by: bool,
    pub has_limit: bool,
    pub is_select_star: bool,
    /// False when any non-deterministic function is present.
    pub deterministic: bool,
    /// Estimated rows scanned; populated from query_history during replay,
    /// unknown (None) in the live hot path until stats exist.
    pub scan_estimate_rows: Option<u64>,
}

pub fn extract(statement: &Statement) -> QueryFeatures {
    let mut tables = BTreeSet::new();
    let _ = visit_relations(statement, |relation| {
        tables.insert(relation.to_string().to_uppercase());
        ControlFlow::<()>::Continue(())
    });

    let mut aggregates = BTreeSet::new();
    let mut deterministic = true;
    let mut predicates_count = 0usize;
    let _ = visit_expressions(statement, |expr: &Expr| {
        match expr {
            Expr::Function(f) => {
                let name = f
                    .name
                    .0
                    .last()
                    .map(|p| p.to_string().to_uppercase())
                    .unwrap_or_default();
                if NON_DETERMINISTIC_FNS.contains(&name.as_str()) {
                    deterministic = false;
                }
                if AGGREGATE_FNS.contains(&name.as_str()) {
                    aggregates.insert(name);
                }
            }
            Expr::BinaryOp { op, .. } => {
                use sqlparser::ast::BinaryOperator::*;
                if matches!(op, Eq | NotEq | Lt | LtEq | Gt | GtEq) {
                    predicates_count += 1;
                }
            }
            Expr::Like { .. } | Expr::ILike { .. } | Expr::InList { .. } | Expr::Between { .. } => {
                predicates_count += 1;
            }
            _ => {}
        }
        ControlFlow::<()>::Continue(())
    });

    let mut features = QueryFeatures {
        tables: tables.into_iter().collect(),
        aggregates: aggregates.into_iter().collect(),
        predicates_count,
        deterministic,
        ..Default::default()
    };

    if let Statement::Query(query) = statement {
        inspect_query(query, &mut features);
    }
    features
}

fn inspect_query(query: &Query, features: &mut QueryFeatures) {
    features.has_order_by |= query.order_by.is_some();
    features.has_limit |= query.limit_clause.is_some();
    inspect_set_expr(&query.body, features);
}

fn inspect_set_expr(body: &SetExpr, features: &mut QueryFeatures) {
    match body {
        SetExpr::Select(select) => {
            features.joins_count += select.from.iter().map(|t| t.joins.len()).sum::<usize>();
            // Implicit joins: more than one table in FROM.
            features.joins_count += select.from.len().saturating_sub(1);
            features.has_group_by |= !matches!(
                &select.group_by,
                sqlparser::ast::GroupByExpr::Expressions(exprs, mods)
                    if exprs.is_empty() && mods.is_empty()
            );
            features.is_select_star |= select
                .projection
                .iter()
                .any(|p| matches!(p, sqlparser::ast::SelectItem::Wildcard(_)));
        }
        SetExpr::Query(q) => inspect_query(q, features),
        SetExpr::SetOperation { left, right, .. } => {
            inspect_set_expr(left, features);
            inspect_set_expr(right, features);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sql::parse::parse_single;

    fn features(sql: &str) -> QueryFeatures {
        extract(&parse_single(sql).unwrap())
    }

    #[test]
    fn extracts_tables_and_joins() {
        let f = features(
            "SELECT c.id, SUM(o.amount) FROM customers c \
             JOIN orders o ON o.customer_id = c.id \
             WHERE o.ts > '2026-01-01' GROUP BY c.id ORDER BY 2 DESC LIMIT 10",
        );
        assert_eq!(f.tables, vec!["CUSTOMERS", "ORDERS"]);
        assert_eq!(f.joins_count, 1);
        assert_eq!(f.aggregates, vec!["SUM"]);
        assert!(f.has_group_by && f.has_order_by && f.has_limit);
        assert!(f.deterministic);
        assert!(f.predicates_count >= 1);
    }

    #[test]
    fn select_star_detected() {
        assert!(features("SELECT * FROM t").is_select_star);
        assert!(!features("SELECT a FROM t").is_select_star);
    }

    #[test]
    fn non_determinism_detected() {
        assert!(!features("SELECT UUID_STRING() FROM t").deterministic);
        assert!(!features("SELECT * FROM t WHERE ts < CURRENT_TIMESTAMP()").deterministic);
        assert!(features("SELECT a FROM t").deterministic);
    }
}
