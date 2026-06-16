//! The Tier-1 rule pack (PRD §11.3). Five of the ten P0 rules are live;
//! the rest land as the pack grows — the trait makes each rule an
//! independent, testable unit.

use std::ops::ControlFlow;

use sqlparser::ast::{
    visit_expressions_mut, BinaryOperator, DuplicateTreatment, Expr, Function, FunctionArg,
    FunctionArgExpr, FunctionArgumentList, FunctionArguments, Ident, ObjectName, ObjectNamePart,
    SelectItem, SetExpr, Statement, Value,
};

use super::RewriteRule;
use crate::sql::Hints;

fn fn_call(name: &str, args: Vec<Expr>) -> Expr {
    Expr::Function(Function {
        name: ObjectName(vec![ObjectNamePart::Identifier(Ident::new(name))]),
        uses_odbc_syntax: false,
        parameters: FunctionArguments::None,
        args: FunctionArguments::List(FunctionArgumentList {
            duplicate_treatment: None,
            args: args
                .into_iter()
                .map(|e| FunctionArg::Unnamed(FunctionArgExpr::Expr(e)))
                .collect(),
            clauses: vec![],
        }),
        filter: None,
        null_treatment: None,
        over: None,
        within_group: vec![],
    })
}

fn string_literal(s: &str) -> Expr {
    Expr::Value(Value::SingleQuotedString(s.to_string()).into())
}

/// Rule 3: `COUNT(DISTINCT x)` → `APPROX_COUNT_DISTINCT(x)`, only when the
/// query is hinted `/*+ chukei:tolerance=approximate */`.
pub struct ApproxCountDistinct;

impl RewriteRule for ApproxCountDistinct {
    fn name(&self) -> &'static str {
        "approx_count_distinct"
    }

    fn apply(&self, statement: &mut Statement, hints: &Hints) -> bool {
        if hints.get("tolerance") != Some("approximate") {
            return false;
        }
        let mut changed = false;
        let _ = visit_expressions_mut(statement, |expr: &mut Expr| {
            if let Expr::Function(f) = expr {
                let is_count = f
                    .name
                    .0
                    .last()
                    .is_some_and(|p| p.to_string().eq_ignore_ascii_case("COUNT"));
                if is_count {
                    if let FunctionArguments::List(list) = &mut f.args {
                        if list.duplicate_treatment == Some(DuplicateTreatment::Distinct)
                            && list.args.len() == 1
                        {
                            f.name = ObjectName(vec![ObjectNamePart::Identifier(Ident::new(
                                "APPROX_COUNT_DISTINCT",
                            ))]);
                            list.duplicate_treatment = None;
                            changed = true;
                        }
                    }
                }
            }
            ControlFlow::<()>::Continue(())
        });
        changed
    }
}

/// Rule 7: `SELECT DISTINCT a, b … GROUP BY a, b` — the DISTINCT is
/// redundant when every selected expression appears in the GROUP BY.
pub struct EliminateRedundantDistinct;

impl RewriteRule for EliminateRedundantDistinct {
    fn name(&self) -> &'static str {
        "eliminate_redundant_distinct"
    }

    fn apply(&self, statement: &mut Statement, _hints: &Hints) -> bool {
        let Statement::Query(query) = statement else {
            return false;
        };
        let SetExpr::Select(select) = query.body.as_mut() else {
            return false;
        };
        if select.distinct.is_none() {
            return false;
        }
        let sqlparser::ast::GroupByExpr::Expressions(group_exprs, mods) = &select.group_by else {
            return false;
        };
        if group_exprs.is_empty() || !mods.is_empty() {
            return false;
        }
        let group_set: Vec<String> = group_exprs.iter().map(|e| e.to_string()).collect();
        let all_grouped = select.projection.iter().all(|item| match item {
            SelectItem::UnnamedExpr(e) => group_set.contains(&e.to_string()),
            SelectItem::ExprWithAlias { expr, .. } => group_set.contains(&expr.to_string()),
            _ => false,
        });
        if all_grouped {
            select.distinct = None;
            true
        } else {
            false
        }
    }
}

/// Rule 5: `c = 'a' OR c = 'b' OR c = 'c'` → `c IN ('a', 'b', 'c')`.
pub struct OrChainToInList;

