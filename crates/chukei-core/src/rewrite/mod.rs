//! ③ `plug-rewrite` — Tier-1 deterministic SQL rewrites (PRD §11.3).
//!
//! Each rule is an AST→AST transform that must be provably
//! semantics-preserving (or gated behind an explicit tolerance hint, as with
//! `approx_count_distinct`). Tier-2 (LLM-assisted) never runs here — it only
//! ever *adds rules to this pack* via the chukei-lab cold path.

mod rules;

use async_trait::async_trait;
use sqlparser::ast::Statement;

use crate::config::RewriteConfig;
use crate::plugin::{Decision, Plugin, QueryContext};
use crate::sql::Hints;
use crate::Result;

/// One deterministic rewrite rule.
pub trait RewriteRule: Send + Sync {
    fn name(&self) -> &'static str;
    /// Apply in place; return true if the statement changed.
    fn apply(&self, statement: &mut Statement, hints: &Hints) -> bool;
}

pub fn all_rules() -> Vec<Box<dyn RewriteRule>> {
    vec![
        Box::new(rules::ApproxCountDistinct),
        Box::new(rules::EliminateRedundantDistinct),
        Box::new(rules::OrChainToInList),
        Box::new(rules::LikePrefixToStartswith),
        Box::new(rules::NullOrEmptyToIfnull),
    ]
}

pub struct RewritePlugin {
    rules: Vec<Box<dyn RewriteRule>>,
}

impl RewritePlugin {
    pub fn new(config: RewriteConfig) -> Self {
        let selected: Vec<Box<dyn RewriteRule>> =
            if config.rules.is_empty() || config.rules.iter().any(|r| r == "all") {
                all_rules()
            } else {
                all_rules()
                    .into_iter()
                    .filter(|r| config.rules.iter().any(|name| name == r.name()))
                    .collect()
            };
        Self { rules: selected }
    }

    /// Run the pack over one statement; returns the rewritten SQL and the
    /// names of the rules that fired, if anything changed.
    pub fn rewrite(
        &self,
        statement: &Statement,
        hints: &Hints,
    ) -> Option<(String, Vec<&'static str>)> {
        let mut stmt = statement.clone();
        let mut fired = Vec::new();
        for rule in &self.rules {
            if rule.apply(&mut stmt, hints) {
                fired.push(rule.name());
            }
        }
        if fired.is_empty() {
            None
        } else {
            Some((stmt.to_string(), fired))
        }
    }
}

#[async_trait]
impl Plugin for RewritePlugin {
    fn name(&self) -> &'static str {
        "rewrite"
    }
    fn order(&self) -> i32 {
        30
    }

    async fn decide(&self, ctx: &QueryContext<'_>) -> Result<Decision> {
        if !crate::sql::parse::is_read_only(&ctx.analysis.statement) {
            return Ok(Decision::Passthrough);
        }
        match self.rewrite(&ctx.analysis.statement, &ctx.analysis.hints) {
            Some((sql, fired)) => {
                let mut tags = crate::plugin::QueryTags::new();
                tags.insert("chukei.rewrite_rules".into(), fired.join(","));
                Ok(Decision::Compose(vec![
                    Decision::Rewrite(sql),
                    Decision::Annotate(tags),
                ]))
            }
            None => Ok(Decision::Passthrough),
        }
    }
}
