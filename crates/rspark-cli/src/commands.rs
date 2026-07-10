use crate::cli::{Cli, Command};
use crate::repl::start_repl;
use rspark_api::run_server;
use rspark_cluster::job::JobRequest;
use rspark_cluster::master::Master;
use rspark_cluster::state::{ClusterState, WorkerInfo};
use rspark_core::error::Result;
use rspark_exec::{ExecutionContext, LocalExecutor};
use rspark_sql::Planner;
use rspark_sql::SessionState;
use rspark_storage::writer::{render_table, OutputWriter};
use rspark_storage::SourceRegistry;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use tracing::info;

pub async fn dispatch(cli: Cli) -> Result<()> {
    match cli.command {
        Command::Master {
            api_addr,
            dashboard_addr,
            master_id,
            load,
            examples,
        } => run_master(api_addr, dashboard_addr, master_id, load, examples).await,
        Command::Worker {
            master,
            bind,
            cores,
            memory_mb,
        } => run_worker(master, bind, cores, memory_mb).await,
        Command::Sql {
            file,
            sql,
            input_format,
            input,
            catalog,
            output,
        } => run_sql(file, sql, input_format, input, catalog, output),
        Command::Submit {
            master,
            file,
            name,
            parallelism,
        } => run_submit(master, file, name, parallelism).await,
        Command::Shell {
            input_format,
            input,
        } => run_shell(input_format, input).await,
        Command::Dashboard { addr, master } => run_dashboard(addr, master).await,
    }
}

async fn run_master(
    api_addr: String,
    dashboard_addr: String,
    master_id: String,
    load: Vec<String>,
    examples: bool,
) -> Result<()> {
    let state = ClusterState::new(master_id);
    let master = Arc::new(Master::new(state.clone()));
    let catalog = Arc::new(SessionState::new());

    if examples {
        let registry = SourceRegistry::with_defaults();
        let _ = rspark_storage::s3_source::try_register_s3(&registry).await;
        for (name, path) in [
            ("employees", "examples/data/employees.csv"),
            ("sales", "examples/data/sales.csv"),
            ("events", "examples/data/events.json"),
        ] {
            let source = if path.ends_with(".json") {
                "json"
            } else {
                "csv"
            };
            if let Ok(src) = registry.get(source) {
                if let Ok(schema) = src.infer_schema(path) {
                    let _ = catalog.register(name, path, source, schema);
                }
            }
        }
    }
    for spec in &load {
        if let Some((name, path)) = spec.split_once('=') {
            let lower = path.to_ascii_lowercase();
            let source = if lower.ends_with(".json") {
                "json"
            } else {
                "csv"
            };
            let registry = SourceRegistry::with_defaults();
            let _ = rspark_storage::s3_source::try_register_s3(&registry).await;
            if let Ok(src) = registry.get(source) {
                if let Ok(schema) = src.infer_schema(path) {
                    let _ = catalog.register(name, path, source, schema);
                }
            }
        }
    }

    let api_addr: SocketAddr = api_addr
        .parse()
        .map_err(|e| rspark_core::error::Error::InvalidState(format!("bad api addr: {e}")))?;
    let dashboard_addr: SocketAddr = dashboard_addr
        .parse()
        .map_err(|e| rspark_core::error::Error::InvalidState(format!("bad dashboard addr: {e}")))?;
    let api_master = master.clone();
    let api_catalog = catalog.clone();
    let api_task = tokio::spawn(async move {
        if let Err(err) = run_server(api_addr, api_master, api_catalog).await {
            tracing::error!(?err, "api server failed");
        }
    });
    let dash_master = master.clone();
    let dash_catalog = catalog.clone();
    let dash_task = tokio::spawn(async move {
        if let Err(err) =
            rspark_dashboard::run_dashboard(dashboard_addr, dash_master, dash_catalog).await
        {
            tracing::error!(?err, "dashboard server failed");
        }
    });
    info!("rspark master ready: api={api_addr}, dashboard=http://{dashboard_addr}");
    let _ = tokio::join!(api_task, dash_task);
    Ok(())
}

