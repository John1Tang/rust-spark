use rspark_core::error::{Error, Result};
use sqlparser::dialect::GenericDialect;
use sqlparser::parser::Parser;

/// Parse a SQL string into one or more [`sqlparser::ast::Statement`] values.
pub fn parse_sql(sql: &str) -> Result<Vec<sqlparser::ast::Statement>> {
    let dialect = GenericDialect {};
    let trimmed = sql.trim();
    if trimmed.is_empty() {
        return Err(Error::Sql("empty SQL statement".into()));
    }
    let normalized = normalize_trailing_semicolon(trimmed);
    let statements = Parser::parse_sql(&dialect, &normalized)
        .map_err(|e| Error::Sql(format!("syntax error: {e}")))?;
    if statements.is_empty() {
        return Err(Error::Sql("no statements parsed".into()));
    }
    Ok(statements)
}

fn normalize_trailing_semicolon(sql: &str) -> String {
    sql.strip_suffix(';')
        .map(str::to_string)
        .unwrap_or_else(|| sql.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_select() {
        let stmts = parse_sql("SELECT a, b FROM t").unwrap();
        assert_eq!(stmts.len(), 1);
    }

    #[test]
    fn parses_with_where() {
        let stmts = parse_sql("SELECT * FROM t WHERE x > 1").unwrap();
        assert_eq!(stmts.len(), 1);
    }

    #[test]
    fn empty_sql_errors() {
        let err = parse_sql("   ").unwrap_err();
        match err {
            Error::Sql(msg) => assert!(msg.contains("empty")),
            other => panic!("expected sql error, got {other:?}"),
        }
    }
}
