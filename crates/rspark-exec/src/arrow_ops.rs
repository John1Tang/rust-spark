//! Arrow-backed kernels for the operators we can do cleanly column-wise:
//! filter (boolean mask), limit (head/take), sort (lexicographic over a
//! single key), and column subset.
//!
//! Operators that need expression semantics — project, aggregate, join —
//! stay in [`crate::operators`]. The plan is "boundary only": convert at
//! the top of [`crate::executor::LocalExecutor::execute`], convert back
//! at the bottom, and run a few hot ops on Arrow inside.

use std::sync::Arc;

use arrow::array::{Array, BooleanArray, RecordBatch as ArrowRecordBatch, UInt32Array};
use arrow::compute::{filter_record_batch, sort_to_indices, take_record_batch, SortOptions};
use arrow::datatypes::SchemaRef;

use rspark_core::error::{Error, Result};

use crate::arrow_batch::ArrowBatch;

/// Return a new [`ArrowBatch`] containing only the rows where `mask` is
/// true. `mask` must be a `BooleanArray` with the same number of rows
/// as the input.
pub fn filter(batch: &ArrowBatch, mask: &BooleanArray) -> Result<ArrowBatch> {
    if mask.len() != batch.num_rows() {
        return Err(Error::Execution(format!(
            "filter mask length {} != batch rows {}",
            mask.len(),
            batch.num_rows()
        )));
    }
    let out = filter_record_batch(&batch.0, mask)
        .map_err(|e| Error::Execution(format!("arrow filter_record_batch: {e}")))?;
    Ok(ArrowBatch(out))
}

/// Return the first `n` rows of the batch.
pub fn limit(batch: &ArrowBatch, n: usize) -> Result<ArrowBatch> {
    let n = n.min(batch.num_rows());
    if n == 0 {
        // Empty result, preserve schema.
        return Ok(ArrowBatch(ArrowRecordBatch::new_empty(
            batch.arrow_schema(),
        )));
    }
    let indices = UInt32Array::from_iter_values(0..n as u32);
    let out = take_record_batch(&batch.0, &indices)
        .map_err(|e| Error::Execution(format!("arrow take_record_batch: {e}")))?;
    Ok(ArrowBatch(out))
}

/// Sort by a single column index. `ascending` controls direction.
/// Multi-key sorts fall back to the row-wise [`crate::operators::sort_batch`].
pub fn sort_by_column(batch: &ArrowBatch, col_idx: usize, ascending: bool) -> Result<ArrowBatch> {
    if col_idx >= batch.num_cols() {
        return Err(Error::Execution(format!(
            "sort column index {col_idx} out of range (cols={})",
            batch.num_cols()
        )));
    }
    let col = batch.0.column(col_idx).as_ref();
    let opts = SortOptions {
        descending: !ascending,
        nulls_first: true,
    };
    let indices = sort_to_indices(col, Some(opts), None)
        .map_err(|e| Error::Execution(format!("arrow sort_to_indices: {e}")))?;
    let out = take_record_batch(&batch.0, &indices)
        .map_err(|e| Error::Execution(format!("arrow take_record_batch (sort): {e}")))?;
    Ok(ArrowBatch(out))
}

/// Subset of columns. `indices` may be empty (returns a 0-column batch
/// with the original schema minus those fields).
pub fn select_columns(batch: &ArrowBatch, indices: &[usize]) -> Result<ArrowBatch> {
    let schema: SchemaRef = batch.arrow_schema();
    let new_fields: Vec<arrow::datatypes::FieldRef> = indices
        .iter()
        .map(|&i| Arc::new(schema.field(i).as_ref().clone()))
        .collect();
    let new_schema = Arc::new(arrow::datatypes::Schema::new(new_fields));
    let new_cols: Vec<Arc<dyn Array>> =
        indices.iter().map(|&i| batch.0.column(i).clone()).collect();
    let out = ArrowRecordBatch::try_new(new_schema, new_cols)
        .map_err(|e| Error::Execution(format!("arrow select_columns: {e}")))?;
    Ok(ArrowBatch(out))
}

/// Run a [`FnMut(row_idx) -> bool`] predicate as a columnar filter by
/// materializing it to a `BooleanArray` and then calling [`filter`].
/// This is the bridge for the row-wise `eval_predicate`: we don't want
/// to rewrite the whole predicate evaluator, so we evaluate it row at a
/// time into a mask, then do the columnar filter pass.
pub fn filter_via<F>(batch: &ArrowBatch, mut predicate: F) -> Result<ArrowBatch>
where
    F: FnMut(usize) -> Result<bool>,
{
    let mut values = Vec::with_capacity(batch.num_rows());
    let mut validity = Vec::with_capacity(batch.num_rows());
    for row in 0..batch.num_rows() {
        match predicate(row) {
            Ok(b) => {
                values.push(b);
                validity.push(true);
            }
            Err(e) => return Err(e),
        }
    }
    let mask = BooleanArray::from(values);
    // The unused `validity` is what we'd use to keep rows whose predicate
    // errored; we surface the error eagerly above.
    let _ = validity;
    filter(batch, &mask)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arrow_batch::arrow_from_core;
    use arrow_array::cast::AsArray;
    use rspark_core::schema::{DataType, Field, Schema};
    use rspark_core::{Record, RecordBatch};

    fn batch() -> ArrowBatch {
        let schema = Schema::new(vec![
            Field::new("id", DataType::Int64),
            Field::new("name", DataType::String),
        ]);
        let rows = vec![
            Record::new(vec![1i64.into(), "alice".into()]),
            Record::new(vec![2i64.into(), "bob".into()]),
            Record::new(vec![3i64.into(), "carol".into()]),
        ];
        let rb = RecordBatch::from_records(schema, rows).unwrap();
        arrow_from_core(&rb).unwrap()
    }

    #[test]
    fn limit_keeps_first_n_rows() {
        let b = batch();
        let out = limit(&b, 2).unwrap();
        assert_eq!(out.num_rows(), 2);
        // First row's id should still be 1.
        let col = out
            .0
            .column(0)
            .as_primitive::<arrow_array::types::Int64Type>();
        assert_eq!(col.value(0), 1);
        assert_eq!(col.value(1), 2);
    }

    #[test]
    fn filter_via_drops_rows() {
        let b = batch();
        let out = filter_via(&b, |i| Ok(i % 2 == 0)).unwrap();
        assert_eq!(out.num_rows(), 2);
    }

    #[test]
    fn sort_by_column_ascending() {
        let b = batch();
        let out = sort_by_column(&b, 0, true).unwrap();
        let col = out
            .0
            .column(0)
            .as_primitive::<arrow_array::types::Int64Type>();
        assert_eq!((col.value(0), col.value(2)), (1, 3));
    }

    #[test]
    fn select_columns_subset() {
        let b = batch();
        let out = select_columns(&b, &[1]).unwrap();
        assert_eq!(out.num_cols(), 1);
        assert_eq!(out.arrow_schema().field(0).name(), "name");
    }
}