impl OrChainToInList {
    /// Collect `col = literal` (or already-folded `col IN (literals)`)
    /// leaves of an OR tree; false if any leaf doesn't fit the shape.
    /// The visitor rewrites bottom-up, so inner OR pairs may already be
    /// IN-lists by the time the outer OR is visited — merge them.
    fn collect(expr: &Expr, col: &mut Option<(String, Expr)>, values: &mut Vec<Expr>) -> bool {
        match expr {
            Expr::BinaryOp {
                left,
                op: BinaryOperator::Or,
                right,
            } => Self::collect(left, col, values) && Self::collect(right, col, values),
            Expr::BinaryOp {
                left,
                op: BinaryOperator::Eq,
                right,
            } => {
                let (Expr::Identifier(_) | Expr::CompoundIdentifier(_)) = left.as_ref() else {
                    return false;
                };
                if !matches!(right.as_ref(), Expr::Value(_)) {
                    return false;
                }
                Self::accept(col, left, std::iter::once(right.as_ref().clone()), values)
            }
            Expr::InList {
                expr: list_col,
                list,
                negated: false,
            } => {
                let (Expr::Identifier(_) | Expr::CompoundIdentifier(_)) = list_col.as_ref() else {
                    return false;
                };
                if !list.iter().all(|e| matches!(e, Expr::Value(_))) {
                    return false;
                }
                Self::accept(col, list_col, list.iter().cloned(), values)
            }
            Expr::Nested(inner) => Self::collect(inner, col, values),
            _ => false,
        }
    }

    fn accept(
        col: &mut Option<(String, Expr)>,
        column: &Expr,
        new_values: impl Iterator<Item = Expr>,
        values: &mut Vec<Expr>,
    ) -> bool {
        let name = column.to_string();
        match col {
            Some((existing, _)) if *existing != name => false,
            _ => {
                if col.is_none() {
                    *col = Some((name, column.clone()));
                }
                values.extend(new_values);
                true
            }
        }
    }
}

impl RewriteRule for OrChainToInList {
    fn name(&self) -> &'static str {
        "or_chain_to_in_list"
    }

    fn apply(&self, statement: &mut Statement, _hints: &Hints) -> bool {
        let mut changed = false;
        let _ = visit_expressions_mut(statement, |expr: &mut Expr| {
            if matches!(
                expr,
                Expr::BinaryOp {
                    op: BinaryOperator::Or,
                    ..
                }
            ) {
                let mut col = None;
                let mut values = Vec::new();
                if Self::collect(expr, &mut col, &mut values) && values.len() >= 2 {
                    let (_, column) = col.expect("collect sets col when it returns true");
                    *expr = Expr::InList {
                        expr: Box::new(column),
                        list: values,
                        negated: false,
                    };
                    changed = true;
                }
            }
            ControlFlow::<()>::Continue(())
        });
        changed
    }
}

/// Rule 8: `c LIKE 'foo%'` (prefix-only pattern) → `STARTSWITH(c, 'foo')`,
/// which can prune on clustering metadata where LIKE cannot.
pub struct LikePrefixToStartswith;

impl RewriteRule for LikePrefixToStartswith {
    fn name(&self) -> &'static str {
        "like_prefix_to_startswith"
    }

    fn apply(&self, statement: &mut Statement, _hints: &Hints) -> bool {
        let mut changed = false;
        let _ = visit_expressions_mut(statement, |expr: &mut Expr| {
            if let Expr::Like {
                negated: false,
                expr: col,
                pattern,
                escape_char: None,
                ..
            } = expr
            {
                if let Expr::Value(v) = pattern.as_ref() {
                    if let Value::SingleQuotedString(p) = &v.value {
                        if let Some(prefix) = p.strip_suffix('%') {
                            let clean = !prefix.contains('%') && !prefix.contains('_');
                            if clean && !prefix.is_empty() {
                                *expr = fn_call(
                                    "STARTSWITH",
                                    vec![col.as_ref().clone(), string_literal(prefix)],
                                );
                                changed = true;
                            }
                        }
                    }
                }
            }
            ControlFlow::<()>::Continue(())
        });
        changed
    }
}

/// Rule 9: `c IS NULL OR c = ''` → `IFNULL(c, '') = ''` — one predicate
/// instead of two, and sargable on the IFNULL expression.
pub struct NullOrEmptyToIfnull;

