use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json};
use axum::routing::{get, post};
use axum::Router;
use chrono::Utc;
use rspark_cluster::job::{JobRequest, JobStatus};
use rspark_cluster::master::Master;
use rspark_cluster::state::WorkerInfo;
use rspark_cluster::task::Task;
use rspark_core::error::Error;
use rspark_exec::{ExecutionContext, LocalExecutor};
use rspark_sql::planner::Catalog;
use rspark_sql::{render_create_table, try_show_create, Planner, SessionState};
use rspark_storage::SourceRegistry;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tower_http::trace::TraceLayer;

// ----- Pipelines -----
//
// The API keeps an in-memory map of named pipeline specs (loaded by
// `POST /v1/pipelines`). They are not persisted across master restarts
// — that's a future job for the Pipeline CRD + operator. For now this
// is enough to power the dashboard's DAG tab and the demo `curl`
// workflows in `docs/pipelines.md`.

#[derive(Clone, Default)]
pub struct PipelineRegistry {
    inner: std::sync::Arc<
        std::sync::RwLock<std::collections::HashMap<String, rspark_pipelines::Pipeline>>,
    >,
}

impl PipelineRegistry {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn insert(&self, p: rspark_pipelines::Pipeline) {
        let mut g = self.inner.write().expect("pipeline registry lock poisoned");
        g.insert(p.pipeline.clone(), p);
    }
    pub fn list(&self) -> Vec<rspark_pipelines::Pipeline> {
        let g = self.inner.read().expect("pipeline registry lock poisoned");
        g.values().cloned().collect()
    }
    pub fn get(&self, name: &str) -> Option<rspark_pipelines::Pipeline> {
        self.inner
            .read()
            .expect("pipeline registry lock poisoned")
            .get(name)
            .cloned()
    }
}

#[derive(Clone)]
pub struct ApiState {
    pub master: Arc<Master>,
    pub catalog: Arc<SessionState>,
    pub source_registry: Arc<SourceRegistry>,
    pub pipelines: PipelineRegistry,
}

impl ApiState {
    pub fn new(
        master: Arc<Master>,
        catalog: Arc<SessionState>,
        source_registry: Arc<SourceRegistry>,
    ) -> Self {
        Self {
            master,
            catalog,
            source_registry,
            pipelines: PipelineRegistry::new(),
        }
    }

    pub fn with_pipelines(mut self, pipelines: PipelineRegistry) -> Self {
        self.pipelines = pipelines;
        self
    }
}