async fn run_worker(master: String, bind: String, cores: usize, memory_mb: usize) -> Result<()> {
    let registry = Arc::new(SourceRegistry::with_defaults());
    let _ = rspark_storage::s3_source::try_register_s3(&registry).await;
    let _context = ExecutionContext::new(registry.clone());
    let info = WorkerInfo::new(bind, cores, memory_mb);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .map_err(|e| rspark_core::error::Error::Network(e.to_string()))?;
    let register_url = format!("{}/v1/workers", master.trim_end_matches('/'));

    // Retry registration until the master is reachable. Workers used
    // to fail-fast on the first network error, which made rolling
    // upgrades of the master a race: workers that started before the
    // new master came up would exit. The retry keeps them in the pool.
    let mut backoff_ms = 200u64;
    let registered: WorkerInfo = loop {
        match client.post(&register_url).json(&info).send().await {
            Ok(resp) if resp.status().is_success() => match resp.json().await {
                Ok(w) => break w,
                Err(e) => {
                    tracing::warn!(error = %e, "register response not JSON; retrying");
                }
            },
            Ok(resp) => {
                tracing::warn!(status = %resp.status(), "registration failed; retrying");
            }
            Err(e) => {
                tracing::warn!(error = %e, "registration network error; retrying");
            }
        }
        sleep(Duration::from_millis(backoff_ms)).await;
        backoff_ms = (backoff_ms * 2).min(5_000);
    };
    info!(id=%registered.id, %master, "worker registered");
    let worker_id = registered.id.clone();
    loop {
        let url = format!(
            "{}/v1/workers/{}/task",
            master.trim_end_matches('/'),
            worker_id
        );
        let poll = client.get(&url).send().await;
        match poll {
            Ok(resp) if resp.status().as_u16() == 200 => {
                match resp.json::<rspark_cluster::task::Task>().await {
                    Ok(task) => {
                        info!(task_id=%task.id, partition=%task.partition_label, "received task");
                        let registry = registry.clone();
                        let context = ExecutionContext::new(registry);
                        let worker = rspark_cluster::Worker::new(
                            ClusterState::new(worker_id.clone()),
                            worker_id.clone(),
                            cores,
                            memory_mb,
                            context,
                        );
                        let catalog = SessionState::new();
                        match worker.execute_task(&task, &catalog) {
                            Ok(batch) => {
                                let complete_url = format!(
                                    "{}/v1/tasks/{}/complete",
                                    master.trim_end_matches('/'),
                                    task.id
                                );
                                let body = serde_json::json!({ "rows": batch.len() });
                                let res = client.post(&complete_url).json(&body).send().await;
                                if let Err(err) = res {
                                    tracing::error!(?err, "failed to report task completion");
                                }
                            }
                            Err(err) => {
                                let fail_url = format!(
                                    "{}/v1/tasks/{}/fail",
                                    master.trim_end_matches('/'),
                                    task.id
                                );
                                let body = serde_json::json!({ "error": err.to_string() });
                                let _ = client.post(&fail_url).json(&body).send().await;
                            }
                        }
                    }
                    Err(err) => {
                        tracing::error!(?err, "failed to decode task payload");
                    }
                }
            }
            Ok(resp) if resp.status().as_u16() == 204 => {
                sleep(Duration::from_millis(500)).await;
            }
            Ok(resp) => {
                tracing::warn!(status=%resp.status(), "unexpected status from master");
                sleep(Duration::from_secs(2)).await;
            }
            Err(err) => {
                tracing::error!(?err, "poll failed");
                sleep(Duration::from_secs(2)).await;
            }
        }
        let _ = client
            .post(format!(
                "{}/v1/workers/{}/heartbeat",
                master.trim_end_matches('/'),
                worker_id
            ))
            .send()
            .await;
    }
}

fn run_sql(
    file: Option<String>,
    sql: Option<String>,
    input_format: String,
    input: Vec<String>,
    catalog: Option<String>,
    output: Option<String>,
) -> Result<()> {
    let sql_text = match (file, sql) {
        (Some(path), _) => std::fs::read_to_string(path)
            .map_err(|e| rspark_core::error::Error::Storage(e.to_string()))?,
        (None, Some(s)) => s,
        (None, None) => {
            return Err(rspark_core::error::Error::Sql(
                "either --file or a positional sql argument is required".into(),
            ));
        }
    };
    let registry = Arc::new(SourceRegistry::with_defaults());
    let context = ExecutionContext::new(registry);
    let session = build_session(catalog.as_deref(), &input_format, &input)?;
    if let Some(show) = rspark_sql::try_show_create(&sql_text)? {
        let ddl = rspark_sql::render_create_table(session.as_ref(), &show.table_name)?;
        println!("{ddl}");
        return Ok(());
    }
    let planner = Planner::new();
    let plan = planner.plan_sql(&sql_text, session.as_ref())?;
    let executor = LocalExecutor::new(&context);
    let batch = executor.execute(&plan)?;
    if let Some(out_path) = output {
        OutputWriter::write(&batch, &out_path)?;
        println!("wrote {} rows to {out_path}", batch.len());
    } else {
        print!("{}", render_table(&batch));
    }
    Ok(())
}

