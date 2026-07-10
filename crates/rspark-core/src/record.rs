use crate::error::{Error, Result};
use crate::schema::Schema;
use crate::value::Value;
use serde::{Deserialize, Serialize};

/// A typed row of values aligned to a [`Schema`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Record {
    values: Vec<Value>,
}

impl Record {
    pub fn new(values: Vec<Value>) -> Self {
        Self { values }
    }

    pub fn values(&self) -> &[Value] {
        &self.values
    }

    pub fn get(&self, index: usize) -> Option<&Value> {
        self.values.get(index)
    }

    pub fn get_by_name(&self, schema: &Schema, name: &str) -> Option<&Value> {
        schema.index_of(name).and_then(|idx| self.values.get(idx))
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    pub fn push(&mut self, value: Value) {
        self.values.push(value);
    }

    pub fn validate(&self, schema: &Schema) -> Result<()> {
        if self.values.len() != schema.field_count() {
            return Err(Error::Schema(format!(
                "record has {} fields but schema expects {}",
                self.values.len(),
                schema.field_count()
            )));
        }
        for (idx, (value, field)) in self.values.iter().zip(schema.fields()).enumerate() {
            if value.is_null() && !field.nullable {
                return Err(Error::Schema(format!(
                    "field '{}' at index {idx} is null but declared not null",
                    field.name
                )));
            }
        }
        Ok(())
    }
}

/// A collection of records sharing a single schema — the in-memory analogue of an Arrow `RecordBatch`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordBatch {
    schema: Schema,
    records: Vec<Record>,
}

impl RecordBatch {
    pub fn new(schema: Schema) -> Self {
        Self {
            schema,
            records: Vec::new(),
        }
    }

    pub fn from_records(schema: Schema, records: Vec<Record>) -> Result<Self> {
        for record in &records {
            record.validate(&schema)?;
        }
        Ok(Self { schema, records })
    }

    pub fn schema(&self) -> &Schema {
        &self.schema
    }

    pub fn records(&self) -> &[Record] {
        &self.records
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    pub fn push(&mut self, record: Record) -> Result<()> {
        record.validate(&self.schema)?;
        self.records.push(record);
        Ok(())
    }

    pub fn extend(&mut self, other: RecordBatch) -> Result<()> {
        if self.schema != other.schema {
            return Err(Error::Schema(
                "cannot extend RecordBatch with mismatched schema".into(),
            ));
        }
        self.records.extend(other.records);
        Ok(())
    }

    pub fn into_records(self) -> Vec<Record> {
        self.records
    }

    pub fn into_schema(self) -> Schema {
        self.schema
    }

    pub fn iter(&self) -> std::slice::Iter<'_, Record> {
        self.records.iter()
    }

    pub fn memory_bytes(&self) -> usize {
        self.records
            .iter()
            .map(|r| {
                r.values()
                    .iter()
                    .map(|v| match v {
                        Value::Null => 8,
                        Value::Boolean(_) => 8,
                        Value::Int32(_) => 8,
                        Value::Int64(_) => 8,
                        Value::Float32(_) => 8,
                        Value::Float64(_) => 8,
                        Value::String(s) => 24 + s.len(),
                    })
                    .sum::<usize>()
            })
            .sum()
    }
}
