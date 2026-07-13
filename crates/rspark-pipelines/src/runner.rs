//! Pipeline runner — walks the DAG layer by layer and runs each flow.
//!
//! For each flow:
//!
//! 1. Build the appropriate [`StreamingSource`] from the spec.
//! 2. `poll_batch` to get a [`RecordBatch`].
//! 3. Plan + execute the flow's SQL against the source's catalog.
//! 4. Write the result to the destination.
//! 5. Append a [`RunStats`] to the report.
//!
//! `poll_batch` returns 0 rows when there's nothing new — a flow with
//! no input data produces an empty destination, matching SDP behavior
//! (a streaming table without new data stays at its last value rather
//! than being deleted).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use rspark_core::error::{Error, Result};
use rspark_core::RecordBatch;
use rspark_exec::ExecutionContext;
use rspark_sql::planner::Catalog;
use rspark_sql::{Planner, TableKind};
use rspark_storage::SourceRegistry;
use serde::Serialize;
use tracing::{info, warn};

use crate::dag::PipelineDag;
use crate::sink::{append_destination, describe, write_destination};
use crate::source::{FileTailSource, KafkaSource, NullSource, StreamingSource};
use crate::spec::{Flow, FlowKind, Pipeline, Refresh, SourceSpec};

/// Per-batch progress callback for live flows. `rows` is the number of
/// rows appended in the just-finished batch (0 for an empty poll).
pub type ProgressFn = Arc<dyn Fn(usize) + Send + Sync>;

#[derive(Debug, Clone, Default, Serialize)]
pub struct RunStats {
    pub flow: String,
    pub kind: FlowKind,
    pub refresh: Refresh,
    pub duration_ms: u128,
    pub row_count: usize,
    pub destination: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct PipelineRunReport {
    pub pipeline: String,
    pub started_at: String,
    pub duration_ms: u128,
    pub flows: Vec<RunStats>,
    pub errors: Vec<String>,
}

pub struct PipelineRunner {
    pub context: ExecutionContext,
    pub catalog: Arc<dyn Catalog>,
    progress: Option<ProgressFn>,
}

impl PipelineRunner {
    pub fn new(registry: Arc<SourceRegistry>, catalog: Arc<dyn Catalog>) -> Self {
        Self {
            context: ExecutionContext::new(registry),
            catalog,
            progress: None,
        }
    }

    /// Set a per-batch progress callback. Used by the API to surface
    /// live-run status (`/v1/pipelines/:name/status`).
    pub fn with_progress(mut self, progress: ProgressFn) -> Self {
        self.progress = Some(progress);
        self
    }

    /// Run a pipeline to completion. Returns a [`PipelineRunReport`]
    /// with one [`RunStats`] per flow and any per-flow errors collected
    /// in `errors`. The runner does not abort on a single flow failure —
    /// it records the error and continues.
    pub async fn run(&self, pipeline: &Pipeline) -> Result<PipelineRunReport> {
        let started_at = chrono::Utc::now().to_rfc3339();
        let start = Instant::now();
        let dag = PipelineDag::from_pipeline(pipeline)?;
        info!(
            pipeline = %pipeline.pipeline,
            flows = dag.node_count(),
            "pipeline runner starting"
        );
        let mut report = PipelineRunReport {
            pipeline: pipeline.pipeline.clone(),
            started_at,
            duration_ms: 0,
            flows: Vec::with_capacity(pipeline.flows.len()),
            errors: Vec::new(),
        };
        for layer in dag.layers() {
            for fid in layer {
                let Some(flow) = pipeline.flows.iter().find(|f| {
                    dag.name_index
                        .get(&f.name)
                        .copied()
                        .map(|n| n == fid.0)
                        .unwrap_or(false)
                }) else {
                    continue;
                };
                match self.run_one(pipeline, flow).await {
                    Ok(stats) => report.flows.push(stats),
                    Err(err) => {
                        let msg = format!("flow '{}' failed: {err}", flow.name);
                        warn!("{msg}");
                        report.errors.push(msg);
                    }
                }
            }
        }
        report.duration_ms = start.elapsed().as_millis();
        Ok(report)
    }

