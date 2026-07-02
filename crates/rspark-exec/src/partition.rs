use rspark_core::schema::Schema;
use rspark_core::RecordBatch;

/// An opaque partition reference (file path slice, in-memory range, …).
/// Workers use these to compute batches in parallel.
#[derive(Debug, Clone)]
pub enum Partition {
    /// Read the entire file as a single partition (used for small inputs).
    WholeFile { path: String },
    /// Read rows `[start, end)` of the CSV file as one partition.
    CsvSlice { path: String, start: u64, end: u64 },
}

impl Partition {
    pub fn label(&self) -> String {
        match self {
            Partition::WholeFile { path } => format!("whole:{path}"),
            Partition::CsvSlice { path, start, end } => {
                format!("slice:{path}[{start}..{end}]")
            }
        }
    }
}

pub struct PartitionedBatch {
    pub partition: Partition,
    pub batch: RecordBatch,
    pub output_schema: Schema,
}
