//! Columnar boundary between `rspark_core::RecordBatch` (the public
//! row format) and `arrow::array::RecordBatch` (the columnar internal
//! representation).
//!
//! The user picked "Arrow as a columnar boundary only": the SQL layer
//! keeps `Value`/`Record`/`RecordBatch` as the public type, and the
//! physical operators see [`ArrowBatch`] between ops. The conversion is
//! lossless for the seven `Value` variants we ship; `Date`/`Timestamp`
//! fall through to `StringArray` because we don't have an Arrow
//! temporal type roundtrip yet.

use std::sync::Arc;

use arrow_array::builder::{
    BooleanBuilder, Float32Builder, Float64Builder, Int32Builder, Int64Builder, StringBuilder,
};
use arrow_array::cast::AsArray;
use arrow_array::types::{Float32Type, Float64Type};
use arrow_array::{
    Array, BooleanArray, Int32Array, Int64Array, NullArray, RecordBatch as ArrowRecordBatch,
    StringArray,
};
use arrow_schema::{DataType as ArrowDataType, Field as ArrowField, Schema as ArrowSchema};

use rspark_core::error::{Error, Result};
use rspark_core::schema::{DataType, Field, Schema};
use rspark_core::{Record, RecordBatch, Value};

/// Columnar wrapper around `arrow::array::RecordBatch`. Internal-only;
/// the public APIs in [`rspark_exec`] still return [`RecordBatch`].
#[derive(Debug, Clone)]
pub struct ArrowBatch(pub ArrowRecordBatch);

impl ArrowBatch {
    pub fn num_rows(&self) -> usize {
        self.0.num_rows()
    }

    pub fn num_cols(&self) -> usize {
        self.0.num_columns()
    }

    /// Borrow the underlying Arrow schema.
    pub fn arrow_schema(&self) -> Arc<ArrowSchema> {
        self.0.schema()
    }

    /// Build a core [`Schema`] from this batch's Arrow schema. We use
    /// the Arrow type as the source of truth; if a column maps cleanly
    /// to one of our seven native types, use that. Anything else falls
    /// back to `DataType::String`.
    pub fn core_schema(&self) -> Schema {
        let fields = self
            .0
            .schema()
            .fields()
            .iter()
            .map(|f| {
                let dt = match f.data_type() {
                    ArrowDataType::Boolean => DataType::Boolean,
                    ArrowDataType::Int32 => DataType::Int32,
                    ArrowDataType::Int64 => DataType::Int64,
                    ArrowDataType::Float32 => DataType::Float32,
                    ArrowDataType::Float64 => DataType::Float64,
                    ArrowDataType::Utf8 | ArrowDataType::LargeUtf8 => DataType::String,
                    _ => DataType::String,
                };
                Field {
                    name: f.name().clone(),
                    data_type: dt,
                    nullable: f.is_nullable(),
                }
            })
            .collect();
        Schema::new(fields)
    }
}

/// Build an Arrow schema from a core [`Schema`].
pub fn arrow_schema_from_core(schema: &Schema) -> Result<Arc<ArrowSchema>> {
    let fields: Result<Vec<ArrowField>> = schema
        .fields()
        .iter()
        .map(|f| {
            let dt = match f.data_type {
                DataType::Boolean => ArrowDataType::Boolean,
                DataType::Int32 => ArrowDataType::Int32,
                DataType::Int64 => ArrowDataType::Int64,
                DataType::Float32 => ArrowDataType::Float32,
                DataType::Float64 => ArrowDataType::Float64,
                DataType::String | DataType::Date | DataType::Timestamp => ArrowDataType::Utf8,
                DataType::Null => ArrowDataType::Null,
            };
            Ok(ArrowField::new(&f.name, dt, f.nullable))
        })
        .collect();
    Ok(Arc::new(ArrowSchema::new(fields?)))
}

