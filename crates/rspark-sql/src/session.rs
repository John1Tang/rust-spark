use crate::planner::Catalog;
use crate::table_kind::TableKind;
use rspark_core::error::{Error, Result};
use rspark_core::schema::{DataType, Field, Schema};
use std::collections::HashMap;
use std::sync::RwLock;

/// Simple in-memory catalog mapping table names to
/// `(path, source, schema, kind)`. `kind` defaults to `Batch`; pipeline
/// flow outputs are registered as `StreamingTable` / `MaterializedView`.
pub struct SessionState {
    tables: RwLock<HashMap<String, TableEntry>>,
}

pub struct TableEntry {
    pub path: String,
    pub source: String,
    pub schema: Schema,
    pub kind: TableKind,
}

impl Default for SessionState {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionState {
    pub fn new() -> Self {
        Self {
            tables: RwLock::new(HashMap::new()),
        }
    }

    pub fn register(
        &self,
        name: &str,
        path: &str,
        source: &str,
        schema: Schema,
    ) -> Result<()> {
        self.register_with_kind(name, path, source, schema, TableKind::Batch)
    }

    pub fn register_with_kind(
        &self,
        name: &str,
        path: &str,
        source: &str,
        schema: Schema,
        kind: TableKind,
    ) -> Result<()> {
        self.tables
            .write()
            .map_err(|e| Error::InvalidState(format!("catalog lock poisoned: {e}")))?
            .insert(
                name.to_string(),
                TableEntry {
                    path: path.to_string(),
                    source: source.to_string(),
                    schema,
                    kind,
                },
            );
        Ok(())
    }

    pub fn unregister(&self, name: &str) -> Result<()> {
        self.tables
            .write()
            .map_err(|e| Error::InvalidState(format!("catalog lock poisoned: {e}")))?
            .remove(name);
        Ok(())
    }

    pub fn register_inferred(
        &self,
        name: &str,
        path: &str,
        source: &str,
        field_names: &[String],
    ) -> Result<()> {
        let fields = field_names
            .iter()
            .map(|n| Field::new(n.clone(), DataType::String))
            .collect();
        self.register(name, path, source, Schema::new(fields))
    }
}

impl Catalog for SessionState {
    fn table_schema(&self, name: &str) -> Result<Schema> {
        self.tables
            .read()
            .map_err(|e| Error::InvalidState(format!("catalog lock poisoned: {e}")))?
            .get(name)
            .map(|t| t.schema.clone())
            .ok_or_else(|| Error::NotFound(format!("table '{name}' not found")))
    }
    fn table_location(&self, name: &str) -> Result<(String, String)> {
        self.tables
            .read()
            .map_err(|e| Error::InvalidState(format!("catalog lock poisoned: {e}")))?
            .get(name)
            .map(|t| (t.path.clone(), t.source.clone()))
            .ok_or_else(|| Error::NotFound(format!("table '{name}' not found")))
    }
    fn list_tables(&self) -> Result<Vec<String>> {
        Ok(self
            .tables
            .read()
            .map_err(|e| Error::InvalidState(format!("catalog lock poisoned: {e}")))?
            .keys()
            .cloned()
            .collect())
    }
    fn register_table(
        &mut self,
        name: &str,
        path: &str,
        source: &str,
        schema: Schema,
    ) -> Result<()> {
        self.register(name, path, source, schema)
    }
    fn table_kind(&self, name: &str) -> Result<TableKind> {
        self.tables
            .read()
            .map_err(|e| Error::InvalidState(format!("catalog lock poisoned: {e}")))?
            .get(name)
            .map(|t| t.kind)
            .ok_or_else(|| Error::NotFound(format!("table '{name}' not found")))
    }
    fn list_tables_with_kind(&self) -> Result<Vec<(String, TableKind)>> {
        Ok(self
            .tables
            .read()
            .map_err(|e| Error::InvalidState(format!("catalog lock poisoned: {e}")))?
            .iter()
            .map(|(n, t)| (n.clone(), t.kind))
            .collect())
    }
    fn register_with_kind(
        &self,
        name: &str,
        path: &str,
        source: &str,
        schema: Schema,
        kind: TableKind,
    ) -> Result<()> {
        SessionState::register_with_kind(self, name, path, source, schema, kind)
    }
}