pub fn build_router(state: ApiState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/v1/cluster/snapshot", get(snapshot))
        .route("/v1/workers", post(register_worker))
        .route("/v1/workers/:id/heartbeat", post(heartbeat))
        .route("/v1/workers/:id/task", get(poll_task))
        .route("/v1/tasks/:id/complete", post(complete_task))
        .route("/v1/tasks/:id/fail", post(fail_task))
        .route("/v1/jobs", post(submit_job))
        .route("/v1/jobs", get(list_jobs))
        .route("/v1/jobs/:id", get(get_job))
        .route("/v1/sql", post(execute_sql))
        .route("/v1/catalog/tables", get(list_tables))
        .route("/v1/catalog/tables", post(register_table))
        .route(
            "/v1/catalog/tables/:name",
            axum::routing::delete(unregister_table),
        )
        .route("/v1/catalog/suggestions", get(suggestions))
        .route("/v1/pipelines", post(submit_pipeline))
        .route("/v1/pipelines", get(list_pipelines))
        .route("/v1/pipelines/:name/dag", get(pipeline_dag))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

async fn health() -> impl IntoResponse {
    Json(serde_json::json!({ "status": "ok", "now": Utc::now() }))
}

async fn snapshot(State(state): State<ApiState>) -> impl IntoResponse {
    Json(state.master.state().snapshot())
}

async fn register_worker(
    State(state): State<ApiState>,
    Json(worker): Json<WorkerInfo>,
) -> impl IntoResponse {
    state.master.register_worker(worker.clone());
    (StatusCode::CREATED, Json(worker))
}

async fn heartbeat(State(state): State<ApiState>, Path(id): Path<String>) -> impl IntoResponse {
    if state.master.state().worker(&id).is_none() {
        return (
            StatusCode::NOT_FOUND,
            "worker not registered".into_response(),
        );
    }
    state.master.state().update_worker_heartbeat(&id);
    (StatusCode::NO_CONTENT, ().into_response())
}

async fn poll_task(
    State(state): State<ApiState>,
    Path(worker_id): Path<String>,
) -> impl IntoResponse {
    match state.master.try_assign_task(&worker_id) {
        Ok(Some(task)) => (StatusCode::OK, Json(Some(task))).into_response(),
        Ok(None) => (StatusCode::NO_CONTENT, ().into_response()).into_response(),
        Err(err) => err_response(err),
    }
}

#[derive(Debug, Deserialize)]
pub struct CompleteTaskRequest {
    pub rows: usize,
}

async fn complete_task(
    State(state): State<ApiState>,
    Path(task_id): Path<String>,
    Json(body): Json<CompleteTaskRequest>,
) -> impl IntoResponse {
    match state.master.complete_task(&task_id, body.rows, true, None) {
        Ok(()) => (StatusCode::OK, Json(serde_json::json!({ "status": "ok" }))).into_response(),
        Err(err) => err_response(err),
    }
}

#[derive(Debug, Deserialize)]
pub struct FailTaskRequest {
    pub error: String,
}

async fn fail_task(
    State(state): State<ApiState>,
    Path(task_id): Path<String>,
    Json(body): Json<FailTaskRequest>,
) -> impl IntoResponse {
    match state
        .master
        .complete_task(&task_id, 0, false, Some(body.error))
    {
        Ok(()) => (StatusCode::OK, Json(serde_json::json!({ "status": "ok" }))).into_response(),
        Err(err) => err_response(err),
    }
}

#[derive(Debug, Serialize)]
pub struct SubmitJobResponse {
    pub job: rspark_cluster::job::Job,
    pub stages: Vec<rspark_cluster::stage::Stage>,
    pub tasks: Vec<Task>,
}

async fn submit_job(
    State(state): State<ApiState>,
    Json(request): Json<JobRequest>,
) -> impl IntoResponse {
    match state.master.submit_job(request, state.catalog.as_ref()) {
        Ok(job) => {
            let stages = state.master.state().stages_for_job(&job.id);
            let tasks = state.master.state().tasks_for_job(&job.id);
            let _ = state.master.state().inc_running_round();
            (
                StatusCode::CREATED,
                Json(SubmitJobResponse { job, stages, tasks }),
            )
                .into_response()
        }
        Err(err) => err_response(err),
    }
}

async fn list_jobs(State(state): State<ApiState>) -> impl IntoResponse {
    Json(state.master.state().list_jobs())
}

async fn get_job(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> axum::response::Response {
    match state.master.state().job(&id) {
        Some(job) => (StatusCode::OK, Json(job)).into_response(),
        None => (StatusCode::NOT_FOUND, "job not found").into_response(),
    }
}

#[derive(Debug, Deserialize)]
pub struct ExecuteSqlRequest {
    pub sql: String,
    pub job_name: Option<String>,
    pub parallelism: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct ColumnMeta {
    pub name: String,
    pub data_type: String,
}

#[derive(Debug, Serialize)]
pub struct ExecuteSqlResponse {
    pub job: rspark_cluster::job::Job,
    pub columns: Vec<ColumnMeta>,
    pub rows: Vec<Vec<serde_json::Value>>,
    pub row_count: usize,
    pub duration_ms: u128,
}

async fn execute_sql(
    State(state): State<ApiState>,
    Json(body): Json<ExecuteSqlRequest>,
) -> impl IntoResponse {
    let started = std::time::Instant::now();

    if let Some(show) = match try_show_create(&body.sql) {
        Ok(s) => s,
        Err(err) => return err_response(err),
    } {
        let name = body.job_name.clone().unwrap_or_else(|| "ad-hoc".into());
        let request = JobRequest::new(name, body.sql.clone()).with_parallelism(1);
        let mut job = match state.master.submit_job_skip_plan(request) {
            Ok(j) => j,
            Err(err) => return err_response(err),
        };
        let _ = state.master.state().inc_running_round();
        match render_create_table(state.catalog.as_ref(), &show.table_name) {
            Ok(ddl) => {
                job.status = JobStatus::Succeeded;
                job.completed_at = Some(Utc::now());
                job.result_rows = Some(1);
                state.master.state().update_job(job.clone());
                state.master.state().record_completed_round();
                let row = vec![serde_json::Value::String(ddl)];
                return (
                    StatusCode::OK,
                    Json(ExecuteSqlResponse {
                        job,
                        columns: vec![ColumnMeta {
                            name: "create_statement".into(),
                            data_type: "String".into(),
                        }],
                        rows: vec![row],
                        row_count: 1,
                        duration_ms: started.elapsed().as_millis(),
                    }),
                )
                    .into_response();
            }
            Err(err) => return err_response(err),
        }
    }

    let name = body.job_name.clone().unwrap_or_else(|| "ad-hoc".into());
    let request = JobRequest::new(name.clone(), body.sql.clone()).with_parallelism(1);

    let job = match state.master.submit_job(request, state.catalog.as_ref()) {
        Ok(j) => j,
        Err(err) => return err_response(err),
    };
    let _ = state.master.state().inc_running_round();

    let planner = Planner::new();
    let plan = match planner.plan_sql(&body.sql, state.catalog.as_ref()) {
        Ok(p) => p,
        Err(err) => return err_response(err),
    };

    let context = ExecutionContext::new(state.source_registry.clone());
    let executor = LocalExecutor::new(&context);
    let batch = match executor.execute(&plan) {
        Ok(b) => b,
        Err(err) => {
            let mut failed = job.clone();
            failed.status = JobStatus::Failed(err.to_string());
            failed.completed_at = Some(Utc::now());
            state.master.state().update_job(failed);
            return err_response(err);
        }
    };

    let mut job = job;
    let row_count = batch.len();
    job.status = JobStatus::Succeeded;
    job.completed_at = Some(Utc::now());
    job.result_rows = Some(row_count);
    state.master.state().update_job(job.clone());

    let _ = state.master.state().inc_running_round();
    state.master.state().record_completed_round();

    let columns: Vec<ColumnMeta> = batch
        .schema()
        .fields()
        .iter()
        .map(|f| ColumnMeta {
            name: f.name.clone(),
            data_type: format!("{:?}", f.data_type),
        })
        .collect();
    let rows: Vec<Vec<serde_json::Value>> = batch
        .records()
        .iter()
        .map(|r| r.values().iter().map(value_to_json).collect())
        .collect();

    (
        StatusCode::OK,
        Json(ExecuteSqlResponse {
            job,
            columns,
            rows,
            row_count,
            duration_ms: started.elapsed().as_millis(),
        }),
    )
        .into_response()
}

fn value_to_json(v: &rspark_core::value::Value) -> serde_json::Value {
    use rspark_core::value::Value::*;
    match v {
        Null => serde_json::Value::Null,
        Boolean(b) => serde_json::Value::Bool(*b),
        Int32(i) => serde_json::Value::from(*i),
        Int64(i) => serde_json::Value::from(*i),
        Float32(f) => serde_json::Value::from(*f as f64),
        Float64(f) => serde_json::Value::from(*f),
        String(s) => serde_json::Value::from(s.as_str()),
    }
}

#[derive(Debug, Serialize)]
pub struct TableSummary {
    pub name: String,
    pub path: String,
    pub source: String,
    pub columns: Vec<ColumnMeta>,
}

async fn list_tables(State(state): State<ApiState>) -> axum::response::Response {
    let names = match state.catalog.list_tables() {
        Ok(n) => n,
        Err(err) => return err_response(err),
    };
    let mut out = Vec::with_capacity(names.len());
    for name in names {
        let schema = match state.catalog.table_schema(&name) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let (path, source) = match state.catalog.table_location(&name) {
            Ok(p) => p,
            Err(_) => continue,
        };
        out.push(TableSummary {
            name: name.clone(),
            path,
            source,
            columns: schema
                .fields()
                .iter()
                .map(|f| ColumnMeta {
                    name: f.name.clone(),
                    data_type: format!("{:?}", f.data_type),
                })
                .collect(),
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    (StatusCode::OK, Json(out)).into_response()
}

#[derive(Debug, Deserialize)]
pub struct RegisterTableRequest {
    pub name: String,
    pub path: String,
    pub source: Option<String>,
    /// `batch` (default), `streaming_table`, or `materialized_view`.
    /// Without this, the catalog entry is always `batch`. The seed
    /// script uses it to re-promote `click_events` to `streaming_table`
    /// after a rolling restart, since the pipeline runner's first
    /// registration flips the kind but points the catalog at a
    /// pipe-delimited output file that the CSV reader can't parse.
    #[serde(default)]
    pub kind: Option<String>,
}

async fn register_table(
    State(state): State<ApiState>,
    Json(body): Json<RegisterTableRequest>,
) -> impl IntoResponse {
    let source = body.source.unwrap_or_else(|| {
        let lower = body.path.to_ascii_lowercase();
        if lower.ends_with(".json") {
            "json".to_string()
        } else {
            "csv".to_string()
        }
    });
    let source_obj = match state.source_registry.get(&source) {
        Ok(s) => s,
        Err(err) => return err_response(err),
    };
    let schema = match source_obj.infer_schema(&body.path) {
        Ok(s) => s,
        Err(err) => return err_response(err),
    };
    let kind = match body.kind.as_deref() {
        Some("streaming_table") => rspark_sql::TableKind::StreamingTable,
        Some("materialized_view") => rspark_sql::TableKind::MaterializedView,
        _ => rspark_sql::TableKind::Batch,
    };
    match state
        .catalog
        .register_with_kind(&body.name, &body.path, &source, schema, kind)
    {
        Ok(()) => (
            StatusCode::CREATED,
            Json(serde_json::json!({ "status": "ok", "name": body.name })),
        )
            .into_response(),
        Err(err) => err_response(err),
    }
}

async fn unregister_table(
    State(state): State<ApiState>,
    Path(name): Path<String>,
) -> axum::response::Response {
    match state.catalog.unregister(&name) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(err) => err_response(err),
    }
}

const SQL_KEYWORDS: &[&str] = &[
    "SELECT",
    "FROM",
    "WHERE",
    "GROUP",
    "BY",
    "ORDER",
    "HAVING",
    "LIMIT",
    "OFFSET",
    "JOIN",
    "INNER",
    "LEFT",
    "RIGHT",
    "FULL",
    "OUTER",
    "CROSS",
    "ON",
    "USING",
    "AS",
    "AND",
    "OR",
    "NOT",
    "IN",
    "BETWEEN",
    "LIKE",
    "IS",
    "NULL",
    "TRUE",
    "FALSE",
    "DISTINCT",
    "ALL",
    "UNION",
    "INTERSECT",
    "EXCEPT",
    "ASC",
    "DESC",
    "CASE",
    "WHEN",
    "THEN",
    "ELSE",
    "END",
    "COALESCE",
    "NVL",
];

const SQL_FUNCTIONS: &[&str] = &[
    "COUNT",
    "SUM",
    "AVG",
    "MIN",
    "MAX",
    "ABS",
    "UPPER",
    "UCASE",
    "LOWER",
    "LCASE",
    "LENGTH",
    "CHAR_LENGTH",
    "CHARACTER_LENGTH",
    "COALESCE",
    "NVL",
];

#[derive(Debug, Serialize)]
pub struct Suggestions {
    pub tables: Vec<TableSuggestion>,
    pub columns: Vec<String>,
    pub functions: Vec<String>,
    pub keywords: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct TableSuggestion {
    pub name: String,
    /// `batch`, `streaming_table`, or `materialized_view`.
    pub kind: String,
}

async fn suggestions(State(state): State<ApiState>) -> axum::response::Response {
    let names_with_kind = match state.catalog.list_tables_with_kind() {
        Ok(n) => n,
        Err(err) => return err_response(err),
    };
    let mut columns = std::collections::BTreeSet::new();
    let tables: Vec<TableSuggestion> = names_with_kind
        .iter()
        .map(|(name, kind)| {
            if let Ok(schema) = state.catalog.table_schema(name) {
                for f in schema.fields() {
                    columns.insert(f.name.clone());
                }
            }
            TableSuggestion {
                name: name.clone(),
                kind: kind.as_str().to_string(),
            }
        })
        .collect();
    (
        StatusCode::OK,
        Json(Suggestions {
            tables,
            columns: columns.into_iter().collect(),
            functions: SQL_FUNCTIONS.iter().map(|s| s.to_string()).collect(),
            keywords: SQL_KEYWORDS.iter().map(|s| s.to_string()).collect(),
        }),
    )
        .into_response()
}

fn err_response(err: Error) -> axum::response::Response {
    let body = serde_json::json!({
        "error": err.to_string(),
        "kind": format!("{err:?}"),
    });
    (StatusCode::INTERNAL_SERVER_ERROR, Json(body)).into_response()
}

async fn submit_pipeline(State(state): State<ApiState>, body: String) -> axum::response::Response {
    let pipeline = match rspark_pipelines::Pipeline::from_yaml(&body) {
        Ok(p) => p,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": e.to_string(), "kind": "spec_error"})),
            )
                .into_response();
        }
    };
    let name = pipeline.pipeline.clone();
    state.pipelines.insert(pipeline.clone());
    let registry = state.source_registry.clone();
    let catalog = state.catalog.clone();
    let report = match run_pipeline_now(&pipeline, registry, catalog).await {
        Ok(r) => r,
        Err(e) => return err_response(e),
    };
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "name": name,
            "flows": pipeline.flows.iter().map(|f| f.name.clone()).collect::<Vec<_>>(),
            "report": report,
        })),
    )
        .into_response()
}

