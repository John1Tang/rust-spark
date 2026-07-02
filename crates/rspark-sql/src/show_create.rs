use crate::planner::Catalog;
use rspark_core::error::{Error, Result};
use rspark_core::schema::DataType;
use sqlparser::ast::Statement;
use sqlparser::dialect::GenericDialect;
use sqlparser::parser::Parser;

/// Return a `CREATE TABLE` statement that would recreate `table_name` from
/// the catalog's stored schema. Returns an error if the table is not
/// registered.
pub fn render_create_table(catalog: &dyn Catalog, table_name: &str) -> Result<String> {
    let schema = catalog.table_schema(table_name)?;
    let (path, source) = catalog.table_location(table_name)?;
    let column_lines: Vec<String> = schema
        .fields()
        .iter()
        .map(|f| {
            let nullable = if f.nullable { "" } else { " NOT NULL" };
            format!("  {} {}{}", quote_ident(&f.name), sql_type(&f.data_type), nullable)
        })
        .collect();
    let columns = if column_lines.is_empty() {
        String::new()
    } else {
        format!("(\n{}\n)", column_lines.join(",\n"))
    };
    Ok(format!(
        "CREATE TABLE {} {}\nUSING {}\nLOCATION '{}'",
        quote_ident(table_name),
        columns,
        source,
        path.replace('\'', "''")
    ))
}

fn sql_type(d: &DataType) -> &'static str {
    match d {
        DataType::Null => "NULL",
        DataType::Boolean => "BOOLEAN",
        DataType::Int32 => "INT",
        DataType::Int64 => "BIGINT",
        DataType::Float32 => "FLOAT",
        DataType::Float64 => "DOUBLE",
        DataType::String => "STRING",
        DataType::Date => "DATE",
        DataType::Timestamp => "TIMESTAMP",
    }
}

fn quote_ident(name: &str) -> String {
    if name.is_empty() {
        return "\"\"".into();
    }
    let needs_quote = name
        .chars()
        .next()
        .map(|c| !c.is_ascii_alphabetic() && c != '_')
        .unwrap_or(true)
        || name.chars().any(|c| !c.is_ascii_alphanumeric() && c != '_');
    if needs_quote {
        format!("\"{}\"", name.replace('"', "\"\""))
    } else {
        name.to_string()
    }
}

/// Parse a single statement and return the rendered `CREATE TABLE` for
/// `SHOW CREATE TABLE <name>` if the statement is one; otherwise None.
pub fn try_show_create(sql: &str) -> Result<Option<ShowCreateRequest>> {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    let dialect = GenericDialect {};
    let statements = Parser::parse_sql(&dialect, trimmed)
        .map_err(|e| Error::Sql(format!("syntax error: {e}")))?;
    let Some(stmt) = statements.into_iter().next() else {
        return Ok(None);
    };
    match stmt {
        Statement::ShowCreate {
            obj_type: sqlparser::ast::ShowCreateObject::Table,
            obj_name,
        } => {
            let name = obj_name.to_string();
            Ok(Some(ShowCreateRequest { table_name: name }))
        }
        _ => Ok(None),
    }
}

#[derive(Debug, Clone)]
pub struct ShowCreateRequest {
    pub table_name: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use rspark_core::schema::{Field, Schema};
    use std::collections::HashMap;
    use std::sync::RwLock;

    struct TestCatalog {
        tables: RwLock<HashMap<String, (String, String, Schema)>>,
    }
    impl Catalog for TestCatalog {
        fn table_schema(&self, name: &str) -> Result<Schema> {
            self.tables
                .read()
                .unwrap()
                .get(name)
                .map(|t| t.2.clone())
                .ok_or_else(|| Error::NotFound(format!("table '{name}' not found")))
        }
        fn table_location(&self, name: &str) -> Result<(String, String)> {
            self.tables
                .read()
                .unwrap()
                .get(name)
                .map(|t| (t.0.clone(), t.1.clone()))
                .ok_or_else(|| Error::NotFound(format!("table '{name}' not found")))
        }
        fn list_tables(&self) -> Result<Vec<String>> {
            Ok(self.tables.read().unwrap().keys().cloned().collect())
        }
        fn register_table(
            &mut self,
            name: &str,
            path: &str,
            source: &str,
            schema: Schema,
        ) -> Result<()> {
            self.tables.write().unwrap().insert(
                name.to_string(),
                (path.to_string(), source.to_string(), schema),
            );
            Ok(())
        }
    }

    #[test]
    fn renders_basic_create_table() {
        let cat = TestCatalog {
            tables: RwLock::new(HashMap::from([(
                "employees".to_string(),
                (
                    "/data/employees.csv".to_string(),
                    "csv".to_string(),
                    Schema::new(vec![
                        Field::new("id", DataType::Int64),
                        Field::new("name", DataType::String),
                    ]),
                ),
            )])),
        };
        let ddl = render_create_table(&cat, "employees").unwrap();
        assert!(ddl.contains("CREATE TABLE employees"));
        assert!(ddl.contains("id BIGINT"));
        assert!(ddl.contains("name STRING"));
        assert!(ddl.contains("USING csv"));
        assert!(ddl.contains("LOCATION '/data/employees.csv'"));
    }

    #[test]
    fn quotes_identifiers_with_special_chars() {
        assert_eq!(quote_ident("plain"), "plain");
        assert_eq!(quote_ident("needs space"), "\"needs space\"");
        assert_eq!(quote_ident("with\"quote"), "\"with\"\"quote\"");
        assert_eq!(quote_ident("1leading"), "\"1leading\"");
    }

    #[test]
    fn detects_show_create_table() {
        let req = try_show_create("SHOW CREATE TABLE employees").unwrap();
        assert_eq!(req.unwrap().table_name, "employees");
    }

    #[test]
    fn ignores_other_statements() {
        assert!(try_show_create("SELECT * FROM t").unwrap().is_none());
        assert!(try_show_create("SHOW CREATE VIEW v").unwrap().is_none());
    }

    #[test]
    fn unknown_table_errors() {
        let cat = TestCatalog {
            tables: RwLock::new(HashMap::new()),
        };
        let err = render_create_table(&cat, "ghost").unwrap_err();
        assert!(matches!(err, Error::NotFound(_)));
    }
}