/// Convert a core [`RecordBatch`] into an [`ArrowBatch`].
///
/// We build typed Arrow builders from the field types, then walk each
/// record and append a value per column. A `Value::Null` always maps
/// to a null in the corresponding Arrow column — nullable is set per
/// field, but every builder in this code path accepts nulls because
/// row format encodes nullability at the value level.
pub fn arrow_from_core(batch: &RecordBatch) -> Result<ArrowBatch> {
    let schema = arrow_schema_from_core(batch.schema())?;
    let mut builders: Vec<Box<dyn ArrayBuilder>> = Vec::with_capacity(batch.schema().field_count());
    for field in batch.schema().fields() {
        builders.push(make_builder(field)?);
    }
    for record in batch.records() {
        for (idx, value) in record.values().iter().enumerate() {
            builders[idx].append_value(value);
        }
    }
    let columns: Vec<Arc<dyn Array>> = builders.into_iter().map(|mut b| b.finish()).collect();
    let rb = ArrowRecordBatch::try_new(schema, columns)
        .map_err(|e| Error::Execution(format!("arrow RecordBatch build failed: {e}")))?;
    Ok(ArrowBatch(rb))
}

/// Convert an [`ArrowBatch`] back into a core [`RecordBatch`]. Every
/// row becomes a `Record` of `Value`s; the output schema is the Arrow
/// schema translated back through [`ArrowBatch::core_schema`].
pub fn arrow_to_core(batch: &ArrowBatch) -> Result<RecordBatch> {
    let schema = batch.core_schema();
    let mut records = Vec::with_capacity(batch.num_rows());
    for row in 0..batch.num_rows() {
        let mut values = Vec::with_capacity(batch.num_cols());
        for col_idx in 0..batch.num_cols() {
            values.push(value_at(batch.0.column(col_idx).as_ref(), row)?);
        }
        records.push(Record::new(values));
    }
    RecordBatch::from_records(schema, records)
}

trait ArrayBuilder {
    fn append_value(&mut self, v: &Value);
    fn finish(&mut self) -> Arc<dyn Array>;
}

struct BoolBuilder(BooleanBuilder);
impl ArrayBuilder for BoolBuilder {
    fn append_value(&mut self, v: &Value) {
        match v {
            Value::Boolean(b) => self.0.append_value(*b),
            Value::Null => self.0.append_null(),
            other => self.0.append_value(other.cast_to_bool().unwrap_or(false)),
        }
    }
    fn finish(&mut self) -> Arc<dyn Array> {
        Arc::new(self.0.finish())
    }
}

struct I32Builder(Int32Builder);
impl ArrayBuilder for I32Builder {
    fn append_value(&mut self, v: &Value) {
        match v {
            Value::Int32(i) => self.0.append_value(*i),
            Value::Null => self.0.append_null(),
            other => self
                .0
                .append_value(other.cast_to_i64().unwrap_or(0).try_into().unwrap_or(0)),
        }
    }
    fn finish(&mut self) -> Arc<dyn Array> {
        Arc::new(self.0.finish())
    }
}

struct I64Builder(Int64Builder);
impl ArrayBuilder for I64Builder {
    fn append_value(&mut self, v: &Value) {
        match v {
            Value::Int64(i) => self.0.append_value(*i),
            Value::Null => self.0.append_null(),
            other => self.0.append_value(other.cast_to_i64().unwrap_or(0)),
        }
    }
    fn finish(&mut self) -> Arc<dyn Array> {
        Arc::new(self.0.finish())
    }
}

struct F32Builder(Float32Builder);
impl ArrayBuilder for F32Builder {
    fn append_value(&mut self, v: &Value) {
        match v {
            Value::Float32(f) => self.0.append_value(*f),
            Value::Null => self.0.append_null(),
            other => self
                .0
                .append_value(other.cast_to_f64().unwrap_or(0.0) as f32),
        }
    }
    fn finish(&mut self) -> Arc<dyn Array> {
        Arc::new(self.0.finish())
    }
}

