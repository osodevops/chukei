//! Thin wrapper around sqlparser-rs with the Snowflake dialect.

use sqlparser::ast::Statement;
use sqlparser::dialect::SnowflakeDialect;
use sqlparser::parser::Parser;

use crate::error::{Error, Result};

/// Parse a payload that must contain exactly one statement.
pub fn parse_single(sql: &str) -> Result<Statement> {
    let mut statements = parse_all(sql)?;
    match statements.len() {
        1 => Ok(statements.remove(0)),
        n => Err(Error::SqlParse(format!(
            "expected exactly 1 statement, got {n}"
        ))),
    }
}

pub fn parse_all(sql: &str) -> Result<Vec<Statement>> {
    Parser::parse_sql(&SnowflakeDialect {}, sql).map_err(|e| Error::SqlParse(e.to_string()))
}

/// True if the statement is a pure read — P0 only ever intercepts reads;
/// everything else passes through untouched (PRD §3.3: not a write-path tool).
pub fn is_read_only(statement: &Statement) -> bool {
    matches!(statement, Statement::Query(_))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_snowflake_flavoured_sql() {
        // QUALIFY is Snowflake-specific.
        let sql = "SELECT id, ROW_NUMBER() OVER (PARTITION BY id ORDER BY ts) AS rn \
                   FROM events QUALIFY rn = 1";
        assert!(parse_single(sql).is_ok());
    }

    #[test]
    fn rejects_multi_statement() {
        assert!(parse_single("SELECT 1; SELECT 2").is_err());
    }

    #[test]
    fn read_only_detection() {
        assert!(is_read_only(&parse_single("SELECT 1").unwrap()));
        assert!(!is_read_only(
            &parse_single("INSERT INTO t VALUES (1)").unwrap()
        ));
    }
}