impl NullOrEmptyToIfnull {
    fn matches(expr: &Expr) -> Option<Expr> {
        let Expr::BinaryOp {
            left,
            op: BinaryOperator::Or,
            right,
        } = expr
        else {
            return None;
        };
        let (is_null_side, eq_side) = match (left.as_ref(), right.as_ref()) {
            (Expr::IsNull(c), other) => (c, other),
            (other, Expr::IsNull(c)) => (c, other),
            _ => return None,
        };
        let Expr::BinaryOp {
            left: eq_col,
            op: BinaryOperator::Eq,
            right: eq_val,
        } = eq_side
        else {
            return None;
        };
        let empty = matches!(
            eq_val.as_ref(),
            Expr::Value(v) if matches!(&v.value, Value::SingleQuotedString(s) if s.is_empty())
        );
        if empty && eq_col.to_string() == is_null_side.to_string() {
            Some(is_null_side.as_ref().clone())
        } else {
            None
        }
    }
}

impl RewriteRule for NullOrEmptyToIfnull {
    fn name(&self) -> &'static str {
        "null_or_empty_to_ifnull"
    }

    fn apply(&self, statement: &mut Statement, _hints: &Hints) -> bool {
        let mut changed = false;
        let _ = visit_expressions_mut(statement, |expr: &mut Expr| {
            if let Some(col) = Self::matches(expr) {
                *expr = Expr::BinaryOp {
                    left: Box::new(fn_call("IFNULL", vec![col, string_literal("")])),
                    op: BinaryOperator::Eq,
                    right: Box::new(string_literal("")),
                };
                changed = true;
            }
            ControlFlow::<()>::Continue(())
        });
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sql::{hints, parse::parse_single};

    fn apply(rule: &dyn RewriteRule, sql: &str, hint_sql: &str) -> Option<String> {
        let mut stmt = parse_single(sql).unwrap();
        let h = hints::parse(hint_sql);
        rule.apply(&mut stmt, &h).then(|| stmt.to_string())
    }

    #[test]
    fn approx_count_distinct_requires_hint() {
        let sql = "SELECT COUNT(DISTINCT user_id) FROM events";
        assert_eq!(apply(&ApproxCountDistinct, sql, sql), None);
        let out = apply(
            &ApproxCountDistinct,
            sql,
            "/*+ chukei:tolerance=approximate */",
        )
        .unwrap();
        assert!(out.contains("APPROX_COUNT_DISTINCT(user_id)"), "{out}");
        assert!(!out.to_uppercase().contains("DISTINCT USER_ID"), "{out}");
    }

    #[test]
    fn redundant_distinct_removed_only_when_fully_grouped() {
        let out = apply(
            &EliminateRedundantDistinct,
            "SELECT DISTINCT a, b FROM t GROUP BY a, b",
            "",
        )
        .unwrap();
        assert!(!out.contains("DISTINCT"), "{out}");
        // b not in GROUP BY → DISTINCT is load-bearing, keep it.
        assert_eq!(
            apply(
                &EliminateRedundantDistinct,
                "SELECT DISTINCT a, b FROM t GROUP BY a",
                ""
            ),
            None
        );
    }

    #[test]
    fn or_chain_collapses_to_in_list() {
        let out = apply(
            &OrChainToInList,
            "SELECT * FROM t WHERE region = 'a' OR region = 'b' OR region = 'c'",
            "",
        )
        .unwrap();
        assert!(out.contains("region IN ('a', 'b', 'c')"), "{out}");
        // Mixed columns must not collapse.
        assert_eq!(
            apply(&OrChainToInList, "SELECT * FROM t WHERE a = 1 OR b = 2", ""),
            None
        );
    }

    #[test]
    fn like_prefix_becomes_startswith() {
        let out = apply(
            &LikePrefixToStartswith,
            "SELECT * FROM t WHERE name LIKE 'foo%'",
            "",
        )
        .unwrap();
        assert!(out.contains("STARTSWITH(name, 'foo')"), "{out}");
        // Wildcard mid-pattern → not a pure prefix, leave alone.
        assert_eq!(
            apply(
                &LikePrefixToStartswith,
                "SELECT * FROM t WHERE name LIKE 'f%o%'",
                ""
            ),
            None
        );
        assert_eq!(
            apply(
                &LikePrefixToStartswith,
                "SELECT * FROM t WHERE name LIKE '%foo'",
                ""
            ),
            None
        );
    }

    #[test]
    fn null_or_empty_collapses() {
        let out = apply(
            &NullOrEmptyToIfnull,
            "SELECT * FROM t WHERE email IS NULL OR email = ''",
            "",
        )
        .unwrap();
        assert!(out.contains("IFNULL(email, '') = ''"), "{out}");
        // Different columns → no rewrite.
        assert_eq!(
            apply(
                &NullOrEmptyToIfnull,
                "SELECT * FROM t WHERE a IS NULL OR b = ''",
                ""
            ),
            None
        );
    }
}