async fn list_pipelines(State(state): State<ApiState>) -> impl IntoResponse {
    let items: Vec<_> = state
        .pipelines
        .list()
        .into_iter()
        .map(|p| {
            serde_json::json!({
                "name": p.pipeline,
                "flows": p.flows.iter().map(|f| f.name.clone()).collect::<Vec<_>>(),
            })
        })
        .collect();
    Json(items)
}

async fn pipeline_dag(
    State(state): State<ApiState>,
    Path(name): Path<String>,
) -> axum::response::Response {
    let Some(pipeline) = state.pipelines.get(&name) else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": format!("pipeline '{name}' not found")})),
        )
            .into_response();
    };
    match rspark_pipelines::PipelineDag::from_pipeline(&pipeline) {
        Ok(dag) => {
            let layers: Vec<Vec<String>> = dag
                .layers()
                .into_iter()
                .map(|layer| {
                    layer
                        .into_iter()
                        .filter_map(|fid| dag.flow(fid).map(|s| s.to_string()))
                        .collect()
                })
                .collect();
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "name": pipeline.pipeline,
                    "layers": layers,
                    "flows": pipeline.flows.iter().map(|f| {
                        serde_json::json!({
                            "name": f.name,
                            "kind": f.kind,
                            "depends_on": f.depends_on,
                        })
                    }).collect::<Vec<_>>(),
                })),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": e.to_string(), "kind": "spec_error"})),
        )
            .into_response(),
    }
}

