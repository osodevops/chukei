//! chukei hint comments (PRD §11.4, §11.6):
//!
//! ```sql
//! /*+ chukei:team=growth dag=daily_revenue tolerance=approximate size=L */
//! SELECT ...
//! ```
//!
//! Hints ride inside an ordinary SQL comment so they survive every driver
//! and are invisible to Snowflake. Parsed from the raw text because the
//! parser discards comments.

use std::collections::BTreeMap;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Hints {
    pub values: BTreeMap<String, String>,
}

impl Hints {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(String::as_str)
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}

/// Extract `key=value` pairs from every `/*+ chukei:... */` comment.
pub fn parse(raw_sql: &str) -> Hints {
    let mut values = BTreeMap::new();
    let mut rest = raw_sql;
    while let Some(start) = rest.find("/*+") {
        let after = &rest[start + 3..];
        let Some(end) = after.find("*/") else { break };
        let body = after[..end].trim();
        if let Some(spec) = body.strip_prefix("chukei:") {
            for pair in spec.split_whitespace() {
                if let Some((k, v)) = pair.split_once('=') {
                    if !k.is_empty() && !v.is_empty() {
                        values.insert(k.to_lowercase(), v.to_string());
                    }
                }
            }
        }
        rest = &after[end + 2..];
    }
    Hints { values }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hint_comment() {
        let h = parse("/*+ chukei:team=growth dag=daily_revenue */ SELECT 1");
        assert_eq!(h.get("team"), Some("growth"));
        assert_eq!(h.get("dag"), Some("daily_revenue"));
    }

    #[test]
    fn ignores_plain_hints_and_no_hints() {
        assert!(parse("/*+ parallel(4) */ SELECT 1").is_empty());
        assert!(parse("SELECT 1").is_empty());
    }

    #[test]
    fn multiple_comments_merge() {
        let h = parse("/*+ chukei:team=a */ SELECT 1 /*+ chukei:size=L */");
        assert_eq!(h.get("team"), Some("a"));
        assert_eq!(h.get("size"), Some("L"));
    }
}
