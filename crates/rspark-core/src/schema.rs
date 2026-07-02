use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DataType {
    Boolean,
    Int32,
    Int64,
    Float32,
    Float64,
    String,
    Date,
    Timestamp,
    Null,
}

impl DataType {
    pub fn name(&self) -> &'static str {
        match self {
            DataType::Boolean => "boolean",
            DataType::Int32 => "int",
            DataType::Int64 => "bigint",
            DataType::Float32 => "float",
            DataType::Float64 => "double",
            DataType::String => "string",
            DataType::Date => "date",
            DataType::Timestamp => "timestamp",
            DataType::Null => "null",
        }
    }

    pub fn spark_name(&self) -> &'static str {
        match self {
            DataType::Boolean => "BOOLEAN",
            DataType::Int32 => "INT",
            DataType::Int64 => "BIGINT",
            DataType::Float32 => "FLOAT",
            DataType::Float64 => "DOUBLE",
            DataType::String => "STRING",
            DataType::Date => "DATE",
            DataType::Timestamp => "TIMESTAMP",
            DataType::Null => "VOID",
        }
    }

    pub fn is_numeric(&self) -> bool {
        matches!(
            self,
            DataType::Int32 | DataType::Int64 | DataType::Float32 | DataType::Float64
        )
    }
}

impl fmt::Display for DataType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl FromStr for DataType {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        match s.to_ascii_uppercase().as_str() {
            "BOOLEAN" | "BOOL" => Ok(DataType::Boolean),
            "INT" | "INTEGER" | "INT32" => Ok(DataType::Int32),
            "BIGINT" | "INT64" | "LONG" => Ok(DataType::Int64),
            "FLOAT" | "FLOAT32" => Ok(DataType::Float32),
            "DOUBLE" | "FLOAT64" => Ok(DataType::Float64),
            "STRING" | "VARCHAR" | "TEXT" => Ok(DataType::String),
            "DATE" => Ok(DataType::Date),
            "TIMESTAMP" => Ok(DataType::Timestamp),
            "VOID" | "NULL" | "NONE" => Ok(DataType::Null),
            other => Err(Error::Type(format!("unknown data type: {other}"))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Field {
    pub name: String,
    pub data_type: DataType,
    pub nullable: bool,
}

impl Field {
    pub fn new(name: impl Into<String>, data_type: DataType) -> Self {
        Self {
            name: name.into(),
            data_type,
            nullable: true,
        }
    }

    pub fn not_null(name: impl Into<String>, data_type: DataType) -> Self {
        Self {
            name: name.into(),
            data_type,
            nullable: false,
        }
    }

    pub fn with_nullable(mut self, nullable: bool) -> Self {
        self.nullable = nullable;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Schema {
    fields: Vec<Field>,
}

impl Schema {
    pub fn new(fields: Vec<Field>) -> Self {
        Self { fields }
    }

    pub fn empty() -> Self {
        Self { fields: vec![] }
    }

    pub fn fields(&self) -> &[Field] {
        &self.fields
    }

    pub fn field_names(&self) -> Vec<&str> {
        self.fields.iter().map(|f| f.name.as_str()).collect()
    }

    pub fn field_count(&self) -> usize {
        self.fields.len()
    }

    pub fn field(&self, name: &str) -> Option<&Field> {
        self.fields.iter().find(|f| f.name == name)
    }

    pub fn index_of(&self, name: &str) -> Option<usize> {
        self.fields.iter().position(|f| f.name == name)
    }

    pub fn try_merge(self, other: Schema) -> Result<Schema> {
        let mut fields = self.fields;
        fields.extend(other.fields);
        Ok(Schema { fields })
    }
}

impl fmt::Display for Schema {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let parts: Vec<String> = self
            .fields
            .iter()
            .map(|fld| format!("{}:{}", fld.name, fld.data_type.spark_name()))
            .collect();
        write!(f, "struct<{}>", parts.join(", "))
    }
}