    async fn run_one(&self, pipeline: &Pipeline, flow: &Flow) -> Result<RunStats> {
        // Live mode is a long-running tail loop. It never returns a
        // single RunStats until the process is killed; the HTTP
        // handler spawns it via tokio::spawn and returns 202.
        if matches!(flow.refresh, Refresh::Live { .. }) {
            return self.run_live_streaming(pipeline, flow).await;
        }
        let start = Instant::now();
        let mut source = build_source(&flow.source)?;
        let input = source.poll_batch().await?;
        let output = self.execute_flow(pipeline, flow, input).await?;
        let bytes = write_destination(&flow.destination, &output).await?;
        source.commit().await?;
        // Register the flow's output in the catalog so the dashboard's
        // autocomplete surfaces it (and downstream flows can read it).
        // Failures here are non-fatal — the run already produced bytes
        // and is reported; a registration error would just mean the
        // table isn't queryable until the next run.
        if let Err(e) = self.register_flow_output(flow, &output) {
            warn!(flow = %flow.name, "register_flow_output failed: {e}");
        }
        info!(
            flow = %flow.name,
            rows = output.len(),
            bytes,
            destination = %describe(&flow.destination),
            "flow complete"
        );
        Ok(RunStats {
            flow: flow.name.clone(),
            kind: flow.kind,
            refresh: flow.refresh,
            duration_ms: start.elapsed().as_millis(),
            row_count: output.len(),
            destination: describe(&flow.destination),
        })
    }