struct F64Builder(Float64Builder);
impl ArrayBuilder for F64Builder {
    fn append_value(&mut self, v: &Value) {
        match v {
            Value::Float64(f) => self.0.append_value(*f),
            Value::Null => self.0.append_null(),
            other => self.0.append_value(other.cast_to_f64().unwrap_or(0.0)),
        }
    }
    fn finish(&mut self) -> Arc<dyn Array> {
        Arc::new(self.0.finish())
    }
}

struct StrBuilder(StringBuilder);
impl ArrayBuilder for StrBuilder {
    fn append_value(&mut self, v: &Value) {
        match v {
            Value::String(s) => self.0.append_value(s),
            Value::Null => self.0.append_null(),
            other => self.0.append_value(other.cast_to_string()),
        }
    }
    fn finish(&mut self) -> Arc<dyn Array> {
        Arc::new(self.0.finish())
    }
}

struct NullColumn;
impl ArrayBuilder for NullColumn {
    fn append_value(&mut self, _v: &Value) {
        // Always null; nothing to track.
    }
    fn finish(&mut self) -> Arc<dyn Array> {
        Arc::new(NullArray::new(0))
    }
}

fn make_builder(field: &Field) -> Result<Box<dyn ArrayBuilder>> {
    match field.data_type {
        DataType::Boolean => Ok(Box::new(BoolBuilder(BooleanBuilder::new()))),
        DataType::Int32 => Ok(Box::new(I32Builder(Int32Builder::new()))),
        DataType::Int64 => Ok(Box::new(I64Builder(Int64Builder::new()))),
        DataType::Float32 => Ok(Box::new(F32Builder(Float32Builder::new()))),
        DataType::Float64 => Ok(Box::new(F64Builder(Float64Builder::new()))),
        DataType::String | DataType::Date | DataType::Timestamp => {
            Ok(Box::new(StrBuilder(StringBuilder::new())))
        }
        DataType::Null => Ok(Box::new(NullColumn)),
    }
}

fn value_at(col: &dyn Array, row: usize) -> Result<Value> {
    if col.is_null(row) {
        return Ok(Value::Null);
    }
    match col.data_type() {
        ArrowDataType::Boolean => {
            let arr = col
                .as_any()
                .downcast_ref::<BooleanArray>()
                .ok_or_else(|| Error::Execution("arrow downcast to BooleanArray failed".into()))?;
            Ok(Value::Boolean(arr.value(row)))
        }
        ArrowDataType::Int32 => {
            let arr = col
                .as_any()
                .downcast_ref::<Int32Array>()
                .ok_or_else(|| Error::Execution("arrow downcast to Int32Array failed".into()))?;
            Ok(Value::Int32(arr.value(row)))
        }
        ArrowDataType::Int64 => {
            let arr = col
                .as_any()
                .downcast_ref::<Int64Array>()
                .ok_or_else(|| Error::Execution("arrow downcast to Int64Array failed".into()))?;
            Ok(Value::Int64(arr.value(row)))
        }
        ArrowDataType::Float32 => {
            let arr = col.as_primitive::<Float32Type>();
            Ok(Value::Float32(arr.value(row)))
        }
        ArrowDataType::Float64 => {
            let arr = col.as_primitive::<Float64Type>();
            Ok(Value::Float64(arr.value(row)))
        }
        ArrowDataType::Utf8 => {
            let arr = col
                .as_any()
                .downcast_ref::<StringArray>()
                .ok_or_else(|| Error::Execution("arrow downcast to StringArray failed".into()))?;
            Ok(Value::String(arr.value(row).to_string()))
        }
        ArrowDataType::Null => Ok(Value::Null),
        _ => Err(Error::Execution(format!(
            "unsupported arrow type in value_at: {:?}",
            col.data_type()
        ))),
    }
}
