//! `rspark-pipeline` CLI: read a YAML spec, run it, print the report.

use clap::Parser;
use rspark_pipelines::Pipeline;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Debug, Parser)]
#[command(name = "rspark-pipeline", about = "Run a declarative pipeline spec")]
struct Args {
    /// Path to the pipeline YAML file.
    file: PathBuf,
}

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();
    let args = Args::parse();
    let pipeline =
        Pipeline::from_file(&args.file).map_err(|e| anyhow::anyhow!("spec error: {e}"))?;
    let registry = Arc::new(rspark_storage::SourceRegistry::with_defaults());
    let _ = rspark_storage::s3_source::try_register_s3(&registry).await;
    let catalog: Arc<dyn rspark_sql::planner::Catalog> = Arc::new(EmptyCatalog);
    let runner = rspark_pipelines::PipelineRunner::new(registry, catalog);
    let report = runner
        .run(&pipeline)
        .await
        .map_err(|e| anyhow::anyhow!("run failed: {e}"))?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

struct EmptyCatalog;
impl rspark_sql::planner::Catalog for EmptyCatalog {
    fn table_schema(&self, _name: &str) -> rspark_core::error::Result<rspark_core::schema::Schema> {
        Err(rspark_core::error::Error::NotFound("no tables".into()))
    }
    fn table_location(&self, _name: &str) -> rspark_core::error::Result<(String, String)> {
        Err(rspark_core::error::Error::NotFound("no tables".into()))
    }
    fn list_tables(&self) -> rspark_core::error::Result<Vec<String>> {
        Ok(vec![])
    }
    fn register_table(
        &mut self,
        _name: &str,
        _path: &str,
        _source: &str,
        _schema: rspark_core::schema::Schema,
    ) -> rspark_core::error::Result<()> {
        Ok(())
    }
}
