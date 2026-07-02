use crate::planner::Catalog;
use rspark_core::error::{Error, Result};
use rspark_core::schema::{DataType, Field, Schema};
use std::collections::HashMap;
use std::sync::RwLock;

/// Simple in-memory catalog mapping table names to (path, source format, schema).
pub struct SessionState {
    tables: RwLock<HashMap<String, (String, String, Schema)>>,
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
        self.tables
            .write()
            .map_err(|e| Error::InvalidState(format!("catalog lock poisoned: {e}")))?
            .insert(name.to_string(), (path.to_string(), source.to_string(), schema));
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
            .map(|t| t.2.clone())
            .ok_or_else(|| Error::NotFound(format!("table '{name}' not found")))
    }
    fn table_location(&self, name: &str) -> Result<(String, String)> {
        self.tables
            .read()
            .map_err(|e| Error::InvalidState(format!("catalog lock poisoned: {e}")))?
            .get(name)
            .map(|t| (t.0.clone(), t.1.clone()))
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
}