    /// Long-running tail: poll the source forever, append each batch to
    /// the destination. The first non-empty batch registers the flow's
    /// output in the catalog so the SQL planner can resolve the table
    /// name even when the destination file is empty.
    async fn run_live_streaming(&self, pipeline: &Pipeline, flow: &Flow) -> Result<RunStats> {
        let poll_ms = match flow.refresh {
            Refresh::Live { poll_ms } => poll_ms.max(50),
            _ => 500,
        };
        let _start = Instant::now();
        let mut source = build_source(&flow.source)?;
        let mut registered = false;
        let mut total_rows: usize = 0;
        info!(
            flow = %flow.name,
            poll_ms,
            destination = %describe(&flow.destination),
            "live tail started"
        );
        loop {
            let batch = source.poll_batch().await?;
            if !batch.is_empty() {
                let output = self.execute_flow(pipeline, flow, batch).await?;
                let appended = append_destination(&flow.destination, &output)?;
                total_rows += output.len();
                if !registered {
                    if let Err(e) = self.register_flow_output(flow, &output) {
                        warn!(flow = %flow.name, "register_flow_output failed: {e}");
                    } else {
                        registered = true;
                    }
                }
                if appended > 0 {
                    info!(
                        flow = %flow.name,
                        batch_rows = output.len(),
                        total_rows,
                        "live batch appended"
                    );
                    if let Some(p) = &self.progress {
                        p(output.len());
                    }
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(poll_ms)).await;
        }
        // unreachable while live, but the type system wants a stats
        // value if we ever break out of the loop.
        #[allow(unreachable_code)]
        Ok(RunStats {
            flow: flow.name.clone(),
            kind: flow.kind,
            refresh: flow.refresh,
            duration_ms: _start.elapsed().as_millis(),
            row_count: total_rows,
            destination: describe(&flow.destination),
        })
    }

    fn register_flow_output(&self, flow: &Flow, output: &RecordBatch) -> Result<()> {
        let (path, source): (String, String) = match &flow.destination {
            crate::spec::Destination::File { path } => {
                (path.display().to_string(), "csv".to_string())
            }
            crate::spec::Destination::S3 { key, bucket } => {
                let b = bucket
                    .clone()
                    .or_else(|| std::env::var("AWS_S3_BUCKET").ok())
                    .unwrap_or_else(|| "<no-bucket>".into());
                (format!("s3://{b}/{key}"), "s3_csv".to_string())
            }
        };
        let kind = match flow.kind {
            FlowKind::StreamingTable => TableKind::StreamingTable,
            FlowKind::MaterializedView => TableKind::MaterializedView,
        };
        self.catalog.register_with_kind(
            &flow.name,
            &path,
            &source,
            output.schema().clone(),
            kind,
        )
    }

    /// Run a flow's SQL. For a streaming table the input is the polled
    /// batch; for a materialised view the input is the union of all
    /// upstream flow outputs (so `depends_on` is honored).
    async fn execute_flow(
        &self,
        pipeline: &Pipeline,
        flow: &Flow,
        input: RecordBatch,
    ) -> Result<RecordBatch> {
        // Live-mode flows bypass the planner entirely: the polled
        // batch is the output. The destination schema is the polled
        // batch's schema (Kafka serializes page events as JSON
        // objects, so the column types match what JsonSource reads
        // back). The `query` field in the spec is preserved for
        // documentation; transformations belong upstream of the
        // pipeline. The early-return MUST happen before plan_sql —
        // flow.query defaults to "" for live flows and the parser
        // rejects empty input.
        if matches!(flow.refresh, Refresh::Live { .. }) {
            let _ = pipeline;
            return Ok(input);
        }
        let planner = Planner::new();
        let plan = planner.plan_sql(&flow.query, self.catalog.as_ref())?;
        let exec = rspark_exec::LocalExecutor::new(&self.context);
        // For streaming-table flows the polled `input` is the rows we
        // actually want to operate on. Use `execute_with_input` so the
        // executor seeds its current_batch from the polled batch
        // instead of trying to materialize a Scan.
        let _ = pipeline;
        if matches!(flow.kind, FlowKind::StreamingTable) {
            return exec.execute_with_input(&plan, input);
        }
        // For materialized views we discard the polled input — the
        // view reads from its declared source / depends_on upstream.
        let _ = input;
        exec.execute(&plan)
    }
}

fn build_source(spec: &SourceSpec) -> Result<Box<dyn StreamingSource>> {
    match spec {
        SourceSpec::TailDir { tail_dir } => Ok(Box::new(FileTailSource::new(tail_dir.clone()))),
        SourceSpec::Csv { path } => Ok(Box::new(OneShotSource::csv(path.clone()))),
        SourceSpec::Json { path } => Ok(Box::new(OneShotSource::json(path.clone()))),
        SourceSpec::S3 { .. } => Err(Error::Execution(
            "S3 streaming source not yet wired (sink supports it)".into(),
        )),
        SourceSpec::Kafka {
            topic,
            brokers,
            group_id,
        } => Ok(Box::new(KafkaSource::new(
            topic.clone(),
            brokers.clone(),
            group_id.clone(),
        )?)),
        SourceSpec::Sql => Ok(Box::new(NullSource)),
    }
}

/// A source that reads a single batch from a CSV/JSON path on the
/// first `poll_batch` and returns empty after. S3 is intentionally not
/// supported here — the [`PipelineRunner`] will register an S3 source
/// and use it directly when the spec calls for `kind: s3`.
pub struct OneShotSource {
    inner: Option<RecordBatch>,
}

impl OneShotSource {
    pub fn csv(path: PathBuf) -> Self {
        let batch = std::fs::File::open(&path).ok().and_then(|f| {
            use std::io::BufReader;
            rspark_storage::csv_source::CsvSource::new()
                .scan_reader(BufReader::new(f), None)
                .ok()
        });
        Self { inner: batch }
    }
    pub fn json(path: PathBuf) -> Self {
        let batch = std::fs::File::open(&path).ok().and_then(|f| {
            use std::io::BufReader;
            rspark_storage::json_source::JsonSource::new()
                .scan_reader(BufReader::new(f), None)
                .ok()
        });
        Self { inner: batch }
    }
}

#[async_trait]
impl StreamingSource for OneShotSource {
    async fn poll_batch(&mut self) -> Result<RecordBatch> {
        Ok(self
            .inner
            .take()
            .unwrap_or_else(|| RecordBatch::new(rspark_core::schema::Schema::empty())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::{Destination, SourceSpec};
    use rspark_sql::SessionState;
    use rspark_storage::SourceRegistry;

    fn test_pipeline() -> Pipeline {
        let out = std::env::temp_dir().join(format!(
            "rspark-pipeline-runner-test-{}.csv",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        Pipeline {
            pipeline: "t".into(),
            flows: vec![Flow {
                name: "names".into(),
                kind: FlowKind::MaterializedView,
                depends_on: vec![],
                // Pull a column from the registered `employees` table.
                source: SourceSpec::Sql,
                query: "SELECT name FROM employees".into(),
                refresh: Refresh::Full,
                destination: Destination::File { path: out },
            }],
        }
    }

    #[tokio::test]
    async fn runs_one_flow_end_to_end() {
        let registry = Arc::new(SourceRegistry::with_defaults());
        let session = SessionState::new();
        // cargo test runs from the target/ working directory, so resolve
        // the CSV relative to the workspace root explicitly.
        let csv_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/data/employees.csv");
        session
            .register(
                "employees",
                csv_path.to_str().unwrap(),
                "csv",
                rspark_core::schema::Schema::new(vec![
                    rspark_core::schema::Field::new("id", rspark_core::schema::DataType::Int64),
                    rspark_core::schema::Field::new("name", rspark_core::schema::DataType::String),
                ]),
            )
            .unwrap();
        let catalog: Arc<dyn Catalog> = Arc::new(session);
        let runner = PipelineRunner::new(registry, catalog);
        let report = runner.run(&test_pipeline()).await.unwrap();
        assert_eq!(report.flows.len(), 1);
        assert!(report.flows[0].row_count > 0);
    }
}