async fn run_pipeline_now(
    pipeline: &rspark_pipelines::Pipeline,
    registry: Arc<SourceRegistry>,
    catalog: Arc<SessionState>,
) -> rspark_core::error::Result<rspark_pipelines::PipelineRunReport> {
    let runner = rspark_pipelines::PipelineRunner::new(registry, catalog);
    runner.run(pipeline).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use rspark_cluster::state::ClusterState;
    use rspark_core::schema::{DataType, Field, Schema};
    use std::sync::Arc;

    #[tokio::test]
    async fn health_endpoint_works() {
        let state = ClusterState::new("test-master");
        let master = Arc::new(Master::new(state));
        let catalog = Arc::new(SessionState::new());
        let registry = Arc::new(SourceRegistry::with_defaults());
        let api_state = ApiState::new(master, catalog, registry);
        let app = build_router(api_state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let client = reqwest::Client::new();
        let resp = client
            .get(format!("http://{addr}/health"))
            .send()
            .await
            .unwrap();
        assert!(resp.status().is_success());
    }

    #[test]
    fn value_to_json_serializes_correctly() {
        use rspark_core::value::Value;
        assert_eq!(value_to_json(&Value::Null), serde_json::Value::Null);
        assert_eq!(
            value_to_json(&Value::Boolean(true)),
            serde_json::json!(true)
        );
        assert_eq!(value_to_json(&Value::Int64(42)), serde_json::json!(42));
        assert_eq!(value_to_json(&Value::Float64(2.5)), serde_json::json!(2.5));
        assert_eq!(
            value_to_json(&Value::String("hello".into())),
            serde_json::json!("hello")
        );
    }

    #[test]
    fn table_summary_round_trip() {
        let schema = Schema::new(vec![
            Field::new("id", DataType::Int64),
            Field::new("name", DataType::String),
        ]);
        let summary = TableSummary {
            name: "users".into(),
            path: "/tmp/users.csv".into(),
            source: "csv".into(),
            columns: schema
                .fields()
                .iter()
                .map(|f| ColumnMeta {
                    name: f.name.clone(),
                    data_type: format!("{:?}", f.data_type),
                })
                .collect(),
        };
        let json = serde_json::to_string(&summary).unwrap();
        assert!(json.contains("users"));
        assert!(json.contains("Int64"));
    }

    #[test]
    fn suggestions_includes_tables_and_columns() {
        let schema = Schema::new(vec![
            Field::new("id", DataType::Int64),
            Field::new("name", DataType::String),
        ]);
        let catalog = SessionState::new();
        catalog
            .register("employees", "/data/employees.csv", "csv", schema)
            .unwrap();
        let sugg = Suggestions {
            tables: catalog
                .list_tables_with_kind()
                .unwrap()
                .into_iter()
                .map(|(name, kind)| TableSuggestion {
                    name,
                    kind: kind.as_str().to_string(),
                })
                .collect(),
            columns: vec!["id".into(), "name".into()],
            functions: SQL_FUNCTIONS.iter().map(|s| s.to_string()).collect(),
            keywords: SQL_KEYWORDS.iter().map(|s| s.to_string()).collect(),
        };
        assert!(sugg.tables.iter().any(|t| t.name == "employees"));
        assert_eq!(sugg.tables[0].kind, "batch");
        assert!(sugg.columns.contains(&"name".to_string()));
        assert!(sugg.functions.contains(&"COUNT".to_string()));
        assert!(sugg.keywords.contains(&"SELECT".to_string()));
    }

    #[tokio::test]
    async fn pipelines_list_endpoint_returns_empty_initially() {
        let state = ClusterState::new("test-master");
        let master = Arc::new(Master::new(state));
        let catalog = Arc::new(SessionState::new());
        let registry = Arc::new(SourceRegistry::with_defaults());
        let api_state = ApiState::new(master, catalog, registry);
        let app = build_router(api_state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let client = reqwest::Client::new();
        let resp = client
            .get(format!("http://{addr}/v1/pipelines"))
            .send()
            .await
            .unwrap();
        assert!(resp.status().is_success());
        let body: serde_json::Value = resp.json().await.unwrap();
        assert!(body.as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn pipelines_dag_returns_404_for_unknown() {
        let state = ClusterState::new("test-master");
        let master = Arc::new(Master::new(state));
        let catalog = Arc::new(SessionState::new());
        let registry = Arc::new(SourceRegistry::with_defaults());
        let api_state = ApiState::new(master, catalog, registry);
        let app = build_router(api_state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let client = reqwest::Client::new();
        let resp = client
            .get(format!("http://{addr}/v1/pipelines/missing/dag"))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), axum::http::StatusCode::NOT_FOUND);
    }

    #[test]
    fn pipelines_registry_insert_and_get_round_trip() {
        let reg = PipelineRegistry::new();
        let yaml = "pipeline: p1\nflows:\n  - name: a\n    kind: materialized_view\n    source: { kind: sql }\n    query: \"SELECT 1\"\n    destination: { kind: file, path: /tmp/a }\n";
        let p = rspark_pipelines::Pipeline::from_yaml(yaml).unwrap();
        reg.insert(p.clone());
        let got = reg.get("p1").unwrap();
        assert_eq!(got.pipeline, "p1");
        assert_eq!(got.flows.len(), 1);
        assert_eq!(reg.list().len(), 1);
    }
}