async fn run_submit(
    master: Option<String>,
    file: String,
    name: String,
    parallelism: usize,
) -> Result<()> {
    let sql = std::fs::read_to_string(&file)
        .map_err(|e| rspark_core::error::Error::Storage(e.to_string()))?;
    let request = JobRequest::new(name, sql).with_parallelism(parallelism);
    let url = match master {
        Some(addr) => format!("{}/v1/jobs", addr.trim_end_matches('/')),
        None => "http://127.0.0.1:7077/v1/jobs".to_string(),
    };
    let client = reqwest::Client::new();
    let resp = client
        .post(url)
        .json(&request)
        .send()
        .await
        .map_err(|e| rspark_core::error::Error::Network(e.to_string()))?;
    if !resp.status().is_success() {
        return Err(rspark_core::error::Error::Cluster(format!(
            "submit failed: {}",
            resp.status()
        )));
    }
    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| rspark_core::error::Error::Network(e.to_string()))?;
    println!(
        "{}",
        serde_json::to_string_pretty(&body).unwrap_or_default()
    );
    Ok(())
}

async fn run_shell(input_format: String, input: Vec<String>) -> Result<()> {
    let session = build_session(None, &input_format, &input)?;
    start_repl(session).await;
    Ok(())
}

async fn run_dashboard(addr: String, master: Option<String>) -> Result<()> {
    let addr: SocketAddr = addr
        .parse()
        .map_err(|e| rspark_core::error::Error::InvalidState(format!("bad dashboard addr: {e}")))?;
    let (state, master_arc, catalog) = if let Some(master_url) = master {
        fetch_remote_state(master_url).await?
    } else {
        let state = ClusterState::new("dashboard-local");
        let master = Arc::new(Master::new(state.clone()));
        let catalog = Arc::new(SessionState::new());
        (state, master, catalog)
    };
    let _ = state;
    rspark_dashboard::run_dashboard(addr, master_arc, catalog)
        .await
        .map_err(|e| rspark_core::error::Error::Cluster(e.to_string()))
}

async fn fetch_remote_state(
    master: String,
) -> Result<(ClusterState, Arc<Master>, Arc<SessionState>)> {
    let client = reqwest::Client::new();
    let url = format!("{}/v1/cluster/snapshot", master.trim_end_matches('/'));
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| rspark_core::error::Error::Network(e.to_string()))?;
    if !resp.status().is_success() {
        return Err(rspark_core::error::Error::Cluster(format!(
            "remote snapshot fetch failed: {}",
            resp.status()
        )));
    }
    let snapshot: rspark_cluster::state::ClusterSnapshot = resp
        .json()
        .await
        .map_err(|e| rspark_core::error::Error::Network(e.to_string()))?;
    let state = ClusterState::new(snapshot.master_id.clone());
    for w in snapshot.workers {
        state.register_worker(w);
    }
    for j in snapshot.jobs {
        state.insert_job(j);
    }
    for s in snapshot.stages {
        state.insert_stage(s);
    }
    for t in snapshot.tasks {
        state.insert_task(t);
    }
    let master = Arc::new(Master::new(state.clone()));
    let catalog = Arc::new(SessionState::new());
    Ok((state, master, catalog))
}

pub fn build_session(
    catalog: Option<&str>,
    input_format: &str,
    inputs: &[String],
) -> Result<Arc<SessionState>> {
    let session = Arc::new(SessionState::new());
    if let Some(catalog_path) = catalog {
        let content = std::fs::read_to_string(catalog_path)
            .map_err(|e| rspark_core::error::Error::Storage(e.to_string()))?;
        let entries: serde_json::Value = serde_json::from_str(&content)?;
        if let Some(arr) = entries.as_array() {
            for entry in arr {
                let name = entry.get("name").and_then(|v| v.as_str()).ok_or_else(|| {
                    rspark_core::error::Error::Storage("catalog entry missing 'name'".into())
                })?;
                let path = entry.get("path").and_then(|v| v.as_str()).ok_or_else(|| {
                    rspark_core::error::Error::Storage("catalog entry missing 'path'".into())
                })?;
                let source = entry
                    .get("source")
                    .and_then(|v| v.as_str())
                    .unwrap_or(input_format);
                let schema = infer_schema(&session, path, source)?;
                session.register(name, path, source, schema)?;
            }
        }
    }
    if !inputs.is_empty() {
        for path in inputs {
            let table_name = Path::new(path)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("table")
                .to_string();
            let schema = infer_schema(&session, path, input_format)?;
            session.register(&table_name, path, input_format, schema)?;
        }
    }
    Ok(session)
}

fn infer_schema(
    _session: &Arc<SessionState>,
    path: &str,
    source: &str,
) -> Result<rspark_core::Schema> {
    let registry = SourceRegistry::with_defaults();
    let src = registry.get(source)?;
    src.infer_schema(path)
}
